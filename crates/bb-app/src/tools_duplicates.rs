//! The duplicate finder's window: one scan in flight, off the UI thread, with progress and a Stop, and
//! the groups it found held here so the guard that never lets every copy of a group go
//! (`duplicates_removable`) is asked of exactly what the window shows. It never runs off a
//! keystroke: a whole-PC scan is a full pass over the index followed by real reads, so it is reached
//! from an explicit Scan and says how far along it is throughout. Spec: ViewModels/DuplicatesViewModel.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bb_core::cancel::CancelToken;
use bb_core::data::FsIndexRepository;
use bb_core::duplicates::{
    DuplicateFinder, DuplicateIndexFacts, DuplicateRules, DuplicateScanAvailability,
    DuplicateScanPhase, DuplicateScanProgress, DuplicateScanRequest,
    IndexedDuplicateCandidateSource, KeepStrategy,
};
use bb_core::format::{byte_size, thousands};
use bb_core::fs::IndexCrawler;
use bb_core::mft::MftIndex;
use bb_paths::PathKey;
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::events::Model;
use crate::ids::AppResult;

/// The floors the size picker offers; a value the settings file holds that is not one of them is
/// shown alongside, or the picker would show an empty box and then quietly rewrite it.
const MIN_SIZES: &[(&str, i64)] = &[
    ("1 KB", 1024),
    ("100 KB", 100 * 1024),
    ("1 MB", 1024 * 1024),
    ("10 MB", 10 * 1024 * 1024),
    ("100 MB", 100 * 1024 * 1024),
    ("1 GB", 1024 * 1024 * 1024),
];

/// How often progress may reach the window: the reading threads report per file.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

#[derive(Default)]
pub(crate) struct DuplicatesSession {
    generation: u64,
    /// Moves whenever the groups are replaced or renumbered; the window's ticks are by position.
    version: u64,
    scanning: bool,
    has_scanned: bool,
    cancel: CancelToken,
    /// The folder being searched; None is "This PC".
    root: Option<String>,
    availability: Option<DuplicateScanAvailability>,
    cancelled: bool,
    incomplete: bool,
    headline: String,
    detail: String,
    fraction: f64,
    indeterminate: bool,
    last_emit: Option<Instant>,
    /// The groups as the window shows them.
    groups: Vec<Value>,
}

/// The real index's answer, for a scope it covers.
struct LiveFacts<'a>(&'a dyn MftIndex);

impl DuplicateIndexFacts for LiveFacts<'_> {
    fn is_building(&self) -> bool {
        self.0.is_building()
    }
    fn any_indexed(&self) -> bool {
        self.0.any_indexed()
    }
    fn is_indexed(&self, path_key: &str) -> bool {
        self.0.is_indexed(path_key)
    }
}

/// A folder this app has just crawled into the index itself: complete, as far as the scan cares.
struct Crawled;

impl DuplicateIndexFacts for Crawled {
    fn is_building(&self) -> bool {
        false
    }
    fn any_indexed(&self) -> bool {
        true
    }
    fn is_indexed(&self, _path_key: &str) -> bool {
        true
    }
}

fn group_json(g: &bb_core::duplicates::DuplicateGroup) -> Value {
    json!({
        "sizeBytes": g.size_bytes,
        "hash": g.hash,
        "keep": {
            "newest": DuplicateRules::choose_keeper(g, KeepStrategy::Newest),
            "oldest": DuplicateRules::choose_keeper(g, KeepStrategy::Oldest),
            "shallowest": DuplicateRules::choose_keeper(g, KeepStrategy::Shallowest),
        },
        "files": g.files.iter().map(|f| json!({
            "path": f.display_path,
            "name": f.name,
            "dir": f.relative_dir_display,
            "sizeBytes": f.size_bytes,
            "modified": f.modified_utc.0,
            "hidden": f.hidden,
            "hardlinks": f.hardlink_paths,
        })).collect::<Vec<_>>(),
    })
}

fn wasted(groups: &[Value]) -> i64 {
    groups
        .iter()
        .map(|g| {
            let n = g["files"].as_array().map_or(0, Vec::len) as i64;
            g["sizeBytes"].as_i64().unwrap_or(0) * (n - 1).max(0)
        })
        .sum()
}

impl Inner {
    /// Starts a scan of `root` (None: this PC), superseding any in flight. A floor or the system
    /// folders switch given here is remembered for next time, as the window's knobs are.
    pub(crate) fn duplicates_scan(
        self: &Arc<Self>,
        root: Option<String>,
        min_size: Option<i64>,
        skip_system: Option<bool>,
    ) -> AppResult<()> {
        {
            let mut s = lock(&self.settings);
            if let Some(m) = min_size.filter(|m| *m > 0) {
                s.duplicate_min_size_bytes = m;
            }
            if let Some(k) = skip_system {
                s.duplicate_skip_system_folders = k;
            }
        }
        if min_size.is_some() || skip_system.is_some() {
            self.save_settings_now()?;
        }
        let (min, skip, hidden) = {
            let s = lock(&self.settings);
            (
                s.duplicate_min_size_bytes,
                s.duplicate_skip_system_folders,
                s.show_hidden_items,
            )
        };
        let root = root.filter(|r| !r.trim().is_empty());
        let (generation, cancel) = {
            let mut d = lock(&self.extra.tools.duplicates);
            d.cancel.cancel();
            d.cancel = CancelToken::new();
            d.generation += 1;
            d.version += 1;
            d.scanning = true;
            d.has_scanned = true;
            d.root = root.clone();
            d.availability = None;
            d.cancelled = false;
            d.incomplete = false;
            d.headline = "Looking for files of the same size\u{2026}".into();
            d.detail.clear();
            d.fraction = 0.0;
            d.indeterminate = true;
            d.groups.clear();
            (d.generation, d.cancel.clone())
        };
        lock(&self.extra.tools.duplicate_groups).clear();
        self.hub.model(Model::Duplicates);
        self.schedule(move |inner| {
            inner.duplicates_run(generation, root, min, skip, hidden, &cancel)
        });
        Ok(())
    }

    /// The window opened on `root` (None: this PC): pointed there, with nothing scanned, because a
    /// scan reads real files and waits to be asked. A scan still running elsewhere is stopped.
    pub(crate) fn duplicates_load(&self, root: Option<String>) {
        {
            let mut d = lock(&self.extra.tools.duplicates);
            d.cancel.cancel();
            let generation = d.generation + 1;
            let version = d.version + 1;
            *d = DuplicatesSession {
                generation,
                version,
                root: root.filter(|r| !r.trim().is_empty()),
                ..Default::default()
            };
        }
        lock(&self.extra.tools.duplicate_groups).clear();
        self.hub.model(Model::Duplicates);
    }

    /// Stop: the scan hands back what it had already confirmed.
    pub(crate) fn duplicates_cancel(&self) {
        let mut d = lock(&self.extra.tools.duplicates);
        if d.scanning {
            d.headline = "Stopping\u{2026}".into();
            d.cancel.cancel();
        }
        drop(d);
        self.hub.model(Model::Duplicates);
    }

    fn duplicates_run(
        &self,
        generation: u64,
        root: Option<String>,
        min: i64,
        skip: bool,
        hidden: bool,
        cancel: &CancelToken,
    ) {
        let repo = Arc::new(FsIndexRepository::new(self.db.clone()));
        // A folder the live index covers is answered from it; anything else is crawled into the
        // index first, so the finder works without the MFT helper. This PC is the index's or nothing.
        let index = &*self.seams.index;
        let covered = root
            .as_deref()
            .is_none_or(|r| PathKey::canonicalize(r).is_ok_and(|k| index.is_indexed(&k)));
        let crawl_failed = match root.as_deref() {
            Some(r) if !covered => IndexCrawler::new(repo.clone())
                .crawl(r, cancel, None)
                .is_err(),
            _ => false,
        };
        let candidates = IndexedDuplicateCandidateSource::new(&repo);
        let hasher = bb_core::checksums::FileSystemFileHasher::new();
        let request = DuplicateScanRequest {
            root_path: root.clone(),
            min_size_bytes: min,
            include_hidden: hidden,
            skip_system_folders: skip,
        };
        let report = |p: DuplicateScanProgress| self.duplicates_progress(generation, &p);
        let live = LiveFacts(index);
        let facts: &dyn DuplicateIndexFacts = if covered { &live } else { &Crawled };
        let result =
            DuplicateFinder::new(&candidates, &hasher, facts).scan(&request, Some(&report), cancel);

        let mut d = lock(&self.extra.tools.duplicates);
        if d.generation != generation {
            return; // a superseded scan does not own the window any more
        }
        d.scanning = false;
        d.version += 1;
        d.headline.clear();
        d.detail.clear();
        match result {
            Ok(outcome) => {
                d.availability = Some(outcome.availability);
                d.cancelled = outcome.cancelled || cancel.is_cancelled();
                d.incomplete = outcome.incomplete || crawl_failed;
                d.groups = outcome.groups.iter().map(group_json).collect();
                *lock(&self.extra.tools.duplicate_groups) = outcome
                    .groups
                    .iter()
                    .map(|g| g.files.iter().map(|f| f.display_path.clone()).collect())
                    .collect();
            }
            Err(_) => {
                d.cancelled = cancel.is_cancelled();
                d.incomplete = !d.cancelled;
                d.availability = Some(DuplicateScanAvailability::Ready);
            }
        }
        drop(d);
        self.hub.model(Model::Duplicates);
    }

    fn duplicates_progress(&self, generation: u64, p: &DuplicateScanProgress) {
        let mut d = lock(&self.extra.tools.duplicates);
        if d.generation != generation || !d.scanning || d.cancel.is_cancelled() {
            return;
        }
        match p.phase {
            DuplicateScanPhase::Shortlisting => {
                d.headline = "Looking for files of the same size\u{2026}".into();
                d.detail.clear();
                d.indeterminate = true;
            }
            phase => {
                d.headline = if phase == DuplicateScanPhase::Sampling {
                    "Comparing the start of each file\u{2026}".into()
                } else {
                    "Comparing in full\u{2026}".into()
                };
                d.indeterminate = false;
                d.fraction = if p.bytes_total > 0 {
                    (p.bytes_done as f64 / p.bytes_total as f64).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                d.detail = format!(
                    "{} of {} files \u{b7} {} of {}",
                    thousands(p.done as i64),
                    thousands(p.total as i64),
                    byte_size(p.bytes_done),
                    byte_size(p.bytes_total)
                );
            }
        }
        let due = d.last_emit.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY);
        if due {
            d.last_emit = Some(Instant::now());
        }
        drop(d);
        if due {
            self.hub.model(Model::Duplicates);
        }
    }

    /// These copies went: out of their groups, and a group left with one copy is not a group.
    pub(crate) fn duplicates_prune(&self, removed: &[String]) {
        let gone: std::collections::HashSet<String> =
            removed.iter().map(|p| p.to_lowercase()).collect();
        let mut d = lock(&self.extra.tools.duplicates);
        for g in d.groups.iter_mut() {
            if let Some(files) = g["files"].as_array_mut() {
                files.retain(|f| !gone.contains(&f["path"].as_str().unwrap_or("").to_lowercase()));
            }
        }
        d.groups
            .retain(|g| g["files"].as_array().is_some_and(|f| f.len() >= 2));
        d.version += 1;
        *lock(&self.extra.tools.duplicate_groups) = d
            .groups
            .iter()
            .map(|g| {
                g["files"]
                    .as_array()
                    .map(|f| {
                        f.iter()
                            .map(|x| x["path"].as_str().unwrap_or("").to_string())
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect();
        drop(d);
        self.hub.model(Model::Duplicates);
    }

    /// What the window shows: `{root, rootDisplay, scanning, hasScanned, availability, message,
    /// caveat, emptyMessage, canRetry, progress:{headline, detail, fraction, indeterminate},
    /// minSize, minSizes:[{label, bytes}], skipSystem, wastedBytes, groups}`.
    pub(crate) fn duplicates_model(&self) -> Value {
        let (min, skip) = {
            let s = lock(&self.settings);
            (s.duplicate_min_size_bytes, s.duplicate_skip_system_folders)
        };
        let mut sizes: Vec<(String, i64)> =
            MIN_SIZES.iter().map(|(l, b)| (l.to_string(), *b)).collect();
        if !sizes.iter().any(|(_, b)| *b == min) {
            sizes.push((byte_size(min), min));
            sizes.sort_by_key(|(_, b)| *b);
        }
        let index = &*self.seams.index;
        let d = lock(&self.extra.tools.duplicates);
        let availability = d.availability.unwrap_or(DuplicateScanAvailability::Ready);
        let status = index.status_text();
        let message = match availability {
            DuplicateScanAvailability::Building if !status.is_empty() => format!(
                "{status} \u{2014} anything found so far is only part of the answer."
            ),
            DuplicateScanAvailability::Building => {
                "Still indexing \u{2014} anything found so far is only part of the answer.".into()
            }
            DuplicateScanAvailability::NoSizeData => "This drive is indexed by name only. Without file sizes there is nothing to compare, so duplicates cannot be found here.".into(),
            DuplicateScanAvailability::NotIndexed => "This drive isn't indexed, and finding duplicates starts from the index rather than from reading every folder.".into(),
            DuplicateScanAvailability::Ready => String::new(),
        };
        let caveat = match (d.cancelled, d.incomplete) {
            (true, true) => {
                "Stopped early, and some files could not be read \u{2014} there may be more."
            }
            (true, false) => "Stopped early \u{2014} there may be more.",
            (false, true) => "Some files could not be read, so there may be more.",
            _ => "",
        };
        let empty = if d.scanning || !d.groups.is_empty() {
            ""
        } else if !d.has_scanned {
            "Press Scan to look for identical files."
        } else if availability == DuplicateScanAvailability::Ready {
            "No duplicates here."
        } else {
            "Needs the search index \u{2014} see above."
        };
        json!({
            "version": d.version,
            "root": d.root,
            "rootDisplay": d.root.clone().unwrap_or_else(|| "This PC".into()),
            "scanning": d.scanning,
            "hasScanned": d.has_scanned,
            "availability": format!("{availability:?}"),
            "message": message,
            "caveat": caveat,
            "emptyMessage": empty,
            "canRetry": availability == DuplicateScanAvailability::NotIndexed && index.can_start(),
            "progress": {
                "headline": d.headline,
                "detail": d.detail,
                "fraction": d.fraction,
                "indeterminate": d.indeterminate,
            },
            "minSize": min,
            "minSizes": sizes.iter().map(|(l, b)| json!({ "label": l, "bytes": b })).collect::<Vec<_>>(),
            "skipSystem": skip,
            "wastedBytes": wasted(&d.groups),
            "groups": d.groups,
        })
    }
}
