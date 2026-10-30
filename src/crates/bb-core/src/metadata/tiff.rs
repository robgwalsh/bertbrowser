//! A TIFF picture — which *is* an EXIF block, from its first byte to its last: the same
//! directories, with the picture's own strips of pixels hanging off the first one.
//! Spec: Core/Services/Metadata/TiffCodec.cs.
//!
//! So it is edited by `ExifBlock` exactly as a JPEG's block is, and for the same reason nothing
//! existing may move: the directory that says where each strip of pixels is holds their positions
//! as offsets from the start of the file. Changed values go over their own old bytes or on the
//! end; the pixels are never read, moved or rewritten.
//!
//! **The digest reads the file its own way**, not through that type: a walk of the directory
//! chain that hashes each picture's shape and every strip or tile its offsets point at. It is the
//! check on the editor, so it must not share the editor's reading.
//!
//! The whole file is held in memory while it is edited, so it is bounded; a larger one is refused
//! by name. BigTIFF and the camera raw formats built on TIFF are not this codec's to touch.

use std::collections::{HashSet, VecDeque};
use std::io::SeekFrom;

use sha2::{Digest, Sha256};

use super::codec::{
    Destination, MetadataCodec, MetadataError, MetadataResult, Source, finish, read_fully,
    stream_len,
};
use super::exif::{ExifBlock, ExifDirectory, TYPE_BYTE, u16_at, u32_at};
use super::exif_fields::{ExifFields, FIELDS};
use super::fields::{MetadataFamily, MetadataField};
use super::models::{MetadataDocument, MetadataEdit};
use super::xmp::XmpPacket;
use crate::cancel::CancelToken;

/// The largest file edited. Past this it is a scan or a stitched panorama, and holding two copies
/// of it in memory to change a title is the wrong trade.
pub const MAX_BYTES: u64 = 256 << 20;

const XMP_TAG: u16 = 700;

/// What goes from the main directory when everything is removed: descriptions, names, software
/// and dates, and the three places other programs keep their own records.
const PERSONAL: [u16; 19] = [
    270, 271, 272, 305, 306, 315, 33432, 0x4746, 0x4749, 0x9C9B, 0x9C9C, 0x9C9D, 0x9C9E, 0x9C9F,
    XMP_TAG, 33723, 34377, 37724, 50341,
];

pub struct TiffCodec;

fn not_tiff() -> MetadataError {
    MetadataError::format("This is not a readable TIFF picture, so it was left as it is.")
}

fn load(source: &mut dyn Source) -> MetadataResult<Vec<u8>> {
    let length = stream_len(source)?;
    if length > MAX_BYTES {
        return Err(MetadataError::format(
            "This TIFF is larger than 256 MB, which is more than can be edited here.",
        ));
    }

    let mut data = vec![0u8; length as usize];
    source.seek(SeekFrom::Start(0))?;
    if read_fully(source, &mut data)? != data.len() || data.len() < 8 {
        return Err(not_tiff());
    }

    let little = data[0] == b'I' && data[1] == b'I';
    if !little && !(data[0] == b'M' && data[1] == b'M') {
        return Err(not_tiff());
    }

    match u16_at(&data, 2, little) {
        42 => Ok(data),
        43 => Err(MetadataError::format(
            "This is a BigTIFF, which cannot be edited here.",
        )),
        _ => Err(not_tiff()),
    }
}

/// A tag's values as numbers, whichever of the three whole-number types holds them.
fn values(data: &[u8], entry: usize, little: bool) -> MetadataResult<Vec<u64>> {
    let kind = u16_at(data, entry + 2, little);
    let count = u32_at(data, entry + 4, little) as u64;
    let size: u64 = match kind {
        1 => 1,
        3 => 2,
        4 => 4,
        _ => 0,
    };
    if size == 0 || count > 16_000_000 {
        return Err(not_tiff());
    }

    let at = if size * count <= 4 {
        entry as u64 + 8
    } else {
        u32_at(data, entry + 8, little) as u64
    };
    if at + size * count > data.len() as u64 {
        return Err(not_tiff());
    }

    let at = at as usize;
    Ok((0..count as usize)
        .map(|i| match size {
            1 => data[at + i] as u64,
            2 => u16_at(data, at + 2 * i, little) as u64,
            _ => u32_at(data, at + 4 * i, little) as u64,
        })
        .collect())
}

impl MetadataCodec for TiffCodec {
    fn id(&self) -> &'static str {
        "tiff"
    }

    fn family(&self) -> MetadataFamily {
        MetadataFamily::Image
    }

    fn fields(&self) -> &[MetadataField] {
        &FIELDS
    }

    fn read(&self, source: &mut dyn Source) -> MetadataResult<MetadataDocument> {
        let block = ExifBlock::parse(&load(source)?)?;
        let located = block.has_location()
            || block
                .get(ExifDirectory::Image, XMP_TAG)
                .is_some_and(|xmp| XmpPacket::has_location(&xmp));

        Ok(
            MetadataDocument::new(MetadataFamily::Image, ExifFields::read(&block))
                .with_location(located),
        )
    }

    fn write(
        &self,
        source: &mut dyn Source,
        destination: &mut dyn Destination,
        edit: &MetadataEdit,
        cancel: &CancelToken,
    ) -> MetadataResult<()> {
        let mut block = ExifBlock::parse(&load(source)?)?;

        if edit.remove_all {
            block.remove_photo_directory();
            block.remove_location()?;
            for tag in PERSONAL {
                block.remove(ExifDirectory::Image, tag);
            }
        } else {
            if let Some(packet) = block.get(ExifDirectory::Image, XMP_TAG) {
                let mut synced = packet.clone();
                let mut changed = false;
                if edit.remove_location
                    && let Some(cleaned) = XmpPacket::without_location(&synced)?
                {
                    synced = cleaned;
                    changed = true;
                }
                if let Some(with) = XmpPacket::with_changes(&synced, &edit.changes) {
                    synced = with;
                    changed = true;
                }
                if changed {
                    block.set(ExifDirectory::Image, XMP_TAG, TYPE_BYTE, synced);
                }
            }

            if edit.remove_location {
                block.remove_location()?;
            }
        }

        ExifFields::apply(&mut block, edit)?;

        cancel.check()?;
        destination.write_all(&block.to_bytes(usize::MAX)?)?;
        Ok(())
    }

    fn payload_digest(&self, source: &mut dyn Source) -> MetadataResult<Vec<u8>> {
        let data = load(source)?;
        let little = data[0] == b'I';
        let mut hash = Sha256::new();

        let mut seen: HashSet<u64> = HashSet::new();
        let mut pending: VecDeque<u64> = VecDeque::new();
        pending.push_back(u32_at(&data, 4, little) as u64);

        while let Some(offset) = pending.pop_front() {
            if offset == 0 {
                continue;
            }
            if !seen.insert(offset) || seen.len() > 10_000 || offset + 2 > data.len() as u64 {
                return Err(not_tiff());
            }

            let offset = offset as usize;
            let count = u16_at(&data, offset, little) as usize;
            let end = offset + 2 + 12 * count;
            if end + 4 > data.len() {
                return Err(not_tiff());
            }

            let mut offsets: Option<Vec<u64>> = None;
            let mut lengths: Option<Vec<u64>> = None;
            hash.update(b"IFD");

            for i in 0..count {
                let at = offset + 2 + 12 * i;
                let tag = u16_at(&data, at, little);

                match tag {
                    // Where the pixels are, and how much of them: strips, or tiles.
                    273 | 324 => offsets = Some(values(&data, at, little)?),
                    279 | 325 => lengths = Some(values(&data, at, little)?),

                    // More pictures hanging off this one — reduced copies, pages of a raw file.
                    330 => {
                        for child in values(&data, at, little)? {
                            pending.push_back(child);
                        }
                    }

                    // What shape the pixels are and how to read them: size, depth, compression,
                    // colour model, channel count, layout, predictor, palette, tile size.
                    256 | 257 | 258 | 259 | 262 | 277 | 278 | 284 | 317 | 320 | 322 | 323 | 338
                    | 339 => {
                        hash.update(tag.to_le_bytes());
                        for value in values(&data, at, little)? {
                            hash.update((value as i64).to_le_bytes());
                        }
                    }
                    _ => {}
                }
            }

            if offsets.is_some() || lengths.is_some() {
                let (Some(offsets), Some(lengths)) = (offsets, lengths) else {
                    return Err(not_tiff());
                };
                if offsets.len() != lengths.len() {
                    return Err(not_tiff());
                }

                for (at, length) in offsets.iter().zip(&lengths) {
                    if at + length > data.len() as u64 {
                        return Err(not_tiff());
                    }
                    hash.update((*length as i64).to_le_bytes());
                    hash.update(&data[*at as usize..(*at + *length) as usize]);
                }
            }

            pending.push_back(u32_at(&data, end, little) as u64);
        }

        Ok(finish(hash))
    }
}
