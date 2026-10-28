//! Indexes one NTFS volume and keeps it live via the USN change journal. The initial pass reads
//! the raw `$MFT` (`MftReader`) to get names, parents, sizes and timestamps in one sweep,
//! populating the `fs_entry` search index *and* a full `dir_size_cache` roll-up so every folder
//! shows its size instantly. If the raw read cannot parse the volume it falls back to the names-only
//! USN enumeration (no sizes; the volume is then *unmeasured*, and nothing fills the gap in on
//! demand). `tail` then applies create / delete / rename records as they happen. Spec:
//! Core/Services/Mft/MftVolumeIndexer.cs, Core/Interop/NtfsNative.cs.
//!
//! Everything is keyed by 48-bit MFT record numbers (root = 5). Journal references are the same
//! number in their low 48 bits, so the tail masks them to look up the resident directory map.
//!
//! The volume is a `VolumeHandle`: the elevated FSCTLs sit behind it (`RawVolume`), and the fake in
//! `fixtures` drives this whole type without a token.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bb_paths::{PathKey, fold_upper};

use super::dir_size_builder::MftDirectorySizeBuilder;
use super::ntfs_layout::{NtfsLayout, u64_at};
use super::path_builder::{DirCache, MftNode, MftPathBuilder, ResolvedDir, combine};
use super::reader::{MftFileRecord, MftReader, ReadError, VolumeRead};
use super::usn_reason;
use super::usn_record::{UsnRecord, UsnRecordParser};
use crate::FileTime;
use crate::cancel::{CancelToken, Cancelled};
use crate::changes::{ChangeEvent, ChangeKind, ChangeLogRules, ChangeRecorder};
use crate::data::{DirSizeRepository, FsEntryRow, FsIndexRepository};

/// What FSCTL_QUERY_USN_JOURNAL answers, the three fields the indexer keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalData {
    pub journal_id: u64,
    pub next_usn: i64,
    pub max_usn: i64,
}

/// One open volume: raw reads for the `$MFT`, and the three USN FSCTLs. Every call needs an
/// administrator token on the real thing, which is why it is a trait.
pub trait VolumeHandle: VolumeRead + Send {
    /// The journal, creating one when the volume has none. None when it cannot be had.
    fn query_journal(&mut self) -> Option<JournalData>;

    /// One FSCTL_ENUM_USN_DATA call from `start_frn`: the bytes written into `buffer` (cursor
    /// first), or None at the end of the table or on error.
    fn enum_usn_data(&mut self, start_frn: u64, high_usn: i64, buffer: &mut [u8]) -> Option<usize>;

    /// One FSCTL_READ_USN_JOURNAL call from `start_usn`, not waiting: the bytes written (next USN
    /// first), or None on journal loss.
    fn read_usn_journal(
        &mut self,
        start_usn: i64,
        journal_id: u64,
        buffer: &mut [u8],
    ) -> Option<usize>;
}

/// Finds and opens volumes: the fixed NTFS ones on a real machine, or whatever a fake offers.
pub trait VolumeAccess: Send + Sync {
    /// Bare drive letters of the fixed NTFS volumes.
    fn ntfs_volumes(&self) -> Vec<String>;

    /// Opens a volume raw. None when it cannot be opened (not elevated, or not a volume).
    fn open(&self, drive_letter: &str) -> Option<Box<dyn VolumeHandle>>;
}

/// Why the initial build stopped.
#[derive(Debug)]
pub enum BuildError {
    Cancelled,
    /// A repository write failed; the volume is left as it was.
    Database(crate::data::Error),
}

impl From<Cancelled> for BuildError {
    fn from(_: Cancelled) -> BuildError {
        BuildError::Cancelled
    }
}

impl From<crate::data::Error> for BuildError {
    fn from(e: crate::data::Error) -> BuildError {
        BuildError::Database(e)
    }
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Cancelled => f.write_str("cancelled"),
            BuildError::Database(e) => write!(f, "{e}"),
        }
    }
}

pub struct MftVolumeIndexer {
    repository: Arc<FsIndexRepository>,
    dir_size_repository: Arc<DirSizeRepository>,
    recorder: Option<ChangeRecorder>,
    drive_letter: String,
    /// "C:\".
    drive_root: String,
    /// Canonical "C:\".
    root_key: String,
    handle: Option<Box<dyn VolumeHandle>>,
    journal_id: u64,
    next_usn: i64,
    max_usn: i64,
    /// Directory record number → resolved path: built by the initial pass, kept by the tail, the
    /// source of truth for resolving change paths.
    dirs: DirCache,
    /// Old paths captured from RENAME_OLD_NAME records, keyed by record number.
    pending_renames: HashMap<u64, String>,
    /// How long an idle tail sleeps between polls.
    poll_interval: Duration,
}

impl MftVolumeIndexer {
    const ENUM_BUFFER_SIZE: usize = 1 << 20; // 1 MB
    const TAIL_BUFFER_SIZE: usize = 64 * 1024;
    const UPSERT_CHUNK: usize = 20_000;
    const TAIL_POLL_INTERVAL: Duration = Duration::from_millis(1000);

    /// `recorder` is where the tail notes each change it applies, for the change timeline; None
    /// records nothing. It sees exactly the records the index acts on.
    pub fn new(
        repository: Arc<FsIndexRepository>,
        dir_size_repository: Arc<DirSizeRepository>,
        drive_letter: &str,
        recorder: Option<ChangeRecorder>,
    ) -> MftVolumeIndexer {
        let drive_root = format!("{drive_letter}:\\");
        MftVolumeIndexer {
            repository,
            dir_size_repository,
            recorder,
            drive_letter: drive_letter.to_string(),
            root_key: fold_upper(&drive_root),
            drive_root,
            handle: None,
            journal_id: 0,
            next_usn: 0,
            max_usn: 0,
            dirs: HashMap::new(),
            pending_renames: HashMap::new(),
            poll_interval: Self::TAIL_POLL_INTERVAL,
        }
    }

    /// A faster idle poll, for the self-test; the real cadence is a second.
    pub fn with_poll_interval(mut self, interval: Duration) -> MftVolumeIndexer {
        self.poll_interval = interval;
        self
    }

    pub fn root_key(&self) -> &str {
        &self.root_key
    }

    pub fn drive_root(&self) -> &str {
        &self.drive_root
    }

    /// The resident directory map, for a self-test to look at.
    pub fn resident_directories(&self) -> usize {
        self.dirs.len()
    }

    /// Opens the raw volume and activates its USN journal. False (rather than failing) if the
    /// volume cannot be opened or has no usable journal.
    pub fn open(&mut self, access: &dyn VolumeAccess) -> bool {
        let Some(mut handle) = access.open(&self.drive_letter) else {
            return false;
        };
        let Some(journal) = handle.query_journal() else {
            return false;
        };
        self.journal_id = journal.journal_id;
        self.next_usn = journal.next_usn;
        self.max_usn = journal.max_usn;
        self.handle = Some(handle);
        true
    }

    /// Builds the index for the volume: raw `$MFT` first (names + sizes + dates + folder-size
    /// roll-up), falling back to the names-only USN enumeration if the raw read fails. On success
    /// the volume root is registered complete and the resident directory map is ready.
    pub fn build_initial_index(&mut self, cancel: &CancelToken) -> Result<(), BuildError> {
        if self.handle.is_none() {
            return Ok(());
        }
        let crawled_utc = FileTime::now();
        let crawl_gen = crawled_utc.to_unix_millis();

        if !self.build_from_raw_mft(cancel, crawl_gen)? {
            cancel.check()?;
            self.build_from_usn_enum(cancel, crawl_gen)?;
        }
        cancel.check()?;

        self.repository.sweep_vanished(&self.root_key, crawl_gen)?;
        self.repository
            .upsert_root(&self.root_key, &self.drive_root, crawled_utc, true)?;
        Ok(())
    }

    /// Raw-`$MFT` build: `fs_entry` rows with real sizes/dates plus the `dir_size_cache` roll-up.
    /// `Ok(false)` (without writing `fs_entry`) if the volume cannot be parsed.
    fn build_from_raw_mft(
        &mut self,
        cancel: &CancelToken,
        crawl_gen: i64,
    ) -> Result<bool, BuildError> {
        let mut records: Vec<MftFileRecord> = Vec::new();
        {
            let handle = self.handle.as_deref().expect("open");
            let reader = MftReader::new(handle);
            match reader.try_read_all(cancel, |rec| records.push(rec)) {
                Ok(true) => {}
                Ok(false) => return Ok(false),
                Err(ReadError::Cancelled) => return Err(BuildError::Cancelled),
                // Unexpected layout / read error: fall back to the USN enumeration.
                Err(ReadError::Io(_)) => return Ok(false),
            }
        }

        let computed_utc = FileTime::now();

        // Resolve every directory's path first: needed both for fs_entry keys and to stat the
        // handful of heavily fragmented files that came back with an unknown (-1) size.
        let mut dir_nodes: HashMap<u64, MftNode> = HashMap::new();
        for rec in &records {
            if rec.is_directory {
                dir_nodes.insert(
                    rec.record_number,
                    MftNode::new(&rec.name, rec.parent_record_number, true, rec.attributes),
                );
            }
        }
        self.dirs.clear();
        for recno in dir_nodes.keys() {
            MftPathBuilder::try_resolve(
                &dir_nodes,
                *recno,
                &self.drive_root,
                &mut self.dirs,
                NtfsLayout::ROOT_RECORD_NUMBER,
            );
        }

        let mut sizes = MftDirectorySizeBuilder::new();
        let mut rows: Vec<FsEntryRow> = Vec::with_capacity(Self::UPSERT_CHUNK);
        for raw in &records {
            cancel.check()?;
            let rec = if raw.size >= 0 {
                raw.clone()
            } else {
                MftFileRecord {
                    size: self.stat_file_size(raw),
                    ..raw.clone()
                }
            };
            sizes.add(&rec);
            if let Some((key, hidden)) = self.try_resolve_record(&rec) {
                rows.push(FsEntryRow {
                    path_key: key,
                    name: rec.name.clone(),
                    is_directory: rec.is_directory,
                    size_bytes: if rec.is_directory { 0 } else { rec.size },
                    modified_utc: rec.modified_utc,
                    hidden,
                    attributes: rec.attributes,
                    created_utc: rec.created_utc,
                });
                if rows.len() >= Self::UPSERT_CHUNK {
                    self.repository.upsert_entries(&rows, crawl_gen)?;
                    rows.clear();
                }
            }
        }
        self.repository.upsert_entries(&rows, crawl_gen)?;

        let results = sizes.build(&self.drive_root, computed_utc, &mut self.dirs);
        for chunk in results.chunks(Self::UPSERT_CHUNK) {
            self.dir_size_repository.upsert_many(chunk)?;
        }
        Ok(true)
    }

    /// A file's real length from disk, used only for the rare fragmented files whose size the base
    /// MFT record cannot report. Best-effort; a vanished file counts as 0.
    fn stat_file_size(&self, rec: &MftFileRecord) -> i64 {
        let parent_path = if rec.parent_record_number == NtfsLayout::ROOT_RECORD_NUMBER {
            self.drive_root.clone()
        } else if let Some(parent) = self.dirs.get(&rec.parent_record_number) {
            parent.path.clone()
        } else {
            return 0;
        };
        let path = combine(&parent_path, &rec.name);
        std::fs::metadata(&path)
            .map(|m| m.len().min(i64::MAX as u64) as i64)
            .unwrap_or(0)
    }

    /// A raw record's canonical key and effective-hidden from the resident directory map.
    /// Directories look up their own resolved path; files hang off their parent.
    fn try_resolve_record(&self, rec: &MftFileRecord) -> Option<(String, bool)> {
        if rec.is_directory {
            let dir = self.dirs.get(&rec.record_number)?;
            return Some((fold_upper(&dir.path), dir.hidden));
        }
        let (parent_path, parent_hidden) =
            if rec.parent_record_number == NtfsLayout::ROOT_RECORD_NUMBER {
                (self.drive_root.as_str(), false)
            } else {
                let parent = self.dirs.get(&rec.parent_record_number)?;
                (parent.path.as_str(), parent.hidden)
            };
        let display = combine(parent_path, &rec.name);
        Some((fold_upper(&display), parent_hidden || rec.hidden()))
    }

    /// Fallback build via FSCTL_ENUM_USN_DATA: names and attributes only. Every row is written
    /// with size 0 and no timestamps, and no `dir_size_cache` is filled. Attributes come through
    /// intact, which is why `is:readonly` answers on a volume where `size:` and `dm:` cannot.
    /// Anything reading sizes has to treat this volume as unknown rather than empty
    /// (`DiskUsageRules` recognises the shape by the largest size in range being zero).
    fn build_from_usn_enum(
        &mut self,
        cancel: &CancelToken,
        crawl_gen: i64,
    ) -> Result<(), BuildError> {
        let map = self.enumerate_mft(cancel);
        if cancel.is_cancelled() {
            return Ok(());
        }

        self.dirs.clear();
        let mut rows: Vec<FsEntryRow> = Vec::with_capacity(Self::UPSERT_CHUNK);
        for (frn, node) in &map {
            cancel.check()?;
            if *frn == NtfsLayout::ROOT_RECORD_NUMBER || node.name == "." || node.name == ".." {
                continue;
            }
            let Some((display, hidden)) = MftPathBuilder::try_resolve(
                &map,
                *frn,
                &self.drive_root,
                &mut self.dirs,
                NtfsLayout::ROOT_RECORD_NUMBER,
            ) else {
                continue;
            };
            rows.push(FsEntryRow {
                path_key: fold_upper(&display),
                name: node.name.clone(),
                is_directory: node.is_directory,
                size_bytes: 0,
                modified_utc: FileTime::UNKNOWN,
                hidden,
                attributes: node.attributes,
                created_utc: FileTime::UNKNOWN,
            });
            if rows.len() >= Self::UPSERT_CHUNK {
                self.repository.upsert_entries(&rows, crawl_gen)?;
                rows.clear();
            }
        }
        self.repository.upsert_entries(&rows, crawl_gen)?;
        Ok(())
    }

    fn enumerate_mft(&mut self, cancel: &CancelToken) -> HashMap<u64, MftNode> {
        let mut map = HashMap::new();
        let max_usn = self.max_usn;
        let handle = self.handle.as_deref_mut().expect("open");
        let mut buffer = vec![0u8; Self::ENUM_BUFFER_SIZE];
        let mut start_frn: u64 = 0;
        while !cancel.is_cancelled() {
            // ERROR_HANDLE_EOF is the normal end of the table.
            let Some(bytes) = handle.enum_usn_data(start_frn, max_usn, &mut buffer) else {
                break;
            };
            if bytes <= 8 {
                break;
            }
            for rec in UsnRecordParser::parse(&buffer, 8, bytes) {
                map.insert(
                    NtfsLayout::record_number(rec.file_reference_number),
                    MftNode::new(
                        &rec.name,
                        NtfsLayout::record_number(rec.parent_file_reference_number),
                        rec.is_directory(),
                        rec.attributes,
                    ),
                );
            }
            start_frn = u64_at(&buffer, 0);
        }
        map
    }

    /// Reads the USN journal in a loop, applying batches of changes until cancelled. On journal
    /// loss (overflow/deleted) the root is flagged stale and the loop exits so the orchestrator can
    /// re-enumerate.
    pub fn tail(&mut self, cancel: &CancelToken) {
        if self.handle.is_none() {
            return;
        }
        let mut buffer = vec![0u8; Self::TAIL_BUFFER_SIZE];
        while !cancel.is_cancelled() {
            let (next_usn, journal_id) = (self.next_usn, self.journal_id);
            let read = self.handle.as_deref_mut().expect("open").read_usn_journal(
                next_usn,
                journal_id,
                &mut buffer,
            );
            let Some(bytes) = read else {
                let _ = self.repository.mark_root_stale(&self.root_key);
                return;
            };

            let next = u64_at(&buffer, 0) as i64;
            if bytes > 8 && next != self.next_usn {
                let records = UsnRecordParser::parse(&buffer, 8, bytes);
                self.apply(&records);
                self.next_usn = next;
                // One transaction per drained batch: the batch is what the journal handed over,
                // so it is already the natural unit, and a flush of nothing touches nothing.
                if let Some(recorder) = &mut self.recorder {
                    recorder.flush();
                }
            } else {
                self.next_usn = next;
                // Idle polls flush too, so a policy that changed to "off" wipes promptly rather
                // than on the next file write; with nothing buffered it costs no I/O.
                if let Some(recorder) = &mut self.recorder {
                    recorder.flush();
                }
                if sleep_unless_cancelled(cancel, self.poll_interval) {
                    return;
                }
            }
        }
    }

    /// Applies one drained batch of journal records. Records arrive in USN order, so a directory's
    /// create always precedes its children's. Non-CLOSE records are ignored except
    /// RENAME_OLD_NAME, which is captured to pair with the matching new-name record.
    pub fn apply(&mut self, records: &[UsnRecord]) {
        for rec in records {
            if rec.reason & usn_reason::RENAME_OLD_NAME != 0 {
                if let Some((old_path, _)) = self.try_resolve_path(rec) {
                    self.pending_renames.insert(
                        NtfsLayout::record_number(rec.file_reference_number),
                        old_path,
                    );
                }
                continue;
            }
            if rec.reason & usn_reason::CLOSE == 0 {
                continue; // wait for the coalescing CLOSE record
            }
            if rec.reason & usn_reason::FILE_DELETE != 0 {
                self.apply_delete(rec);
            } else if rec.reason & usn_reason::RENAME_NEW_NAME != 0 {
                self.apply_rename(rec);
            } else {
                self.apply_upsert(rec);
            }
        }
    }

    fn apply_delete(&mut self, rec: &UsnRecord) {
        let Some((display, key)) = self.try_resolve_path(rec) else {
            return;
        };
        // Before the index write and the map removal, while the parent can still answer for the
        // hidden bit a deleted directory's row carried.
        let hidden = self.effective_hidden(rec);
        self.note(rec, &display, &key, ChangeKind::Deleted, None, hidden);
        let _ = self.repository.delete_subtree(&key);
        if rec.is_directory() {
            self.remove_dir_subtree(NtfsLayout::record_number(rec.file_reference_number), &key);
        }
    }

    fn apply_rename(&mut self, rec: &UsnRecord) {
        let Some((new_display, new_key)) = self.try_resolve_path(rec) else {
            return;
        };
        let crawl_gen = FileTime::now().to_unix_millis();
        let frn = NtfsLayout::record_number(rec.file_reference_number);
        if let Some(old_path) = self.pending_renames.remove(&frn) {
            let old_key = fold_upper(&old_path);
            let hidden = self.effective_hidden(rec);
            self.note(
                rec,
                &new_display,
                &new_key,
                ChangeKind::Renamed,
                Some(&old_path),
                hidden,
            );
            let _ = self
                .repository
                .rename(&old_key, &new_key, &rec.name, crawl_gen);
            if rec.is_directory() {
                self.rewrite_dir_subtree(&old_key, &new_display, rec);
            }
        } else {
            // No captured old name (moved in from elsewhere / lost): treat as a fresh entry.
            self.apply_upsert(rec);
        }
    }

    fn apply_upsert(&mut self, rec: &UsnRecord) {
        let Some((display, key)) = self.try_resolve_path(rec) else {
            return;
        };
        let hidden = self.effective_hidden(rec);
        // Reached both directly and from an unpaired rename; `classify` draws the same line
        // `apply` does, so a move-in from nowhere the map knew is "created" on both sides.
        self.note(
            rec,
            &display,
            &key,
            ChangeLogRules::classify(rec.reason, false),
            None,
            hidden,
        );
        // Attributes come from the record itself; only the timestamps and the length need disk.
        let (size, modified, created) = stat_best_effort(&display, rec.is_directory());
        let crawl_gen = FileTime::now().to_unix_millis();
        let _ = self.repository.upsert_entries(
            &[FsEntryRow {
                path_key: key,
                name: rec.name.clone(),
                is_directory: rec.is_directory(),
                size_bytes: size,
                modified_utc: modified,
                hidden,
                attributes: rec.attributes,
                created_utc: created,
            }],
            crawl_gen,
        );
        if rec.is_directory() {
            self.dirs.insert(
                NtfsLayout::record_number(rec.file_reference_number),
                ResolvedDir {
                    path: display,
                    hidden,
                },
            );
        }
    }

    /// The entry's own Hidden bit OR'd down from its parent: what `fs_entry.hidden` stores, and
    /// what the change log stores beside it so the two can never disagree.
    fn effective_hidden(&self, rec: &UsnRecord) -> bool {
        self.parent_hidden(NtfsLayout::record_number(rec.parent_file_reference_number))
            || rec.is_hidden()
    }

    /// Hands one applied change to the recorder, dated by the record's own stamp so a batch
    /// drained late is dated when it happened. A journal record always carries one; the fallback
    /// is for the parser's unknown, not for real use.
    fn note(
        &mut self,
        rec: &UsnRecord,
        display: &str,
        key: &str,
        kind: ChangeKind,
        old_display_path: Option<&str>,
        hidden: bool,
    ) {
        let Some(recorder) = &mut self.recorder else {
            return;
        };
        let utc = if rec.timestamp_utc.is_unknown() {
            FileTime::now()
        } else {
            rec.timestamp_utc
        };
        recorder.add(ChangeEvent {
            path_key: key.to_string(),
            display_path: display.to_string(),
            is_directory: rec.is_directory(),
            hidden,
            kind,
            old_display_path: old_display_path.map(str::to_string),
            utc,
        });
    }

    /// The display path and canonical key of a change record from its parent directory in the
    /// resident map. None when the parent is unknown: the record is skipped, best-effort, matching
    /// the crawl fallback's tolerance.
    fn try_resolve_path(&self, rec: &UsnRecord) -> Option<(String, String)> {
        let parent_record = NtfsLayout::record_number(rec.parent_file_reference_number);
        let display = if parent_record == NtfsLayout::ROOT_RECORD_NUMBER {
            format!("{}{}", self.drive_root, rec.name)
        } else {
            let parent = self.dirs.get(&parent_record)?;
            combine(&parent.path, &rec.name)
        };
        let key = fold_upper(&display);
        Some((display, key))
    }

    fn parent_hidden(&self, parent_record: u64) -> bool {
        parent_record != NtfsLayout::ROOT_RECORD_NUMBER
            && self.dirs.get(&parent_record).is_some_and(|p| p.hidden)
    }

    /// Drops a deleted directory and all descendant directories from the resident map.
    fn remove_dir_subtree(&mut self, record_number: u64, key: &str) {
        self.dirs.remove(&record_number);
        let Ok((lo, hi)) = PathKey::prefix_bounds(key) else {
            return;
        };
        self.dirs.retain(|_, dir| {
            let upper = fold_upper(&dir.path);
            !(upper.as_str() >= lo.as_str() && upper.as_str() < hi.as_str())
        });
    }

    /// Rewrites the moved directory and every descendant directory's cached path so later child
    /// changes resolve correctly. Directory renames are rare relative to file churn, so the linear
    /// scan over the (small) directory map is acceptable.
    fn rewrite_dir_subtree(&mut self, old_key: &str, new_display: &str, rec: &UsnRecord) {
        let hidden = self.effective_hidden(rec);
        self.dirs.insert(
            NtfsLayout::record_number(rec.file_reference_number),
            ResolvedDir {
                path: new_display.to_string(),
                hidden,
            },
        );
        let Ok((lo, hi)) = PathKey::prefix_bounds(old_key) else {
            return;
        };
        for dir in self.dirs.values_mut() {
            let upper = fold_upper(&dir.path);
            if upper.as_str() >= lo.as_str() && upper.as_str() < hi.as_str() {
                let rest = dir.path.get(old_key.len()..).unwrap_or("");
                dir.path = format!("{new_display}{rest}");
            }
        }
    }
}

/// Sleeps in small steps so a cancel is seen promptly. True when cancelled.
fn sleep_unless_cancelled(cancel: &CancelToken, total: Duration) -> bool {
    let step = Duration::from_millis(25).min(total);
    let mut slept = Duration::ZERO;
    while slept < total {
        if cancel.is_cancelled() {
            return true;
        }
        std::thread::sleep(step);
        slept += step;
    }
    cancel.is_cancelled()
}

/// One stat, three fields. Best-effort: a path that cannot be read is zero and unknown.
fn stat_best_effort(path: &str, is_directory: bool) -> (i64, FileTime, FileTime) {
    use std::os::windows::fs::MetadataExt;
    match std::fs::metadata(path) {
        Ok(m) => {
            let size = if is_directory {
                0
            } else {
                m.len().min(i64::MAX as u64) as i64
            };
            (
                size,
                FileTime(m.last_write_time() as i64),
                FileTime(m.creation_time() as i64),
            )
        }
        Err(_) => (0, FileTime::UNKNOWN, FileTime::UNKNOWN),
    }
}
