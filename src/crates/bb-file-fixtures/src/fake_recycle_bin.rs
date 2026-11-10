//! A Recycle Bin that really moves files, for the delete executor's tests and the undo history's.

use std::cell::Cell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bb_base::cancel::CancelToken;
use bb_files::delete::{
    DeleteProgress, DeletedItem, PlannedDelete, RecycleBin, RecycleProbe, RecycleResult,
    RecycledItem,
};

/// A stand-in for the Windows Recycle Bin that really moves files, so tests can assert on contents.
/// It models the three behaviours the executor has to cope with: a volume the bin does not serve,
/// an item the shell erases instead of holding, and a bin that has been emptied.
pub struct FakeRecycleBin {
    bin_root: PathBuf,
    no_bin: HashSet<String>,
    erases: HashSet<String>,
    next: Cell<usize>,
}

impl FakeRecycleBin {
    pub fn new(bin_root: PathBuf) -> FakeRecycleBin {
        FakeRecycleBin {
            bin_root,
            no_bin: HashSet::new(),
            erases: HashSet::new(),
            next: Cell::new(0),
        }
    }

    /// Makes `path` look like it lives on a share or on media with the bin turned off.
    pub fn refuse_volume_for(&mut self, path: &str) {
        self.no_bin.insert(path.to_uppercase());
    }

    /// Makes the bin accept `path` but erase it — what the shell does with an item larger than
    /// the bin's quota.
    pub fn erase_instead_of_holding(&mut self, path: &str) {
        self.erases.insert(path.to_uppercase());
    }

    pub fn empty(&self) {
        if self.bin_root.is_dir() {
            std::fs::remove_dir_all(&self.bin_root).unwrap();
        }
    }
}

impl RecycleProbe for FakeRecycleBin {
    fn can_recycle(&self, path: &str) -> bool {
        !self.no_bin.contains(&path.to_uppercase())
    }
}

impl RecycleBin for FakeRecycleBin {
    fn recycle(
        &self,
        items: &[PlannedDelete],
        _cancel: &CancelToken,
        progress: Option<&dyn Fn(DeleteProgress)>,
    ) -> RecycleResult {
        std::fs::create_dir_all(&self.bin_root).unwrap();
        let mut recycled = Vec::new();

        for (done, item) in items.iter().enumerate() {
            if let Some(report) = progress {
                report(DeleteProgress {
                    done,
                    total: items.len(),
                    current_name: item.name().to_string(),
                });
            }

            if self.erases.contains(&item.source_path.to_uppercase()) {
                if item.is_directory {
                    std::fs::remove_dir_all(&item.source_path).unwrap();
                } else {
                    std::fs::remove_file(&item.source_path).unwrap();
                }
                recycled.push(RecycledItem {
                    source_path: item.source_path.clone(),
                    is_directory: item.is_directory,
                    recycled_path: None,
                });
                continue;
            }

            // The real bin renames to $R<id><ext>; the shape matters more than the exact name.
            let n = self.next.get();
            self.next.set(n + 1);
            let name = item.name();
            let extension = name.rfind('.').map_or("", |dot| &name[dot..]);
            let target = self.bin_root.join(format!("$R{n:06}{extension}"));
            std::fs::rename(&item.source_path, &target).unwrap();
            recycled.push(RecycledItem {
                source_path: item.source_path.clone(),
                is_directory: item.is_directory,
                recycled_path: Some(target.to_string_lossy().into_owned()),
            });
        }

        RecycleResult {
            recycled,
            failed: Vec::new(),
        }
    }

    fn restore(&self, item: &DeletedItem) -> bool {
        let Some(held) = &item.recycled_path else {
            return false;
        };
        let present = if item.is_directory {
            Path::new(held).is_dir()
        } else {
            Path::new(held).is_file()
        };
        if !present {
            return false;
        }
        std::fs::rename(held, &item.source_path).unwrap();
        true
    }
}
