//! File-list columns: the setting shape and the id rule. The catalogue, layout rules and candidates
//! arrive with the rest of M1. Spec: Core/Services/Columns/ColumnSetting.cs.

use serde::{Deserialize, Serialize};

/// What the user chose: one column, in order, with the width they dragged it to. There is
/// deliberately no `enabled` flag: presence in the list *is* enabledness.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ColumnSetting {
    pub id: String,
    /// Width in device-independent pixels. Anything unusable is repaired by the layout rules.
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

/// The rule separating the two id spaces.
pub struct ColumnId;

impl ColumnId {
    /// A registry-backed canonical name is nothing like this long; the cap is what stops a
    /// hand-edited settings file putting an arbitrary string into a header.
    pub const MAX_LENGTH: usize = 128;

    /// Whether an id looks like a Windows canonical property name (`System.Photo.DateTaken`) rather
    /// than one of this app's built-in column ids. A built-in id the catalogue does not know came
    /// from a newer build and is dropped; a canonical name it does not know is kept and renders blank.
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
