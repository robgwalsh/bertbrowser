//! EXIF in a WebP: a RIFF file whose `EXIF` chunk holds the block a JPEG carries.
//! Spec: Core/Services/Metadata/WebPCodec.cs.
//!
//! **A WebP with metadata has to be the "extended" kind.** The simple form is one image chunk and
//! nothing else; anything more needs a `VP8X` header in front that states the canvas size and has
//! a flag bit for each optional chunk. So giving a simple file its first EXIF means writing that
//! header too, with the size read out of the image chunk's own first bytes — and keeping the
//! flags true on every later edit, since a decoder believes them over what it finds.
//!
//! The image chunks are copied through from the source, never held or decoded.

use std::io::{SeekFrom, Write};

use sha2::{Digest, Sha256};

use super::codec::{
    Destination, MetadataCodec, MetadataError, MetadataResult, Source, finish, read_fully,
    stream_len,
};
use super::exif::ExifBlock;
use super::exif_fields::{ExifFields, FIELDS};
use super::fields::{MetadataFamily, MetadataField};
use super::models::{MetadataDocument, MetadataEdit};
use super::xmp::XmpPacket;
use crate::cancel::CancelToken;

const EXIF_FLAG: u8 = 0x08;
const XMP_FLAG: u8 = 0x04;
const ALPHA_FLAG: u8 = 0x10;

/// The flags that mean a decoder draws the file differently: a colour profile, animation.
const DRAWING_FLAGS: u8 = 0x20 | 0x02;

const MAX_METADATA_CHUNK: u64 = 16 << 20;

pub struct WebPCodec;

/// `offset` is where the chunk's data starts.
struct Chunk {
    kind: [u8; 4],
    offset: u64,
    length: u64,
}

impl Chunk {
    fn is(&self, kind: &[u8; 4]) -> bool {
        &self.kind == kind
    }

    fn is_image(&self) -> bool {
        self.is(b"VP8 ") || self.is(b"VP8L")
    }
}

fn not_webp() -> MetadataError {
    MetadataError::format("This is not a readable WebP picture, so it was left as it is.")
}

fn walk(source: &mut dyn Source) -> MetadataResult<Vec<Chunk>> {
    let total = stream_len(source)?;
    let mut head = [0u8; 12];
    source.seek(SeekFrom::Start(0))?;
    if read_fully(source, &mut head)? != 12 || &head[..4] != b"RIFF" || &head[8..12] != b"WEBP" {
        return Err(not_webp());
    }

    let end = total.min(8 + u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as u64);
    let mut chunks = Vec::new();
    let mut position: u64 = 12;

    while position + 8 <= end {
        source.seek(SeekFrom::Start(position))?;
        if read_fully(source, &mut head[..8])? != 8 {
            return Err(not_webp());
        }

        let length = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as u64;
        if position + 8 + length > total || chunks.len() > 1_000_000 {
            return Err(not_webp());
        }

        chunks.push(Chunk {
            kind: [head[0], head[1], head[2], head[3]],
            offset: position + 8,
            length,
        });
        position += 8 + length + (length & 1);
    }

    if !chunks.iter().any(|c| c.is_image() || c.is(b"ANMF")) {
        return Err(not_webp());
    }
    Ok(chunks)
}

fn load(source: &mut dyn Source, chunk: &Chunk) -> MetadataResult<Vec<u8>> {
    if chunk.length > MAX_METADATA_CHUNK {
        return Err(not_webp());
    }
    let mut data = vec![0u8; chunk.length as usize];
    source.seek(SeekFrom::Start(chunk.offset))?;
    if read_fully(source, &mut data)? != data.len() {
        return Err(not_webp());
    }
    Ok(data)
}

/// Some writers put a JPEG's `Exif\0\0` label in front of the block; the block itself starts at
/// its byte-order mark either way.
fn exif_of(data: &[u8]) -> &[u8] {
    data.strip_prefix(b"Exif\0\0".as_slice()).unwrap_or(data)
}

/// The extended header for a file that had none: no flags yet, and the canvas size taken from the
/// one image chunk a simple file consists of.
fn new_header(source: &mut dyn Source, chunks: &[Chunk]) -> MetadataResult<Vec<u8>> {
    let image = chunks.iter().find(|c| c.is_image()).ok_or_else(not_webp)?;

    let mut head = [0u8; 10];
    source.seek(SeekFrom::Start(image.offset))?;
    if read_fully(source, &mut head)? != head.len() {
        return Err(not_webp());
    }

    let (width, height, alpha);
    if image.is(b"VP8L") {
        if head[0] != 0x2F {
            return Err(not_webp());
        }
        let bits = u32::from_le_bytes([head[1], head[2], head[3], head[4]]);
        width = (bits & 0x3FFF) as i64 + 1;
        height = ((bits >> 14) & 0x3FFF) as i64 + 1;
        alpha = (bits >> 28) & 1 != 0;
    } else {
        // A key frame: three bytes of frame tag, a start code, then both sizes in 14 bits each.
        if head[3] != 0x9D || head[4] != 0x01 || head[5] != 0x2A {
            return Err(not_webp());
        }
        width = (u16::from_le_bytes([head[6], head[7]]) & 0x3FFF) as i64;
        height = (u16::from_le_bytes([head[8], head[9]]) & 0x3FFF) as i64;
        alpha = false;
    }

    if width < 1 || height < 1 {
        return Err(not_webp());
    }

    let mut header = vec![0u8; 10];
    header[0] = if alpha { ALPHA_FLAG } else { 0 };
    header[4] = (width - 1) as u8;
    header[5] = ((width - 1) >> 8) as u8;
    header[6] = ((width - 1) >> 16) as u8;
    header[7] = (height - 1) as u8;
    header[8] = ((height - 1) >> 8) as u8;
    header[9] = ((height - 1) >> 16) as u8;
    Ok(header)
}

fn write_chunk(destination: &mut dyn Write, kind: &[u8; 4], data: &[u8]) -> MetadataResult<()> {
    destination.write_all(kind)?;
    destination.write_all(&(data.len() as u32).to_le_bytes())?;
    destination.write_all(data)?;
    if data.len() & 1 != 0 {
        destination.write_all(&[0])?;
    }
    Ok(())
}

fn copy(
    source: &mut dyn Source,
    destination: &mut dyn Destination,
    offset: u64,
    length: u64,
    cancel: &CancelToken,
) -> MetadataResult<()> {
    let mut buffer = vec![0u8; super::codec::COPY_BUFFER];
    let total = stream_len(source)?;
    source.seek(SeekFrom::Start(offset))?;
    let mut length = length.min(total.saturating_sub(offset));
    while length > 0 {
        cancel.check()?;
        let want = (buffer.len() as u64).min(length) as usize;
        let read = source.read(&mut buffer[..want])?;
        if read == 0 {
            return Err(not_webp());
        }
        destination.write_all(&buffer[..read])?;
        length -= read as u64;
    }

    // A last chunk of odd length whose padding byte the file left off still needs one here.
    if destination.stream_position()? & 1 != 0 {
        destination.write_all(&[0])?;
    }
    Ok(())
}

impl MetadataCodec for WebPCodec {
    fn id(&self) -> &'static str {
        "webp"
    }

    fn family(&self) -> MetadataFamily {
        MetadataFamily::Image
    }

    fn fields(&self) -> &[MetadataField] {
        &FIELDS
    }

    fn read(&self, source: &mut dyn Source) -> MetadataResult<MetadataDocument> {
        let chunks = walk(source)?;
        let block = match chunks.iter().find(|c| c.is(b"EXIF")) {
            Some(exif) => Some(ExifBlock::parse(exif_of(&load(source, exif)?))?),
            None => None,
        };

        let mut located = block.as_ref().is_some_and(ExifBlock::has_location);
        if !located {
            for chunk in chunks.iter().filter(|c| c.is(b"XMP ")) {
                if XmpPacket::has_location(&load(source, chunk)?) {
                    located = true;
                    break;
                }
            }
        }

        let values = block.as_ref().map(ExifFields::read).unwrap_or_default();
        Ok(MetadataDocument::new(MetadataFamily::Image, values).with_location(located))
    }

    fn write(
        &self,
        source: &mut dyn Source,
        destination: &mut dyn Destination,
        edit: &MetadataEdit,
        cancel: &CancelToken,
    ) -> MetadataResult<()> {
        let chunks = walk(source)?;
        let existing = chunks.iter().find(|c| c.is(b"EXIF"));

        let mut block;
        let wanted;

        if edit.remove_all {
            let orientation = match existing {
                Some(chunk) => load(source, chunk)
                    .and_then(|d| ExifBlock::parse(exif_of(&d)))
                    .ok()
                    .and_then(|b| ExifFields::orientation(&b)),
                None => None,
            };

            block = ExifBlock::empty();
            let drawn = matches!(orientation, Some(o) if o > 1 && o <= 8);
            if let Some(o) = orientation.filter(|_| drawn) {
                ExifFields::set_orientation(&mut block, o);
            }
            wanted = drawn || !edit.changes.is_empty();
        } else {
            block = match existing {
                Some(chunk) => ExifBlock::parse(exif_of(&load(source, chunk)?))?,
                None => ExifBlock::empty(),
            };
            wanted = existing.is_some() || !edit.changes.is_empty();
            if edit.remove_location {
                block.remove_location()?;
            }
        }

        ExifFields::apply(&mut block, edit)?;
        let exif = if wanted {
            Some(block.to_bytes(usize::MAX)?)
        } else {
            None
        };

        let mut packets: Vec<Vec<u8>> = Vec::new();
        if !edit.remove_all {
            for chunk in chunks.iter().filter(|c| c.is(b"XMP ")) {
                let mut packet = load(source, chunk)?;
                if edit.remove_location
                    && let Some(cleaned) = XmpPacket::without_location(&packet)?
                {
                    packet = cleaned;
                }
                if let Some(synced) = XmpPacket::with_changes(&packet, &edit.changes) {
                    packet = synced;
                }
                packets.push(packet);
            }
        }

        // Nothing optional left and none there before: the file stays the simple kind it was.
        let header = chunks.iter().find(|c| c.is(b"VP8X"));
        let mut extended: Option<Vec<u8>> = None;
        if header.is_some() || exif.is_some() || !packets.is_empty() {
            let mut data = match header {
                Some(chunk) => load(source, chunk)?,
                None => new_header(source, &chunks)?,
            };
            if data.len() < 10 {
                return Err(not_webp());
            }

            data[0] = (data[0] & !(EXIF_FLAG | XMP_FLAG))
                | if exif.is_some() { EXIF_FLAG } else { 0 }
                | if packets.is_empty() { 0 } else { XMP_FLAG };

            // And back again: a header left describing one plain image and nothing else is one
            // a simple file never had, so a picture stripped of everything is the file it
            // started as.
            let rest: Vec<&Chunk> = chunks
                .iter()
                .filter(|c| !(c.is(b"VP8X") || c.is(b"EXIF") || c.is(b"XMP ")))
                .collect();
            let plain = rest.len() == 1 && rest[0].is_image();
            if !(exif.is_none() && packets.is_empty() && data[0] & DRAWING_FLAGS == 0 && plain) {
                extended = Some(data);
            }
        }

        destination.write_all(b"RIFF\0\0\0\0WEBP")?;

        if let Some(extended) = &extended {
            write_chunk(destination, b"VP8X", extended)?;
        }
        for chunk in &chunks {
            cancel.check()?;
            if chunk.is(b"VP8X") || chunk.is(b"EXIF") || chunk.is(b"XMP ") {
                continue;
            }
            copy(
                source,
                destination,
                chunk.offset - 8,
                8 + chunk.length + (chunk.length & 1),
                cancel,
            )?;
        }

        // After the image, in the order the container asks for: EXIF, then XMP.
        if let Some(exif) = &exif {
            write_chunk(destination, b"EXIF", exif)?;
        }
        for packet in &packets {
            write_chunk(destination, b"XMP ", packet)?;
        }

        let size = destination.stream_position()? - 8;
        if size > u32::MAX as u64 {
            return Err(not_webp());
        }
        destination.seek(SeekFrom::Start(4))?;
        destination.write_all(&(size as u32).to_le_bytes())?;
        destination.seek(SeekFrom::End(0))?;
        Ok(())
    }

    fn payload_digest(&self, source: &mut dyn Source) -> MetadataResult<Vec<u8>> {
        let mut hash = Sha256::new();
        let mut buffer = vec![0u8; super::codec::COPY_BUFFER];

        for chunk in walk(source)? {
            if chunk.is(b"EXIF") || chunk.is(b"XMP ") {
                continue;
            }

            if chunk.is(b"VP8X") {
                // The header counts only when it says something about drawing. Its metadata
                // flags are ours to change, and a file given its first EXIF gains a header it
                // never had — one that says nothing the image chunk did not already.
                let mut data = load(source, &chunk)?;
                if data.len() < 10 {
                    return Err(not_webp());
                }
                if data[0] & DRAWING_FLAGS == 0 {
                    continue;
                }
                data[0] &= !(EXIF_FLAG | XMP_FLAG);
                hash.update(b"VP8X");
                hash.update(&data);
                continue;
            }

            hash.update(chunk.kind);
            source.seek(SeekFrom::Start(chunk.offset))?;
            let mut left = chunk.length;
            while left > 0 {
                let want = (buffer.len() as u64).min(left) as usize;
                let read = source.read(&mut buffer[..want])?;
                if read == 0 {
                    return Err(not_webp());
                }
                hash.update(&buffer[..read]);
                left -= read as u64;
            }
        }

        Ok(finish(hash))
    }
}
