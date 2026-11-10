//! `ops_query`: the operations UI's models, as JSON text. One private verb enum, one small method per
//! model.

use std::collections::HashSet;

use bb_core::archives::{ArchiveFormats, ArchivePath, ArchiveWriteRules};
use bb_core::rename::RenameRule;
use bb_core::shellmenu::{
    Guid, ShellExtensionKind, ShellFileType, ShellHandlerRegistration, ShellMenuContext,
    ShellMenuKeys, ShellMenuRules, ShellMenuTarget, ShellVerbRegistration,
};
use bb_core::transfer::{
    DragOutAction, DragOutContract, DropEffect, DropInContract, DropOrigin, TransferVerb,
};
use bb_paths::file_name;
use serde_json::{Value, json};

use super::{invalid, parse, strings, text, tidy_json};
use crate::app::App;
use crate::app::lock;
use crate::ids::AppResult;

verb_enum! {
    /// Every model `App::ops_query` answers.
    QueryVerb {
        Queue = "queue",
        History = "history",
        Conflicts = "conflicts",
        Confirm = "confirm",
        DeletePreview = "delete-preview",
        DeleteSurvey = "delete-survey",
        Notice = "notice",
        Elevation = "elevation",
        RightDropMenu = "right-drop-menu",
        DropAllowed = "drop-allowed",
        DropDecide = "drop-decide",
        DragoutDecide = "dragout-decide",
        RenamePreview = "rename-preview",
        ContextMenu = "context-menu",
        ShellKeys = "shell-keys",
        ShellCatalog = "shell-catalog",
        ShellVisible = "shell-visible",
        MenuArrange = "menu-arrange",
        MenuTidy = "menu-tidy",
        CompressDefaults = "compress-defaults",
        CompressCheck = "compress-check",
        ExtractDestination = "extract-destination",
        ExtractCheck = "extract-check",
        ArchiveOf = "archive-of",
    }
}

impl App {
    /// A model of the operations UI, as JSON text.
    pub fn ops_query(&self, what: &str, arg: &str) -> AppResult<String> {
        let inner = &self.inner;
        let what: QueryVerb = what
            .parse()
            .map_err(|()| invalid(format!("'{what}' is not an operations model.")))?;
        let value = match what {
            QueryVerb::Queue => inner.queue_json(),
            QueryVerb::History => inner.history_json(),
            QueryVerb::Conflicts => self.query_conflicts(),
            QueryVerb::Confirm => json!({ "pending": inner.extra.ops.questions.confirm_model() }),
            QueryVerb::DeletePreview => self.query_delete_preview(arg)?,
            QueryVerb::DeleteSurvey => inner.delete_survey(),
            QueryVerb::Notice => json!({ "pending": inner.extra.ops.questions.notice_model() }),
            QueryVerb::Elevation => self.query_elevation(),
            QueryVerb::RightDropMenu => {
                let a = parse(arg)?;
                inner.right_drop_json(&strings(&a["sources"]), text(&a["destination"]))
            }
            QueryVerb::DropAllowed => self.query_drop_allowed(arg)?,
            QueryVerb::DropDecide => Self::query_drop_decide(arg)?,
            QueryVerb::DragoutDecide => Self::query_dragout_decide(arg)?,
            QueryVerb::RenamePreview => self.query_rename_preview(arg)?,
            QueryVerb::ContextMenu => inner.context_menu_json(),
            QueryVerb::ShellKeys => Self::query_shell_keys(arg)?,
            QueryVerb::ShellCatalog => Self::query_shell_catalog(arg)?,
            QueryVerb::ShellVisible => self.query_shell_visible(arg)?,
            QueryVerb::MenuArrange => inner.menu_arrange_json(&parse(arg)?),
            QueryVerb::MenuTidy => tidy_json(&parse(arg)?),
            QueryVerb::CompressDefaults => Self::query_compress_defaults(arg)?,
            QueryVerb::CompressCheck => Self::query_compress_check(arg)?,
            QueryVerb::ExtractDestination => self.query_extract_destination(arg)?,
            QueryVerb::ExtractCheck => Self::query_extract_check(arg)?,
            QueryVerb::ArchiveOf => self.query_archive_of(arg),
        };
        Ok(value.to_string())
    }

    fn query_conflicts(&self) -> Value {
        let inner = &self.inner;
        let pending = inner.extra.ops.questions.conflict_model();
        let last = inner
            .probes
            .as_ref()
            .map(|p| p.last_conflict_model())
            .unwrap_or(Value::Null);
        json!({ "pending": pending, "last": last,
                "asked": inner.probes.as_ref().map_or(0, |p| p.conflict_asks()) })
    }

    /// `{"permanent": bool, "paths": [...]}`; no paths means the active tab's selection.
    fn query_delete_preview(&self, arg: &str) -> AppResult<Value> {
        let a = parse(arg)?;
        self.inner.delete_preview(
            strings(&a["paths"]),
            a["permanent"].as_bool().unwrap_or(false),
        )
    }

    fn query_elevation(&self) -> Value {
        let inner = &self.inner;
        json!({
            "pending": inner.extra.ops.questions.elevation_model(),
            "last": lock(&inner.extra.ops.elevation_offer).clone(),
            "offers": inner.probes.as_ref().map_or(0, |p| p.elevation_offers().len()),
        })
    }

    fn query_drop_allowed(&self, arg: &str) -> AppResult<Value> {
        let a = parse(arg)?;
        let verb = if text(&a["verb"]) == "move" {
            TransferVerb::Move
        } else {
            TransferVerb::Copy
        };
        let sources = strings(&a["sources"]);
        let dest = text(&a["destination"]);
        let archive = crate::ops::arch::archive_of(dest).is_some()
            || (std::path::Path::new(dest).is_file()
                && ArchiveFormats::is_archive_name(file_name(dest)));
        let ok = if archive {
            !sources.iter().any(|s| ArchivePath::looks_virtual(s))
        } else {
            self.inner.drop_has_work(&sources, dest, verb)
        };
        Ok(json!({ "ok": ok }))
    }

    fn query_drop_decide(arg: &str) -> AppResult<Value> {
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
        Ok(json!({
            "verb": if d.verb == TransferVerb::Move { "move" } else { "copy" },
            "report": d.report.0,
            "acceptable": DropInContract::can_accept(allowed),
        }))
    }

    fn query_dragout_decide(arg: &str) -> AppResult<Value> {
        let a = parse(arg)?;
        let eff = |v: &Value| v.as_u64().map(|n| DropEffect(n as u32));
        let action = DragOutContract::decide(
            a["handledInApp"].as_bool().unwrap_or(false),
            DropEffect(a["returned"].as_u64().unwrap_or(0) as u32),
            eff(&a["logical"]),
            eff(&a["performed"]),
        );
        Ok(json!(match action {
            DragOutAction::Nothing => "nothing",
            DragOutAction::RefreshOnly => "refresh",
            DragOutAction::RemoveSources => "remove",
        }))
    }

    fn query_rename_preview(&self, arg: &str) -> AppResult<Value> {
        let a = parse(arg)?;
        let rule = if a["rule"].is_null() {
            None
        } else {
            Some(
                serde_json::from_value::<RenameRule>(a["rule"].clone())
                    .map_err(|e| invalid(format!("not a rename rule: {e}")))?,
            )
        };
        Ok(self
            .inner
            .rename_preview_json(&strings(&a["paths"]), rule, text(&a["pattern"])))
    }

    fn query_shell_keys(arg: &str) -> AppResult<Value> {
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
        Ok(json!(keys))
    }

    fn query_shell_catalog(arg: &str) -> AppResult<Value> {
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
                        has_delegate_execute: v["hasDelegateExecute"].as_bool().unwrap_or(false),
                        has_sub_commands: v["hasSubCommands"].as_bool().unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let catalog = ShellMenuRules::catalog(&handlers, &verbs, &HashSet::new());
        Ok(json!(
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
        ))
    }

    fn query_shell_visible(&self, arg: &str) -> AppResult<Value> {
        let a = parse(arg)?;
        let settings = lock(&self.inner.settings);
        let hidden = settings.hidden_shell_extensions.clone();
        let enabled = settings.show_shell_extensions;
        drop(settings);
        let ids = strings(&a["ids"]);
        let hidden: HashSet<String> = hidden.iter().map(|h| h.to_uppercase()).collect();
        Ok(json!(
            ids.into_iter()
                .filter(|i| enabled && !hidden.contains(&i.to_uppercase()))
                .collect::<Vec<_>>()
        ))
    }

    fn query_compress_defaults(arg: &str) -> AppResult<Value> {
        let a = parse(arg)?;
        Ok(json!({
            "name": ArchiveWriteRules::suggest_name(&strings(&a["sources"]), text(&a["directory"])),
            "formats": ArchiveWriteRules::formats().iter().map(|f| json!({
                "label": f.label, "suffix": f.suffix, "supportsLevel": f.supports_level,
            })).collect::<Vec<_>>(),
        }))
    }

    /// `{name, directory, suffix}` -> `{problem, path}`: what Compress would create, checked as
    /// the name is typed (a name Windows refuses, or a file already there, is refused here).
    /// Spec: CreateArchiveDialog.Update.
    fn query_compress_check(arg: &str) -> AppResult<Value> {
        let a = parse(arg)?;
        let name = text(&a["name"]).trim().to_string();
        let suffix = text(&a["suffix"]);
        let file = if name.to_lowercase().ends_with(&suffix.to_lowercase()) {
            name.clone()
        } else {
            format!("{name}{suffix}")
        };
        let path = format!("{}\\{file}", text(&a["directory"]).trim_end_matches('\\'));
        let problem = if name.is_empty() {
            Some("Give the archive a name.".to_string())
        } else {
            bb_core::rename::RenamePattern::validate(&name)
        }
        .or_else(|| {
            std::path::Path::new(&path)
                .exists()
                .then(|| format!("{file} is already there."))
        });
        Ok(json!({ "problem": problem, "path": path }))
    }

    /// `{archive}` -> `{destination}`: where Extract here puts it - beside the archive when it
    /// carries its own top folder, else in a new folder named after it. Spec: ExtractRules.
    fn query_extract_destination(&self, arg: &str) -> AppResult<Value> {
        let a = parse(arg)?;
        let archive = text(&a["archive"]);
        let index = bb_core::archives::ArchiveBrowser::read_archive(&*self.inner.fs, archive);
        Ok(json!({
            "destination": bb_core::archives::ExtractRules::destination_for(
                &index,
                archive,
                |p| std::path::Path::new(p).is_dir(),
                |p| std::path::Path::new(p).is_file(),
            ),
        }))
    }

    /// `{destination}` -> `{problem}`. Spec: ExtractDialog.Validate.
    fn query_extract_check(arg: &str) -> AppResult<Value> {
        let a = parse(arg)?;
        let path = text(&a["destination"]).trim();
        let qualified = path.starts_with("\\\\")
            || (path.len() >= 3
                && path.as_bytes()[1] == b':'
                && matches!(path.as_bytes()[2], b'\\' | b'/'));
        let problem = if path.is_empty() {
            Some("Choose a folder to extract into.")
        } else if !qualified {
            Some("That is not a full path.")
        } else if std::path::Path::new(path).is_file() {
            Some("That is a file, not a folder.")
        } else {
            None
        };
        Ok(json!({ "problem": problem }))
    }

    fn query_archive_of(&self, arg: &str) -> Value {
        let inner = &self.inner;
        match inner.archive_file_for(arg) {
            Some(file) => {
                let index = bb_core::archives::ArchiveBrowser::read_archive(&*inner.fs, &file);
                json!({
                    "archive": file,
                    "entry": crate::ops::arch::archive_of(arg).map(|a| a.entry_path).unwrap_or_default(),
                    "locked": index.failure == bb_core::archives::ArchiveFailure::PasswordRequired,
                    "refused": inner.extra.ops.passwords.was_refused(&file),
                })
            }
            None => Value::Null,
        }
    }
}
