//! `ops_do`: a verb of the operations UI. One private verb enum, one small method per verb.

use std::collections::HashMap;

use bb_core::archives::ArchiveEdit;
use bb_core::transfer::{ConflictResolution, Resolutions, TransferPlanner, TransferVerb};
use bb_core::undo::UndoBudget;
use bb_integration::shellmenu::StaticVerbCommand;
use bb_paths::file_name;
use serde_json::json;

use super::{invalid, op_json, parse, strings, text};
use crate::app::App;
use crate::app::lock;
use crate::ids::{AppResult, TabId};
use crate::ops::ui::parse_resolution;

verb_enum! {
    /// Every verb `App::ops_do` answers.
    OpsVerb {
        QueuePause = "queue-pause",
        QueueResume = "queue-resume",
        QueueMove = "queue-move",
        QueueCancel = "queue-cancel",
        CancelRunning = "cancel-running",
        PoseTransfer = "pose-transfer",
        ConflictAnswer = "conflict-answer",
        ConfirmAnswer = "confirm-answer",
        DeleteConfirm = "delete-confirm",
        DeleteCancel = "delete-cancel",
        NoticeDismiss = "notice-dismiss",
        ElevationAnswer = "elevation-answer",
        HarnessConflict = "h:conflict",
        HarnessConflictPlan = "h:conflict-plan",
        HarnessElevationReset = "h:elevation-reset",
        Extract = "extract",
        Compress = "compress",
        Unlock = "unlock",
        ArchiveAdd = "archive-add",
        Paste = "paste",
        Shortcut = "shortcut",
        ShellVerb = "shell-verb",
        UndoBudget = "undo-budget",
    }
}

impl App {
    /// A verb of the operations UI. Answers JSON: `{"op": N}` when it started work.
    pub fn ops_do(&self, verb: &str, arg: &str) -> AppResult<String> {
        let inner = &self.inner;
        let verb: OpsVerb = verb
            .parse()
            .map_err(|()| invalid(format!("'{verb}' is not an operations verb.")))?;
        match verb {
            // ---- the queue ----
            OpsVerb::QueuePause => {
                inner.set_paused(true);
                Ok("{}".into())
            }
            OpsVerb::QueueResume => {
                inner.set_paused(false);
                Ok("{}".into())
            }
            OpsVerb::QueueMove => self.do_queue_move(arg),
            OpsVerb::QueueCancel => {
                inner
                    .queue_cancel(arg.trim().parse().map_err(|_| invalid("not a job id"))?)
                    .map_err(invalid)?;
                Ok("{}".into())
            }
            OpsVerb::CancelRunning => {
                inner.cancel_running();
                Ok("{}".into())
            }
            OpsVerb::PoseTransfer => {
                let (kind, dest) = arg.split_once('|').unwrap_or((arg, ""));
                inner.pose_transfer(kind.trim(), dest);
                Ok("{}".into())
            }

            // ---- clashes ----
            OpsVerb::ConflictAnswer => self.do_conflict_answer(arg),
            OpsVerb::ConfirmAnswer => {
                inner
                    .extra
                    .ops
                    .questions
                    .answer_confirm(arg.trim() == "yes");
                Ok("{}".into())
            }
            OpsVerb::DeleteConfirm => {
                let op = inner.delete_confirm()?;
                Ok(json!({ "op": op.0 }).to_string())
            }
            OpsVerb::DeleteCancel => {
                inner.delete_cancel();
                Ok("{}".into())
            }
            OpsVerb::NoticeDismiss => {
                inner.extra.ops.questions.dismiss_notice(&inner.hub);
                Ok("{}".into())
            }
            OpsVerb::ElevationAnswer => {
                inner
                    .extra
                    .ops
                    .questions
                    .answer_elevation(arg.trim() == "yes");
                Ok("{}".into())
            }
            OpsVerb::HarnessConflict => self.do_harness_conflict(arg),
            OpsVerb::HarnessConflictPlan => self.do_harness_conflict_plan(arg),
            OpsVerb::HarnessElevationReset => Ok("{}".into()),

            // ---- archives and shortcuts ----
            OpsVerb::Extract => self.do_extract(arg),
            OpsVerb::Compress => self.do_compress(arg),
            OpsVerb::Unlock => self.do_unlock(arg),
            OpsVerb::ArchiveAdd => self.do_archive_add(arg),
            OpsVerb::Paste => self.do_paste(arg),
            OpsVerb::Shortcut => {
                let a = parse(arg)?;
                Ok(op_json(inner.op_shortcut(
                    strings(&a["sources"]),
                    text(&a["directory"]).to_string(),
                )?))
            }

            // ---- a registry verb a program added to the menu ----
            OpsVerb::ShellVerb => self.do_shell_verb(arg),

            // ---- the history ----
            OpsVerb::UndoBudget => self.do_undo_budget(arg),
        }
    }

    fn tab_path(&self, tab: TabId) -> AppResult<String> {
        Ok(self.tab_info(tab)?.path)
    }

    fn do_queue_move(&self, arg: &str) -> AppResult<String> {
        let a = parse(arg)?;
        self.inner
            .queue_move(
                a["id"].as_u64().unwrap_or(0),
                a["delta"].as_i64().unwrap_or(0) as i32,
            )
            .map_err(invalid)?;
        Ok("{}".into())
    }

    fn do_conflict_answer(&self, arg: &str) -> AppResult<String> {
        let a = parse(arg)?;
        let q = &self.inner.extra.ops.questions;
        if a["cancel"].as_bool().unwrap_or(false) {
            q.answer_conflicts(None);
        } else {
            let means = q
                .conflict_model()
                .and_then(|m| parse_resolution(text(&m["replaceMeans"])))
                .unwrap_or(ConflictResolution::Replace);
            let mut map: Resolutions = HashMap::new();
            if let Some(obj) = a["answers"].as_object() {
                for (key, word) in obj {
                    let Some(mut r) = parse_resolution(text(word)) else {
                        continue;
                    };
                    if r == ConflictResolution::Replace {
                        r = means;
                    }
                    map.insert(key.clone(), r);
                }
            }
            q.answer_conflicts(Some(map));
        }
        Ok("{}".into())
    }

    fn do_harness_conflict(&self, arg: &str) -> AppResult<String> {
        let probes = self
            .inner
            .probes
            .as_ref()
            .ok_or_else(|| invalid("not a harness run"))?;
        let a = parse(arg)?;
        match text(&a["mode"]) {
            "cancel" => probes.cancel_conflicts(true),
            "all" => probes.answer_all_conflicts(
                parse_resolution(text(&a["answer"])).ok_or_else(|| invalid("not an answer"))?,
            ),
            _ => {
                let mut map = HashMap::new();
                if let Some(obj) = a["answers"].as_object() {
                    for (name, word) in obj {
                        map.insert(
                            name.clone(),
                            parse_resolution(text(word))
                                .ok_or_else(|| invalid(format!("'{word}' is not an answer")))?,
                        );
                    }
                }
                probes.answer_named_conflicts(map);
            }
        }
        Ok("{}".into())
    }

    fn do_harness_conflict_plan(&self, arg: &str) -> AppResult<String> {
        let probes = self
            .inner
            .probes
            .as_ref()
            .ok_or_else(|| invalid("not a harness run"))?;
        let a = parse(arg)?;
        let verb = if text(&a["verb"]) == "move" {
            TransferVerb::Move
        } else {
            TransferVerb::Copy
        };
        let dest = text(&a["destination"]);
        let plan = TransferPlanner::default().plan(&strings(&a["sources"]), dest, verb);
        let expanded = bb_core::transfer::TransferMergeExpander::new(Box::new(
            bb_core::transfer::FileSystemTransferProbe,
        ))
        .expand(plan, &bb_core::cancel::CancelToken::none())
        .map_err(|_| invalid("cancelled"))?;
        probes.offer_conflicts(&expanded);
        Ok(probes.last_conflict_model().to_string())
    }

    fn do_extract(&self, arg: &str) -> AppResult<String> {
        let a = parse(arg)?;
        let tab = TabId(a["tab"].as_u64().unwrap_or(0));
        // An archive file named outright is its own subject; otherwise the tab's place is.
        let path = match a["archive"].as_str() {
            Some(file) => file.to_string(),
            None => self.tab_path(tab)?,
        };
        let op = self.inner.op_extract(
            &path,
            &strings(&a["names"]),
            text(&a["destination"]),
            crate::ops::arch::extract_conflict_of(text(&a["conflict"])),
        )?;
        Ok(op_json(op))
    }

    fn do_compress(&self, arg: &str) -> AppResult<String> {
        let a = parse(arg)?;
        let format_ = crate::ops::arch::write_format_of(text(&a["format"]))
            .ok_or_else(|| invalid("That is not a format this app can write."))?;
        let op = self.inner.op_compress(
            strings(&a["sources"]),
            text(&a["target"]).to_string(),
            format_,
            crate::ops::arch::level_of(text(&a["level"])),
        )?;
        Ok(op_json(op))
    }

    fn do_unlock(&self, arg: &str) -> AppResult<String> {
        let inner = &self.inner;
        let a = parse(arg)?;
        let tab = TabId(a["tab"].as_u64().unwrap_or(0));
        let path = self.tab_path(tab)?;
        let archive = inner
            .archive_file_for(&path)
            .ok_or_else(|| invalid("This tab is not showing an archive."))?;
        inner
            .extra
            .ops
            .passwords
            .remember(&archive, text(&a["password"]));
        self.refresh(tab)?;
        Ok("{}".into())
    }

    fn do_archive_add(&self, arg: &str) -> AppResult<String> {
        let inner = &self.inner;
        let a = parse(arg)?;
        let archive = text(&a["archive"]).to_string();
        let dir = text(&a["entryDir"]);
        // Files only, and always a copy: folders are refused rather than walked - a tree would be
        // an entry per file and a name rule per entry, which is what Compress is for.
        // Spec: DropPipeline.DropIntoArchiveAsync.
        let files: Vec<String> = strings(&a["sources"])
            .into_iter()
            .filter(|s| std::path::Path::new(s).is_file())
            .collect();
        if files.is_empty() {
            inner.set_active_status(
                "Only files can be added to an archive. Use Compress to make a new one.",
            );
            return Ok(json!({ "op": 0 }).to_string());
        }
        let mut edits = Vec::new();
        for s in files {
            let name = file_name(s.trim_end_matches('\\')).to_string();
            let entry = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            edits.push(ArchiveEdit::add_file(&s, &entry));
        }
        Ok(op_json(inner.op_archive_edit(archive, edits, "Added")?))
    }

    /// A paste: a drop by another name, but it says so when there is nothing to do.
    fn do_paste(&self, arg: &str) -> AppResult<String> {
        let a = parse(arg)?;
        let verb = if a["move"].as_bool().unwrap_or(false) {
            TransferVerb::Move
        } else {
            TransferVerb::Copy
        };
        Ok(op_json(self.inner.op_transfer_as(
            strings(&a["sources"]),
            text(&a["destination"]).to_string(),
            verb,
            true,
        )?))
    }

    fn do_shell_verb(&self, arg: &str) -> AppResult<String> {
        let inner = &self.inner;
        let a = parse(arg)?;
        let target = text(&a["target"]);
        let resolved = StaticVerbCommand::resolve(
            a["command"].as_str(),
            target,
            text(&a["workingDirectory"]),
            |p| std::path::Path::new(p).exists(),
        )
        .ok_or_else(|| invalid("That entry's command could not be read."))?;
        // The program with its arguments and the folder the menu was over: "Git Bash Here"
        // takes its folder from the working directory.
        let request = crate::LaunchRequest {
            file: resolved.executable.clone(),
            arguments: resolved.arguments.clone(),
            working_directory: text(&a["workingDirectory"]).to_string(),
            elevated: false,
        };
        match inner.seams.launcher.start(&request) {
            Ok(()) => {
                inner.set_active_status(format!("Started {}", file_name(&resolved.executable)))
            }
            Err(e) => inner.set_active_status(e),
        }
        Ok("{}".into())
    }

    fn do_undo_budget(&self, arg: &str) -> AppResult<String> {
        let inner = &self.inner;
        let a = parse(arg)?;
        let current = lock(&inner.undo).budget();
        let steps = a["steps"]
            .as_i64()
            .map_or(current.max_entries, |n| n as i32);
        let bytes = a["bytes"].as_i64().unwrap_or(current.max_held_bytes);
        inner.set_undo_budget(UndoBudget::new(steps, bytes));
        Ok("{}".into())
    }
}
