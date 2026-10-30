//! The IPTC record older software writes into a JPEG, inside Photoshop's resource segment.
//! Spec: Core/Services/Metadata/IptcBlock.cs.
//!
//! **Only ever brought into step, never created** — the rule `XmpPacket` follows, for the same
//! reason. A picture that already states its title here would otherwise go on stating the old one
//! to every program that reads this record first.
//!
//! **What cannot be said truthfully is not said.** Each field here has a fixed maximum length
//! and, unless the record declares UTF-8, a single-byte character set. A value that does not fit
//! either is taken out of the record rather than cut short or written with question marks: a
//! reader then falls back to the EXIF or XMP, which hold it whole.
//!
//! Photoshop also stores a checksum of this record so other programs can tell whether it was
//! edited behind XMP's back. It is removed whenever the record changes: a stale one says the
//! record is out of date, which after this edit it is not.

use std::collections::BTreeMap;

use super::fields::{MetadataField, MetadataFields};

const IPTC_RESOURCE: u16 = 0x0404;
const IPTC_DIGEST_RESOURCE: u16 = 0x0425;
const LABEL: &[u8] = b"Photoshop 3.0\0";

const KEYWORDS: u8 = 25;
const CAPTION: u8 = 120;

/// Dataset number in record 2, and the most bytes it may hold.
fn single(field: MetadataField) -> Option<(u8, usize)> {
    match field {
        MetadataField::Title => Some((5, 64)),
        MetadataField::Author => Some((80, 32)),
        MetadataField::Copyright => Some((116, 128)),
        _ => None,
    }
}

struct Resource {
    id: u16,
    /// The resource's name exactly as stored: a length byte, the characters, and the padding that
    /// makes the whole an even number of bytes.
    name: Vec<u8>,
    data: Vec<u8>,
}

struct Dataset {
    record: u8,
    number: u8,
    data: Vec<u8>,
}

pub struct IptcBlock;

impl IptcBlock {
    /// The segment with the same changes made to its IPTC record, or None when it has no such
    /// record, needs no change, or is not laid out as expected — in which case it is left alone.
    pub fn with_changes(
        segment: &[u8],
        changes: &BTreeMap<MetadataField, String>,
    ) -> Option<Vec<u8>> {
        if changes.is_empty() || !segment.starts_with(LABEL) {
            return None;
        }
        let mut resources = resources(&segment[LABEL.len()..])?;
        let index = resources.iter().position(|r| r.id == IPTC_RESOURCE)?;
        let mut datasets = datasets(&resources[index].data)?;

        let utf8 = datasets
            .iter()
            .any(|d| d.record == 1 && d.number == 90 && d.data == [0x1B, 0x25, 0x47]);
        let mut touched = false;

        let mut replace = |number: u8, values: &[String], max: usize, only_if_present: bool| {
            let before = datasets.len();
            datasets.retain(|d| !(d.record == 2 && d.number == number));
            let had = datasets.len() != before;
            if only_if_present && !had {
                return;
            }
            touched = true;

            for value in values {
                if let Some(bytes) = encode(value, utf8)
                    && bytes.len() <= max
                {
                    datasets.push(Dataset {
                        record: 2,
                        number,
                        data: bytes,
                    });
                }
            }
        };

        for (field, value) in changes {
            let one: Vec<String> = if value.is_empty() {
                vec![]
            } else {
                vec![value.clone()]
            };
            if let Some((dataset, max)) = single(*field) {
                replace(dataset, &one, max, false);
            }
            if *field == MetadataField::Keywords {
                replace(KEYWORDS, &MetadataFields::split_list(value), 64, false);
            }
            // The caption follows the title only where there already is one: see XmpPacket.
            if *field == MetadataField::Title {
                replace(CAPTION, &one, 2000, true);
            }
        }

        if !touched {
            return None;
        }

        // Records in order, as the standard asks; within a record, as they were.
        datasets.sort_by_key(|d| d.record);
        let mut record = Vec::new();
        for dataset in &datasets {
            record.extend_from_slice(&[
                0x1C,
                dataset.record,
                dataset.number,
                (dataset.data.len() >> 8) as u8,
                dataset.data.len() as u8,
            ]);
            record.extend_from_slice(&dataset.data);
        }

        resources[index].data = record;
        resources.retain(|r| r.id != IPTC_DIGEST_RESOURCE);

        let mut output = LABEL.to_vec();
        for resource in &resources {
            output.extend_from_slice(b"8BIM");
            output.extend_from_slice(&resource.id.to_be_bytes());
            output.extend_from_slice(&resource.name);
            output.extend_from_slice(&(resource.data.len() as u32).to_be_bytes());
            output.extend_from_slice(&resource.data);
            if resource.data.len() % 2 != 0 {
                output.push(0);
            }
        }
        Some(output)
    }
}

fn encode(value: &str, utf8: bool) -> Option<Vec<u8>> {
    if utf8 {
        return Some(value.as_bytes().to_vec());
    }
    value.chars().map(|c| u8::try_from(c as u32).ok()).collect()
}

fn resources(data: &[u8]) -> Option<Vec<Resource>> {
    let mut resources = Vec::new();
    let mut at = 0usize;

    while at < data.len() {
        if data.len() - at < 12 || &data[at..at + 4] != b"8BIM" {
            return None;
        }
        let id = u16::from_be_bytes([data[at + 4], data[at + 5]]);
        let mut name_length = 1 + data[at + 6] as usize;
        if !name_length.is_multiple_of(2) {
            name_length += 1;
        }
        if at + 6 + name_length + 4 > data.len() {
            return None;
        }
        let name = data[at + 6..at + 6 + name_length].to_vec();
        let size_at = at + 6 + name_length;
        let size = u32::from_be_bytes([
            data[size_at],
            data[size_at + 1],
            data[size_at + 2],
            data[size_at + 3],
        ]) as usize;
        let start = size_at + 4;
        if size > data.len() - start {
            return None;
        }
        resources.push(Resource {
            id,
            name,
            data: data[start..start + size].to_vec(),
        });
        at = start + size + (size % 2);
    }

    Some(resources)
}

fn datasets(data: &[u8]) -> Option<Vec<Dataset>> {
    let mut datasets = Vec::new();
    let mut at = 0usize;

    while at < data.len() {
        // Trailing padding is common; anything else that is not a dataset is not ours to rewrite.
        if data[at] == 0 && data[at..].iter().all(|&b| b == 0) {
            break;
        }
        if data.len() - at < 5 || data[at] != 0x1C {
            return None;
        }
        let length = u16::from_be_bytes([data[at + 3], data[at + 4]]) as usize;
        // The extended-length form, for datasets over 32 KB. Nothing this app writes needs one,
        // and a record that uses one is carried through untouched instead.
        if length & 0x8000 != 0 || at + 5 + length > data.len() {
            return None;
        }
        datasets.push(Dataset {
            record: data[at + 1],
            number: data[at + 2],
            data: data[at + 5..at + 5 + length].to_vec(),
        });
        at += 5 + length;
    }

    Some(datasets)
}
