//! The last few archives read, so walking around inside one does not re-read its directory on
//! every step. Spec: Core/Services/Archives/ArchiveCache.cs.
//!
//! **Keyed by canonical path plus length plus last-write time**: a listing that outlived an edit is
//! something the user then extracts from, so a rewritten archive gets a new key and is read again.
//! Bounded two ways because either alone fails: eight entries would hold a gigabyte if each were a
//! 200,000-entry index, and a byte bound alone would hold thousands of tiny ones. Eviction is
//! least-recently-used by a counter rather than a clock, so the order cannot depend on how fast the
//! machine is. A password is part of the key only in as much as an index read *with* one supersedes
//! one read without: unlocking re-reads and replaces. The password itself is never stored here.

use std::collections::HashMap;
use std::os::windows::fs::MetadataExt;
use std::sync::{Arc, Mutex};

use bb_paths::PathKey;

use super::{ArchiveFailure, ArchiveIndex};

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    path: String,
    length: u64,
    write: i64,
}

struct Slot {
    index: Arc<ArchiveIndex>,
    weight: i64,
    unlocked: bool,
    used: i64,
}

struct State {
    slots: HashMap<Key, Slot>,
    clock: i64,
    weight: i64,
}

pub struct ArchiveCache {
    max_entries: usize,
    max_weight: i64,
    state: Mutex<State>,
}

impl Default for ArchiveCache {
    fn default() -> Self {
        ArchiveCache::new(8, 64 * 1024 * 1024)
    }
}

impl ArchiveCache {
    pub fn new(max_entries: usize, max_weight: i64) -> ArchiveCache {
        ArchiveCache {
            max_entries,
            max_weight,
            state: Mutex::new(State {
                slots: HashMap::new(),
                clock: 0,
                weight: 0,
            }),
        }
    }

    /// The index for `archive_file`, reading it through `read` only when nothing usable is cached.
    /// Returns a failure index rather than panicking when the file cannot even be stat-ed.
    pub fn get(
        &self,
        archive_file: &str,
        password: Option<&str>,
        read: impl FnOnce(&str, Option<&str>) -> ArchiveIndex,
    ) -> Arc<ArchiveIndex> {
        let key = match std::fs::metadata(archive_file) {
            Ok(meta) if !meta.is_dir() => Key {
                path: PathKey::canonicalize(archive_file)
                    .unwrap_or_else(|_| archive_file.to_string()),
                length: meta.len(),
                write: meta.last_write_time() as i64,
            },
            Ok(_) => {
                return Arc::new(ArchiveIndex::failed(
                    ArchiveFailure::Unreadable,
                    "The archive is no longer there.",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Arc::new(ArchiveIndex::failed(
                    ArchiveFailure::Unreadable,
                    "The archive is no longer there.",
                ));
            }
            Err(_) => {
                return Arc::new(ArchiveIndex::failed(
                    ArchiveFailure::Unreadable,
                    "The archive could not be opened.",
                ));
            }
        };

        let unlocked = password.is_some();

        {
            let mut state = self.state.lock().unwrap();
            // A cached index read without a password is not good enough once one has been given:
            // that is exactly the re-read unlocking is asking for.
            let next = state.clock + 1;
            let State { slots, clock, .. } = &mut *state;
            if let Some(hit) = slots.get_mut(&key)
                && (hit.unlocked || !unlocked)
            {
                hit.used = next;
                *clock = next;
                return hit.index.clone();
            }
        }

        let index = Arc::new(read(archive_file, password));

        // A failure is not cached. Re-reading a broken archive costs one open, and caching the
        // failure would make a transient one, a file still being written, stick until it changed.
        if !index.ok() {
            return index;
        }

        let mut state = self.state.lock().unwrap();
        if let Some(replaced) = state.slots.remove(&key) {
            state.weight -= replaced.weight;
        }
        let weight = Self::weigh(&index);
        state.clock += 1;
        let used = state.clock;
        state.slots.insert(
            key,
            Slot {
                index: index.clone(),
                weight,
                unlocked,
                used,
            },
        );
        state.weight += weight;
        self.evict(&mut state);
        index
    }

    /// Forgets everything. Used when the app can no longer vouch for what it holds.
    pub fn clear(&self) {
        let mut state = self.state.lock().unwrap();
        state.slots.clear();
        state.weight = 0;
    }

    fn evict(&self, state: &mut State) {
        while state.slots.len() > self.max_entries
            || (state.weight > self.max_weight && state.slots.len() > 1)
        {
            let oldest = state
                .slots
                .iter()
                .min_by_key(|(_, slot)| slot.used)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else {
                break;
            };
            if let Some(gone) = state.slots.remove(&oldest) {
                state.weight -= gone.weight;
            }
        }
    }

    /// A rough byte cost: node count times a per-node estimate. The point is to be proportional,
    /// not exact; an exact figure would cost more to compute than it saves.
    fn weigh(index: &ArchiveIndex) -> i64 {
        index.len() as i64 * 256
    }
}
