//! Turns a selection into the flat list of files a new archive will hold, and the name each gets
//! inside it. Spec: Core/Services/Archives/ArchiveSourceWalk.cs.
//!
//! Separate from the creator so the naming, which is all a user ever notices about a created
//! archive, is testable without writing one. **Reparse points are skipped rather than followed**:
//! a junction is one entry, and following it can walk a tree into itself or drag in gigabytes
//! nobody selected.

use std::os::windows::fs::MetadataExt;

use bb_paths::file_name;

use crate::attributes::{DIRECTORY, HIDDEN, REPARSE_POINT};
use crate::cancel::{CancelToken, Cancelled};

/// One file that is going into a new archive, and the name it will have inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveSource {
    pub path: String,
    pub entry_name: String,
    pub size_bytes: i64,
}

pub struct ArchiveSourceWalk;

impl ArchiveSourceWalk {
    /// Collects the files under `sources`, named relative to the folder that holds them.
    /// `include_hidden` is the browse setting, so what goes in matches what is on show.
    pub fn collect(
        sources: &[String],
        include_hidden: bool,
        cancel: &CancelToken,
    ) -> Result<Vec<ArchiveSource>, Cancelled> {
        let mut collected = Vec::new();

        for source in sources {
            cancel.check()?;

            // One unreadable source costs the others nothing.
            let Ok(meta) = std::fs::metadata(source) else {
                continue;
            };
            let attributes = meta.file_attributes();

            if attributes & DIRECTORY == 0 {
                if !Self::skip(attributes, include_hidden) {
                    collected.push(ArchiveSource {
                        path: source.clone(),
                        entry_name: file_name(source).to_string(),
                        size_bytes: meta.file_size() as i64,
                    });
                }
                continue;
            }

            let trimmed = source.trim_end_matches('\\');
            let root = match bb_paths::split_last(trimmed) {
                ("", _) => source.as_str(),
                (parent, _) => parent,
            };
            Self::walk(source, root, include_hidden, &mut collected, cancel)?;
        }

        Ok(collected)
    }

    fn walk(
        directory: &str,
        root: &str,
        include_hidden: bool,
        into: &mut Vec<ArchiveSource>,
        cancel: &CancelToken,
    ) -> Result<(), Cancelled> {
        let Ok(meta) = std::fs::symlink_metadata(directory) else {
            return Ok(());
        };
        if Self::skip(meta.file_attributes(), include_hidden) {
            return Ok(());
        }
        // A junction is the one entry it is, never the tree it points at.
        if meta.file_attributes() & REPARSE_POINT != 0 {
            return Ok(());
        }

        let Ok(children) = std::fs::read_dir(directory) else {
            return Ok(());
        };

        for child in children {
            cancel.check()?;
            let Ok(child) = child else {
                continue;
            };
            let Ok(meta) = child.metadata() else {
                continue;
            };
            let attributes = meta.file_attributes();
            let full = child.path().to_string_lossy().into_owned();

            if attributes & DIRECTORY != 0 {
                Self::walk(&full, root, include_hidden, into, cancel)?;
                continue;
            }

            if Self::skip(attributes, include_hidden) || attributes & REPARSE_POINT != 0 {
                continue;
            }

            into.push(ArchiveSource {
                entry_name: Self::entry_name_for(&full, root),
                path: full,
                size_bytes: meta.file_size() as i64,
            });
        }
        Ok(())
    }

    /// The name an entry gets: forward slashes, relative to the folder holding what was selected,
    /// so a selected folder keeps its own name at the top of the archive.
    pub fn entry_name_for(path: &str, root: &str) -> String {
        let root = root.trim_end_matches(['\\', '/']);
        let relative = if path.len() > root.len()
            && path.is_char_boundary(root.len())
            && path[..root.len()].eq_ignore_ascii_case(root)
            && matches!(path.as_bytes()[root.len()], b'\\' | b'/')
        {
            &path[root.len() + 1..]
        } else {
            path
        };
        relative.replace('\\', "/")
    }

    fn skip(attributes: u32, include_hidden: bool) -> bool {
        !include_hidden && attributes & HIDDEN != 0
    }
}
