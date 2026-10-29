//! File-list columns: what a column is (the catalogue), what the user chose (the setting), how a
//! saved list becomes the columns a list shows (the layout rules), what the "Add column" popup
//! offers (the candidates), how two cell values order, and whether a shell-property column may open
//! a file at all. Pure throughout: the property store read itself belongs to the shell layer.
//! Spec: Core/Services/Columns/*.cs.

mod candidates;
mod catalog;
mod layout_rules;
mod metadata_read_rules;
pub mod preview_metadata;
mod spec;
mod value;

pub use candidates::{
    ColumnCandidate, ColumnCandidateGroup, ColumnCandidateKind, ColumnCandidates,
};
pub use catalog::ColumnCatalog;
pub use layout_rules::ColumnLayoutRules;
pub use metadata_read_rules::MetadataReadRules;
pub use spec::{ColumnKind, ColumnSpec, PropertyKey, ResolvedColumn};
pub use value::{ColumnComparison, ColumnValue};

use serde::{Deserialize, Serialize};

/// What the user chose: one column, in order, with the width they dragged it to. Plain data so the
/// settings file stays hand-editable. There is deliberately **no `enabled` flag**: presence in the
/// list *is* enabledness, and a flag would be a second source of truth for one fact.
/// Spec: Core/Services/Columns/ColumnSetting.cs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ColumnSetting {
    pub id: String,
    /// Width in device-independent pixels. Anything unusable — a hand-edited absurdity, a `NaN`
    /// from an auto-sized column — is repaired by [`ColumnLayoutRules::sane_width`] rather than
    /// reaching the view.
    pub width: f64,
}

impl ColumnSetting {
    pub fn new(id: &str, width: f64) -> ColumnSetting {
        ColumnSetting {
            id: id.to_string(),
            width,
        }
    }
}

/// The rule separating the two id spaces, kept pure so it can be tested without `propsys.dll`.
pub struct ColumnId;

impl ColumnId {
    /// A registry-backed canonical name is nothing like this long; the cap is what stops a
    /// hand-edited settings file putting an arbitrary string into a header.
    pub const MAX_LENGTH: usize = 128;

    /// Whether an id looks like a Windows canonical property name (`System.Photo.DateTaken`) rather
    /// than one of this app's built-in column ids.
    ///
    /// The distinction decides what happens to an id the catalogue does not know. A built-in id it
    /// does not recognise came from a newer build and names a column this one cannot render, so it
    /// is dropped. A canonical name it does not recognise may be a perfectly good property this
    /// machine has no handler for, so it is kept and renders blank — unknown, never wrong.
    pub fn looks_canonical(id: &str) -> bool {
        if id.is_empty() || id.len() > Self::MAX_LENGTH || id.starts_with('.') || id.ends_with('.')
        {
            return false;
        }
        let mut dot = false;
        for c in id.chars() {
            if c == '.' {
                dot = true;
                continue;
            }
            if !c.is_ascii_alphanumeric() && c != '_' {
                return false;
            }
        }
        dot
    }
}
