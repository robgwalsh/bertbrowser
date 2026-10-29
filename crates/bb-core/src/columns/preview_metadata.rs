//! The slice of the preview pane's metadata rules the column catalogue is composed from.
//!
//! These live in `crate::preview` (`PreviewMetadata`, `PreviewClassifier`) and are re-exported
//! here so the curated set is *composed* from the one copy of these canonical names rather than
//! retyped, as the C# does: a second copy would drift with nothing to notice.

pub use crate::preview::{DOCUMENT_ORDER, IMAGE_ORDER, MEDIA_ORDER, shorten_canonical};

/// The attribute bits that mark a cloud placeholder: a file whose bytes are not here, and which a
/// sync provider would download on open.
pub const PLACEHOLDER: u32 = crate::preview::PreviewClassifier::PLACEHOLDER;

/// Whether reading this file's bytes would make a sync provider download it. One predicate rather
/// than the mask repeated: a shell-metadata column opens files too, and two copies of this test
/// could disagree where one cannot.
pub fn is_cloud_placeholder(attributes: u32) -> bool {
    crate::preview::PreviewClassifier::is_cloud_placeholder(attributes)
}
