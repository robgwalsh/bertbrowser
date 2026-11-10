//! The vocabulary of a folder comparison. Spec: Core/Services/Compare/CompareModels.cs.

// The two types the definition of "the same file" is written in live with it, in bb-files.
pub use bb_files::equality::{CompareEntry, CompareVerdict};

/// Which side of a comparison a listing belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompareSide {
    Left,
    Right,
}

/// What one row shows: the per-side projection of a [`CompareVerdict`]. Separate from the verdict
/// because "only here" and "older" mean opposite things on the two sides, and a row can only ever
/// say something about itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompareRowState {
    /// Not part of a comparison, or nothing to say on this side.
    None,
    Unknown,
    Same,
    OnlyHere,
    Newer,
    Older,
    Differs,
}

/// Where a side's listing came from. Not shown to the user: a compare always works, so "the index
/// cannot answer this" selects a source rather than producing a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompareSourceKind {
    /// A range scan of `fs_entry`.
    Index,
    /// A live walk of the directory tree.
    Walk,
}

/// Slicing relative keys apart. Ordinal throughout: relative keys are already uppercased.
pub struct CompareKeys;

impl CompareKeys {
    pub const SEPARATOR: char = '\\';

    /// Every ancestor key of `relative_key`, nearest first, excluding the key itself and excluding
    /// the root (which is the empty string and has no row).
    pub fn ancestors(relative_key: &str) -> impl Iterator<Item = &str> {
        let mut remaining = relative_key;
        std::iter::from_fn(move || {
            let cut = remaining.rfind(Self::SEPARATOR)?;
            if cut == 0 {
                return None;
            }
            remaining = &remaining[..cut];
            Some(remaining)
        })
    }

    /// True when `key` is at or below `folder_key`. The separator check is what stops `SRC2\A`
    /// reading as being inside `SRC`.
    pub fn is_at_or_under(key: &str, folder_key: &str) -> bool {
        if folder_key.is_empty() {
            return true;
        }
        if !key.starts_with(folder_key) {
            return false;
        }
        key.len() == folder_key.len() || key.as_bytes()[folder_key.len()] == b'\\'
    }
}
