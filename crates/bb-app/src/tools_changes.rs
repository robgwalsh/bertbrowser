//! The change timeline ("What changed") and the History switch behind it.
//! Spec: ViewModels/ChangeTimelineViewModel.cs, ChangeEventViewModel.cs, App.ApplyChangeLogPolicy.
//!
//! **Off by default, and turning it off wipes the table.** The log is written only by the index
//! helper's USN tail under a policy the app pushes; nothing here reads the journal. The window shows
//! why it is empty (recording off, still indexing, the helper not running, a drive not indexed) so a
//! switched-off feature never looks like a quiet disk.

use std::sync::Arc;

use bb_core::FileTime;
use bb_core::changes::{
    ChangeEvent, ChangeKind, ChangeLogRules, ChangeQuery, ChangeRange, ChangeTimelineAvailability,
};
use bb_core::data::ChangeLogRepository;
use bb_core::format::relative_time;
use bb_core::mft::IndexerPresence;
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::ids::{AppError, AppResult};

pub(crate) struct ChangesSession {
    scope: Option<String>,
    range: ChangeRange,
    kinds: [bool; 4],
    include_hidden: bool,
}

fn range_of(name: &str) -> Option<ChangeRange> {
    Some(match name {
        "15m" => ChangeRange::Last15Minutes,
        "1h" => ChangeRange::LastHour,
        "6h" => ChangeRange::Last6Hours,
        "24h" => ChangeRange::Last24Hours,
        "mark" => ChangeRange::SinceMark,
        _ => return None,
    })
}

const RANGES: [(&str, &str, ChangeRange); 5] = [
    ("15m", "Last 15 minutes", ChangeRange::Last15Minutes),
    ("1h", "Last hour", ChangeRange::LastHour),
    ("6h", "Last 6 hours", ChangeRange::Last6Hours),
    ("24h", "Last 24 hours", ChangeRange::Last24Hours),
    ("mark", "Since the mark", ChangeRange::SinceMark),
];

fn kind_label(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Created => "Created",
        ChangeKind::Modified => "Modified",
        ChangeKind::Deleted => "Deleted",
        ChangeKind::Renamed => "Renamed",
    }
}

impl ChangesSession {
    pub fn new(scope: Option<String>, include_hidden: bool) -> ChangesSession {
        ChangesSession {
            scope: scope.filter(|s| !s.is_empty()),
            range: ChangeRange::LastHour,
            kinds: [true; 4],
            include_hidden,
        }
    }

    /// `{range?, created?, modified?, deleted?, renamed?, hidden?}`.
    pub fn set(&mut self, args: &Value) {
        if let Some(r) = args["range"].as_str().and_then(range_of) {
            self.range = r;
        }
        for (i, key) in ["created", "modified", "deleted", "renamed"]
            .iter()
            .enumerate()
        {
            if let Some(b) = args[*key].as_bool() {
                self.kinds[i] = b;
            }
        }
        if let Some(b) = args["hidden"].as_bool() {
            self.include_hidden = b;
        }
    }

    pub fn use_mark_range(&mut self) {
        self.range = ChangeRange::SinceMark;
    }

    pub fn model(&self, inner: &Arc<Inner>) -> Value {
        let (recording_on, policy, mark) = {
            let s = lock(&inner.settings);
            (
                s.record_file_changes,
                s.effective_change_log_policy(),
                s.change_timeline_mark_utc,
            )
        };
        let index = &inner.seams.index;
        let scoped = self.scope.is_some();
        let scope_indexed = self
            .scope
            .as_deref()
            .and_then(|s| bb_paths::PathKey::canonicalize(s).ok())
            .is_some_and(|k| index.is_indexed(&k));
        let availability = ChangeLogRules::availability(
            recording_on,
            scoped,
            index.any_indexed(),
            scope_indexed,
            index.is_building(),
            // NotApplicable (an in-process indexer, or a harness run) counts as running: those hosts
            // index without a helper, and reporting one missing would be a lie about why.
            index.presence() != IndexerPresence::NotRunning,
        );
        let status = index.status_text();
        let message = match availability {
            ChangeTimelineAvailability::Ready => String::new(),
            ChangeTimelineAvailability::RecordingOff => "File changes aren't being recorded. Turn recording on under Settings \u{203a} History to see what changes here.".into(),
            ChangeTimelineAvailability::Building if !status.is_empty() => {
                format!("{status} Recording starts once the index is built.")
            }
            ChangeTimelineAvailability::Building => {
                "Still indexing \u{2014} recording starts once the index is built.".into()
            }
            ChangeTimelineAvailability::IndexerUnavailable if !status.is_empty() => {
                format!("{status} Nothing is being recorded.")
            }
            ChangeTimelineAvailability::IndexerUnavailable => {
                "The search index isn't running, so nothing is being recorded.".into()
            }
            ChangeTimelineAvailability::ScopeNotIndexed => {
                "This drive isn't indexed, so changes on it aren't recorded.".into()
            }
        };

        let now = FileTime::now();
        let since = ChangeLogRules::since_utc_within(self.range, now, mark, policy);
        let mut rows: Vec<Value> = Vec::new();
        let mut truncated = false;
        // Off: nothing is written, and what was is being wiped. Querying would only flash rows on
        // their way out. No mark: nothing to count from yet.
        if availability != ChangeTimelineAvailability::RecordingOff
            && let Some(since) = since
        {
            let kinds = ChangeKind::ALL
                .iter()
                .enumerate()
                .filter(|(i, _)| self.kinds[*i])
                .map(|(_, k)| *k)
                .collect();
            let query = ChangeQuery {
                since_utc: since,
                scope_path_key: self
                    .scope
                    .as_deref()
                    .and_then(|s| bb_paths::PathKey::canonicalize(s).ok()),
                kinds,
                include_hidden: self.include_hidden,
                limit: ChangeLogRules::QUERY_LIMIT,
            };
            if let Ok((found, more)) = ChangeLogRepository::new(inner.db.clone()).query(&query) {
                truncated = more;
                rows = found
                    .iter()
                    .map(|r| {
                        let name = bb_paths::file_name(&r.display_path);
                        let renamed_from = r
                            .old_display_path
                            .as_deref()
                            .filter(|_| r.kind == ChangeKind::Renamed)
                            .map(bb_paths::file_name)
                            .filter(|n| !n.is_empty())
                            .map(|n| format!("was {n}"))
                            .unwrap_or_default();
                        json!({
                            "kind": kind_label(r.kind),
                            "name": if name.is_empty() { r.display_path.as_str() } else { name },
                            "path": r.display_path,
                            "folder": bb_paths::split_last(&r.display_path).0,
                            "isDirectory": r.is_directory,
                            "deleted": r.kind == ChangeKind::Deleted,
                            "renamedFrom": renamed_from,
                            "countText": if r.count > 1 { format!("\u{d7}{}", bb_core::format::thousands(i64::from(r.count))) } else { String::new() },
                            "when": relative_time(r.last_utc, now),
                            "lastUtc": r.last_utc.0,
                            "firstUtc": r.first_utc.0,
                            "count": r.count,
                            "oldPath": r.old_display_path,
                        })
                    })
                    .collect();
            }
        }

        let summary = match (rows.len(), truncated) {
            (0, _) => String::new(),
            (n, true) => format!(
                "Showing the newest {} changes.",
                bb_core::format::thousands(n as i64)
            ),
            (1, _) => "1 change".into(),
            (n, _) => format!("{} changes", bb_core::format::thousands(n as i64)),
        };
        let empty_message =
            if !rows.is_empty() || availability == ChangeTimelineAvailability::RecordingOff {
                String::new()
            } else if self.range == ChangeRange::SinceMark && mark.is_none() {
                "Press \"Mark now\", then do the thing you want to watch.".into()
            } else {
                ChangeLogRules::empty_message(self.range, scoped)
            };
        json!({
            "scope": self.scope,
            "scopeDisplay": self.scope.clone().unwrap_or_else(|| "This PC".into()),
            "range": RANGES.iter().find(|r| r.2 == self.range).map(|r| r.0),
            "ranges": RANGES.iter().map(|r| json!({"id": r.0, "label": r.1})).collect::<Vec<_>>(),
            "created": self.kinds[0], "modified": self.kinds[1],
            "deleted": self.kinds[2], "renamed": self.kinds[3],
            "hidden": self.include_hidden,
            "markText": mark.map(|m| format!("Marked at {}", m.to_local().hhmm())).unwrap_or_default(),
            "recording": recording_on,
            "availability": format!("{availability:?}"),
            "hasMessage": availability != ChangeTimelineAvailability::Ready,
            "message": message,
            "canOpenSettings": availability == ChangeTimelineAvailability::RecordingOff,
            "canRetry": availability == ChangeTimelineAvailability::IndexerUnavailable && index.can_start(),
            "rows": rows,
            "summary": summary,
            "emptyMessage": empty_message,
        })
    }
}

impl Inner {
    /// Pushes the recording setting to the helper and holds the table to it: off wipes it, on prunes
    /// to the retention. Called when either setting moves, never on a keystroke elsewhere.
    pub(crate) fn change_log_policy_changed(&self) {
        let policy = lock(&self.settings).effective_change_log_policy();
        self.seams.index.set_change_log(policy);
        let repo = ChangeLogRepository::new(self.db.clone());
        if policy.enabled {
            let _ = repo.prune(FileTime::now(), policy.retention, ChangeLogRules::MAX_ROWS);
        } else {
            let _ = repo.clear();
        }
    }

    /// Stamps "now" as the mark the "since the mark" range counts from.
    pub(crate) fn change_mark_now(&self) -> AppResult<()> {
        lock(&self.settings).change_timeline_mark_utc = Some(FileTime::now());
        self.save_settings_now()
    }

    /// The history switch's "Clear now".
    /// What the live window polls instead of re-running its query: moves whenever a row is added or
    /// folded, and with the recording switch. Spec: ChangeTimelineViewModel.OnTick.
    pub(crate) fn change_log_stamp(&self) -> Value {
        let stamp = ChangeLogRepository::new(self.db.clone()).stamp().ok();
        let recording = crate::app::lock(&self.settings).record_file_changes;
        json!({
            "stamp": stamp.map(|s| format!("{}|{}", s.max_id, s.last_utc.map_or(0, |t| t.0))).unwrap_or_default(),
            "recording": recording,
        })
    }

    pub(crate) fn change_log_count(&self) -> i64 {
        ChangeLogRepository::new(self.db.clone())
            .count()
            .unwrap_or(0)
    }

    pub(crate) fn change_log_clear(&self) -> AppResult<()> {
        ChangeLogRepository::new(self.db.clone())
            .clear()
            .map_err(|e| AppError::Invalid(e.to_string()))
    }

    /// Writes a spread of recorded changes under `root`, through the repository the index helper
    /// writes through, and turns recording on so the window will show them (the harness's
    /// `changes-seed`). Posed rather than provoked: nothing here tails a journal. What it exercises
    /// is everything after the record: coalescing (the log written five times is one row, x5), a
    /// rename with its old name, a hidden entry, a row too old for the hour preset, and one past
    /// retention that the prune drops before the window ever sees it. Returns the row count.
    pub(crate) fn change_log_seed(&self, root: &str) -> AppResult<i64> {
        lock(&self.settings).record_file_changes = true;
        self.save_settings_now()?;
        let repo = ChangeLogRepository::new(self.db.clone());
        let now = FileTime::now();
        let path_of = |rel: &str| format!("{}\\{}", root.trim_end_matches('\\'), rel);
        let ev = |rel: &str,
                  kind: ChangeKind,
                  secs_ago: i64,
                  is_dir: bool,
                  hidden: bool,
                  from: Option<&str>| {
            let path = path_of(rel);
            ChangeEvent {
                path_key: bb_paths::PathKey::canonicalize(&path)
                    .unwrap_or_else(|_| path.to_uppercase()),
                display_path: path,
                is_directory: is_dir,
                hidden,
                kind,
                old_display_path: from.map(path_of),
                utc: FileTime(now.0 - secs_ago * bb_core::time::TICKS_PER_SECOND),
            }
        };
        use ChangeKind::{Created, Deleted, Modified, Renamed};
        let mut events = vec![
            ev("Old\\stale.txt", Created, 30 * 3600, false, false, None),
            ev("Docs\\readme.txt", Created, 5 * 3600, false, false, None),
            ev("Setup\\setup.tmp", Created, 120, false, false, None),
            ev("Setup\\setup.tmp", Created, 110, false, false, None),
            ev("App\\app.exe.config", Modified, 90, false, false, None),
            ev("Setup\\setup.tmp", Deleted, 60, false, false, None),
            ev(
                "Downloads\\installer.msi",
                Renamed,
                45,
                false,
                false,
                Some("Downloads\\installer.part"),
            ),
            ev("App\\plugins", Created, 40, true, false, None),
            ev(".cache\\index", Created, 30, false, true, None),
            ev("App\\app.exe", Modified, 20, false, false, None),
        ];
        for secs in [15, 14, 13, 12, 11] {
            events.push(ev("Logs\\app.log", Modified, secs, false, false, None));
        }
        events.push(ev("App\\old-plugin.dll", Deleted, 5, false, false, None));
        repo.record(&events)
            .map_err(|e| AppError::Invalid(e.to_string()))?;
        let policy = lock(&self.settings).effective_change_log_policy();
        let _ = repo.prune(now, policy.retention, ChangeLogRules::MAX_ROWS);
        repo.count().map_err(|e| AppError::Invalid(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_range_the_window_offers_round_trips() {
        for (id, _, range) in RANGES {
            assert_eq!(range_of(id), Some(range));
        }
        assert_eq!(range_of("yesterday"), None);
    }

    #[test]
    fn kinds_and_hidden_are_settable_and_the_rest_left_alone() {
        let mut s = ChangesSession::new(Some("C:\\x".into()), false);
        s.set(&json!({"deleted": false, "hidden": true, "range": "24h"}));
        assert_eq!(s.kinds, [true, true, false, true]);
        assert!(s.include_hidden);
        assert_eq!(s.range, ChangeRange::Last24Hours);
        s.set(&json!({"range": "nonsense"}));
        assert_eq!(s.range, ChangeRange::Last24Hours);
    }

    #[test]
    fn an_empty_scope_means_every_drive() {
        assert!(
            ChangesSession::new(Some(String::new()), false)
                .scope
                .is_none()
        );
    }
}
