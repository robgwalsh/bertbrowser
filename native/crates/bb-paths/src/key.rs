//! Canonical path keys for database storage. Windows paths are case-insensitive, but SQLite's NOCASE
//! collation folds ASCII only, so case is folded here and SQLite compares with plain BINARY.
//! Spec: src/BertBrowser.Core/Paths/PathKey.cs.

use std::collections::HashSet;
use std::fmt;

use windows_sys::Win32::Storage::FileSystem::GetFullPathNameW;

/// A path that cannot become a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// Empty or whitespace only.
    Empty,
    /// Windows refused to resolve it (an invalid drive-relative form, for example).
    Unresolvable(String),
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::Empty => write!(f, "path must be non-empty"),
            PathError::Unresolvable(p) => write!(f, "path could not be resolved: {p}"),
        }
    }
}

impl std::error::Error for PathError {}

pub struct PathKey;

impl PathKey {
    /// Canonicalizes a path into the form used as a database key: fully qualified, `\` separators,
    /// no trailing separator (except drive roots like `C:\`), upper-cased with the simple
    /// per-character mapping.
    pub fn canonicalize(path: &str) -> Result<String, PathError> {
        Ok(fold_upper(&Self::normalize_display(path)?))
    }

    /// The same normalization as [`canonicalize`](Self::canonicalize) but preserving the original
    /// casing, for display.
    pub fn normalize_display(path: &str) -> Result<String, PathError> {
        if path.trim().is_empty() {
            return Err(PathError::Empty);
        }
        let full = full_path(path).ok_or_else(|| PathError::Unresolvable(path.to_string()))?;
        Ok(trim_trailing_separator(full))
    }

    /// The half-open range `[lo, hi)` such that a canonical key `k` is strictly inside the directory
    /// (at any depth) iff `lo <= k < hi` under BINARY comparison. `lo = dir + "\"`, `hi = dir + "]"`
    /// because `]` (U+005D) is the character after `\` (U+005C). Subtree queries become index range
    /// scans with no LIKE escaping.
    pub fn prefix_bounds(dir_key: &str) -> Result<(String, String), PathError> {
        // Accepts either a canonical key or a raw path; re-canonicalizing is cheap and safe.
        let key = Self::canonicalize(dir_key)?;
        // A drive root keeps its separator ("C:\"), so lo must not double it.
        let body = key.strip_suffix('\\').unwrap_or(&key);
        Ok((format!("{body}\\"), format!("{body}]")))
    }

    /// True if `key` is strictly under `dir_key`. One question only: re-canonicalizes and allocates.
    pub fn is_under(key: &str, dir_key: &str) -> Result<bool, PathError> {
        let (lo, hi) = Self::prefix_bounds(dir_key)?;
        Ok(key >= lo.as_str() && key < hi.as_str())
    }

    /// [`is_under`](Self::is_under) for a caller that already holds two canonical keys: the same
    /// answer with no path call and no allocation. A key is inside `[dir + "\", dir + "]")` exactly
    /// when it starts with `dir + "\"`. Handed a raw path it answers wrongly rather than failing.
    pub fn is_under_key(key: &str, dir_key: &str) -> bool {
        // A drive root keeps its separator ("C:\"), everything else has none.
        let body = dir_key.strip_suffix('\\').unwrap_or(dir_key);
        key.len() > body.len() && key.as_bytes()[body.len()] == b'\\' && key.starts_with(body)
    }

    /// True if some *other* key in `dir_keys` is an ancestor of `key`: the planners' "does this
    /// travel with a folder that was selected too" question. All canonical. Walks up the key's own
    /// separators, so the cost follows the depth of one path rather than the number of folders
    /// selected; each prefix is tried with and without its separator because a root keeps it.
    pub fn has_ancestor_key_in(key: &str, dir_keys: &HashSet<String>) -> bool {
        if dir_keys.is_empty() {
            return false;
        }
        let bytes = key.as_bytes();
        for i in (1..bytes.len()).rev() {
            if bytes[i] != b'\\' {
                continue;
            }
            if dir_keys.contains(&key[..i]) {
                return true;
            }
            if i + 1 < bytes.len() && dir_keys.contains(&key[..=i]) {
                return true;
            }
        }
        false
    }
}

/// The parent of a canonical key, as `Path.GetDirectoryName` gave it: `C:\FOO\BAR` to `C:\FOO`,
/// `C:\FOO` to `C:\`, and None at a root (`C:\`, `\\SERVER\SHARE`).
pub fn parent_key(key: &str) -> Option<String> {
    let root = root_len(key);
    if key.len() <= root {
        return None;
    }
    let cut = key.rfind('\\')?;
    // The separator sits inside the root ("C:\FOO"): the parent is the root, separator and all.
    if cut < root {
        return Some(key[..root].to_string());
    }
    Some(key[..cut].to_string())
}

/// Simple (one character to one character) upper-casing, which is what `ToUpperInvariant` did. The
/// full mapping would turn `ß` into `SS` and give two distinct Windows paths one key.
pub fn fold_upper(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let mut upper = c.to_uppercase();
        match (upper.next(), upper.next()) {
            (Some(u), None) => out.push(u),
            _ => out.push(c),
        }
    }
    out
}

/// `GetFullPathNameW`: resolves `.`/`..`, turns `/` into `\`, and qualifies against the current
/// directory, as `Path.GetFullPath` did (it calls the same function).
fn full_path(path: &str) -> Option<String> {
    let wide = crate::wide(path);
    let mut buffer: Vec<u16> = vec![0; 512];
    loop {
        // SAFETY: both pointers are valid for the call; the buffer length is passed with it.
        let needed = unsafe {
            GetFullPathNameW(
                wide.as_ptr(),
                buffer.len() as u32,
                buffer.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        };
        if needed == 0 {
            return None;
        }
        if (needed as usize) < buffer.len() {
            buffer.truncate(needed as usize);
            return Some(String::from_utf16_lossy(&buffer));
        }
        buffer.resize(needed as usize + 1, 0);
    }
}

/// Keeps the trailing separator only when it is the root itself (`C:\`, `\\?\C:\`).
fn trim_trailing_separator(full: String) -> String {
    if !full.ends_with(['\\', '/']) {
        return full;
    }
    let root = root_len(&full);
    if full.len() > root {
        let trimmed = full.trim_end_matches(['\\', '/']);
        // Never trim into the root ("\\server\share\" keeps "\\server\share").
        return trimmed[..trimmed.len().max(root.min(trimmed.len()))].to_string();
    }
    full
}

/// The length of the path root as `Path.GetPathRoot` reported it: `C:\` is 3, `\\server\share` has
/// no trailing separator, and a `\\?\` or `\\.\` prefix is carried through.
pub fn root_len(path: &str) -> usize {
    let b = path.as_bytes();
    let is_sep = |c: u8| c == b'\\' || c == b'/';
    let is_drive = |s: &[u8]| s.len() >= 2 && s[0].is_ascii_alphabetic() && s[1] == b':';

    if b.len() >= 4
        && is_sep(b[0])
        && is_sep(b[1])
        && (b[2] == b'?' || b[2] == b'.')
        && is_sep(b[3])
    {
        let rest = &b[4..];
        if rest.len() >= 4 && rest[..3].eq_ignore_ascii_case(b"UNC") && is_sep(rest[3]) {
            return 8 + unc_share_len(&b[8..]);
        }
        if is_drive(rest) {
            return if rest.len() >= 3 && is_sep(rest[2]) {
                7
            } else {
                6
            };
        }
        // A device name: up to the next separator.
        let end = rest.iter().position(|&c| is_sep(c)).unwrap_or(rest.len());
        return 4 + end;
    }
    if b.len() >= 2 && is_sep(b[0]) && is_sep(b[1]) {
        return 2 + unc_share_len(&b[2..]);
    }
    if is_drive(b) {
        return if b.len() >= 3 && is_sep(b[2]) { 3 } else { 2 };
    }
    if !b.is_empty() && is_sep(b[0]) { 1 } else { 0 }
}

/// Length of `server\share` from the start of the slice, without any trailing separator.
fn unc_share_len(rest: &[u8]) -> usize {
    let is_sep = |c: u8| c == b'\\' || c == b'/';
    let mut seps = 0;
    for (i, &c) in rest.iter().enumerate() {
        if is_sep(c) {
            seps += 1;
            if seps == 2 {
                return i;
            }
        }
    }
    rest.len()
}
