//! Rolls per-file sizes up the directory tree to produce a `DirSizeResult` for every directory on
//! the volume, the instant-folder-size trick. Fed the same `MftReader` stream that builds the
//! search index, it buckets each file onto its direct parent, then does one iterative post-order
//! pass from the root record (5) summing subtree bytes / file / dir counts. Reparse-point junctions
//! have no MFT children under their own record, so they contribute nothing, matching the DFS
//! scanner's deliberate skip. Because the raw read sees every record regardless of ACLs, results
//! are never `incomplete`. Spec: Core/Services/Mft/MftDirectorySizeBuilder.cs.

use std::collections::{HashMap, HashSet};

use super::ntfs_layout::NtfsLayout;
use super::path_builder::{DirCache, MftNode, MftPathBuilder};
use super::reader::MftFileRecord;
use crate::FileTime;
use crate::data::DirSizeResult;
use bb_paths::fold_upper;

#[derive(Default)]
pub struct MftDirectorySizeBuilder {
    dir_nodes: HashMap<u64, MftNode>,
    direct_bytes: HashMap<u64, i64>,
    direct_files: HashMap<u64, i32>,
    child_dirs: HashMap<u64, Vec<u64>>,
}

/// What a folder with no subfolders has. Shared and never added to.
const NO_CHILDREN: &[u64] = &[];

impl MftDirectorySizeBuilder {
    pub fn new() -> MftDirectorySizeBuilder {
        MftDirectorySizeBuilder::default()
    }

    pub fn add(&mut self, rec: &MftFileRecord) {
        if rec.is_directory {
            self.dir_nodes.insert(
                rec.record_number,
                MftNode::new(&rec.name, rec.parent_record_number, true, rec.attributes),
            );
            self.child_dirs
                .entry(rec.parent_record_number)
                .or_default()
                .push(rec.record_number);
        } else {
            *self
                .direct_bytes
                .entry(rec.parent_record_number)
                .or_insert(0) += rec.size;
            *self
                .direct_files
                .entry(rec.parent_record_number)
                .or_insert(0) += 1;
        }
    }

    /// Every directory's subtree totals, one `DirSizeResult` per directory. `dir_cache` is filled
    /// with each directory's resolved display path (and effective-hidden state) as a side effect:
    /// the caller reuses it both to build the file rows of the search index and to seed the USN
    /// tail's resident directory map.
    pub fn build(
        &self,
        drive_root: &str,
        computed_utc: FileTime,
        dir_cache: &mut DirCache,
    ) -> Vec<DirSizeResult> {
        let mut subtree_bytes: HashMap<u64, i64> = HashMap::with_capacity(self.dir_nodes.len());
        let mut subtree_files: HashMap<u64, i32> = HashMap::with_capacity(self.dir_nodes.len());
        let mut subtree_dirs: HashMap<u64, i32> = HashMap::with_capacity(self.dir_nodes.len());
        let mut results = Vec::with_capacity(self.dir_nodes.len());

        let children = |frn: u64| -> &[u64] {
            self.child_dirs
                .get(&frn)
                .map(Vec::as_slice)
                .unwrap_or(NO_CHILDREN)
        };

        let mut seen: HashSet<u64> = HashSet::new();
        let mut done: HashSet<u64> = HashSet::new();
        let mut stack: Vec<(u64, bool)> = Vec::new();
        for &top in children(NtfsLayout::ROOT_RECORD_NUMBER) {
            if seen.insert(top) {
                stack.push((top, false));
            }
        }

        while let Some((frn, expanded)) = stack.pop() {
            if !expanded {
                stack.push((frn, true));
                for &child in children(frn) {
                    if seen.insert(child) {
                        stack.push((child, false));
                    }
                }
                continue;
            }
            if !done.insert(frn) {
                continue;
            }

            let mut bytes = self.direct_bytes.get(&frn).copied().unwrap_or(0);
            let mut files = self.direct_files.get(&frn).copied().unwrap_or(0);
            let mut dirs = 0i32;
            for child in children(frn) {
                bytes += subtree_bytes.get(child).copied().unwrap_or(0);
                files += subtree_files.get(child).copied().unwrap_or(0);
                dirs += subtree_dirs.get(child).copied().unwrap_or(0) + 1;
            }
            subtree_bytes.insert(frn, bytes);
            subtree_files.insert(frn, files);
            subtree_dirs.insert(frn, dirs);

            if let Some((display, _)) = MftPathBuilder::try_resolve(
                &self.dir_nodes,
                frn,
                drive_root,
                dir_cache,
                NtfsLayout::ROOT_RECORD_NUMBER,
            ) {
                results.push(DirSizeResult {
                    path_key: fold_upper(&display),
                    size_bytes: bytes,
                    file_count: files,
                    dir_count: dirs,
                    incomplete: false,
                    computed_utc,
                });
            }
        }

        results
    }
}
