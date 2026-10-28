//! Picks the handful of properties worth putting under a preview, and puts them in a useful order.
//!
//! Selected by *canonical* name (`System.Image.Dimensions`, never "Dimensions"). The property
//! store's display names are localised, so matching on them works perfectly on the machine it was
//! written on and silently returns nothing on a German or Japanese Windows. The label shown is
//! still the localised one; only the choosing is done on the invariant key.
//!
//! The order is the point as much as the selection: the Properties dialog already exists for the
//! exhaustive dump, so this is the opposite, what a person wants at a glance, first.
//! Spec: Core/Services/Preview/PreviewMetadata.cs.

use std::collections::HashMap;

use super::PreviewKind;

/// One property as the Windows property system handed it over: the canonical key, the localised
/// label, and the display-formatted value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellPropertyRow {
    pub canonical: String,
    pub name: String,
    pub value: String,
}

/// One row of the preview pane's details strip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataRow {
    pub label: String,
    pub value: String,
}

pub struct PreviewMetadata;

impl PreviewMetadata {
    /// Rows past this are dropped. The strip sits under the preview, not instead of it.
    pub const MAX_ROWS: usize = 12;

    pub fn select(kind: PreviewKind, properties: &[ShellPropertyRow]) -> Vec<MetadataRow> {
        let wanted = order_for(kind);
        if wanted.is_empty() || properties.is_empty() {
            return Vec::new();
        }

        let by_canonical = index(
            properties
                .iter()
                .filter(|p| !p.canonical.is_empty() && !p.value.is_empty()),
        );

        let mut rows = Vec::with_capacity(wanted.len().min(Self::MAX_ROWS));
        for canonical in wanted {
            if rows.len() == Self::MAX_ROWS {
                break;
            }
            let Some(property) = by_canonical.get(&fold(canonical)) else {
                continue;
            };

            // A property with no registered display name still has a canonical one; showing the
            // tail of the key beats showing a blank label.
            let label = if property.name.is_empty() {
                shorten_canonical(canonical)
            } else {
                &property.name
            };
            rows.push(MetadataRow {
                label: label.to_string(),
                value: property.value.clone(),
            });
        }
        rows
    }

    /// The facts worth showing about any file, whatever kind it is: every curated property the
    /// shell reported a value for, pictures' first, then media's, then documents'. For the
    /// metadata pane, which shows them read-only beneath what it can edit.
    pub fn details(properties: &[ShellPropertyRow]) -> Vec<MetadataRow> {
        let by_canonical = index(properties.iter());

        let mut seen = std::collections::HashSet::new();
        let mut rows = Vec::new();
        for canonical in IMAGE_ORDER.iter().chain(MEDIA_ORDER).chain(DOCUMENT_ORDER) {
            let key = fold(canonical);
            if !seen.insert(key.clone()) {
                continue;
            }
            if let Some(property) = by_canonical.get(&key)
                && !property.value.trim().is_empty()
            {
                rows.push(MetadataRow {
                    label: property.name.clone(),
                    value: property.value.trim().to_string(),
                });
            }
        }
        rows
    }
}

/// First one wins, as `TryAdd` did; keyed case-insensitively.
fn index<'a>(
    properties: impl Iterator<Item = &'a ShellPropertyRow>,
) -> HashMap<String, &'a ShellPropertyRow> {
    let mut by_canonical = HashMap::new();
    for property in properties {
        by_canonical
            .entry(fold(&property.canonical))
            .or_insert(property);
    }
    by_canonical
}

fn fold(canonical: &str) -> String {
    canonical.to_lowercase()
}

/// `System.Image.Dimensions` -> `Dimensions`. The last-resort header for a property with no
/// registered display name, shared with the column catalogue so the two cannot differ.
pub fn shorten_canonical(canonical: &str) -> &str {
    match canonical.rfind('.') {
        Some(dot) if dot + 1 < canonical.len() => &canonical[dot + 1..],
        _ => canonical,
    }
}

fn order_for(kind: PreviewKind) -> &'static [&'static str] {
    match kind {
        PreviewKind::Image => IMAGE_ORDER,
        PreviewKind::Media => MEDIA_ORDER,
        PreviewKind::Document => DOCUMENT_ORDER,
        // Text, archives and fonts have better numbers of their own (the encoding, the entry
        // count, the family name) and the shell has nothing to add to them.
        _ => &[],
    }
}

/// The facts worth showing about a picture, at a glance, in this order. Also the seed for the
/// column catalogue's curated shell columns: one copy of these canonical names, because a second
/// would drift with nothing to notice.
pub const IMAGE_ORDER: &[&str] = &[
    "System.Image.Dimensions",
    "System.Image.BitDepth",
    "System.Image.ColorSpace",
    "System.Photo.DateTaken",
    "System.Photo.CameraManufacturer",
    "System.Photo.CameraModel",
    "System.Photo.LensModel",
    "System.Photo.FNumber",
    "System.Photo.ExposureTime",
    "System.Photo.ISOSpeed",
    "System.Photo.FocalLength",
    "System.GPS.Latitude",
    "System.GPS.Longitude",
];

/// Audio and video.
pub const MEDIA_ORDER: &[&str] = &[
    "System.Title",
    "System.Music.Artist",
    "System.Music.AlbumTitle",
    "System.Music.TrackNumber",
    "System.Music.Genre",
    "System.Media.Year",
    "System.Media.Duration",
    "System.Video.FrameWidth",
    "System.Video.FrameHeight",
    "System.Video.FrameRate",
    "System.Video.Compression",
    "System.Video.EncodingBitrate",
    "System.Audio.Format",
    "System.Audio.EncodingBitrate",
    "System.Audio.SampleRate",
    "System.Audio.ChannelCount",
];

/// Office-style documents. Shares `System.Title` with media.
pub const DOCUMENT_ORDER: &[&str] = &[
    "System.Title",
    "System.Author",
    "System.Subject",
    "System.Document.PageCount",
    "System.Document.WordCount",
    "System.Company",
    "System.ApplicationName",
    "System.Document.DateCreated",
    "System.Document.DateSaved",
];
