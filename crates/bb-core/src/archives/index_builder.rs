//! Turns a container's flat list of entries into the directory tree the file list browses.
//! Spec: Core/Services/Archives/ArchiveIndexBuilder.cs.
//!
//! Pure: entries in, tree out, so every rule here is testable with no archive, no temp file and
//! no library. **Intermediate directories are synthesized from path prefixes, and an explicit
//! directory entry only decorates a node that already exists.** A zip carrying `src/lib/util.js`
//! and no `src/` entry is completely ordinary, and trusting explicit entries leaves a folder that
//! is in every path and cannot be entered. **An entry whose key escapes the root is refused here**
//! rather than in the extractor: that is Zip Slip, and refusing it at read time means the entry
//! never exists to be extracted.

use std::collections::HashMap;

use bb_paths::fold_upper;

use super::{ArchiveCapabilities, ArchiveFailure, ArchiveIndex, ArchiveNode, RawArchiveEntry};

pub struct ArchiveIndexBuilder;

struct Counters {
    refused: i32,
    files: i32,
}

impl ArchiveIndexBuilder {
    /// Builds the tree. Never fails: a container past the cap comes back as a failure index.
    pub fn build(
        entries: impl IntoIterator<Item = RawArchiveEntry>,
        capabilities: ArchiveCapabilities,
        max_entries: usize,
    ) -> ArchiveIndex {
        let mut nodes: Vec<ArchiveNode> = vec![ArchiveNode::directory("", "")];
        let mut by_path: HashMap<String, usize> = HashMap::new();
        let mut counts = Counters {
            refused: 0,
            files: 0,
        };
        let mut seen = 0usize;

        for entry in entries {
            let Some(key) = Self::normalize(entry.key.as_deref()) else {
                counts.refused += 1;
                continue;
            };

            seen += 1;
            if seen > max_entries {
                return ArchiveIndex::failed(
                    ArchiveFailure::TooManyEntries,
                    &format!(
                        "This archive holds more than {} entries, which is more than can be browsed.",
                        crate::format::thousands(max_entries as i64)
                    ),
                );
            }

            if entry.is_directory {
                let dir = Self::ensure_directory(&mut nodes, &mut by_path, &key, &mut counts);
                // An explicit directory entry contributes a timestamp and nothing else. It must
                // never be what creates the node, or a folder that exists only implicitly is lost.
                if entry.modified.is_some() {
                    nodes[dir].modified = entry.modified;
                }
                continue;
            }

            let (parent_path, name) = match key.rfind('\\') {
                Some(cut) => (&key[..cut], &key[cut + 1..]),
                None => ("", key.as_str()),
            };
            if name.is_empty() {
                counts.refused += 1;
                continue;
            }

            // Nearly every entry's folder is already there, since a container lists a folder's
            // files together, so it is looked up whole and only a miss walks the path to build it.
            let parent = if parent_path.is_empty() {
                0
            } else {
                match by_path.get(&fold_upper(parent_path)) {
                    Some(&known) if nodes[known].is_directory => known,
                    _ => Self::ensure_directory(&mut nodes, &mut by_path, parent_path, &mut counts),
                }
            };

            let upper = fold_upper(&key);
            if let Some(&existing) = by_path.get(&upper) {
                // A file whose name a directory already holds is dropped: the directory is
                // reachable and has contents, and the file would make them unreachable.
                if nodes[existing].is_directory {
                    counts.refused += 1;
                    continue;
                }
                // Duplicate keys are legal in a zip. Last wins, and the earlier one is counted
                // rather than left behind as a second row with the same path. It was already
                // counted as a file, so the replacement below must not count twice.
                nodes[parent].children.retain(|&c| c != existing);
                counts.files -= 1;
                counts.refused += 1;
            }

            let slot = nodes.len();
            nodes.push(ArchiveNode {
                path: key.clone(),
                name: name.to_string(),
                is_directory: false,
                size_bytes: entry.size.max(0),
                compressed_bytes: entry.compressed_size.max(0),
                modified: entry.modified,
                is_encrypted: entry.is_encrypted,
                link_target: entry.link_target,
                children: Vec::new(),
            });
            by_path.insert(upper, slot);
            nodes[parent].children.push(slot);
            counts.files += 1;
        }

        Self::total(&mut nodes, 0);
        Self::sort(&mut nodes, 0);

        ArchiveIndex {
            nodes,
            by_path,
            file_count: counts.files,
            capabilities,
            refused_count: counts.refused,
            failure: ArchiveFailure::None,
            error: None,
        }
    }

    /// Normalises an entry key, or None when the entry must not exist at all. Refuses a missing
    /// or blank key, a rooted or UNC key, a drive-qualified key, and any `..` segment. The `..`
    /// case is Zip Slip; the rooted cases are the same attack spelled absolutely, and a container
    /// is equally free to spell it either way.
    pub fn normalize(key: Option<&str>) -> Option<String> {
        let key = key?;
        if key.trim().is_empty() {
            return None;
        }

        let path = key.replace('/', "\\");

        // "\\server\share\x" and "\x" are both rooted; "C:\x" and "C:x" are drive-qualified.
        if path.starts_with('\\') {
            return None;
        }
        if path.as_bytes().get(1) == Some(&b':') {
            return None;
        }

        // The ordinary key needs nothing done to it, and is handed back as it stands.
        if Self::is_already_normal(&path) {
            return Some(path);
        }

        let mut parts: Vec<&str> = Vec::new();
        for segment in path.split('\\') {
            if segment.is_empty() || segment == "." {
                continue;
            }
            if segment == ".." {
                return None;
            }
            // Windows silently drops a trailing dot or space, so a container using them to
            // collide two entries would produce a path that does not round-trip through the
            // filesystem.
            let trimmed = segment.trim_end_matches([' ', '.']);
            if trimmed.is_empty() {
                return None;
            }
            parts.push(trimmed);
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\\"))
        }
    }

    /// True when no segment is empty or ends in a dot or a space, which rules out `.` and `..`
    /// with them, so the loop in `normalize` would rebuild the very same string.
    fn is_already_normal(path: &str) -> bool {
        path.split('\\')
            .all(|segment| !segment.is_empty() && !segment.ends_with([' ', '.']))
    }

    /// Walks a normalised directory path, creating nodes for anything not yet there. Displacing a
    /// file to make room for a directory is a refusal like any other, an entry the container listed
    /// that this app did not keep, and a refusal nobody counts is one the user is never told about.
    fn ensure_directory(
        nodes: &mut Vec<ArchiveNode>,
        by_path: &mut HashMap<String, usize>,
        path: &str,
        counts: &mut Counters,
    ) -> usize {
        if path.is_empty() {
            return 0;
        }

        let mut current = 0usize;
        let mut accumulated = String::new();
        for segment in path.split('\\') {
            if !accumulated.is_empty() {
                accumulated.push('\\');
            }
            accumulated.push_str(segment);
            let upper = fold_upper(&accumulated);

            if let Some(&existing) = by_path.get(&upper) {
                if nodes[existing].is_directory {
                    current = existing;
                    continue;
                }
                // A directory some other entry's path needs, whose name a file already took. The
                // directory wins, for the same reason as above.
                nodes[current].children.retain(|&c| c != existing);
                counts.files -= 1;
                counts.refused += 1;
            }

            let slot = nodes.len();
            nodes.push(ArchiveNode::directory(&accumulated, segment));
            by_path.insert(upper, slot);
            nodes[current].children.push(slot);
            current = slot;
        }
        current
    }

    /// Sums each directory's contents into it, exactly. Nothing is walked on disk to get this:
    /// the numbers were already in the container's own directory, so the app's rule that nothing
    /// scans the filesystem to size a folder is untouched.
    fn total(nodes: &mut [ArchiveNode], index: usize) -> (i64, i64) {
        if !nodes[index].is_directory {
            return (nodes[index].size_bytes, nodes[index].compressed_bytes);
        }
        let children = nodes[index].children.clone();
        let (mut size, mut compressed) = (0i64, 0i64);
        for child in children {
            let (s, c) = Self::total(nodes, child);
            size += s;
            compressed += c;
        }
        nodes[index].size_bytes = size;
        nodes[index].compressed_bytes = compressed;
        (size, compressed)
    }

    fn sort(nodes: &mut [ArchiveNode], index: usize) {
        let mut children = std::mem::take(&mut nodes[index].children);
        children.sort_by_cached_key(|&c| fold_upper(&nodes[c].name));
        for &child in &children {
            Self::sort(nodes, child);
        }
        nodes[index].children = children;
    }
}
