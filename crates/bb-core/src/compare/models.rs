//! The vocabulary of a folder comparison. Spec: Core/Services/Compare/CompareModels.cs.

use crate::FileTime;

/// One entry on one side of a comparison, addressed relative to that side's root.
///
/// `relative_key` is uppercased, `\`-separated, with no leading separator: a path key with the
/// root sliced off. Pairing the two sides by this string is what makes the comparison agree with
/// the rest of the app about what "the same path" means. There is deliberately no whole display
/// path per entry: over a real 1.65-million-row index that second copy of every path is the
/// largest avoidable thing a comparison holds, so [`CompareResult::display_path`]
/// (super::CompareResult::display_path) rebuilds the few that reach a dialog from the ancestors'
/// names. `modified_utc` of [`FileTime::UNKNOWN`] means *unknown* (the index's name-only build path
/// writes exactly that) and forces [`CompareVerdict::Unknown`] rather than a guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareEntry {
    pub relative_key: String,
    /// The leaf name, display-cased.
    pub name: String,
    pub is_directory: bool,
    /// Bytes for a file; ignored for a folder, whose size is never compared.
    pub size_bytes: i64,
    pub modified_utc: FileTime,
}

impl CompareEntry {
    pub fn new(
        relative_key: impl Into<String>,
        name: impl Into<String>,
        is_directory: bool,
        size_bytes: i64,
        modified_utc: FileTime,
    ) -> CompareEntry {
        CompareEntry {
            relative_key: relative_key.into(),
            name: name.into(),
            is_directory,
            size_bytes,
            modified_utc,
        }
    }
}

/// How the two sides of one relative path relate. Side-neutral: it names the relationship, not
/// what either list should look like; [`CompareRules::row_state`](super::CompareRules::row_state)
/// projects it per side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompareVerdict {
    /// Could not be established, because a side reported no timestamp. Never rendered as a match
    /// and never turned into a sync action: this is the one verdict that must never be produced by
    /// guessing, because `Same` is what authorises a delete.
    Unknown,
    /// Both sides exist and match.
    Same,
    /// Present on the left, absent on the right.
    LeftOnly,
    /// Present on the right, absent on the left.
    RightOnly,
    /// Both exist; the left is measurably newer.
    LeftNewer,
    /// Both exist; the right is measurably newer.
    RightNewer,
    /// Both exist and are not the same, but neither is measurably newer: equal timestamps with
    /// unequal sizes, or a file on one side and a folder on the other.
    Differs,
}

impl CompareVerdict {
    /// Every verdict, for the tests that sweep them as `Enum.GetValues` did.
    pub const ALL: [CompareVerdict; 7] = [
        CompareVerdict::Unknown,
        CompareVerdict::Same,
        CompareVerdict::LeftOnly,
        CompareVerdict::RightOnly,
        CompareVerdict::LeftNewer,
        CompareVerdict::RightNewer,
        CompareVerdict::Differs,
    ];
}

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
