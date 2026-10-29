//! EXIF in a PNG: the `eXIf` chunk, which holds the same block a JPEG carries.
//! Spec: Core/Services/Metadata/PngCodec.cs.
//!
//! A PNG is a signature and then chunks, each with a length, a four-letter type and a checksum.
//! The picture is the `IDAT` chunks, which can be most of a large file — so chunks are found by
//! walking their headers and copied through from the source, never held in memory.
//!
//! The older text chunks (`tEXt`, `iTXt`) are not read as fields: they are free-form keyword/value
//! pairs with no agreed meaning beyond a handful, and writing a title to one of them as well as to
//! EXIF would be inventing a convention. An XMP packet in an `iTXt` is kept in step, exactly as in
//! a JPEG. All of them go when everything is removed.

use std::io::{SeekFrom, Write};

use sha2::{Digest, Sha256};

use super::codec::{
    Destination, MetadataCodec, MetadataError, MetadataResult, Source, copy_range, finish,
    read_fully, stream_len,
};
use super::exif::ExifBlock;
use super::exif_fields::{ExifFields, FIELDS};
use super::fields::{MetadataFamily, MetadataField};
use super::models::{MetadataDocument, MetadataEdit};
use super::xmp::XmpPacket;
use crate::cancel::CancelToken;

const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
const XMP_KEYWORD: &[u8] = b"XML:com.adobe.xmp\0";

/// More than any real block or packet; a chunk claiming more is not read into memory.
const MAX_METADATA_CHUNK: u64 = 16 << 20;

pub struct PngCodec;

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
}

/// The XMP packet inside an international-text chunk, with the bytes before it.
struct Xmp {
    prefix: Vec<u8>,
    packet: Vec<u8>,
    compressed: bool,
}

fn not_png() -> MetadataError {
    MetadataError::format("This is not a readable PNG picture, so it was left as it is.")
}

/// The chunks that describe the picture rather than being it. Everything that changes how it is
/// drawn — palette, gamma, colour profile, transparency — is not among them.
fn is_metadata(kind: &[u8; 4]) -> bool {
    matches!(kind, b"eXIf" | b"tEXt" | b"zTXt" | b"iTXt" | b"tIME")
}

fn walk(source: &mut dyn Source) -> MetadataResult<Vec<Chunk>> {
    let total = stream_len(source)?;
    let mut head = [0u8; 8];
    source.seek(SeekFrom::Start(0))?;
    if read_fully(source, &mut head)? != 8 || head != SIGNATURE {
        return Err(not_png());
    }

    let mut chunks = Vec::new();
    let mut position: u64 = 8;

    loop {
        source.seek(SeekFrom::Start(position))?;
        if read_fully(source, &mut head)? != 8 {
            return Err(not_png());
        }

        let length = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as u64;
        let kind = [head[4], head[5], head[6], head[7]];
        if position + 12 + length > total || chunks.len() > 1_000_000 {
            return Err(not_png());
        }

        chunks.push(Chunk {
            kind,
            offset: position + 8,
            length,
        });
        position += 12 + length;

        if &kind == b"IEND" {
            return Ok(chunks);
        }
    }
}

fn load(source: &mut dyn Source, chunk: &Chunk) -> MetadataResult<Vec<u8>> {
    if chunk.length > MAX_METADATA_CHUNK {
        return Err(not_png());
    }
    let mut data = vec![0u8; chunk.length as usize];
    source.seek(SeekFrom::Start(chunk.offset))?;
    if read_fully(source, &mut data)? != data.len() {
        return Err(not_png());
    }
    Ok(data)
}

/// The XMP packet inside an international-text chunk, or None when the chunk is some other text.
fn xmp_of(data: &[u8]) -> Option<Xmp> {
    if !data.starts_with(XMP_KEYWORD) {
        return None;
    }

    // keyword\0, compression flag, compression method, language\0, translated keyword\0, text
    let mut at = XMP_KEYWORD.len();
    if data.len() < at + 2 {
        return None;
    }
    let compressed = data[at] != 0;
    at += 2;

    let mut terminators = 0;
    while terminators < 2 {
        if at >= data.len() {
            return None;
        }
        if data[at] == 0 {
            terminators += 1;
        }
        at += 1;
    }

    Some(Xmp {
        prefix: data[..at].to_vec(),
        packet: data[at..].to_vec(),
        compressed,
    })
}

fn write_chunk(destination: &mut dyn Write, kind: &[u8; 4], data: &[u8]) -> MetadataResult<()> {
    destination.write_all(&(data.len() as u32).to_be_bytes())?;
    destination.write_all(kind)?;
    destination.write_all(data)?;
    let crc = crc(crc(0xFFFF_FFFF, kind), data) ^ 0xFFFF_FFFF;
    destination.write_all(&crc.to_be_bytes())?;
    Ok(())
}

/// The checksum every chunk ends with. Twenty lines here rather than a second caller of the
/// checksum crate, which the checksum tool keeps to one class on purpose.
fn crc(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        let mut c = (crc ^ b as u32) & 0xFF;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        crc = c ^ (crc >> 8);
    }
    crc
}

fn exif_of(source: &mut dyn Source, chunks: &[Chunk]) -> MetadataResult<Option<ExifBlock>> {
    match chunks.iter().find(|c| c.is(b"eXIf")) {
        Some(exif) => Ok(Some(ExifBlock::parse(&load(source, exif)?)?)),
        None => Ok(None),
    }
}

impl MetadataCodec for PngCodec {
    fn id(&self) -> &'static str {
        "png"
    }

    fn family(&self) -> MetadataFamily {
        MetadataFamily::Image
    }

    fn fields(&self) -> &[MetadataField] {
        &FIELDS
    }

    fn read(&self, source: &mut dyn Source) -> MetadataResult<MetadataDocument> {
        let chunks = walk(source)?;
        let block = exif_of(source, &chunks)?;

        let mut located = block.as_ref().is_some_and(ExifBlock::has_location);
        if !located {
            for chunk in chunks.iter().filter(|c| c.is(b"iTXt")) {
                located = match xmp_of(&load(source, chunk)?) {
                    Some(Xmp {
                        compressed: true, ..
                    }) => true,
                    Some(xmp) => XmpPacket::has_location(&xmp.packet),
                    None => false,
                };
                if located {
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
        let existing = chunks.iter().find(|c| c.is(b"eXIf"));

        let mut block;
        let wanted;

        if edit.remove_all {
            let orientation = match existing {
                Some(chunk) => load(source, chunk)
                    .and_then(|d| ExifBlock::parse(&d))
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
                Some(chunk) => ExifBlock::parse(&load(source, chunk)?)?,
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

        destination.write_all(&SIGNATURE)?;
        let mut placed = false;

        for chunk in &chunks {
            cancel.check()?;

            if chunk.is(b"eXIf") {
                continue;
            }
            if edit.remove_all && is_metadata(&chunk.kind) {
                continue;
            }

            // Before the picture data, where the specification asks for it; before the end
            // marker at the latest, for a file that somehow has no picture data at all.
            if !placed
                && let Some(exif) = &exif
                && (chunk.is(b"IDAT") || chunk.is(b"IEND"))
            {
                write_chunk(destination, b"eXIf", exif)?;
                placed = true;
            }

            if chunk.is(b"iTXt")
                && let Some(xmp) = xmp_of(&load(source, chunk)?)
            {
                if xmp.compressed {
                    if edit.remove_location {
                        return Err(MetadataError::format(
                            "This picture's location is stored in a form that cannot be safely removed.",
                        ));
                    }
                } else {
                    let mut packet = xmp.packet.clone();
                    let mut changed = false;
                    if edit.remove_location
                        && let Some(cleaned) = XmpPacket::without_location(&packet)?
                    {
                        packet = cleaned;
                        changed = true;
                    }
                    if let Some(synced) = XmpPacket::with_changes(&packet, &edit.changes) {
                        packet = synced;
                        changed = true;
                    }

                    if changed {
                        let mut data = xmp.prefix.clone();
                        data.extend_from_slice(&packet);
                        write_chunk(destination, b"iTXt", &data)?;
                        continue;
                    }
                }
            }

            copy_range(
                source,
                destination,
                chunk.offset - 8,
                chunk.length + 12,
                cancel,
                not_png,
            )?;
        }
        Ok(())
    }

    fn payload_digest(&self, source: &mut dyn Source) -> MetadataResult<Vec<u8>> {
        let mut hash = Sha256::new();
        let mut buffer = vec![0u8; super::codec::COPY_BUFFER];

        for chunk in walk(source)? {
            if is_metadata(&chunk.kind) {
                continue;
            }

            hash.update(chunk.kind);
            source.seek(SeekFrom::Start(chunk.offset))?;
            let mut left = chunk.length;
            while left > 0 {
                let want = (buffer.len() as u64).min(left) as usize;
                let read = source.read(&mut buffer[..want])?;
                if read == 0 {
                    return Err(not_png());
                }
                hash.update(&buffer[..read]);
                left -= read as u64;
            }
        }

        Ok(finish(hash))
    }
}
