//! The slice of the preview pane's metadata rules the column catalogue is composed from.
//!
//! TODO(port): extended when Preview is ported — these belong to `preview::PreviewMetadata` and
//! `preview::PreviewClassifier` (Core/Services/Preview/PreviewMetadata.cs, PreviewClassifier.cs)
//! and should move there, leaving the catalogue importing them. They are here so the curated set
//! is *composed* from the one copy of these canonical names rather than retyped, as the C# does: a
//! second copy would drift with nothing to notice.

use crate::attributes;

/// The facts worth showing about a picture, at a glance, in this order.
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

/// `System.Image.Dimensions` -> `Dimensions`. The last-resort header for a property with no
/// registered display name, shared by the preview strip and the column catalogue so the two cannot
/// differ.
pub fn shorten_canonical(canonical: &str) -> &str {
    match canonical.rfind('.') {
        Some(dot) if dot + 1 < canonical.len() => &canonical[dot + 1..],
        _ => canonical,
    }
}

/// The attribute bits that mark a cloud placeholder: a file whose bytes are not here, and which a
/// sync provider would download on open.
pub const PLACEHOLDER: u32 =
    attributes::OFFLINE | attributes::RECALL_ON_OPEN | attributes::RECALL_ON_DATA_ACCESS;

/// Whether reading this file's bytes would make a sync provider download it. One predicate rather
/// than the mask repeated: a shell-metadata column opens files too, and two copies of this test
/// could disagree where one cannot.
pub fn is_cloud_placeholder(attributes: u32) -> bool {
    attributes & PLACEHOLDER != 0
}
