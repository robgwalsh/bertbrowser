//! The "are you sure" before a delete. It names every item and where it lives, says what the planner
//! refused, and - as a survey walks the trees off the UI thread - how much each item amounts to,
//! because "delete 1 item" and "delete 1 folder holding 40,000 files" are not the same question.
//! What is shown is what goes: Delete hands the planned items, and only those, to the operation,
//! whose executor re-checks each against disk. Spec: Views/DeleteDialog,
//! DirectoryTabView.DeleteSelectionAsync.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bb_core::cancel::CancelToken;
use bb_core::delete::{
    DeleteDisposition, DeleteMeasurement, DeleteMode, DeletePlanner, DeleteSource, DeleteSurveyor,
};
use bb_core::format::{byte_size, thousands};
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::events::Model;
use crate::ids::{AppError, AppResult, OpId};

/// How many refusals to spell out before summarising the rest.
const PROBLEM_LINES: usize = 3;

/// The delete the dialog is asking about.
#[derive(Default)]
pub(crate) struct DeleteConfirm {
    pending: Mutex<Option<Pending>>,
}

struct Pending {
    paths: Vec<String>,
    mode: DeleteMode,
    /// Each row's source path, in the order shown; a measurement fills its row in.
    rows: Vec<String>,
    measured: Arc<Mutex<HashMap<String, DeleteMeasurement>>>,
    cancel: CancelToken,
}

fn plural(n: usize, one: &str) -> String {
    format!(
        "{} {one}{}",
        thousands(n as i64),
        if n == 1 { "" } else { "s" }
    )
}

/// The right-hand column: a file is just its size, a folder is what it holds.
fn detail(m: &DeleteMeasurement) -> String {
    if !m.is_directory {
        return if m.incomplete {
            "\u{2014}".into()
        } else {
            byte_size(m.bytes as i64)
        };
    }
    let contents = if m.files == 0 {
        "empty".to_string()
    } else {
        format!("{}, {}", plural(m.files, "file"), byte_size(m.bytes as i64))
    };
    if m.incomplete {
        contents + " *"
    } else {
        contents
    }
}

impl Inner {
    /// What deleting `paths` (the active tab's selection when empty) would do, and starts adding
    /// it up: `{hasWork, archive, permanent, title, heading, undoHint, problems, items:[{name,
    /// location, path}]}`. With nothing the rules allow, `hasWork` is false and `problems` says why.
    pub(crate) fn delete_preview(
        self: &Arc<Self>,
        paths: Vec<String>,
        permanent: bool,
    ) -> AppResult<Value> {
        self.delete_cancel();
        let paths = if paths.is_empty() {
            let tab = lock(&self.shell).active_tab_id().ok_or(AppError::Stale)?;
            self.selected_paths(tab)?
        } else {
            paths
        };
        if paths.is_empty() {
            return Err(AppError::Invalid("Nothing is selected.".into()));
        }
        let undo = self.keymap_gesture("edit.undo");
        let undo = if undo.is_empty() {
            "Undo".to_string()
        } else {
            undo
        };

        // Inside an archive: no planner, no survey and no Recycle Bin. The container is rewritten
        // without the entries, and the whole original is what Undo puts back.
        if crate::ops_arch::archive_of(&paths[0]).is_some() {
            let what = if paths.len() == 1 {
                bb_paths::file_name(&paths[0]).to_string()
            } else {
                format!("{} items", thousands(paths.len() as i64))
            };
            *lock(&self.extra.delete.pending) = Some(Pending {
                paths: paths.clone(),
                mode: DeleteMode::Recycle,
                rows: Vec::new(),
                measured: Arc::default(),
                cancel: CancelToken::new(),
            });
            return Ok(json!({
                "hasWork": true,
                "archive": true,
                "permanent": false,
                "title": "Archive",
                "heading": format!("Remove {what} from this archive?"),
                "undoHint": format!("The whole archive is rewritten to do it. {undo} puts the original back."),
                "problems": null,
                "items": [],
            }));
        }

        let mode = if permanent {
            DeleteMode::Permanent
        } else {
            DeleteMode::Recycle
        };
        let sources: Vec<DeleteSource> = paths
            .iter()
            .map(|p| DeleteSource::new(p, std::path::Path::new(p).is_dir()))
            .collect();
        let plan = DeletePlanner::new(&crate::ops::DELETE_PROBE)
            .recycle_probe(&*crate::ops::RECYCLE_BIN)
            .plan(&sources, mode);

        let problems = plan.problems();
        let problem_text = (!problems.is_empty()).then(|| {
            let mut lines: Vec<String> = problems
                .iter()
                .take(PROBLEM_LINES)
                .map(|p| p.message.clone())
                .collect();
            if problems.len() > PROBLEM_LINES {
                lines.push(format!(
                    "\u{2026}and {} more.",
                    thousands((problems.len() - PROBLEM_LINES) as i64)
                ));
            }
            if plan.has_work() {
                format!(
                    "{} selected item(s) will not be deleted:\n{}",
                    thousands(problems.len() as i64),
                    lines.join("\n")
                )
            } else {
                lines.join("\n\n")
            }
        });
        if !plan.has_work() {
            return Ok(json!({
                "hasWork": false,
                "archive": false,
                "permanent": permanent,
                "title": "Delete",
                "problems": problem_text,
                "items": [],
            }));
        }

        let verb = if permanent {
            "Permanently delete"
        } else {
            "Delete"
        };
        let heading = if plan.deletions.len() == 1 {
            format!("{verb} '{}'?", plan.deletions[0].name())
        } else {
            format!(
                "{verb} these {} items?",
                thousands(plan.deletions.len() as i64)
            )
        };
        let undo_hint = if permanent {
            String::new()
        } else if mode != DeleteMode::Recycle {
            format!(
                "{undo} puts these back. They are held until the next move, rename or delete \u{2014} or until BertBrowser closes \u{2014} and then removed for good."
            )
        } else if !plan.has_staged_fallback() {
            format!(
                "{undo} puts these back. They also stay in your Recycle Bin until you empty it."
            )
        } else {
            let staged = plan
                .deletions
                .iter()
                .filter(|d| d.disposition == DeleteDisposition::Stage)
                .count();
            format!(
                "{undo} puts these back. They stay in your Recycle Bin until you empty it \u{2014} except {} on a drive with no Recycle Bin, which BertBrowser holds only until the next move, rename or delete.",
                thousands(staged as i64)
            )
        };
        let items: Vec<Value> = plan
            .deletions
            .iter()
            .map(
                |d| json!({ "name": d.name(), "location": d.parent_path(), "path": d.source_path, "isDirectory": d.is_directory }),
            )
            .collect();

        // Measured off the UI thread and cancelled when the dialog goes, so a huge folder never
        // holds the answer up: the list and the buttons are usable at once and the totals fill in.
        let measured: Arc<Mutex<HashMap<String, DeleteMeasurement>>> = Arc::default();
        let cancel = CancelToken::new();
        {
            let (plan, measured, cancel, hub) = (
                plan.clone(),
                measured.clone(),
                cancel.clone(),
                self.hub.clone(),
            );
            std::thread::spawn(move || {
                let report = |m: &DeleteMeasurement| {
                    if cancel.is_cancelled() {
                        return;
                    }
                    lock(&measured).insert(m.source_path.clone(), m.clone());
                    hub.model(Model::DeleteSurvey);
                };
                DeleteSurveyor.survey(&plan, &cancel, Some(&report));
            });
        }
        *lock(&self.extra.delete.pending) = Some(Pending {
            paths: plan
                .deletions
                .iter()
                .map(|d| d.source_path.clone())
                .collect(),
            mode,
            rows: plan
                .deletions
                .iter()
                .map(|d| d.source_path.clone())
                .collect(),
            measured,
            cancel,
        });
        Ok(json!({
            "hasWork": true,
            "archive": false,
            "permanent": permanent,
            "title": if permanent { "Delete permanently" } else { "Delete" },
            "heading": heading,
            "undoHint": undo_hint,
            "problems": problem_text,
            "items": items,
        }))
    }

    /// How far the survey has got: `{totals, details: [text per item, in the preview's order]}`.
    pub(crate) fn delete_survey(&self) -> Value {
        let pending = lock(&self.extra.delete.pending);
        let Some(p) = pending.as_ref() else {
            return json!({ "totals": "", "details": [] });
        };
        let measured = lock(&p.measured);
        let details: Vec<String> = p
            .rows
            .iter()
            .map(|k| measured.get(k).map(detail).unwrap_or_default())
            .collect();
        let (mut bytes, mut files, mut dirs, mut incomplete) = (0u64, 0usize, 0usize, false);
        for m in measured.values() {
            bytes += m.bytes;
            files += m.files;
            dirs += m.directories;
            incomplete |= m.incomplete;
        }
        let mut parts = Vec::new();
        if dirs > 0 {
            parts.push(plural(dirs, "folder"));
        }
        if files > 0 {
            parts.push(plural(files, "file"));
        }
        let mut totals = if parts.is_empty() {
            if measured.is_empty() {
                "Adding up what this would remove\u{2026}".to_string()
            } else {
                "Nothing to remove".to_string()
            }
        } else {
            parts.join(" and ")
        };
        if bytes > 0 {
            totals.push_str(&format!(" \u{2014} {}", byte_size(bytes as i64)));
        }
        if !measured.is_empty() && measured.len() < p.rows.len() {
            totals.push_str(" (still counting\u{2026})");
        } else if incomplete {
            totals.push_str(". Some items could not be read, so this is a lower bound.");
        }
        json!({ "totals": totals, "details": details })
    }

    /// The user said Delete: exactly the items that were shown go.
    pub(crate) fn delete_confirm(self: &Arc<Self>) -> AppResult<OpId> {
        let pending = lock(&self.extra.delete.pending).take().ok_or_else(|| {
            AppError::Invalid("There is no delete waiting to be confirmed.".into())
        })?;
        pending.cancel.cancel();
        self.op_delete(pending.paths, pending.mode)
    }

    /// The dialog went without a Delete: stop adding up and forget the plan.
    pub(crate) fn delete_cancel(&self) {
        if let Some(p) = lock(&self.extra.delete.pending).take() {
            p.cancel.cancel();
        }
    }
}
