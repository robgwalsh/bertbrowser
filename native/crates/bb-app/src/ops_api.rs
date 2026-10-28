//! The two doors the host uses for the operations UI: `ops_query` (a model, as JSON) and `ops_do`
//! (a verb and its argument). Spec: ShellViewModel.UndoHistory, DropPipeline, RenameDialog,
//! ContextMenuComposer, ElevationDialog, TransferConflictsViewModel.
//!
//! Strings in, strings out: a page's whole vocabulary is one function, as in the other `*_do`
//! exports, and the models are plain JSON the host draws. Verbs that start work answer
//! `{"op": N}`; the host waits for the operation the way it waits for any other.

use std::collections::{HashMap, HashSet};

use bb_core::FileTime;
use bb_core::archives::{ArchiveEdit, ArchiveFormats, ArchivePath, ArchiveWriteRules};
use bb_core::elevation::ElevationOperation;
use bb_core::format;
use bb_core::rename::{
    RenamePart, RenamePattern, RenamePlanner, RenameRule, RenameSource, RenameTemplate,
};
use bb_core::shellmenu::{
    BuiltInMenuItems, BuiltInMenuPlaces, Guid, MenuLayoutRules, MenuPlacementKind,
    MenuSeparatorRules, MenuSlot, ShellExtensionKind, ShellFileType, ShellHandlerRegistration,
    ShellMenuContext, ShellMenuKeys, ShellMenuRules, ShellMenuTarget, ShellVerbRegistration,
    StaticVerbCommand,
};
use bb_core::transfer::{
    ConflictResolution, DragOutAction, DragOutContract, DropEffect, DropInContract, DropOrigin,
    Resolutions, RightDropContext, RightDropMenuRules, RightDropVerb, TransferPlanner,
    TransferVerb,
};
use bb_core::undo::{StepVerdict, UndoBudget, UndoKind};
use bb_paths::{PathKey, file_name};
use serde_json::{Value, json};

use crate::app::App;
use crate::app::{Inner, lock};
use crate::ids::{AppError, AppResult, TabId};
use crate::ops_ui::parse_resolution;

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
    /// Asks whether to try again with administrator rights. Remembers the offer for the dialog and
    /// for a script. The helper that would carry the retry out is not wired to a transport in this
    /// build, so an accepted offer is reported rather than carried out.
    pub(crate) fn offer_elevation(
        &self,
        operation: ElevationOperation,
        items: &[String],
        is_undo: bool,
    ) -> &'static str {
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
        if self.seams.elevation.offer(&model.to_string()) {
            " \u{2014} administrator rights were agreed to, but the elevated helper is not connected in this build"
        } else {
            ""
        }
    }

    pub(crate) fn history_json(&self) -> Value {
        let undo = lock(&self.undo);
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
        json!({
            "count": entries.len(),
            "cursor": cursor,
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
        let archive_dest = crate::ops_arch::archive_of(destination).is_some()
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
                    for s in &mut sources {
                        s.tags = Some(crate::metadata_pane::tags_of(&s.path, s.is_directory));
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

impl App {
    /// A model of the operations UI, as JSON text.
    pub fn ops_query(&self, what: &str, arg: &str) -> AppResult<String> {
        let inner = &self.inner;
        let value = match what {
            "queue" => inner.queue_json(),
            "history" => inner.history_json(),
            "conflicts" => {
                let pending = inner.extra.ops.questions.conflict_model();
                let last = inner
                    .probes
                    .as_ref()
                    .map(|p| p.last_conflict_model())
                    .unwrap_or(Value::Null);
                json!({ "pending": pending, "last": last,
                        "asked": inner.probes.as_ref().map_or(0, |p| p.conflict_asks()) })
            }
            "elevation" => json!({
                "pending": inner.extra.ops.questions.elevation_model(),
                "last": lock(&inner.extra.ops.elevation_offer).clone(),
                "offers": inner.probes.as_ref().map_or(0, |p| p.elevation_offers().len()),
            }),
            "right-drop-menu" => {
                let a = parse(arg)?;
                inner.right_drop_json(&strings(&a["sources"]), text(&a["destination"]))
            }
            "drop-allowed" => {
                let a = parse(arg)?;
                let verb = if text(&a["verb"]) == "move" {
                    TransferVerb::Move
                } else {
                    TransferVerb::Copy
                };
                let sources = strings(&a["sources"]);
                let dest = text(&a["destination"]);
                let archive = crate::ops_arch::archive_of(dest).is_some()
                    || (std::path::Path::new(dest).is_file()
                        && ArchiveFormats::is_archive_name(file_name(dest)));
                let ok = if archive {
                    !sources.iter().any(|s| ArchivePath::looks_virtual(s))
                } else {
                    inner.drop_has_work(&sources, dest, verb)
                };
                json!({ "ok": ok })
            }
            "drop-decide" => {
                let a = parse(arg)?;
                let origin = if text(&a["origin"]) == "external" {
                    DropOrigin::External
                } else {
                    DropOrigin::InApp
                };
                let allowed = DropEffect(a["allowed"].as_u64().unwrap_or(7) as u32);
                let d = DropInContract::decide(
                    origin,
                    a["control"].as_bool().unwrap_or(false),
                    a["shift"].as_bool().unwrap_or(false),
                    allowed,
                );
                json!({
                    "verb": if d.verb == TransferVerb::Move { "move" } else { "copy" },
                    "report": d.report.0,
                    "acceptable": DropInContract::can_accept(allowed),
                })
            }
            "dragout-decide" => {
                let a = parse(arg)?;
                let eff = |v: &Value| v.as_u64().map(|n| DropEffect(n as u32));
                let action = DragOutContract::decide(
                    a["handledInApp"].as_bool().unwrap_or(false),
                    DropEffect(a["returned"].as_u64().unwrap_or(0) as u32),
                    eff(&a["logical"]),
                    eff(&a["performed"]),
                );
                json!(match action {
                    DragOutAction::Nothing => "nothing",
                    DragOutAction::RefreshOnly => "refresh",
                    DragOutAction::RemoveSources => "remove",
                })
            }
            "rename-preview" => {
                let a = parse(arg)?;
                let rule = if a["rule"].is_null() {
                    None
                } else {
                    Some(
                        serde_json::from_value::<RenameRule>(a["rule"].clone())
                            .map_err(|e| invalid(format!("not a rename rule: {e}")))?,
                    )
                };
                inner.rename_preview_json(&strings(&a["paths"]), rule, text(&a["pattern"]))
            }
            "context-menu" => inner.context_menu_json(),
            "shell-keys" => {
                let a = parse(arg)?;
                let targets: Vec<ShellMenuTarget> = a["targets"]
                    .as_array()
                    .map(|t| {
                        t.iter()
                            .map(|t| {
                                ShellMenuTarget::new(
                                    text(&t["path"]),
                                    t["isDirectory"].as_bool().unwrap_or(false),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let context = if text(&a["context"]) == "background" {
                    ShellMenuContext::Background
                } else {
                    ShellMenuContext::Items
                };
                let file_type = &a["fileType"];
                let keys = ShellMenuKeys::for_targets(&targets, context, |_| {
                    Some(ShellFileType::new(
                        file_type["progId"].as_str(),
                        file_type["perceived"].as_str(),
                    ))
                });
                json!(keys)
            }
            "shell-catalog" => {
                let a = parse(arg)?;
                let handlers: Vec<ShellHandlerRegistration> = a["handlers"]
                    .as_array()
                    .map(|h| {
                        h.iter()
                            .filter_map(|h| {
                                Some(ShellHandlerRegistration {
                                    family: text(&h["family"]).to_string(),
                                    name: text(&h["name"]).to_string(),
                                    clsid: Guid::parse(text(&h["clsid"]))?,
                                    class_name: h["className"].as_str().map(str::to_string),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let verbs: Vec<ShellVerbRegistration> = a["verbs"]
                    .as_array()
                    .map(|v| {
                        v.iter()
                            .map(|v| ShellVerbRegistration {
                                family: text(&v["family"]).to_string(),
                                verb: text(&v["verb"]).to_string(),
                                display_name: v["displayName"].as_str().map(str::to_string),
                                icon: v["icon"].as_str().map(str::to_string),
                                command: v["command"].as_str().map(str::to_string),
                                extended: v["extended"].as_bool().unwrap_or(false),
                                legacy_disable: v["legacyDisable"].as_bool().unwrap_or(false),
                                programmatic_only: v["programmaticOnly"].as_bool().unwrap_or(false),
                                has_delegate_execute: v["hasDelegateExecute"]
                                    .as_bool()
                                    .unwrap_or(false),
                                has_sub_commands: v["hasSubCommands"].as_bool().unwrap_or(false),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let catalog = ShellMenuRules::catalog(&handlers, &verbs, &HashSet::new());
                json!(
                    catalog
                        .iter()
                        .map(|e| json!({
                            "id": e.id,
                            "name": e.name,
                            "kind": if e.kind == ShellExtensionKind::Handler { "handler" } else { "verb" },
                            "clsid": e.clsid.map(|g| g.to_braced()),
                            "families": e.families,
                        }))
                        .collect::<Vec<_>>()
                )
            }
            "shell-visible" => {
                let a = parse(arg)?;
                let settings = lock(&inner.settings);
                let hidden = settings.hidden_shell_extensions.clone();
                let enabled = settings.show_shell_extensions;
                drop(settings);
                let ids = strings(&a["ids"]);
                let hidden: HashSet<String> = hidden.iter().map(|h| h.to_uppercase()).collect();
                json!(
                    ids.into_iter()
                        .filter(|i| enabled && !hidden.contains(&i.to_uppercase()))
                        .collect::<Vec<_>>()
                )
            }
            "menu-arrange" => inner.menu_arrange_json(&parse(arg)?),
            "menu-tidy" => tidy_json(&parse(arg)?),
            "compress-defaults" => {
                let a = parse(arg)?;
                json!({
                    "name": ArchiveWriteRules::suggest_name(&strings(&a["sources"]), text(&a["directory"])),
                    "formats": ArchiveWriteRules::formats().iter().map(|f| json!({
                        "label": f.label, "suffix": f.suffix, "supportsLevel": f.supports_level,
                    })).collect::<Vec<_>>(),
                })
            }
            "archive-of" => match inner.archive_file_for(arg) {
                Some(file) => {
                    let index = bb_core::archives::ArchiveBrowser::read_archive(&*inner.fs, &file);
                    json!({
                        "archive": file,
                        "entry": crate::ops_arch::archive_of(arg).map(|a| a.entry_path).unwrap_or_default(),
                        "locked": index.failure == bb_core::archives::ArchiveFailure::PasswordRequired,
                    })
                }
                None => Value::Null,
            },
            other => return Err(invalid(format!("'{other}' is not an operations model."))),
        };
        Ok(value.to_string())
    }

    /// A verb of the operations UI. Answers JSON: `{"op": N}` when it started work.
    pub fn ops_do(&self, verb: &str, arg: &str) -> AppResult<String> {
        let inner = &self.inner;
        match verb {
            // ---- the queue ----
            "queue-pause" => {
                inner.set_paused(true);
                Ok("{}".into())
            }
            "queue-resume" => {
                inner.set_paused(false);
                Ok("{}".into())
            }
            "queue-move" => {
                let a = parse(arg)?;
                inner
                    .queue_move(
                        a["id"].as_u64().unwrap_or(0),
                        a["delta"].as_i64().unwrap_or(0) as i32,
                    )
                    .map_err(invalid)?;
                Ok("{}".into())
            }
            "queue-cancel" => {
                inner
                    .queue_cancel(arg.trim().parse().map_err(|_| invalid("not a job id"))?)
                    .map_err(invalid)?;
                Ok("{}".into())
            }
            "cancel-running" => {
                inner.cancel_running();
                Ok("{}".into())
            }
            "pose-transfer" => {
                let (kind, dest) = arg.split_once('|').unwrap_or((arg, ""));
                inner.pose_transfer(kind.trim(), dest);
                Ok("{}".into())
            }

            // ---- clashes ----
            "conflict-answer" => {
                let a = parse(arg)?;
                let q = &inner.extra.ops.questions;
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
            "elevation-answer" => {
                inner
                    .extra
                    .ops
                    .questions
                    .answer_elevation(arg.trim() == "yes");
                Ok("{}".into())
            }
            "h:conflict" => {
                let probes = inner
                    .probes
                    .as_ref()
                    .ok_or_else(|| invalid("not a harness run"))?;
                let a = parse(arg)?;
                match text(&a["mode"]) {
                    "cancel" => probes.cancel_conflicts(true),
                    "all" => probes.answer_all_conflicts(
                        parse_resolution(text(&a["answer"]))
                            .ok_or_else(|| invalid("not an answer"))?,
                    ),
                    _ => {
                        let mut map = HashMap::new();
                        if let Some(obj) = a["answers"].as_object() {
                            for (name, word) in obj {
                                map.insert(
                                    name.clone(),
                                    parse_resolution(text(word)).ok_or_else(|| {
                                        invalid(format!("'{word}' is not an answer"))
                                    })?,
                                );
                            }
                        }
                        probes.answer_named_conflicts(map);
                    }
                }
                Ok("{}".into())
            }
            "h:conflict-plan" => {
                let probes = inner
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
            "h:elevation-reset" => Ok("{}".into()),

            // ---- archives and shortcuts ----
            "extract" => {
                let a = parse(arg)?;
                let tab = TabId(a["tab"].as_u64().unwrap_or(0));
                // An archive file named outright is its own subject; otherwise the tab's place is.
                let path = match a["archive"].as_str() {
                    Some(file) => file.to_string(),
                    None => self.tab_path(tab)?,
                };
                let op = inner.op_extract(
                    &path,
                    &strings(&a["names"]),
                    text(&a["destination"]),
                    crate::ops_arch::extract_conflict_of(text(&a["conflict"])),
                )?;
                Ok(op_json(op))
            }
            "compress" => {
                let a = parse(arg)?;
                let format_ = crate::ops_arch::write_format_of(text(&a["format"]))
                    .ok_or_else(|| invalid("That is not a format this app can write."))?;
                let op = inner.op_compress(
                    strings(&a["sources"]),
                    text(&a["target"]).to_string(),
                    format_,
                    crate::ops_arch::level_of(text(&a["level"])),
                )?;
                Ok(op_json(op))
            }
            "unlock" => {
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
            "archive-add" => {
                let a = parse(arg)?;
                let archive = text(&a["archive"]).to_string();
                let dir = text(&a["entryDir"]);
                let mut edits = Vec::new();
                for s in strings(&a["sources"]) {
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
            "shortcut" => {
                let a = parse(arg)?;
                Ok(op_json(inner.op_shortcut(
                    strings(&a["sources"]),
                    text(&a["directory"]).to_string(),
                )?))
            }

            // ---- a registry verb a program added to the menu ----
            "shell-verb" => {
                let a = parse(arg)?;
                let target = text(&a["target"]);
                let resolved = StaticVerbCommand::resolve(
                    a["command"].as_str(),
                    target,
                    text(&a["workingDirectory"]),
                    |p| std::path::Path::new(p).exists(),
                )
                .ok_or_else(|| invalid("That entry's command could not be read."))?;
                match inner.seams.launcher.launch(&resolved.executable, false) {
                    Ok(()) => inner
                        .set_active_status(format!("Started {}", file_name(&resolved.executable))),
                    Err(e) => inner.set_active_status(e),
                }
                Ok("{}".into())
            }

            // ---- the history ----
            "undo-budget" => {
                let a = parse(arg)?;
                let current = lock(&inner.undo).budget();
                let steps = a["steps"]
                    .as_i64()
                    .map_or(current.max_entries, |n| n as i32);
                let bytes = a["bytes"].as_i64().unwrap_or(current.max_held_bytes);
                inner.set_undo_budget(UndoBudget::new(steps, bytes));
                Ok("{}".into())
            }

            other => Err(invalid(format!("'{other}' is not an operations verb."))),
        }
    }

    fn tab_path(&self, tab: TabId) -> AppResult<String> {
        Ok(self.tab_info(tab)?.path)
    }
}
