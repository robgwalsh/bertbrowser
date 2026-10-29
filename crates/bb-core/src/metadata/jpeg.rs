//! EXIF in a JPEG, edited by splicing one header segment. The picture is never decoded.
//! Spec: Core/Services/Metadata/JpegCodec.cs.
//!
//! A JPEG is a run of labelled segments, then the compressed scan, which runs to the end of the
//! file. Only the segments are parsed; everything from the first scan marker on is copied through
//! as it is, which is also why a phone's second image, depth map or motion clip appended after the
//! picture survives an edit untouched.

use std::io::{SeekFrom, Write};

use sha2::{Digest, Sha256};

use super::codec::{
    Destination, MetadataCodec, MetadataError, MetadataResult, Source, copy_to_end, finish,
    index_of, read_fully,
};
use super::exif::{ExifBlock, MAX_LENGTH};
use super::exif_fields::{ExifFields, FIELDS};
use super::fields::{MetadataFamily, MetadataField};
use super::iptc::IptcBlock;
use super::models::{MetadataDocument, MetadataEdit};
use super::xmp::XmpPacket;
use crate::cancel::CancelToken;

const APP0: u8 = 0xE0;
const APP1: u8 = 0xE1;
const APP13: u8 = 0xED;
const COMMENT: u8 = 0xFE;
const START_OF_SCAN: u8 = 0xDA;
const END_OF_IMAGE: u8 = 0xD9;

/// More header than any real picture carries. A file that is all header and no scan is refused
/// rather than read into memory whole.
const MAX_HEADER_BYTES: u64 = 32 << 20;

const EXIF_LABEL: &[u8] = b"Exif\0\0";
const XMP_LABEL: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const EXTENDED_XMP_LABEL: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
const PHOTOSHOP_LABEL: &[u8] = b"Photoshop 3.0\0";

pub struct JpegCodec;

/// One header segment. `fill` is the padding `FF` bytes before the marker, which the format
/// allows and which are written back so nothing but the metadata differs; `data` is the content,
/// or None for a marker that carries none.
#[derive(Clone)]
struct Segment {
    marker: u8,
    fill: usize,
    data: Option<Vec<u8>>,
}

impl Segment {
    fn write_to(&self, destination: &mut dyn Write) -> MetadataResult<()> {
        for _ in 0..self.fill {
            destination.write_all(&[0xFF])?;
        }
        destination.write_all(&[0xFF, self.marker])?;
        let Some(data) = &self.data else {
            return Ok(());
        };
        let length = data.len() + 2;
        if length > u16::MAX as usize {
            return Err(MetadataError::format(
                "There is no room left in this picture's EXIF block.",
            ));
        }
        destination.write_all(&[(length >> 8) as u8, length as u8])?;
        destination.write_all(data)?;
        Ok(())
    }

    fn starts_with(&self, label: &[u8]) -> bool {
        self.data.as_ref().is_some_and(|d| d.starts_with(label))
    }
}

struct Header {
    segments: Vec<Segment>,
    tail_offset: u64,
}

impl Header {
    /// The first EXIF segment. A second one is carried through untouched: readers take the
    /// first, so that is the one worth editing.
    fn exif(&self) -> Option<usize> {
        self.segments.iter().position(is_exif)
    }

    fn parse(source: &mut dyn Source) -> MetadataResult<Header> {
        source.seek(SeekFrom::Start(0))?;
        let mut byte = [0u8; 1];
        let mut next = |source: &mut dyn Source| -> MetadataResult<Option<u8>> {
            Ok((source.read(&mut byte)? == 1).then_some(byte[0]))
        };

        if next(source)? != Some(0xFF) || next(source)? != Some(0xD8) {
            return Err(not_jpeg());
        }

        let mut segments = Vec::new();
        let mut held: u64 = 0;

        loop {
            let start = source.stream_position()?;
            if next(source)? != Some(0xFF) {
                return Err(not_jpeg());
            }

            let mut fill = 0;
            let marker = loop {
                match next(source)? {
                    Some(0xFF) => fill += 1,
                    Some(m) => break m,
                    None => return Err(not_jpeg()),
                }
            };

            if marker == START_OF_SCAN || marker == END_OF_IMAGE {
                return Ok(Header {
                    segments,
                    tail_offset: start,
                });
            }

            // Restart markers and TEM stand alone; nothing else before the scan does.
            if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
                segments.push(Segment {
                    marker,
                    fill,
                    data: None,
                });
                continue;
            }

            let (Some(high), Some(low)) = (next(source)?, next(source)?) else {
                return Err(not_jpeg());
            };
            let length = ((high as usize) << 8) | low as usize;
            if length < 2 {
                return Err(not_jpeg());
            }

            held += length as u64;
            if held > MAX_HEADER_BYTES {
                return Err(not_jpeg());
            }

            let mut data = vec![0u8; length - 2];
            if read_fully(source, &mut data)? != data.len() {
                return Err(not_jpeg());
            }
            segments.push(Segment {
                marker,
                fill,
                data: Some(data),
            });
        }
    }
}

fn not_jpeg() -> MetadataError {
    MetadataError::format("This is not a readable JPEG picture, so it was left as it is.")
}

fn is_exif(segment: &Segment) -> bool {
    segment.marker == APP1 && segment.starts_with(EXIF_LABEL)
}

fn is_xmp(segment: &Segment) -> bool {
    segment.marker == APP1 && segment.starts_with(XMP_LABEL)
}

fn is_extended_xmp(segment: &Segment) -> bool {
    segment.marker == APP1 && segment.starts_with(EXTENDED_XMP_LABEL)
}

/// The segments that describe the picture rather than being it. A colour profile is not among
/// them: change it and the same pixels are a different colour.
fn is_metadata(segment: &Segment) -> bool {
    let Some(data) = &segment.data else {
        return false;
    };
    match segment.marker {
        APP1 => {
            data.starts_with(EXIF_LABEL)
                || data.starts_with(XMP_LABEL)
                || data.starts_with(EXTENDED_XMP_LABEL)
        }
        APP13 => data.starts_with(PHOTOSHOP_LABEL),
        COMMENT => true,
        _ => false,
    }
}

fn exif_block(segment: &Segment) -> MetadataResult<ExifBlock> {
    ExifBlock::parse(&segment.data.as_ref().expect("an EXIF segment has data")[EXIF_LABEL.len()..])
}

fn labelled(label: &[u8], body: &[u8]) -> Vec<u8> {
    let mut data = label.to_vec();
    data.extend_from_slice(body);
    data
}

/// A location is written in up to two places, and removing it from one is not removing it.
fn remove_location(block: &mut ExifBlock, segments: &mut [Segment]) -> MetadataResult<()> {
    block.remove_location()?;

    for segment in segments.iter_mut() {
        // The overflow of a packet too large for one segment arrives in pieces that are not XML
        // on their own. Nothing writes coordinates there in practice; if something has, it cannot
        // be edited safely and saying so beats claiming it is gone.
        if is_extended_xmp(segment)
            && index_of(segment.data.as_deref().unwrap_or(&[]), b"GPS").is_some()
        {
            return Err(MetadataError::format(
                "This picture's location is stored in a form that cannot be safely removed.",
            ));
        }

        if !is_xmp(segment) {
            continue;
        }
        let packet = &segment.data.as_ref().expect("an XMP segment has data")[XMP_LABEL.len()..];
        if let Some(cleaned) = XmpPacket::without_location(packet)? {
            segment.data = Some(labelled(XMP_LABEL, &cleaned));
        }
    }
    Ok(())
}

impl MetadataCodec for JpegCodec {
    fn id(&self) -> &'static str {
        "jpeg"
    }

    fn family(&self) -> MetadataFamily {
        MetadataFamily::Image
    }

    fn fields(&self) -> &[MetadataField] {
        &FIELDS
    }

    fn read(&self, source: &mut dyn Source) -> MetadataResult<MetadataDocument> {
        let header = Header::parse(source)?;
        let block = match header.exif() {
            Some(at) => Some(exif_block(&header.segments[at])?),
            None => None,
        };

        let located = block.as_ref().is_some_and(ExifBlock::has_location)
            || header.segments.iter().any(|s| {
                let data = s.data.as_deref().unwrap_or(&[]);
                (is_xmp(s) && XmpPacket::has_location(&data[XMP_LABEL.len()..]))
                    || (is_extended_xmp(s) && index_of(data, b"GPS").is_some())
            });

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
        let header = Header::parse(source)?;
        let mut segments = header.segments.clone();
        let mut existing = header.exif();

        let mut block;
        let wanted;

        if edit.remove_all {
            // Read before everything goes, and from a block that may be too damaged to read: a
            // picture nobody can parse the EXIF of must still be strippable.
            let orientation = existing
                .and_then(|at| exif_block(&segments[at]).ok())
                .and_then(|b| ExifFields::orientation(&b));

            segments.retain(|s| !is_metadata(s));
            existing = None;

            block = ExifBlock::empty();
            let drawn = matches!(orientation, Some(o) if o > 1 && o <= 8);
            if let Some(o) = orientation.filter(|_| drawn) {
                ExifFields::set_orientation(&mut block, o);
            }
            wanted = drawn || !edit.changes.is_empty();
        } else {
            block = match existing {
                Some(at) => exif_block(&segments[at])?,
                None => ExifBlock::empty(),
            };
            wanted = existing.is_some() || !edit.changes.is_empty();

            if edit.remove_location {
                remove_location(&mut block, &mut segments)?;
            }
        }

        ExifFields::apply(&mut block, edit)?;

        // A packet that already states a field has to change with it; see XmpPacket::with_changes.
        for segment in segments.iter_mut() {
            if is_xmp(segment)
                && let Some(synced) = XmpPacket::with_changes(
                    &segment.data.as_ref().expect("an XMP segment has data")[XMP_LABEL.len()..],
                    &edit.changes,
                )
            {
                segment.data = Some(labelled(XMP_LABEL, &synced));
            }

            // And the older IPTC record, on the same terms.
            if segment.marker == APP13
                && let Some(resources) = &segment.data
                && let Some(record) = IptcBlock::with_changes(resources, &edit.changes)
            {
                segment.data = Some(record);
            }
        }

        if wanted {
            let replacement = Segment {
                marker: APP1,
                fill: 0,
                data: Some(labelled(EXIF_LABEL, &block.to_bytes(MAX_LENGTH)?)),
            };
            match existing {
                Some(at) => {
                    let fill = segments[at].fill;
                    segments[at] = Segment {
                        fill,
                        ..replacement
                    };
                }
                None => {
                    // After the JFIF header when there is one — readers expect it first — else
                    // at the top.
                    let mut at = 0;
                    while at < segments.len() && segments[at].marker == APP0 {
                        at += 1;
                    }
                    segments.insert(at, replacement);
                }
            }
        }

        destination.write_all(&[0xFF, 0xD8])?;
        for segment in &segments {
            cancel.check()?;
            segment.write_to(destination)?;
        }

        copy_to_end(source, destination, header.tail_offset, cancel)
    }

    fn payload_digest(&self, source: &mut dyn Source) -> MetadataResult<Vec<u8>> {
        let header = Header::parse(source)?;
        let mut hash = Sha256::new();

        for segment in &header.segments {
            if is_metadata(segment) {
                continue;
            }
            hash.update([0xFF, segment.marker]);
            if let Some(data) = &segment.data {
                hash.update(data);
            }
        }

        source.seek(SeekFrom::Start(header.tail_offset))?;
        let mut buffer = vec![0u8; super::codec::COPY_BUFFER];
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
        }

        Ok(finish(hash))
    }
}
