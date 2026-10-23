//! Path canonicalization: `PathKey`, `UniquePath`, prefix bounds for subtree scans and natural sort.
//! No dependencies beyond the standard library and `windows-sys`.
//! Spec: src/BertBrowser.Core/Paths/*.cs, src/BertBrowser.App/Interop/NaturalSort.cs.

mod key;
mod natural;
mod text;
mod unique;

pub use key::{PathError, PathKey, fold_upper, parent_key, root_len};
pub use natural::natural_cmp;
pub use text::PathText;
pub use unique::UniquePath;

/// The separator every key uses. Keys never contain `/`.
pub const SEPARATOR: char = '\\';

/// Splits a path into (directory, file name) at the last separator, either kind. The directory
/// keeps no trailing separator; a path with no separator has an empty directory.
pub fn split_last(path: &str) -> (&str, &str) {
    match path.rfind(['\\', '/']) {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    }
}

/// The last component of a path, as `Path.GetFileName` gave it: everything after the last
/// separator, which is empty for a path that ends in one.
pub fn file_name(path: &str) -> &str {
    split_last(path).1
}

/// Encodes for a Win32 call: UTF-16 with a terminating NUL.
pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
