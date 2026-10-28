//! Reconstructs full paths from the flat FRN → node map an MFT enumeration produces. The records
//! only give each entry its own name plus its parent's reference, so a path is the chain of names
//! from the entry up to the NTFS root. Directory resolutions are memoised in a caller-supplied
//! cache, so a bulk pass over a volume is ~O(entries) rather than O(entries × depth). Effective
//! hidden mirrors the walker: an entry is hidden if it or any ancestor below the root is. Spec:
//! Core/Services/Mft/MftPathBuilder.cs.

use std::collections::HashMap;

use crate::attributes::HIDDEN;

/// One MFT entry as it lives in the in-memory reference map. `attributes` is the whole mask rather
/// than the one bit path building needs, because the names-only USN build has no other source for
/// it: `fs_entry.attributes` comes from here on that path, which is what makes `is:readonly` work
/// on a volume with no sizes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MftNode {
    pub name: String,
    pub parent_frn: u64,
    pub is_directory: bool,
    pub attributes: u32,
}

impl MftNode {
    pub fn new(name: &str, parent_frn: u64, is_directory: bool, attributes: u32) -> MftNode {
        MftNode {
            name: name.to_string(),
            parent_frn,
            is_directory,
            attributes,
        }
    }

    /// The entry's own Hidden bit; the effective one is computed up the chain.
    pub fn own_hidden(&self) -> bool {
        self.attributes & HIDDEN != 0
    }
}

/// A directory's resolved display path and effective-hidden state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedDir {
    pub path: String,
    pub hidden: bool,
}

/// Directory record number → resolved path, built by the initial pass and kept by the tail.
pub type DirCache = HashMap<u64, ResolvedDir>;

pub struct MftPathBuilder;

impl MftPathBuilder {
    /// Guards against cycles from a corrupt or racing table.
    const MAX_DEPTH: usize = 512;

    /// The NTFS root's full file reference, the default `root_frn` of the C# overload.
    pub const NTFS_ROOT: u64 = 0x0005_0000_0000_0005;

    /// The display path and effective-hidden state of `frn`, or None when the parent chain is
    /// broken (a missing link, common for entries whose ancestor was deleted mid-enumeration) or
    /// exceeds `MAX_DEPTH`. `dir_cache` is read and populated for directory ancestors.
    pub fn try_resolve(
        map: &HashMap<u64, MftNode>,
        frn: u64,
        drive_root: &str,
        dir_cache: &mut DirCache,
        root_frn: u64,
    ) -> Option<(String, bool)> {
        if frn == root_frn {
            return Some((drive_root.to_string(), false));
        }
        let node = map.get(&frn)?;

        // Walk up collecting the ancestors to compute, stopping at the root or the first cached
        // directory, whichever comes first.
        let mut pending: Vec<(u64, &MftNode)> = Vec::new();
        let mut current = node;
        let mut current_frn = frn;
        let mut base_path = drive_root.to_string();
        let mut base_hidden = false;

        let mut depth = 0usize;
        loop {
            if depth > Self::MAX_DEPTH {
                return None;
            }
            depth += 1;
            pending.push((current_frn, current));

            let parent_frn = current.parent_frn;
            if parent_frn == root_frn || parent_frn == current_frn {
                break; // reached the volume root; base_path stays drive_root
            }
            if let Some(cached) = dir_cache.get(&parent_frn) {
                base_path = cached.path.clone();
                base_hidden = cached.hidden;
                break;
            }
            let parent = map.get(&parent_frn)?; // broken chain
            current = parent;
            current_frn = parent_frn;
        }

        // Unwind from the ancestor nearest the base down to the target, building each path once
        // and caching directory results for later siblings.
        let mut path = base_path;
        let mut effective_hidden = base_hidden;
        for (item_frn, item) in pending.iter().rev() {
            path = combine(&path, &item.name);
            effective_hidden |= item.own_hidden();
            if item.is_directory {
                dir_cache.insert(
                    *item_frn,
                    ResolvedDir {
                        path: path.clone(),
                        hidden: effective_hidden,
                    },
                );
            }
        }
        Some((path, effective_hidden))
    }
}

/// `dir + name`, with one separator between whatever the root ends in.
pub(crate) fn combine(dir: &str, name: &str) -> String {
    if dir.ends_with('\\') {
        format!("{dir}{name}")
    } else {
        format!("{dir}\\{name}")
    }
}
