// The GIF decoder is this port's own (the C# used WPF's), so these are its own tests. The fixture
// is laid out by hand from the spec, the way the harness's `WriteGif` does it: a 128-colour
// palette so every LZW code is exactly 8 bits wide, and a clear code every hundred pixels so the
// table never grows past that width. Uncompressed in all but name.
use bb_core::cancel::CancelToken;
use bb_core::preview::animation::{GifDecoder, GifDisposal, GifError};

struct Frame {
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    indices: Vec<u8>,
    delay_cs: u16,
    disposal: u8,
}

fn write_gif(width: u16, height: u16, frames: &[Frame]) -> Vec<u8> {
    let mut w = Vec::new();
    w.extend_from_slice(b"GIF89a");
    w.extend_from_slice(&width.to_le_bytes());
    w.extend_from_slice(&height.to_le_bytes());
    w.push(0xF6); // global colour table, 128 entries
    w.push(0);
    w.push(0);

    let mut palette = vec![0u8; 128 * 3];
    palette[3..6].copy_from_slice(&[0xE0, 0x30, 0x30]); // 1 red
    palette[6..9].copy_from_slice(&[0x30, 0xC0, 0x50]); // 2 green
    palette[9..12].copy_from_slice(&[0x30, 0x60, 0xE0]); // 3 blue
    w.extend_from_slice(&palette);

    if frames.len() > 1 {
        w.extend_from_slice(&[0x21, 0xFF, 0x0B]);
        w.extend_from_slice(b"NETSCAPE2.0");
        w.extend_from_slice(&[0x03, 0x01, 0x00, 0x00, 0x00]);
    }

    for frame in frames {
        w.extend_from_slice(&[0x21, 0xF9, 0x04, (frame.disposal << 2) | 1]);
        w.extend_from_slice(&frame.delay_cs.to_le_bytes());
        w.extend_from_slice(&[0x00, 0x00]); // transparent index 0, terminator

        w.push(0x2C);
        w.extend_from_slice(&frame.left.to_le_bytes());
        w.extend_from_slice(&frame.top.to_le_bytes());
        w.extend_from_slice(&frame.width.to_le_bytes());
        w.extend_from_slice(&frame.height.to_le_bytes());
        w.push(0);

        w.push(7); // minimum code size: codes start 8 bits wide
        let mut codes = Vec::new();
        for (i, &index) in frame.indices.iter().enumerate() {
            if i % 100 == 0 {
                codes.push(128); // clear
            }
            codes.push(index);
        }
        codes.push(129); // end of information

        for chunk in codes.chunks(255) {
            w.push(chunk.len() as u8);
            w.extend_from_slice(chunk);
        }
        w.push(0);
    }

    w.push(0x3B);
    w
}

fn pixel(pixels: &[u8], index: usize) -> [u8; 4] {
    pixels[index * 4..index * 4 + 4].try_into().unwrap()
}

#[test]
fn frames_delays_disposal_and_transparency_are_read() {
    let bytes = write_gif(
        2,
        1,
        &[
            Frame {
                left: 0,
                top: 0,
                width: 2,
                height: 1,
                indices: vec![1, 3],
                delay_cs: 25,
                disposal: 0,
            },
            Frame {
                left: 1,
                top: 0,
                width: 1,
                height: 1,
                indices: vec![0],
                delay_cs: 0,
                disposal: 2,
            },
        ],
    );

    let animation = GifDecoder::decode(&bytes, &CancelToken::none()).unwrap();

    assert_eq!((2, 1), (animation.width, animation.height));
    assert_eq!(vec![250, 0], animation.delays_ms);
    assert_eq!(2, animation.frames.len());

    let first = &animation.frames[0];
    assert_eq!(
        (0, 0, 2, 1),
        (first.left, first.top, first.width, first.height)
    );
    assert_eq!(GifDisposal::Keep, first.disposal);
    assert_eq!([0x30, 0x30, 0xE0, 0xFF], pixel(&first.pixels, 0)); // red, as BGRA
    assert_eq!([0xE0, 0x60, 0x30, 0xFF], pixel(&first.pixels, 1)); // blue

    let second = &animation.frames[1];
    assert_eq!(
        (1, 0, 1, 1),
        (second.left, second.top, second.width, second.height)
    );
    assert_eq!(GifDisposal::Clear, second.disposal);
    assert_eq!([0, 0, 0, 0], pixel(&second.pixels, 0)); // index 0 is transparent
}

#[test]
fn a_larger_frame_round_trips_every_index() {
    let indices: Vec<u8> = (0..(30 * 20)).map(|i| (i % 127 + 1) as u8).collect();
    let bytes = write_gif(
        30,
        20,
        &[Frame {
            left: 0,
            top: 0,
            width: 30,
            height: 20,
            indices: indices.clone(),
            delay_cs: 10,
            disposal: 1,
        }],
    );

    let animation = GifDecoder::decode(&bytes, &CancelToken::none()).unwrap();
    let pixels = &animation.frames[0].pixels;

    // Indices past the three named colours are black in this palette, but opaque: every pixel
    // was decoded, none was left transparent.
    assert_eq!(30 * 20 * 4, pixels.len());
    assert!(pixels.chunks(4).all(|p| p[3] == 0xFF));
    assert_eq!([0x30, 0x30, 0xE0, 0xFF], pixel(pixels, 0)); // index 1, red
    assert_eq!([0xE0, 0x60, 0x30, 0xFF], pixel(pixels, 2)); // index 3, blue
}

#[test]
fn probe_counts_frames_without_decoding() {
    let bytes = write_gif(
        4,
        4,
        &[
            Frame {
                left: 0,
                top: 0,
                width: 4,
                height: 4,
                indices: vec![1; 16],
                delay_cs: 5,
                disposal: 0,
            },
            Frame {
                left: 0,
                top: 0,
                width: 4,
                height: 4,
                indices: vec![2; 16],
                delay_cs: 5,
                disposal: 0,
            },
            Frame {
                left: 0,
                top: 0,
                width: 4,
                height: 4,
                indices: vec![3; 16],
                delay_cs: 5,
                disposal: 0,
            },
        ],
    );

    let header = GifDecoder::probe(&bytes).unwrap();
    assert_eq!((4, 4, 3), (header.width, header.height, header.frame_count));
}

#[test]
fn a_truncated_file_yields_the_frames_before_the_cut() {
    let whole = write_gif(
        2,
        1,
        &[
            Frame {
                left: 0,
                top: 0,
                width: 2,
                height: 1,
                indices: vec![1, 1],
                delay_cs: 5,
                disposal: 0,
            },
            Frame {
                left: 0,
                top: 0,
                width: 2,
                height: 1,
                indices: vec![2, 2],
                delay_cs: 5,
                disposal: 0,
            },
        ],
    );

    // Cut inside the second frame's data.
    let cut = &whole[..whole.len() - 4];
    let animation = GifDecoder::decode(cut, &CancelToken::none()).unwrap();
    assert_eq!(1, animation.frames.len());
}

#[test]
fn not_a_gif_is_refused() {
    assert_eq!(
        Err(GifError::NotAGif),
        GifDecoder::probe(b"\x89PNG\r\n\x1a\n").map(|_| ())
    );
}
