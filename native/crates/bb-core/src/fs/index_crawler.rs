//! Builds the `fs_entry` search index for a subtree. Spec: Core/Services/IndexCrawler.cs.
//!
//! Entries are written in bounded chunks (one transaction each) so a multi-million-entry crawl
//! never buffers the whole tree in memory and never starves other writers with one giant
//! transaction. A cancelled crawl leaves any rows it already wrote (harmless, they carry real data)
//! but never sweeps or marks the root complete.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use bb_paths::PathKey;

use super::walker::{FileSystemWalker, WalkError};
use crate::FileTime;
use crate::cancel::CancelToken;
use crate::data::{self, FsEntryRow, FsIndexRepository};

const CHUNK_SIZE: usize = 20_000;

/// How many crawls run at once.
const CONCURRENCY: usize = 2;

pub struct IndexCrawler {
    repository: Arc<FsIndexRepository>,
    slots: Mutex<usize>,
    freed: Condvar,
}

impl IndexCrawler {
    pub fn new(repository: Arc<FsIndexRepository>) -> IndexCrawler {
        IndexCrawler {
            repository,
            slots: Mutex::new(CONCURRENCY),
            freed: Condvar::new(),
        }
    }

    /// Crawls `root` into the index, blocking the caller. `Ok(true)` when the crawl completed and
    /// the root was registered as complete and fresh; `Ok(false)` when it was cancelled, before or
    /// during. `progress` is told the running entry count after each chunk.
    pub fn crawl(
        &self,
        root: &str,
        cancel: &CancelToken,
        progress: Option<&dyn Fn(usize)>,
    ) -> data::Result<bool> {
        if !self.acquire(cancel) {
            return Ok(false);
        }
        let result = self.run(root, cancel, progress);
        self.release();
        match result {
            Ok(completed) => Ok(completed),
            Err(data::Error::Cancelled) => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn run(
        &self,
        root: &str,
        cancel: &CancelToken,
        progress: Option<&dyn Fn(usize)>,
    ) -> data::Result<bool> {
        let root_key = PathKey::canonicalize(root)?;
        let root_display = PathKey::normalize_display(root)?;
        let crawled_utc = FileTime::now();
        let crawl_gen = FileTime::now().to_unix_millis();

        let mut buffer: Vec<FsEntryRow> = Vec::with_capacity(CHUNK_SIZE);
        let mut total = 0usize;
        let mut failure: Option<data::Error> = None;

        // Index everything with its hidden flag; queries filter.
        let walked = FileSystemWalker::walk(root, true, false, cancel, |entry| {
            buffer.push(FsEntryRow {
                path_key: entry.path_key.clone(),
                name: entry.name.clone(),
                is_directory: entry.is_directory,
                size_bytes: entry.size_bytes,
                modified_utc: entry.modified_utc,
                hidden: entry.hidden,
                attributes: entry.attributes,
                created_utc: entry.created_utc,
            });
            if buffer.len() >= CHUNK_SIZE {
                if let Err(e) = self.repository.upsert_entries(&buffer, crawl_gen) {
                    failure = Some(e);
                    return false;
                }
                total += buffer.len();
                buffer.clear();
                if let Some(report) = progress {
                    report(total);
                }
            }
            true
        });
        if let Some(e) = failure {
            return Err(e);
        }
        match walked {
            Ok(()) => {}
            Err(WalkError::Cancelled) => return Err(data::Error::Cancelled),
            Err(WalkError::Path(e)) => return Err(data::Error::Path(e)),
        }

        self.repository.upsert_entries(&buffer, crawl_gen)?;
        total += buffer.len();

        self.repository.sweep_vanished(&root_key, crawl_gen)?;
        self.repository
            .upsert_root(&root_key, &root_display, crawled_utc, true)?;
        if let Some(report) = progress {
            report(total);
        }
        Ok(true)
    }

    /// Waits for a crawl slot; false when cancelled first.
    fn acquire(&self, cancel: &CancelToken) -> bool {
        let mut slots = self.slots.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if cancel.is_cancelled() {
                return false;
            }
            if *slots > 0 {
                *slots -= 1;
                return true;
            }
            slots = self
                .freed
                .wait_timeout(slots, Duration::from_millis(50))
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }

    fn release(&self) {
        *self.slots.lock().unwrap_or_else(|p| p.into_inner()) += 1;
        self.freed.notify_one();
    }
}
