//! Reading and editing what a file says about itself: the field catalogue, one codec per format
//! family (EXIF in JPEG/PNG/WebP/TIFF with XMP and IPTC kept in step, audio and video tags, the
//! properties part of Office documents and EPUB books), the planner that decides which files an
//! edit can be written to, the executor that rewrites each beside itself and swaps it in only once
//! its payload digest matches the original's, and the pure half of the Windows property fallback.
//! Spec: Core/Services/Metadata/*.cs.

mod asf;
mod audio;
mod audio_payload;
mod codec;
mod codecs;
mod epub;
mod executor;
mod exif;
mod exif_fields;
mod fields;
mod iptc;
mod jpeg;
mod models;
mod openxml;
mod planner;
mod png;
mod property_fallback;
mod tiff;
mod webp;
mod xml;
mod xmp;
mod zip_document;

pub use asf::AsfTags;
pub use audio::AudioTagCodec;
pub use audio_payload::{AudioPayload, EXTENSIONS as AUDIO_EXTENSIONS};
pub use codec::{Destination, MetadataCodec, MetadataError, MetadataResult, Source};
pub use codecs::MetadataCodecs;
pub use epub::EpubCodec;
pub use executor::{EDITED_MARKER, MetadataEditExecutor, REPLACED_MARKER, REWRITE_MARKER};
pub use exif::{ExifBlock, ExifDirectory};
pub use exif_fields::ExifFields;
pub use fields::{
    MetadataFamily, MetadataField, MetadataFieldKind, MetadataFieldSpec, MetadataFields,
};
pub use iptc::IptcBlock;
pub use jpeg::JpegCodec;
pub use models::{
    FailedMetadataEdit, HeldVersion, MetadataDocument, MetadataEdit, MetadataEditOutcome,
    MetadataEditPlan, MetadataRejection, MetadataSwapResult, PlannedMetadataEdit,
    RejectedMetadataEdit,
};
pub use openxml::OpenXmlCodec;
pub use planner::{FileSystemMetadataProbe, MetadataEditPlanner, MetadataProbe, OfferedByWindows};
pub use png::PngCodec;
pub use property_fallback::{
    PropertyFallback, PropertyFallbackReading, WindowsPropertyFields, WindowsPropertyValue,
    WritabilityMemory,
};
pub use tiff::TiffCodec;
pub use webp::WebPCodec;
pub use xmp::XmpPacket;
