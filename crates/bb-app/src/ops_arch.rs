//! Extract, compress, edits to an archive's contents and shortcuts: the writes that are not a
//! transfer but queue and report like one. Spec: ShellViewModel (PlanExtract, ExecuteExtractAsync,
//! ExecuteCreateArchiveAsync, ArchiveEditAsync, CreateShortcutsAsync) and ShellLink.
//!
//! The archive executors take their own progress sink and gate (`archives::PauseGate`), a twin of
//! the transfer ones until the two are merged, so a pause is mirrored onto `OpsUi::archive_gate`.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use bb_core::archives::{
    ArchiveBrowser, ArchiveCreator, ArchiveEdit, ArchiveEditExecutor, ArchiveEditPlanner,
    ArchiveFormats, ArchivePasswords, ArchivePath, ArchiveSourceWalk, ArchiveWriteFormat,
    CompressionLevel, ExtractConflict, ExtractExecutor, ExtractPlanner, LibraryArchiveReader,
    TransferProgress as ArchiveProgress,
};
use bb_core::format;
use bb_core::shortcuts::{ShortcutExecutor, ShortcutPlanner};
use bb_core::transfer::{TransferEstimate, TransferProgress};
use bb_core::undo::ArchiveEditRecord;
use bb_paths::{PathKey, file_name};

use crate::app::{Inner, lock};
use crate::ids::{AppError, AppResult, OpId};
use crate::ops_ui::{JobMeta, TransferTracker};

static READER: LazyLock<LibraryArchiveReader> = LazyLock::new(LibraryArchiveReader::default);

/// The passwords given this session, by archive. Never written anywhere. One that turned out wrong
/// is forgotten, and remembered only as having been refused, so the next ask says so.
#[derive(Default)]
pub struct SessionPasswords(
    Mutex<HashMap<String, String>>,
    Mutex<std::collections::HashSet<String>>,
);

impl SessionPasswords {
    fn key(archive_file: &str) -> String {
        PathKey::canonicalize(archive_file).unwrap_or_else(|_| archive_file.to_string())
    }

    pub fn remember(&self, archive_file: &str, password: &str) {
        let key = Self::key(archive_file);
        lock(&self.1).remove(&key);
        lock(&self.0).insert(key, password.to_string());
    }

    /// The archive still reads as locked with the password given: it was wrong.
    pub fn refused(&self, archive_file: &str) {
        let key = Self::key(archive_file);
        if lock(&self.0).remove(&key).is_some() {
            lock(&self.1).insert(key);
        }
    }

    pub fn was_refused(&self, archive_file: &str) -> bool {
        lock(&self.1).contains(&Self::key(archive_file))
    }
}

impl ArchivePasswords for SessionPasswords {
    fn for_archive(&self, archive_file: &str) -> Option<String> {
        let key = PathKey::canonicalize(archive_file).unwrap_or_else(|_| archive_file.to_string());
        lock(&self.0).get(&key).cloned()
    }
}

pub(crate) fn archive_of(path: &str) -> Option<ArchivePath> {
    if !ArchivePath::looks_virtual(path) {
        return None;
    }
    // The archive file itself is not "inside" it: deleting or renaming it is an ordinary file operation.
    ArchivePath::parse(path, |p| std::path::Path::new(p).is_file()).filter(|a| !a.is_root())
}

fn as_transfer(p: &ArchiveProgress) -> TransferProgress {
    TransferProgress {
        done: p.done.max(0) as usize,
        total: p.total.max(0) as usize,
        current_name: p.current_name.clone(),
        bytes_done: p.bytes_done,
        current_item_bytes: p.current_item_bytes,
        current_item_total: p.current_item_total,
    }
}

pub fn extract_conflict_of(word: &str) -> ExtractConflict {
    match word.to_ascii_lowercase().as_str() {
        "skip" => ExtractConflict::Skip,
        _ => ExtractConflict::KeepBoth,
    }
}

pub fn write_format_of(word: &str) -> Option<ArchiveWriteFormat> {
    bb_core::archives::ArchiveWriteRules::formats()
        .iter()
        .find(|f| {
            f.label.eq_ignore_ascii_case(word)
                || f.suffix.trim_start_matches('.').eq_ignore_ascii_case(word)
        })
        .map(|f| f.format)
}

pub fn level_of(word: &str) -> CompressionLevel {
    match word.to_ascii_lowercase().as_str() {
        "store" | "fastest" => CompressionLevel::Store,
        "maximum" | "smallest" => CompressionLevel::Maximum,
        _ => CompressionLevel::Normal,
    }
}

impl Inner {
    /// Pulls entries out of the archive `tab_path` is inside. `names` are entry paths relative to
    /// the archive's root (or full virtual paths); none means everything under where the tab is.
    pub(crate) fn op_extract(
        self: &Arc<Self>,
        tab_path: &str,
        names: &[String],
        destination: &str,
        conflict: ExtractConflict,
    ) -> AppResult<OpId> {
        let here = archive_of(tab_path)
            .or_else(|| {
                // The tab may be standing on the archive file itself (an archive row selected).
                std::path::Path::new(tab_path)
                    .is_file()
                    .then(|| ArchivePath::new(tab_path, ""))
            })
            .ok_or_else(|| AppError::Invalid("This tab is not showing an archive.".into()))?;
        let entries: Vec<String> = names
            .iter()
            .map(|n| match archive_of(n) {
                Some(p) => p.entry_path,
                None => n.clone(),
            })
            .collect();
        let index = self.fs.read_archive(&here.archive_file);
        let plan = ExtractPlanner::default().plan(
            &index,
            &here.archive_file,
            &here.entry_path,
            &entries,
            destination,
            conflict,
        );
        if let Some(rejected) = &plan.rejected {
            return Err(AppError::Invalid(rejected.message.clone()));
        }
        if !plan.has_work() {
            return Err(AppError::Invalid("Nothing to extract.".into()));
        }
        let files = plan.items.iter().filter(|i| !i.is_directory).count();
        let estimate = TransferEstimate::new(plan.total_bytes, files, plan.bytes_are_exact);
        let meta = JobMeta::shown(
            "Extract",
            format!(
                "{} item(s) to {}",
                format::thousands(files as i64),
                file_name(destination.trim_end_matches('\\'))
            ),
            files,
            estimate,
        );
        let password = self.extra.ops.passwords.for_archive(&here.archive_file);
        self.submit_with(
            meta,
            false,
            Box::new(move |inner, _op, cancel| {
                let items = plan
                    .items
                    .iter()
                    .filter(|i| !i.is_directory)
                    .map(|i| {
                        (
                            file_name(&i.entry_path).to_string(),
                            plan.destination_directory.clone(),
                        )
                    })
                    .collect();
                inner.tracker_begin(TransferTracker::new("Extracting", items, estimate));
                inner.set_active_status(format!(
                    "Extracting {} file(s)\u{2026}",
                    format::thousands(files as i64)
                ));
                let me = inner.clone();
                let mut sink = move |p: &ArchiveProgress| me.tracker_apply(&as_transfer(p));
                let outcome = ExtractExecutor::new(&*READER).execute(
                    &plan,
                    password.as_deref(),
                    cancel,
                    Some(&mut sink),
                    Some(&inner.extra.ops.archive_gate),
                );
                inner.refresh_tabs_showing(std::slice::from_ref(&plan.destination_directory));
                let mut text = if outcome.cancelled {
                    format!(
                        "Extract cancelled \u{2014} {} file(s) extracted",
                        outcome.files_written
                    )
                } else {
                    format!(
                        "Extracted {} file(s)",
                        format::thousands(i64::from(outcome.files_written))
                    )
                };
                if let Some(first) = outcome.failed.first() {
                    text.push_str(&format!(
                        ", {} failed \u{2014} {}",
                        outcome.failed.len(),
                        first.message
                    ));
                }
                inner.set_final_status(text.clone());
                (outcome.failed.is_empty() && !outcome.cancelled, text)
            }),
        )
    }

    /// Writes a new archive from `sources`.
    pub(crate) fn op_compress(
        self: &Arc<Self>,
        sources: Vec<String>,
        target: String,
        format_: ArchiveWriteFormat,
        level: CompressionLevel,
    ) -> AppResult<OpId> {
        if sources.is_empty() {
            return Err(AppError::Invalid("nothing to compress".into()));
        }
        let include_hidden = lock(&self.settings).show_hidden_items;
        let meta = JobMeta::shown(
            "Compress",
            file_name(&target).to_string(),
            sources.len(),
            TransferEstimate::new(0, 0, false),
        );
        self.submit_with(
            meta,
            false,
            Box::new(move |inner, _op, cancel| {
                let items = sources
                    .iter()
                    .map(|s| (file_name(s).to_string(), target.clone()))
                    .collect();
                inner.tracker_begin(TransferTracker::new(
                    "Compressing",
                    items,
                    TransferEstimate::new(0, 0, false),
                ));
                inner.set_active_status(format!("Compressing into {}\u{2026}", file_name(&target)));
                let collected = match ArchiveSourceWalk::collect(&sources, include_hidden, cancel) {
                    Ok(c) => c,
                    Err(_) => return (false, "cancelled".to_string()),
                };
                let me = inner.clone();
                let mut sink = move |p: &ArchiveProgress| me.tracker_apply(&as_transfer(p));
                let outcome = ArchiveCreator::new().create(
                    &target,
                    format_,
                    level,
                    &collected,
                    cancel,
                    Some(&mut sink),
                    Some(&inner.extra.ops.archive_gate),
                );
                if let Some(dir) = bb_core::rename::paths::directory_name(&target) {
                    inner.refresh_tabs_showing(&[dir]);
                }
                let mut text = if outcome.cancelled {
                    "Compress cancelled".to_string()
                } else {
                    format!(
                        "Compressed {} file(s) into {}",
                        format::thousands(i64::from(outcome.files_written)),
                        file_name(&target)
                    )
                };
                if let Some(first) = outcome.failed.first() {
                    text.push_str(&format!(
                        ", {} failed \u{2014} {first}",
                        outcome.failed.len()
                    ));
                }
                inner.set_final_status(text.clone());
                (outcome.failed.is_empty() && !outcome.cancelled, text)
            }),
        )
    }

    /// Removes entries from, or renames entries in, the archive the paths are inside. Both are
    /// rewrites of the container, held beside it so the undo history can swap them back.
    pub(crate) fn op_archive_edit(
        self: &Arc<Self>,
        archive_file: String,
        edits: Vec<ArchiveEdit>,
        verb: &'static str,
    ) -> AppResult<OpId> {
        let bytes = std::fs::metadata(&archive_file).map_or(0, |m| m.len() as i64);
        let index = self.fs.read_archive(&archive_file);
        let plan = ArchiveEditPlanner::new().plan(&index, &archive_file, bytes, &edits);
        if let Some(rejected) = &plan.rejected {
            self.set_active_status(rejected.message.clone());
            return Err(AppError::Invalid(rejected.message.clone()));
        }
        let meta = JobMeta::shown(
            "Archive edit",
            format!("{} in {}", edits.len(), file_name(&archive_file)),
            edits.len(),
            TransferEstimate::new(bytes, 1, true),
        );
        self.submit_with(
            meta,
            true,
            Box::new(move |inner, _op, cancel| {
                inner.tracker_begin(TransferTracker::new(
                    "Rewriting",
                    vec![(file_name(&archive_file).to_string(), archive_file.clone())],
                    TransferEstimate::new(bytes, 1, true),
                ));
                inner.set_active_status(format!("Rewriting {}\u{2026}", file_name(&archive_file)));
                let me = inner.clone();
                let mut sink = move |p: &ArchiveProgress| me.tracker_apply(&as_transfer(p));
                let outcome = ArchiveEditExecutor::new(&*READER).execute(
                    &plan,
                    cancel,
                    Some(&mut sink),
                    Some(&inner.extra.ops.archive_gate),
                );
                inner.record(
                    ArchiveEditRecord::for_outcome(outcome.clone()).map(|r| Box::new(r) as _),
                );
                inner.refresh_tabs_in_archive(&archive_file);
                let text = match (&outcome.failure, outcome.cancelled) {
                    (Some(f), _) => format!("{verb} failed \u{2014} {f}"),
                    (None, true) => format!("{verb} cancelled"),
                    (None, false) => format!(
                        "{verb} in {} \u{2014} this can be undone",
                        file_name(&archive_file)
                    ),
                };
                inner.set_final_status(text.clone());
                (outcome.failure.is_none() && !outcome.cancelled, text)
            }),
        )
    }

    /// Every tab at, or inside, an archive reloads.
    pub(crate) fn refresh_tabs_in_archive(self: &Arc<Self>, archive_file: &str) {
        let Ok(archive_key) = PathKey::canonicalize(archive_file) else {
            return;
        };
        for (id, cell) in self.all_tabs() {
            let inside = {
                let t = lock(&cell);
                !t.path.is_empty()
                    && PathKey::canonicalize(&t.path)
                        .map(|k| k == archive_key || PathKey::is_under_key(&k, &archive_key))
                        .unwrap_or(false)
            };
            if inside {
                self.refresh_view(id);
            }
        }
    }

    /// Every tab showing the inside of an archive reloads: an undo can have swapped a container
    /// whose folder is not among the directories it reports.
    pub(crate) fn refresh_archive_tabs(self: &Arc<Self>) {
        for (id, cell) in self.all_tabs() {
            let path = lock(&cell).path.clone();
            if !path.is_empty() && self.fs.resolve(&path).is_some() {
                self.refresh_view(id);
            }
        }
    }

    /// Writes a `.lnk` for each source into `directory`. Additive: it takes no place in the history.
    pub(crate) fn op_shortcut(
        self: &Arc<Self>,
        sources: Vec<String>,
        directory: String,
    ) -> AppResult<OpId> {
        if sources.is_empty() {
            return Err(AppError::Invalid("nothing to make a shortcut to".into()));
        }
        self.submit_with(
            JobMeta::hidden("Shortcut"),
            true,
            Box::new(move |inner, _op, _cancel| {
                let plan = ShortcutPlanner::local().plan(&sources, &directory);
                if !plan.has_work() {
                    let text = match plan.problems.first() {
                        Some(p) => format!("No shortcut created \u{2014} {}", p.message),
                        None => "Nothing to make a shortcut to".to_string(),
                    };
                    inner.set_active_status(text.clone());
                    return (false, text);
                }
                let writer = crate::shell_link::ShellLinkWriter;
                let outcome = ShortcutExecutor::local(&writer).execute(&plan);
                // What was made is selected where it was made. Spec: ShellViewModel.CreateShortcutsAsync.
                inner.select_many_after_refresh(&directory, outcome.created.clone());
                inner.refresh_tabs_showing(std::slice::from_ref(&directory));
                let noun = if outcome.created.len() == 1 {
                    "shortcut"
                } else {
                    "shortcuts"
                };
                let refused = outcome.failed.len() + plan.problems.len();
                let mut text = format!("Created {} {noun}", outcome.created.len());
                if refused > 0 {
                    let first = outcome
                        .failed
                        .first()
                        .map(|f| f.message.clone())
                        .or_else(|| plan.problems.first().map(|p| p.message.clone()))
                        .unwrap_or_default();
                    text.push_str(&format!("; {refused} could not be \u{2014} {first}"));
                }
                inner.set_final_status(text.clone());
                (outcome.failed.is_empty(), text)
            }),
        )
    }

    /// The archive a tab is looking at, if it is; used by unlock and extract.
    pub(crate) fn archive_file_for(&self, path: &str) -> Option<String> {
        if let Some(a) = archive_of(path) {
            return Some(a.archive_file);
        }
        let p = std::path::Path::new(path);
        (p.is_file() && ArchiveFormats::is_archive_name(file_name(path))).then(|| path.to_string())
    }
}
