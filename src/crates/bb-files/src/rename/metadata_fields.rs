//! The metadata field catalogue the rename tokens draw on: `{artist}`, `{track:00}`, `{taken}`.
//! The rows now live in `bb_metadata` (Core/Services/Metadata/MetadataFields.cs) and are
//! re-exported here so the rename area keeps its import path.

pub use bb_metadata::{MetadataField, MetadataFieldKind, MetadataFieldSpec, MetadataFields};
