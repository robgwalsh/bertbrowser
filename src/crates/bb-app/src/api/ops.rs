//! The two doors the host uses for the operations UI: `ops_query` (a model, as JSON) and `ops_do`
//! (a verb and its argument). Spec: ShellViewModel.UndoHistory, DropPipeline, RenameDialog,
//! ContextMenuComposer, ElevationDialog, TransferConflictsViewModel.
//!
//! Strings in, strings out: a page's whole vocabulary is one function, as in the other `*_do`
//! exports, and the models are plain JSON the host draws. Verbs that start work answer
//! `{"op": N}`; the host waits for the operation the way it waits for any other.

use std::collections::HashMap;

use bb_core::FileTime;
use bb_core::archives::{ArchiveFormats, ArchivePath};
use bb_core::elevation::{ElevatedOperationRunner, ElevatedRun, ElevationOperation};
use bb_core::format;
use bb_core::rename::{
    RenamePart, RenamePattern, RenamePlanner, RenameRule, RenameSource, RenameTemplate,
};
use bb_core::shellmenu::{
    BuiltInMenuItems, BuiltInMenuPlaces, MenuLayoutRules, MenuPlacementKind, MenuSeparatorRules,
    MenuSlot,
};
use bb_core::transfer::{
    RightDropContext, RightDropMenuRules, RightDropVerb, TransferPlanner, TransferVerb,
};
use bb_core::undo::{StepVerdict, UndoBudget, UndoKind};
use bb_paths::{PathKey, file_name};
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::ids::{AppError, AppResult};

mod query;
mod verbs;

fn invalid(text: impl Into<String>) -> AppError {
    AppError::Invalid(text.into())
}

fn parse(arg: &str) -> AppResult<Value> {
    serde_json::from_str(arg).map_err(|e| invalid(format!("not JSON: {e}")))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

fn op_json(op: crate::ids::OpId) -> String {
    json!({ "op": op.0 }).to_string()
}

impl Inner {
    /// Offers a second, elevated pass over whatever Windows refused, and runs it if the user agrees.
    /// `Some` is what the helper did, for the caller to fold into the first pass's outcome; `None`
    /// is "no second pass", with a note saying why when the user would otherwise wonder (the account
    /// cannot elevate, the helper failed). Declining is the user's choice and needs no note.
    ///
    /// **The account is checked before the dialog, never after.** A standard user shown a shield
    /// gets a credential prompt for somebody else's password, which is a good deal worse than
    /// quietly saying it cannot be done.
    pub(crate) fn elevate<T>(
        &self,
        operation: ElevationOperation,
        items: &[String],
        is_undo: bool,
        run: impl FnOnce(&dyn ElevatedOperationRunner) -> ElevatedRun<T>,
    ) -> (Option<T>, String) {
        if !self.seams.elevated.can_elevate() {
            return (
                None,
                " \u{2014} and this account cannot provide administrator permission".to_string(),
            );
        }
        if !self.offer_elevation(operation, items, is_undo) {
            return (None, String::new());
        }
        let ran = run(self.seams.elevated.as_ref());
        match ran.result {
            Some(second) => (Some(second), String::new()),
            None if ran.detail.is_empty() => (None, String::new()),
            None => (None, format!(" \u{2014} {}", ran.detail)),
        }
    }

    /// Asks whether to try again with administrator rights. Remembers the offer for the dialog and
    /// for a script. `true` means "retry it elevated".
    fn offer_elevation(
        &self,
        operation: ElevationOperation,
        items: &[String],
        is_undo: bool,
    ) -> bool {
        let count = items.len();
        let noun = if count == 1 {
            "1 item".to_string()
        } else {
            format!("{} items", format::thousands(count as i64))
        };
        let verb = match operation {
            ElevationOperation::TransferMove => {
                if is_undo {
                    "moved back"
                } else {
                    "moved"
                }
            }
            ElevationOperation::TransferCopy => "copied",
            ElevationOperation::Delete => {
                if is_undo {
                    "restored"
                } else {
                    "deleted"
                }
            }
            ElevationOperation::Rename => {
                if is_undo {
                    "renamed back"
                } else {
                    "renamed"
                }
            }
            ElevationOperation::NewItem => "created",
            ElevationOperation::TransferUndo => "moved back",
            ElevationOperation::DeleteUndo => "restored",
        };
        let model = json!({
            "operation": format!("{operation:?}"),
            "isUndo": is_undo,
            "headline": format!("{noun} could not be {verb} \u{2014} Windows refused permission."),
            "explanation": "Windows will ask you to confirm. Everything else in this operation has already been done.",
            "items": items,
        });
        *lock(&self.extra.ops.elevation_offer) = Some(model.clone());
        self.seams.elevation.offer(&model.to_string())
    }

    /// The History window's model. While a walk holds the history (and may be waiting on the user),
    /// the last one is answered again rather than blocking the window that has to ask the question.
    pub(crate) fn history_json(&self) -> Value {
        let Some(undo) = crate::app::try_lock(&self.undo) else {
            return lock(&self.extra.history_cache).clone();
        };
        let model = self.history_json_of(&undo);
        *lock(&self.extra.history_cache) = model.clone();
        model
    }

    fn history_json_of(&self, undo: &bb_core::undo::UndoStack) -> Value {
        let now = FileTime::now();
        let cursor = undo.cursor();
        let entries = undo.entries();
        let next_undo = undo.next_undo().map(|e| e.id());
        let next_redo = undo.next_redo().map(|e| e.id());
        let mut rows = Vec::new();
        for (i, entry) in entries.iter().enumerate().rev() {
            let record = entry.record();
            let held = entry.held();
            let held_text = if held.bytes > 0 || !held.complete {
                format!(
                    "\u{b7} {}{} held",
                    if held.complete { "" } else { "\u{2265} " },
                    format::byte_size(held.bytes)
                )
            } else {
                String::new()
            };
            let (badge, problem) = match (entry.last_report(), entry.last_direction()) {
                (Some(report), Some(direction)) => {
                    let undo_dir = direction == bb_core::undo::UndoDirection::Undo;
                    let badge = match report.verdict() {
                        StepVerdict::Failed => {
                            if undo_dir {
                                "Undo failed"
                            } else {
                                "Redo failed"
                            }
                        }
                        StepVerdict::Partial => {
                            if undo_dir {
                                "Partly undone"
                            } else {
                                "Partly redone"
                            }
                        }
                        StepVerdict::Clean => {
                            if entry.is_done() {
                                ""
                            } else {
                                "Undone"
                            }
                        }
                    };
                    let problem = if report.clean() {
                        String::new()
                    } else {
                        let first = report.failures.first().cloned().unwrap_or_default();
                        if report.failures.len() > 1 {
                            format!("{first} (and {} more)", report.failures.len() - 1)
                        } else {
                            first
                        }
                    };
                    (badge, problem)
                }
                _ => ("", String::new()),
            };
            let details = record.details();
            let mut lines: Vec<String> = details
                .iter()
                .take(200)
                .map(|d| {
                    if d.from.eq_ignore_ascii_case(&d.to) {
                        d.from.clone()
                    } else {
                        format!("{}  \u{2192}  {}", d.from, d.to)
                    }
                })
                .collect();
            if details.len() > 200 {
                lines.push(format!(
                    "\u{2026}and {} more",
                    format::thousands((details.len() - 200) as i64)
                ));
            }
            let reveal = details.first().map(|f| {
                if entry.is_done() && record.kind() != UndoKind::Delete {
                    f.to.clone()
                } else {
                    f.from.clone()
                }
            });
            rows.push(json!({
                "id": entry.id().0,
                "description": record.description(),
                "icon": match record.kind() {
                    UndoKind::Rename => "Icon.Rename",
                    UndoKind::Move => "Icon.MoveToFolder",
                    UndoKind::Paste => "Icon.Paste",
                    UndoKind::Delete => "Icon.Delete",
                    UndoKind::ArchiveEdit => "Icon.Archive",
                    UndoKind::Sync => "Icon.Compare",
                    UndoKind::MetadataEdit => "Icon.Metadata",
                    UndoKind::Timestamps => "Icon.FileDates",
                },
                "isDone": entry.is_done(),
                "isNextUndo": next_undo == Some(entry.id()),
                "isNextRedo": next_redo == Some(entry.id()),
                "whenText": format::relative_time(entry.changed_utc(), now),
                "itemsText": bb_core::undo::UndoText::items(record.item_count()),
                "heldText": held_text,
                "badge": badge,
                "problem": problem,
                "details": lines,
                "revealPath": reveal,
                "steps": if entry.is_done() { cursor - i } else { i - cursor + 1 },
            }));
        }
        let held = undo.held();
        let budget = undo.budget();
        let limit = if budget.max_held_bytes % (1 << 30) == 0 {
            format!("{} GB", format::thousands(budget.max_held_bytes >> 30))
        } else {
            format::byte_size(budget.max_held_bytes)
        };
        let holding = if held.bytes == 0 && held.complete {
            format!("Holding no set-aside files (limit {limit})")
        } else {
            format!(
                "Holding {}{} of {limit}",
                if held.complete { "" } else { "at least " },
                format::byte_size(held.bytes)
            )
        };
        let released = if undo.released_count() == 0 {
            String::new()
        } else {
            let mut t = format!(
                "{} released to stay within the limits",
                bb_core::undo::UndoText::items(undo.released_count())
                    .replace("item", "older action")
            );
            if undo.released_bytes() > 0 {
                t.push_str(&format!(
                    " ({} removed for good)",
                    format::byte_size(undo.released_bytes())
                ));
            }
            t
        };
        // The one Clear, for the History window's button, the Settings page's and the command.
        let what = if held.bytes > 0 {
            format!(
                " BertBrowser is holding {}{} of replaced and deleted files for them, which will be removed for good.",
                if held.complete { "" } else { "at least " },
                format::byte_size(held.bytes)
            )
        } else {
            String::new()
        };
        let clear_question = format!(
            "Clear the undo history? None of these actions can be undone or redone afterwards.{what} Items sent to the Recycle Bin stay there."
        );
        json!({
            "count": entries.len(),
            "cursor": cursor,
            "clearQuestion": clear_question,
            "released": undo.released_count(),
            "releasedBytes": undo.released_bytes(),
            "maxEntries": budget.max_entries,
            "maxHeldBytes": budget.max_held_bytes,
            "footer": format!(
                "{holding} \u{b7} {} of {} actions",
                format::thousands(entries.len() as i64),
                format::thousands(i64::from(budget.max_entries))
            ),
            "releasedText": released,
            "canStep": !lock(&self.queue.state).draining,
            "rows": rows,
        })
    }

    pub(crate) fn set_undo_budget(&self, budget: UndoBudget) {
        let released = {
            let mut undo = lock(&self.undo);
            let released = undo.set_budget(budget);
            self.refresh_undo_summary(&undo);
            released
        };
        released.retire_all();
        self.hub.emit(crate::events::AppEvent::UndoChanged);
    }

    fn drop_has_work(&self, sources: &[String], destination: &str, verb: TransferVerb) -> bool {
        TransferPlanner::default()
            .plan(sources, destination, verb)
            .has_work()
    }

    fn right_drop_json(&self, sources: &[String], destination: &str) -> Value {
        let archive_dest = crate::ops::arch::archive_of(destination).is_some()
            || (std::path::Path::new(destination).is_file()
                && ArchiveFormats::is_archive_name(file_name(destination)));
        let virtual_sources = sources.iter().any(|s| ArchivePath::looks_virtual(s));
        let (move_work, copy_work) = if archive_dest {
            (false, !virtual_sources)
        } else if virtual_sources {
            (false, false)
        } else {
            (
                self.drop_has_work(sources, destination, TransferVerb::Move),
                self.drop_has_work(sources, destination, TransferVerb::Copy),
            )
        };
        let entries = RightDropMenuRules::build(RightDropContext {
            destination_is_archive: archive_dest,
            sources_are_virtual: virtual_sources,
            move_has_work: move_work,
            copy_has_work: copy_work,
            item_count: sources.len(),
        });
        json!(
            entries
                .iter()
                .map(|e| json!({
                    "verb": match e.verb {
                        RightDropVerb::Move => "move",
                        RightDropVerb::Copy => "copy",
                        RightDropVerb::Shortcut => "shortcut",
                    },
                    "label": e.label,
                    "enabled": e.enabled,
                }))
                .collect::<Vec<_>>()
        )
    }

    /// What the rename box would do with this, row by row.
    fn rename_preview_json(
        &self,
        paths: &[String],
        rule: Option<RenameRule>,
        simple: &str,
    ) -> Value {
        let mut sources: Vec<RenameSource> = paths
            .iter()
            .map(|p| {
                let mut s = if std::path::Path::new(p).is_dir() {
                    RenameSource::dir(p)
                } else {
                    RenameSource::file(p)
                };
                s.modified = std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .ok()
                    .map(|t| FileTime::from_system_time(t).to_local());
                s
            })
            .collect();
        let planner = RenamePlanner::local();
        let (plan, hint) = match &rule {
            Some(rule) => {
                if RenameTemplate::uses_tags(&rule.template) {
                    let mut cache = lock(&self.extra.tools.rename_tags);
                    if cache.len() > 4096 {
                        cache.clear();
                    }
                    for s in &mut sources {
                        let hit = cache
                            .get(&s.path)
                            .filter(|(when, _)| *when == s.modified)
                            .map(|(_, tags)| tags.clone());
                        let tags = hit.unwrap_or_else(|| {
                            let read =
                                crate::side_pane::metadata_pane::tags_of(&s.path, s.is_directory);
                            cache.insert(s.path.clone(), (s.modified, read.clone()));
                            read
                        });
                        s.tags = Some(tags);
                    }
                }
                let hint = RenameTemplate::parse(rule.template.trim()).ok().and_then(|segments| {
                    let names = RenameTemplate::uses(
                        &segments,
                        &[RenamePart::Name, RenamePart::Base, RenamePart::Extension],
                    );
                    if !rule.find.is_empty() && !names {
                        return Some(
                            "Find and replace has nothing to act on \u{2014} the name has no {name}, {base} or {ext} in it."
                                .to_string(),
                        );
                    }
                    let keeps = RenameTemplate::uses(&segments, &[RenamePart::Name, RenamePart::Extension]);
                    if RenameTemplate::uses(&segments, &[RenamePart::Counter])
                        && !keeps
                        && sources.iter().any(|s| !RenamePattern::split(s).1.is_empty())
                    {
                        return Some(
                            "These names will lose their extensions \u{2014} add {ext} to keep them."
                                .to_string(),
                        );
                    }
                    None
                });
                (planner.plan(&sources, rule), hint)
            }
            None => (planner.plan_simple(&sources, simple), None),
        };
        let key = |p: &str| PathKey::canonicalize(p).unwrap_or_else(|_| p.to_string());
        let mut planned: HashMap<String, &bb_core::rename::PlannedRename> = HashMap::new();
        for r in &plan.renames {
            planned.insert(key(&r.source_path), r);
        }
        let mut refused: HashMap<String, &bb_core::rename::RejectedRename> = HashMap::new();
        for r in &plan.rejected {
            if !r.source_path.is_empty() {
                refused.entry(key(&r.source_path)).or_insert(r);
            }
        }
        let rows: Vec<Value> = sources
            .iter()
            .map(|s| {
                let k = key(&s.path);
                if let Some(r) = planned.get(&k) {
                    json!({"name": s.name(), "newName": r.target_name(), "problem": null,
                           "unchanged": r.is_no_op()})
                } else if let Some(r) = refused.get(&k) {
                    json!({"name": s.name(), "newName": "", "problem": r.message, "unchanged": false})
                } else {
                    json!({"name": s.name(), "newName": s.name(), "problem": null, "unchanged": true})
                }
            })
            .collect();
        let work = plan.work().len();
        json!({
            "suggestion": RenamePattern::suggest_for(&sources),
            "rows": rows,
            "problem": plan.rejected.first().map(|r| r.message.clone()),
            "hint": hint,
            "hasWork": plan.has_work(),
            "work": work,
        })
    }

    fn context_menu_json(&self) -> Value {
        let settings = lock(&self.settings);
        let hidden_built: Vec<String> = settings.hidden_built_in_menu_items.clone();
        let layout = MenuLayoutRules::normalize(
            settings.context_menu_layout.as_deref(),
            hidden_built.iter().map(String::as_str),
        );
        let places = |p: BuiltInMenuPlaces| match p {
            BuiltInMenuPlaces::FileList => "file-list",
            BuiltInMenuPlaces::FolderTree => "folder-tree",
            BuiltInMenuPlaces::Both => "both",
        };
        json!({
            "builtIns": BuiltInMenuItems::all().iter().map(|i| json!({
                "id": i.id,
                "token": MenuLayoutRules::built_in_token(i.id),
                "name": i.name,
                "icon": i.icon,
                "commandId": i.command_id,
                "places": places(i.places),
                "detail": if i.places == BuiltInMenuPlaces::Both { String::new() }
                          else { format!("{} only", BuiltInMenuItems::places_text(i.places)) },
            })).collect::<Vec<_>>(),
            "layout": layout,
            "defaultLayout": MenuLayoutRules::default_layout(),
            "hiddenBuiltIns": hidden_built,
            "hiddenShell": settings.hidden_shell_extensions,
            "showShellExtensions": settings.show_shell_extensions,
            "commands": settings.custom_commands.iter().map(|c| json!({
                "id": c.id, "name": c.name, "command": c.command, "arguments": c.arguments,
                "showInMenu": c.show_in_menu,
                "appliesToFiles": c.applies_to_files,
                "appliesToDirectories": c.applies_to_directories,
                "runElevated": c.run_elevated,
            })).collect::<Vec<_>>(),
        })
    }

    /// The order one opening of a menu lays its entries out in.
    fn menu_arrange_json(&self, arg: &Value) -> Value {
        let settings = lock(&self.settings);
        let layout = MenuLayoutRules::normalize(
            settings.context_menu_layout.as_deref(),
            settings
                .hidden_built_in_menu_items
                .iter()
                .map(String::as_str),
        );
        let built_ins = strings(&arg["builtIns"]);
        let commands = strings(&arg["commands"]);
        let shell = strings(&arg["shell"]);
        let placements = MenuLayoutRules::arrange(&layout, built_ins, &commands, &shell);
        json!(
            placements
                .iter()
                .map(|p| json!({
                    "kind": match p.kind {
                        MenuPlacementKind::BuiltIn => "builtin",
                        MenuPlacementKind::Separator => "separator",
                        MenuPlacementKind::Command => "command",
                        MenuPlacementKind::Shell => "shell",
                    },
                    "id": p.id,
                }))
                .collect::<Vec<_>>()
        )
    }
}

fn tidy_json(arg: &Value) -> Value {
    let slots: Vec<MenuSlot> = arg["slots"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| match s.as_str().unwrap_or("item") {
                    "separator" => MenuSlot::separator(),
                    "hidden" => MenuSlot::item(false),
                    _ => MenuSlot::item(true),
                })
                .collect()
        })
        .unwrap_or_default();
    json!(MenuSeparatorRules::apply(&slots))
}
