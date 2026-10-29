//! The "name (2)" rule: given a path something already occupies, the next free one beside it.
//! One copy, because three parts of the app need the same answer and a user who sees
//! "Report (2).txt" from a paste should see "Report (2).txt" from a New File too. Existence is
//! passed in rather than looked up so the callers' probes, and the tests' fakes, stay in charge.
//! Spec: src/BertBrowser.Core/Paths/UniquePath.cs.

use std::path::Path;

pub struct UniquePath;

impl UniquePath {
    /// The first free path at or beside `path`. Directories number the whole name; files number
    /// before the extension, so "a.txt" becomes "a (2).txt".
    ///
    /// `is_directory` is what is being *placed*, which decides where the number goes. It is a
    /// parameter rather than probed from `path` because a folder named "notes.txt" blocking a new
    /// file "notes.txt" would otherwise produce "notes.txt (2)" instead of "notes (2).txt".
    pub fn for_path(
        path: &str,
        is_directory: bool,
        directory_exists: impl Fn(&str) -> bool,
        file_exists: impl Fn(&str) -> bool,
    ) -> String {
        let exists = |candidate: &str| file_exists(candidate) || directory_exists(candidate);
        if !exists(path) {
            return path.to_string();
        }
        let (directory, name) = crate::split_last(path);
        let (stem, extension) = if is_directory {
            (name, "")
        } else {
            split_extension(name)
        };
        let mut i = 2u64;
        loop {
            let candidate = if directory.is_empty() {
                format!("{stem} ({i}){extension}")
            } else {
                format!("{directory}\\{stem} ({i}){extension}")
            };
            if !exists(&candidate) {
                return candidate;
            }
            i += 1;
        }
    }

    /// The same rule against the real filesystem, for an item already at `path`.
    pub fn for_existing(path: &str) -> String {
        let is_dir = Path::new(path).is_dir();
        Self::for_path(
            path,
            is_dir,
            |p| Path::new(p).is_dir(),
            |p| Path::new(p).is_file(),
        )
    }
}

/// Splits at the last dot, as `Path.GetFileNameWithoutExtension` and `Path.GetExtension` did: the
/// extension keeps its dot, and a name with no dot has an empty extension.
pub fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    }
}
