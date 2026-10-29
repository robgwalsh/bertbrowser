//! Finds the sound in an audio file — the bytes no tag edit may change — without asking the tag
//! library where it is. Spec: Core/Services/Metadata/AudioPayload.cs.
//!
//! Deliberately our own reading rather than the library's. The digest of this range is what the
//! executor compares before and after a rewrite, and a check that asks the code under suspicion
//! where to look is not much of a check.

use std::io::SeekFrom;

use sha2::{Digest, Sha256};

use super::codec::{
    MetadataError, MetadataResult, Source, finish, hash_range, read_at, stream_len,
};

/// The extensions a locator exists for. A container with no locator cannot be edited, however
/// well the library reads it: there would be nothing to check its work with.
pub const EXTENSIONS: [&str; 13] = [
    ".mp3", ".flac", ".wav", ".m4a", ".m4b", ".mp4", ".m4v", ".wma", ".aiff", ".aif", ".opus",
    ".ogg", ".oga",
];

pub struct AudioPayload;

fn not_audio() -> MetadataError {
    MetadataError::format("This is not a readable audio file, so it was left as it is.")
}

fn append(
    hash: &mut Sha256,
    source: &mut dyn Source,
    start: u64,
    length: u64,
) -> MetadataResult<()> {
    hash_range(hash, source, start, length, not_audio)
}

impl AudioPayload {
    pub fn digest(extension: &str, source: &mut dyn Source) -> MetadataResult<Vec<u8>> {
        let mut hash = Sha256::new();

        match extension {
            ".mp3" => {
                let (start, end) = mp3(source)?;
                append(&mut hash, source, start, end - start)?;
            }
            ".flac" => flac(source, &mut hash)?,
            ".wav" => wave(source, &mut hash)?,
            ".m4a" | ".m4b" | ".mp4" | ".m4v" => mpeg4(source, &mut hash)?,
            ".wma" => asf(source, &mut hash)?,
            ".aiff" | ".aif" => aiff(source, &mut hash)?,
            ".opus" | ".ogg" | ".oga" => ogg(source, &mut hash)?,
            _ => {
                return Err(MetadataError::format(
                    "This kind of media file cannot be edited.",
                ));
            }
        }

        Ok(finish(hash))
    }
}

/// FLAC: a marker, a chain of metadata blocks, then frames to the end. The first block says what
/// the frames are (sample rate, channels, a checksum of the decoded sound) and so counts as
/// sound; every other block is description.
fn flac(source: &mut dyn Source, hash: &mut Sha256) -> MetadataResult<()> {
    let mut position = skip_id3v2(source, 0)?;
    let mut head = [0u8; 4];
    if read_at(source, position, &mut head)? != 4 || &head != b"fLaC" {
        return Err(not_audio());
    }
    position += 4;

    loop {
        if read_at(source, position, &mut head)? != 4 {
            return Err(not_audio());
        }
        let length = ((head[1] as u64) << 16) | ((head[2] as u64) << 8) | head[3] as u64;
        if head[0] & 0x7F == 0 {
            append(hash, source, position + 4, length)?;
        }

        position += 4 + length;
        if head[0] & 0x80 != 0 {
            break;
        }
    }

    let mut end = stream_len(source)?;
    let mut tail = [0u8; 3];
    if end >= position + 128 && read_at(source, end - 128, &mut tail)? == 3 && &tail == b"TAG" {
        end -= 128;
    }

    if end <= position {
        return Err(not_audio());
    }
    append(hash, source, position, end - position)
}

/// WAV: a list of chunks, of which two are the sound — how it is encoded, and it.
fn wave(source: &mut dyn Source, hash: &mut Sha256) -> MetadataResult<()> {
    let total = stream_len(source)?;
    let mut head = [0u8; 12];
    if read_at(source, 0, &mut head)? != 12 || &head[..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return Err(not_audio());
    }

    let mut found = false;
    let mut position: u64 = 12;
    let mut chunk = [0u8; 8];

    while read_at(source, position, &mut chunk)? == 8 {
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as u64;
        let is_data = &chunk[..4] == b"data";

        if is_data || &chunk[..4] == b"fmt " {
            // A data chunk's size is routinely wrong in a file cut short; take what is there.
            let available = size.min(total.saturating_sub(position + 8));
            hash.update(&chunk[..4]);
            append(hash, source, position + 8, available)?;
            found |= is_data;
        }

        position += 8 + size + (size & 1);
    }

    if !found {
        return Err(not_audio());
    }
    Ok(())
}

/// MPEG-4 (M4A, MP4): the sound and picture live in `mdat`, and a table in `moov` says where in
/// the file each piece of it starts.
///
/// **Hashing `mdat` is not enough here**, and this is the format where that matters. The tags sit
/// in `moov`, which usually comes first — so growing them pushes `mdat` down the file, and every
/// offset in every track's table has to be moved by the same amount. A writer that misses one
/// leaves the bytes perfect and the file unplayable. So the digest also takes the first bytes
/// found *at* each table entry: unchanged only if every entry still points at what it pointed at
/// before.
fn mpeg4(source: &mut dyn Source, hash: &mut Sha256) -> MetadataResult<()> {
    let mut saw_data = false;
    let mut saw_movie = false;
    let mut offsets: Vec<u64> = Vec::new();

    let total = stream_len(source)?;
    for (kind, start, length) in atoms(source, 0, total)? {
        match &kind {
            b"mdat" => {
                append(hash, source, start, length)?;
                saw_data = true;
            }
            b"moov" => {
                chunk_offsets(source, start, start + length, &mut offsets, 0)?;
                saw_movie = true;
            }
            b"moof" => {
                // Fragmented: the tables are spread through the file in a form this does not check.
                return Err(MetadataError::format(
                    "This is a fragmented MP4, which cannot be edited safely.",
                ));
            }
            _ => {}
        }
    }

    if !saw_data || !saw_movie {
        return Err(not_audio());
    }

    let mut probe = [0u8; 16];
    hash.update((offsets.len() as i32).to_le_bytes());
    for offset in offsets {
        probe.fill(0);
        read_at(source, offset, &mut probe)?;
        hash.update(probe);
    }
    Ok(())
}

fn chunk_offsets(
    source: &mut dyn Source,
    start: u64,
    end: u64,
    offsets: &mut Vec<u64>,
    depth: u32,
) -> MetadataResult<()> {
    if depth > 8 {
        return Err(not_audio());
    }

    for (kind, at, length) in atoms(source, start, end)? {
        match &kind {
            b"trak" | b"mdia" | b"minf" | b"stbl" => {
                chunk_offsets(source, at, at + length, offsets, depth + 1)?;
            }
            b"stco" | b"co64" => {
                let wide = &kind == b"co64";
                let width: u64 = if wide { 8 } else { 4 };
                let mut header = [0u8; 8];
                if read_at(source, at, &mut header)? != 8 {
                    return Err(not_audio());
                }

                let count = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as u64;
                if count > 5_000_000 || count * width > length.saturating_sub(8) {
                    return Err(not_audio());
                }

                let mut table = vec![0u8; (count * width) as usize];
                if read_at(source, at + 8, &mut table)? != table.len() {
                    return Err(not_audio());
                }

                for i in 0..count as usize {
                    offsets.push(if wide {
                        u64::from_be_bytes(table[i * 8..i * 8 + 8].try_into().expect("8 bytes"))
                    } else {
                        u32::from_be_bytes(table[i * 4..i * 4 + 4].try_into().expect("4 bytes"))
                            as u64
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// The atoms in a range: each one's type, and where its content starts and how long it is.
fn atoms(
    source: &mut dyn Source,
    start: u64,
    end: u64,
) -> MetadataResult<Vec<([u8; 4], u64, u64)>> {
    let mut head = [0u8; 16];
    let mut position = start;
    let mut seen = 0;
    let mut out = Vec::new();

    while position + 8 <= end {
        seen += 1;
        if seen > 100_000 || read_at(source, position, &mut head[..8])? != 8 {
            return Err(not_audio());
        }

        let mut size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as u64;
        let mut header_length = 8;

        if size == 1 {
            if read_at(source, position + 8, &mut head[8..16])? != 8 {
                return Err(not_audio());
            }
            size = u64::from_be_bytes(head[8..16].try_into().expect("8 bytes"));
            header_length = 16;
        } else if size == 0 {
            size = end - position;
        }

        if size < header_length || position + size > end {
            return Err(not_audio());
        }

        out.push((
            [head[4], head[5], head[6], head[7]],
            position + header_length,
            size - header_length,
        ));
        position += size;
    }
    Ok(out)
}

/// The header object's GUID, which an ASF file starts with.
pub(crate) const ASF_HEADER_OBJECT: [u8; 16] = [
    0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
/// The data object's GUID: everything from it on is the sound.
pub(crate) const ASF_DATA_OBJECT: [u8; 16] = [
    0x36, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];

/// ASF (WMA): a header object holding every tag, then the data object, then indexes. Everything
/// from the data object on is the sound.
fn asf(source: &mut dyn Source, hash: &mut Sha256) -> MetadataResult<()> {
    let data = asf_data_offset(source)?;
    let total = stream_len(source)?;
    append(hash, source, data, total - data)
}

/// Where the data object starts, checked to be one.
pub(crate) fn asf_data_offset(source: &mut dyn Source) -> MetadataResult<u64> {
    let mut head = [0u8; 24];
    if read_at(source, 0, &mut head)? != 24 || head[..16] != ASF_HEADER_OBJECT {
        return Err(not_audio());
    }

    let data = u64::from_le_bytes(head[16..24].try_into().expect("8 bytes"));
    if data < 24 || read_at(source, data, &mut head)? != 24 || head[..16] != ASF_DATA_OBJECT {
        return Err(not_audio());
    }
    Ok(data)
}

/// AIFF: WAV's big-endian elder. Two chunks are the sound — its description and it.
fn aiff(source: &mut dyn Source, hash: &mut Sha256) -> MetadataResult<()> {
    let total = stream_len(source)?;
    let mut head = [0u8; 12];
    if read_at(source, 0, &mut head)? != 12
        || &head[..4] != b"FORM"
        || !(&head[8..12] == b"AIFF" || &head[8..12] == b"AIFC")
    {
        return Err(not_audio());
    }

    let mut found = false;
    let mut position: u64 = 12;
    let mut chunk = [0u8; 8];

    while read_at(source, position, &mut chunk)? == 8 {
        let size = u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as u64;
        let is_sound = &chunk[..4] == b"SSND";

        if is_sound || &chunk[..4] == b"COMM" {
            hash.update(&chunk[..4]);
            append(
                hash,
                source,
                position + 8,
                size.min(total.saturating_sub(position + 8)),
            )?;
            found |= is_sound;
        }

        position += 8 + size + (size & 1);
    }

    if !found {
        return Err(not_audio());
    }
    Ok(())
}

/// Ogg (Opus, Vorbis): the stream is cut into pages, and the codec's packets are laid across them.
/// The second packet is the comments — the tags — and everything else is the sound or what is
/// needed to decode it.
///
/// **Packets, not pages.** Growing the comments moves every later page boundary and renumbers
/// every page, so no page survives a tag edit byte for byte and hashing pages would refuse every
/// edit. The packets inside them do survive, so those are what is hashed: every packet but the
/// second, in order, together with each page's position in time — which a writer that re-cut the
/// pages carelessly would get wrong while keeping every packet intact. One logical stream only;
/// anything multiplexed or chained is refused.
fn ogg(source: &mut dyn Source, hash: &mut Sha256) -> MetadataResult<()> {
    let total = stream_len(source)?;
    let mut position = skip_id3v2(source, 0)?;
    let mut head = [0u8; 27];
    let mut table = [0u8; 255];
    let mut packet: Vec<u8> = Vec::new();
    let mut serial: Option<u32> = None;
    let mut index = 0u32;
    let mut pages = 0u32;

    while position < total {
        if read_at(source, position, &mut head)? != 27 || &head[..4] != b"OggS" || head[4] != 0 {
            // A trailing ID3v1 tag, which some taggers append even here, is not the stream.
            if pages > 0 && total - position == 128 {
                break;
            }
            return Err(not_audio());
        }

        let stream = u32::from_le_bytes([head[14], head[15], head[16], head[17]]);
        match serial {
            None => serial = Some(stream),
            Some(s) if s != stream => {
                return Err(MetadataError::format(
                    "This Ogg file holds more than one stream, which cannot be edited safely.",
                ));
            }
            _ => {}
        }
        pages += 1;
        if pages > 10_000_000 {
            return Err(not_audio());
        }

        let segments = head[26] as usize;
        if read_at(source, position + 27, &mut table[..segments])? != segments {
            return Err(not_audio());
        }

        let granule = i64::from_le_bytes(head[6..14].try_into().expect("8 bytes"));
        let mut at = position + 27 + segments as u64;
        let mut completed = false;

        for &length in &table[..segments] {
            let length = length as usize;
            if length > 0 {
                let mut piece = vec![0u8; length];
                if read_at(source, at, &mut piece)? != length {
                    return Err(not_audio());
                }

                // The comment packet is the one thing left out; it can also be megabytes of
                // embedded cover, which there is no reason to hold.
                if index != 1 {
                    packet.extend_from_slice(&piece);
                }
                at += length as u64;
            }

            if length == 255 {
                continue; // the packet runs on into the next segment or page
            }

            if index != 1 {
                hash.update((if index == 0 { 0i32 } else { 2i32 }).to_le_bytes());
                hash.update((packet.len() as i32).to_le_bytes());
                hash.update(&packet);
                if index >= 2 {
                    completed = true;
                }
            }

            packet.clear();
            index += 1;
        }

        // Where in time the page ends, for the pages that end a piece of sound.
        if completed && granule > 0 {
            hash.update(granule.to_le_bytes());
        }

        position = at;
    }

    if index < 3 {
        return Err(not_audio());
    }
    Ok(())
}

fn skip_id3v2(source: &mut dyn Source, mut start: u64) -> MetadataResult<u64> {
    let mut head = [0u8; 10];
    while read_at(source, start, &mut head)? == head.len()
        && head[0] == b'I'
        && head[1] == b'D'
        && head[2] == b'3'
        && (head[6] | head[7] | head[8] | head[9]) < 0x80
    {
        let size = ((head[6] as u64) << 21)
            | ((head[7] as u64) << 14)
            | ((head[8] as u64) << 7)
            | head[9] as u64;
        start += 10 + size + if head[5] & 0x10 != 0 { 10 } else { 0 };
    }
    Ok(start)
}

/// An MP3 is frames with tags bolted on either end: ID3v2 in front, and ID3v1, APE and Lyrics3
/// behind in any combination.
fn mp3(source: &mut dyn Source) -> MetadataResult<(u64, u64)> {
    let length = stream_len(source)?;
    let mut start = skip_id3v2(source, 0)?;

    // Padding a tagger left between the tag and the first frame is not sound, and a rewrite may
    // tidy it away. Zeros cannot be the start of a frame, so they cannot be mistaken.
    start = skip_zeros(source, start, length)?;

    let mut end = length;
    let mut tail = [0u8; 32];
    loop {
        if end >= start + 128
            && read_at(source, end - 128, &mut tail[..3])? == 3
            && &tail[..3] == b"TAG"
        {
            end -= 128;
            continue;
        }

        if end >= start + 32
            && read_at(source, end - 32, &mut tail)? == 32
            && &tail[..8] == b"APETAGEX"
        {
            let size = u32::from_le_bytes([tail[12], tail[13], tail[14], tail[15]]) as u64;
            let has_header =
                u32::from_le_bytes([tail[20], tail[21], tail[22], tail[23]]) & 0x8000_0000 != 0;
            let whole = size + if has_header { 32 } else { 0 };
            if whole < 32 || whole > end - start {
                break;
            }
            end -= whole;
            continue;
        }

        if end >= start + 15
            && read_at(source, end - 15, &mut tail[..15])? == 15
            && &tail[6..15] == b"LYRICS200"
            && let Some(lyrics) = std::str::from_utf8(&tail[..6])
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
            && lyrics + 15 <= end - start
        {
            end -= lyrics + 15;
            continue;
        }

        break;
    }

    if end <= start || !starts_with_frame(source, start, end)? {
        return Err(not_audio());
    }
    Ok((start, end))
}

/// Whether real MPEG audio turns up early in the range: a frame header whose fields are all
/// legal, followed exactly one frame later by another.
///
/// Eleven set bits alone is no test at all — every JPEG marker is `FF` and then something, and a
/// picture renamed `.mp3` is full of them. Two headers the right distance apart is what a decoder
/// itself looks for before it believes it has found the stream.
fn starts_with_frame(source: &mut dyn Source, start: u64, end: u64) -> MetadataResult<bool> {
    let mut window = vec![0u8; (64 * 1024).min(end - start) as usize];
    let read = read_at(source, start, &mut window)?;

    let mut i = 0;
    while i + 4 <= read {
        if let Some(length) = frame_length(&window[i..i + 4]) {
            let next = i + length;
            if start + next as u64 + 4 > end {
                return Ok(true); // the last frame of a very short file
            }
            if next + 4 <= read && frame_length(&window[next..next + 4]).is_some() {
                return Ok(true);
            }
        }
        i += 1;
    }

    Ok(false)
}

/// The length of the Layer III frame this header starts, or None when the four bytes are not a
/// header.
fn frame_length(header: &[u8]) -> Option<usize> {
    if header[0] != 0xFF || header[1] & 0xE0 != 0xE0 {
        return None;
    }

    let version = (header[1] >> 3) & 3; // 3 = MPEG-1, 2 = MPEG-2, 0 = MPEG-2.5, 1 = reserved
    let layer = (header[1] >> 1) & 3; // 1 = Layer III
    let bitrate_index = (header[2] >> 4) as usize;
    let rate_index = ((header[2] >> 2) & 3) as usize;
    let padding = ((header[2] >> 1) & 1) as usize;

    if version == 1 || layer != 1 || bitrate_index == 0 || bitrate_index == 15 || rate_index == 3 {
        return None;
    }

    const FIRST: [usize; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const LATER: [usize; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    const RATES: [usize; 3] = [44100, 48000, 32000];

    let bitrate = if version == 3 { FIRST } else { LATER }[bitrate_index] * 1000;
    let rate = RATES[rate_index]
        >> match version {
            3 => 0,
            2 => 1,
            _ => 2,
        };

    Some((if version == 3 { 144 } else { 72 }) * bitrate / rate + padding)
}

fn skip_zeros(source: &mut dyn Source, mut start: u64, length: u64) -> MetadataResult<u64> {
    let mut buffer = [0u8; 4096];
    while start < length {
        let read = read_at(source, start, &mut buffer)?;
        if read == 0 {
            break;
        }
        if let Some(i) = buffer[..read].iter().position(|&b| b != 0) {
            return Ok(start + i as u64);
        }
        start += read as u64;
    }
    Ok(start)
}

/// Where the file's bytes should be read from next, for a caller that walked it: a convenience
/// so the seek sits in one place.
pub(crate) fn rewind(source: &mut dyn Source) -> MetadataResult<()> {
    source.seek(SeekFrom::Start(0))?;
    Ok(())
}
