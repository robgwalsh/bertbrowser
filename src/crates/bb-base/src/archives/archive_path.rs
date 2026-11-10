//! A path that points inside an archive: the container on disk, and the entry path within it.
//!
//! **A virtual path is an ordinary Windows path** (`C:\x\a.zip\src\lib`), so `PathKey`, the
//! breadcrumb, Up and the single-instance wire format all keep working. A `zip://` scheme would have
//! broken every one of them. **Which segment is the archive cannot be decided without touching
//! disk**, because a folder really can be named `foo.zip`: the parser nominates candidates and an
//! injected probe decides. Spec: Core/Services/Archives/ArchivePath.cs.

use std::fmt;

use super::ArchiveFormats;
use crate::cli::NavigationRequest;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArchivePath {
    pub archive_file: String,
    pub entry_path: String,
}

impl ArchivePath {
    pub fn new(archive_file: &str, entry_path: &str) -> ArchivePath {
        ArchivePath {
            archive_file: archive_file.to_string(),
            entry_path: entry_path.to_string(),
        }
    }

    /// The archive's own root, rather than something inside it.
    pub fn is_root(&self) -> bool {
        self.entry_path.is_empty()
    }

    /// The containing entry, or None at the root (whose parent is a real folder).
    pub fn parent(&self) -> Option<ArchivePath> {
        if self.is_root() {
            return None;
        }
        let entry = match self.entry_path.rfind('\\') {
            Some(cut) => &self.entry_path[..cut],
            None => "",
        };
        Some(ArchivePath::new(&self.archive_file, entry))
    }

    /// Joins a container and an entry path back into one virtual path.
    pub fn compose(archive_file: &str, entry_path: &str) -> String {
        if entry_path.is_empty() {
            archive_file.to_string()
        } else {
            format!(
                "{}\\{}",
                archive_file.trim_end_matches('\\'),
                entry_path.trim_matches('\\')
            )
        }
    }

    /// A purely syntactic pre-filter: does any segment of `path` name an archive. Touches nothing.
    /// The last segment counts too: `C:\x\a.zip` is the archive's own root.
    pub fn looks_virtual(path: &str) -> bool {
        path.split('\\')
            .any(|segment| !segment.is_empty() && ArchiveFormats::is_archive_name(segment))
    }

    /// Splits `path` into a container and an entry path, asking `is_archive_file` whether each
    /// candidate prefix is really a file on disk. None when nothing in the path is an archive, or
    /// when the result would be unsafe. **The shortest matching prefix wins**, so `a.zip\b.zip\c.txt`
    /// resolves to the outermost container; nesting then falls out as a refusal.
    pub fn parse(path: &str, is_archive_file: impl Fn(&str) -> bool) -> Option<ArchivePath> {
        if path.is_empty() || !Self::is_acceptable(path) {
            return None;
        }
        let mut start = 0usize;
        loop {
            let cut = path[start..].find('\\').map(|i| start + i);
            let end = cut.unwrap_or(path.len());
            if end > start && ArchiveFormats::is_archive_name(&path[start..end]) {
                let candidate = &path[..end];
                if is_archive_file(candidate) {
                    // The archive file itself is its own root.
                    let entry = match cut {
                        Some(c) => path[c + 1..].trim_matches('\\'),
                        None => "",
                    };
                    // A zip inside a zip would have to be written out before it could be opened:
                    // refused; the status line offers Extract instead.
                    if !entry.is_empty() && ArchiveFormats::is_archive_name(last_segment(entry)) {
                        return None;
                    }
                    return Some(ArchivePath::new(candidate, entry));
                }
            }
            start = cut? + 1;
        }
    }

    /// Whether a string may be treated as a virtual path at all. **The `..` test is on the raw string
    /// and happens before anything canonicalizes**: `C:\x\a.zip\..\..\Windows` is `C:\Windows` once
    /// resolved, a real folder outside the archive. Everything else defers to
    /// `NavigationRequest::is_acceptable_path`, so the two cannot drift.
    pub fn is_acceptable(path: &str) -> bool {
        if !NavigationRequest::is_acceptable_path(path) {
            return false;
        }
        !path.split('\\').any(|s| s == ".." || s == ".")
    }
}

impl fmt::Display for ArchivePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&Self::compose(&self.archive_file, &self.entry_path))
    }
}

fn last_segment(entry_path: &str) -> &str {
    match entry_path.rfind('\\') {
        Some(cut) => &entry_path[cut + 1..],
        None => entry_path,
    }
}
