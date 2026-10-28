//! What an index service knows: which volumes are still building and which have finished, and the
//! one status line that follows. Separate from the service because two implementations answer the
//! same questions (the real indexer and the client mirroring one in another process); both must
//! answer `is_indexed` and word `status_text` identically, or search routing and the status bar
//! disagree depending on which is wired up. Building is keyed by drive letter (what the caller
//! works through) and completion by root key (what callers ask about). Spec:
//! Core/Services/Mft/MftIndexState.cs.

use std::collections::BTreeSet;
use std::sync::Mutex;

use bb_paths::PathKey;

#[derive(Default)]
pub struct MftIndexState {
    completed_roots: Mutex<BTreeSet<String>>,
    building: Mutex<BTreeSet<String>>,
}

impl MftIndexState {
    pub fn new() -> MftIndexState {
        MftIndexState::default()
    }

    fn completed(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.completed_roots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    fn building_set(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.building.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// True once at least one volume's initial enumeration has completed.
    pub fn any_indexed(&self) -> bool {
        !self.completed().is_empty()
    }

    /// True while any volume's initial enumeration is still running.
    pub fn is_building(&self) -> bool {
        !self.building_set().is_empty()
    }

    /// True if `path_key` sits on a volume whose index is complete. Through `PathKey::is_under`
    /// rather than a prefix test: a root of `C:\FOO` must not claim `C:\FOOBAR`.
    pub fn is_indexed(&self, path_key: &str) -> bool {
        self.completed()
            .iter()
            .any(|root| path_key == root || PathKey::is_under(path_key, root).unwrap_or(false))
    }

    /// Every root that has finished building, for a client re-sending its state.
    pub fn completed_roots(&self) -> Vec<String> {
        self.completed().iter().cloned().collect()
    }

    /// The bare drive letters currently building, for a host relaying its state.
    pub fn building_drives(&self) -> Vec<String> {
        self.building_set().iter().cloned().collect()
    }

    /// Marks `drive` (a bare letter) as building.
    pub fn mark_building(&self, drive: &str) {
        self.building_set().insert(drive.to_string());
    }

    /// Marks `drive` as no longer building, whether it finished or failed.
    pub fn clear_building(&self, drive: &str) {
        self.building_set().remove(drive);
    }

    /// Marks `root_key` as fully indexed.
    pub fn mark_complete(&self, root_key: &str) {
        self.completed().insert(root_key.to_string());
    }

    /// Forgets everything: what a client does when it loses its indexer.
    pub fn clear(&self) {
        self.completed().clear();
        self.building_set().clear();
    }

    /// The status-bar line for the current building set; empty when nothing is building. Ordered,
    /// so the line does not shuffle as threads finish in whatever order.
    pub fn format_status(&self) -> String {
        let building: Vec<String> = self.building_set().iter().cloned().collect();
        match building.len() {
            0 => String::new(),
            1 => format!("Indexing {}:…", building[0]),
            _ => format!(
                "Indexing {}…",
                building
                    .iter()
                    .map(|d| format!("{d}:"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}
