// Files built to hurt the reader: arithmetic that wraps, nesting that recurses, and strips that
// each name the whole file. Each must come back as an ordinary error, never a panic or an abort.
use std::io::Cursor;

use bb_metadata::{AudioPayload, AudioTagCodec, MetadataCodec, TiffCodec, XmpPacket};

const ASF_HEADER: [u8; 16] = [
    0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const ASF_DATA: [u8; 16] = [
    0x36, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];

#[test]
fn an_asf_object_whose_length_wraps_is_refused() {
    let mut file = Vec::new();
    file.extend_from_slice(&ASF_HEADER);
    file.extend_from_slice(&54u64.to_le_bytes()); // header object ends where the data object starts
    file.extend_from_slice(&1u32.to_le_bytes());
    file.extend_from_slice(&[1, 2]);
    // One object at 30 whose length added to 30 wraps to a small number.
    file.extend_from_slice(&[7u8; 16]);
    file.extend_from_slice(&(u64::MAX - 5).to_le_bytes());
    assert_eq!(54, file.len());
    file.extend_from_slice(&ASF_DATA);
    file.extend_from_slice(&24u64.to_le_bytes());

    let result = AudioTagCodec::new(".wma").read(&mut Cursor::new(file));
    assert!(result.is_err());
}

#[test]
fn an_mpeg4_atom_whose_size_wraps_is_refused() {
    let mut file = Vec::new();
    file.extend_from_slice(&8u32.to_be_bytes());
    file.extend_from_slice(b"free");
    // A 64-bit size so large that position + size wraps back inside the file.
    file.extend_from_slice(&1u32.to_be_bytes());
    file.extend_from_slice(b"mdat");
    file.extend_from_slice(&(u64::MAX - 2).to_be_bytes());
    file.extend_from_slice(&[0u8; 16]);

    assert!(AudioPayload::digest(".m4a", &mut Cursor::new(file)).is_err());
}

#[test]
fn tiff_strips_that_each_name_the_whole_file_cannot_be_hashed() {
    const STRIPS: usize = 1000;
    let length = 38 + 8 * STRIPS;
    let mut file = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
    file.extend_from_slice(&2u16.to_le_bytes());
    for (tag, array_at) in [(273u16, 38usize), (279u16, 38 + 4 * STRIPS)] {
        file.extend_from_slice(&tag.to_le_bytes());
        file.extend_from_slice(&4u16.to_le_bytes());
        file.extend_from_slice(&(STRIPS as u32).to_le_bytes());
        file.extend_from_slice(&(array_at as u32).to_le_bytes());
    }
    file.extend_from_slice(&0u32.to_le_bytes()); // no next directory
    assert_eq!(38, file.len());
    for _ in 0..STRIPS {
        file.extend_from_slice(&0u32.to_le_bytes()); // every strip starts at the top...
    }
    for _ in 0..STRIPS {
        file.extend_from_slice(&(length as u32).to_le_bytes()); // ...and is the whole file
    }
    assert_eq!(length, file.len());

    let result = TiffCodec.payload_digest(&mut Cursor::new(file));
    assert!(matches!(result, Err(e) if e.is_format()));
}

#[test]
fn xml_nested_past_the_limit_is_not_parsed() {
    let depth = 300;
    let packet = format!("{}GPS{}", "<a>".repeat(depth), "</a>".repeat(depth));
    // Unparseable is treated as "has a location", and a removal refuses.
    assert!(XmpPacket::has_location(packet.as_bytes()));
    assert!(matches!(
        XmpPacket::without_location(packet.as_bytes()),
        Err(e) if e.is_format()
    ));

    // A very deep one must not take the stack down.
    let deep = format!("{}GPS", "<a>".repeat(200_000));
    assert!(XmpPacket::has_location(deep.as_bytes()));
}
