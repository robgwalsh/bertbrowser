//! A disk that is a few hash maps, so the planners can be measured without a disk. Spec:
//! tools/BertBrowser.Bench/Fakes/FakeDisk.cs.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use bb_core::FileTime;
use bb_core::delete::{DeleteProbe, RecycleProbe};
use bb_core::paths::PathKey;
use bb_core::rename::RenameProbe;
use bb_core::transfer::{TransferEntry, TransferEntrySource, TransferProbe};

#[derive(Default)]
struct Inner {
    directories: HashSet<String>,
    files: HashMap<String, i64>,
    children: HashMap<String, Vec<TransferEntry>>,
}

/// Cheap to clone: every clone is the same disk.
#[derive(Clone, Default)]
pub struct FakeDisk {
    inner: Arc<RwLock<Inner>>,
}

fn key(path: &str) -> String {
    PathKey::canonicalize(path).expect("a fake path canonicalizes")
}

fn parent_key(key: &str) -> String {
    bb_core::paths::parent_key(key).unwrap_or_else(|| key.to_string())
}

fn file_name(path: &str) -> &str {
    &path[path.rfind('\\').map_or(0, |i| i + 1)..]
}

const STAMP: FileTime = FileTime(134_024_160_000_000_000);

impl FakeDisk {
    pub fn new() -> FakeDisk {
        FakeDisk::default()
    }

    pub fn add_directory(&self, path: &str) -> &FakeDisk {
        let k = key(path);
        let mut inner = self.inner.write().expect("fake disk");
        if inner.directories.insert(k.clone()) {
            inner
                .children
                .entry(parent_key(&k))
                .or_default()
                .push(TransferEntry {
                    name: file_name(path).to_string(),
                    full_path: path.to_string(),
                    is_directory: true,
                    is_reparse_point: false,
                    size_bytes: 0,
                    modified_utc: STAMP,
                });
        }
        self
    }

    pub fn add_file(&self, path: &str, size: i64) -> &FakeDisk {
        let k = key(path);
        let mut inner = self.inner.write().expect("fake disk");
        if inner.files.insert(k.clone(), size).is_none() {
            inner
                .children
                .entry(parent_key(&k))
                .or_default()
                .push(TransferEntry {
                    name: file_name(path).to_string(),
                    full_path: path.to_string(),
                    is_directory: false,
                    is_reparse_point: false,
                    size_bytes: size,
                    modified_utc: STAMP,
                });
        }
        self
    }

    fn has_directory(&self, path: &str) -> bool {
        self.inner
            .read()
            .expect("fake disk")
            .directories
            .contains(&key(path))
    }

    fn has_file(&self, path: &str) -> bool {
        self.inner
            .read()
            .expect("fake disk")
            .files
            .contains_key(&key(path))
    }
}

impl TransferProbe for FakeDisk {
    fn directory_exists(&self, path: &str) -> bool {
        self.has_directory(path)
    }

    fn file_exists(&self, path: &str) -> bool {
        self.has_file(path)
    }

    fn resolve_final_path(&self, path: &str) -> String {
        path.to_string()
    }
}

impl TransferEntrySource for FakeDisk {
    fn entries(&self, directory: &str) -> Vec<TransferEntry> {
        self.inner
            .read()
            .expect("fake disk")
            .children
            .get(&key(directory))
            .cloned()
            .unwrap_or_default()
    }
}

impl RenameProbe for FakeDisk {
    fn directory_exists(&self, path: &str) -> bool {
        self.has_directory(path)
    }

    fn file_exists(&self, path: &str) -> bool {
        self.has_file(path)
    }
}

impl DeleteProbe for FakeDisk {
    fn directory_exists(&self, path: &str) -> bool {
        self.has_directory(path)
    }

    fn file_exists(&self, path: &str) -> bool {
        self.has_file(path)
    }
}

impl RecycleProbe for FakeDisk {
    fn can_recycle(&self, _path: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_answers_for_what_was_added() {
        let disk = FakeDisk::new();
        disk.add_directory("Q:\\Bench\\Src")
            .add_file("Q:\\Bench\\Src\\a.txt", 5);
        assert!(TransferProbe::directory_exists(&disk, "q:\\bench\\src"));
        assert!(TransferProbe::file_exists(&disk, "Q:\\Bench\\Src\\A.TXT"));
        assert_eq!(disk.entries("Q:\\Bench\\Src").len(), 1);
        assert!(disk.entries("Q:\\Elsewhere").is_empty());
    }
}
