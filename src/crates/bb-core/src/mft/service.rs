//! Owns one `MftVolumeIndexer` per fixed NTFS volume: enumerates the drives, builds each index on
//! its own thread, and keeps that thread tailing the volume's USN journal for the life of the
//! process. The primary index producer; the lazy crawler remains only for roots no NTFS volume
//! covers. Runs inside the elevated helper (`bb-indexer`), never the app. Spec:
//! Core/Services/Mft/MftIndexService.cs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::presence::IndexerPresence;
use super::state::MftIndexState;
use super::volume_indexer::{MftVolumeIndexer, VolumeAccess};
use super::{IndexListener, IndexListeners, IndexStartMode, MftIndex};
use crate::FileTime;
use crate::cancel::CancelToken;
use crate::changes::{ChangeLogPolicy, ChangeRecorder};
use crate::data::{ChangeLogRepository, DirSizeRepository, FsIndexRepository};

/// Where the tail may write a change log: the repository, and the data directory's key, which it
/// never records (or every DB write would log itself for ever).
#[derive(Clone)]
pub struct ChangeRecorderOptions {
    pub repository: Arc<ChangeLogRepository>,
    pub excluded_root_key: String,
}

struct Inner {
    repository: Arc<FsIndexRepository>,
    dir_size_repository: Arc<DirSizeRepository>,
    volumes: Arc<dyn VolumeAccess>,
    changes: Option<ChangeRecorderOptions>,
    lifetime: CancelToken,
    state: MftIndexState,
    /// Read by each volume's recorder once per flush, so a change takes effect on the next poll.
    change_log: Mutex<ChangeLogPolicy>,
    status_text: Mutex<String>,
    started: AtomicBool,
    listeners: IndexListeners,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

pub struct MftIndexService {
    inner: Arc<Inner>,
}

impl MftIndexService {
    /// `changes` is where the tail may write a change log, or None for a host that has none to
    /// offer. Whether it actually writes is `change_log`, off until set.
    pub fn new(
        repository: Arc<FsIndexRepository>,
        dir_size_repository: Arc<DirSizeRepository>,
        volumes: Arc<dyn VolumeAccess>,
        changes: Option<ChangeRecorderOptions>,
    ) -> MftIndexService {
        MftIndexService {
            inner: Arc::new(Inner {
                repository,
                dir_size_repository,
                volumes,
                changes,
                lifetime: CancelToken::new(),
                state: MftIndexState::new(),
                change_log: Mutex::new(ChangeLogPolicy::OFF),
                status_text: Mutex::new(String::new()),
                started: AtomicBool::new(false),
                listeners: IndexListeners::new(),
                threads: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Ends every volume thread and waits for them: what the helper does on its way out, and
    /// what the self-test does to see the tail drain.
    pub fn shutdown(&self) {
        self.inner.lifetime.cancel();
        let threads: Vec<JoinHandle<()>> =
            std::mem::take(&mut *self.inner.threads.lock().unwrap_or_else(|p| p.into_inner()));
        for thread in threads {
            let _ = thread.join();
        }
    }
}

impl Drop for MftIndexService {
    fn drop(&mut self) {
        // The threads are background ones: cancelled, not waited for, as the C# Dispose did.
        self.inner.lifetime.cancel();
    }
}

impl Inner {
    fn run_volume(self: &Arc<Inner>, mut indexer: MftVolumeIndexer, drive: &str) {
        let cancel = self.lifetime.clone();
        if indexer.open(self.volumes.as_ref()) {
            self.state.mark_building(drive);
            self.update_status();

            match indexer.build_initial_index(&cancel) {
                Ok(()) if !cancel.is_cancelled() => {
                    self.state.mark_complete(indexer.root_key());
                    self.state.clear_building(drive);
                    self.update_status();
                    self.listeners.index_refreshed(indexer.root_key());
                    indexer.tail(&cancel);
                }
                // A volume that fails to index is non-fatal: searches on it fall back to a crawl.
                _ => {}
            }
        }
        self.state.clear_building(drive);
        self.update_status();
    }

    fn update_status(&self) {
        *self.status_text.lock().unwrap_or_else(|p| p.into_inner()) = self.state.format_status();
        self.listeners.status_changed();
    }
}

impl MftIndex for MftIndexService {
    /// There is no separate helper to leave running or not, so the mode says nothing here.
    fn start_with(&self, _mode: IndexStartMode) -> Result<(), String> {
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        for drive in self.inner.volumes.ntfs_volumes() {
            let recorder = self.inner.changes.as_ref().map(|changes| {
                let policy_source = self.inner.clone();
                ChangeRecorder::new(
                    changes.repository.clone(),
                    changes.excluded_root_key.clone(),
                    move || {
                        *policy_source
                            .change_log
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                    },
                    FileTime::now,
                )
            });
            let indexer = MftVolumeIndexer::new(
                self.inner.repository.clone(),
                self.inner.dir_size_repository.clone(),
                &drive,
                recorder,
            );
            let inner = self.inner.clone();
            let name = drive.clone();
            let thread = std::thread::Builder::new()
                .name(format!("mft-index-{drive}"))
                .spawn(move || inner.run_volume(indexer, &name))
                .map_err(|e| e.to_string())?;
            self.inner
                .threads
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(thread);
        }
        Ok(())
    }

    /// Nothing to stop: this indexer ends with the process hosting it.
    fn stop(&self) {}

    fn any_indexed(&self) -> bool {
        self.inner.state.any_indexed()
    }

    fn is_building(&self) -> bool {
        self.inner.state.is_building()
    }

    fn building_drives(&self) -> Vec<String> {
        self.inner.state.building_drives()
    }

    fn completed_roots(&self) -> Vec<String> {
        self.inner.state.completed_roots()
    }

    /// This *is* the indexer, so there is no helper to be present or absent.
    fn presence(&self) -> IndexerPresence {
        IndexerPresence::NotApplicable
    }

    fn is_indexed(&self, path_key: &str) -> bool {
        self.inner.state.is_indexed(path_key)
    }

    fn status_text(&self) -> String {
        self.inner
            .status_text
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Always false: the in-process indexer needs nothing the user could grant it.
    fn can_retry(&self) -> bool {
        false
    }

    fn can_start(&self) -> bool {
        false
    }

    fn retry(&self) {
        let _ = self.start();
    }

    fn change_log(&self) -> ChangeLogPolicy {
        *self
            .inner
            .change_log
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    fn set_change_log(&self, policy: ChangeLogPolicy) {
        *self
            .inner
            .change_log
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = policy;
    }

    fn subscribe(&self, listener: Arc<dyn IndexListener>) -> u64 {
        self.inner.listeners.subscribe(listener)
    }

    fn unsubscribe(&self, id: u64) {
        self.inner.listeners.unsubscribe(id);
    }
}
