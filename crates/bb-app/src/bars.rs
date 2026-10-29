//! The menu bar and toolbars as data, and the only thing that writes them (`CommandBarService`),
//! plus the Settings page's editor over them. The arrangement is a `CommandBarSet` in
//! `AppSettings.command_bars`; **None means the default**, and one equal to the default is written
//! back as None, so an uncustomised user gets a later build's additions while a customised one is
//! never edited behind their back. Saved arrangements live beside it in the settings file. It is
//! written at once, like the keymap: an arrangement can be recalled from a menu or the palette
//! while the settings page is open. Spec: CommandBarService, SettingsViewModel.CommandBars,
//! MainWindow.CommandBars.

use bb_core::commandbars::{
    AccessKeys, BarCommandState, BarItem, BarItemKind, BarRow, BarRowKind, BarSource, BarTarget,
    BarTargetKind, CommandBarCaptions, CommandBarDefaults, CommandBarEdit, CommandBarLists,
    CommandBarRules, CommandBarSet, SavedCommandBarRules, SavedCommandBarSet, ToolbarSpec,
};
use bb_core::commands::{CommandCatalog, CommandMatcher};
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::commands::Ctx;
use crate::events::Model;
use crate::ids::{AppError, AppResult};

pub(crate) struct BarsState {
    pub current: CommandBarSet,
}

impl BarsState {
    pub fn new(stored: Option<&CommandBarSet>) -> BarsState {
        BarsState {
            current: CommandBarRules::clean(stored),
        }
    }
}

/// The Settings page's editor state: which bar, how deep, which row, what the picker is filtered to.
pub(crate) struct BarPage {
    pub target: BarTarget,
    pub path: Vec<usize>,
    pub selected_row: usize,
    pub filter: String,
    pub unplaced_only: bool,
    pub selected_saved: Option<String>,
}

impl Default for BarPage {
    fn default() -> Self {
        BarPage {
            target: BarTarget::MENU_BAR,
            path: Vec::new(),
            selected_row: 0,
            filter: String::new(),
            unplaced_only: false,
            selected_saved: None,
        }
    }
}

struct Source<'a> {
    inner: &'a Inner,
    ctx: &'a Ctx,
}

impl BarSource for Source<'_> {
    fn command(&self, id: &str) -> Option<BarCommandState> {
        let s = self.inner.command_state_in(self.ctx, id)?;
        Some(BarCommandState {
            name: s.name,
            icon: s.icon,
            gesture: s.gesture,
            unavailable: s.why_not,
            is_checked: s.checked,
            live_caption: s.caption,
        })
    }

    fn list_has_rows(&self, list_id: &str) -> bool {
        self.inner.list_count(list_id) > 0
    }
}

fn row_json(row: &BarRow) -> Value {
    let kind = match row.kind {
        BarRowKind::Command => "command",
        BarRowKind::Separator => "separator",
        BarRowKind::Menu => "menu",
        BarRowKind::List => "list",
        BarRowKind::Placeholder => "placeholder",
    };
    json!({
        "kind": kind,
        "id": row.id,
        "caption": row.caption,
        "name": row.name,
        "icon": row.icon,
        "gesture": row.gesture,
        "enabled": row.is_enabled,
        "reason": row.reason,
        "checked": row.is_checked,
        "showText": row.show_text,
        "children": row.children.iter().map(row_json).collect::<Vec<_>>(),
    })
}

fn tokens_of(items: &[BarItem]) -> Vec<String> {
    items
        .iter()
        .map(|i| match &i.kind {
            BarItemKind::Separator => "-".to_string(),
            BarItemKind::List => format!("@{}", i.id.clone().unwrap_or_default()),
            BarItemKind::Menu => format!("[{}]", i.caption.clone().unwrap_or_default()),
            _ => i.id.clone().unwrap_or_default(),
        })
        .collect()
}

/// "-" a separator, "@name" a list, anything else a command id.
pub(crate) fn items_from_tokens(tokens: &str) -> Vec<BarItem> {
    tokens
        .split_whitespace()
        .map(|t| match t {
            "-" => BarItem::separator(),
            t if t.starts_with('@') => BarItem::list(&t[1..]),
            t => BarItem::command(t),
        })
        .collect()
}

fn target_of(name: &str) -> Option<BarTarget> {
    match name {
        "menus" | "menu" | "menubar" => Some(BarTarget::MENU_BAR),
        "title" | "titlebar" => Some(BarTarget::TITLE_BAR),
        other => other
            .strip_prefix("row")
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| *n >= 1)
            .map(|n| BarTarget::for_row(n - 1)),
    }
}

fn target_json(set: &CommandBarSet, target: BarTarget) -> Value {
    let (kind, key) = match target.kind {
        BarTargetKind::MenuBar => ("menus", "menus".to_string()),
        BarTargetKind::TitleBar => ("title", "title".to_string()),
        BarTargetKind::Row => ("row", format!("row{}", target.row + 1)),
    };
    json!({
        "key": key,
        "kind": kind,
        "row": target.row,
        "name": CommandBarEdit::name_of(target),
        "icon": if target.kind == BarTargetKind::MenuBar { "Icon.DetailsList" } else { "Icon.Reorder" },
        "showsText": CommandBarEdit::shows_text(set, target),
    })
}

impl Inner {
    pub(crate) fn bars_set_current(&self) -> CommandBarSet {
        lock(&self.extra.bars).current.clone()
    }

    pub(crate) fn bars_saved(&self) -> Vec<SavedCommandBarSet> {
        lock(&self.settings).saved_command_bars.clone()
    }

    pub(crate) fn bars_current_name(&self) -> Option<String> {
        let current = self.bars_set_current();
        let saved = self.bars_saved();
        SavedCommandBarRules::name_matching(&saved, &current).map(str::to_string)
    }

    pub(crate) fn bars_is_default(&self) -> bool {
        CommandBarRules::is_default(&self.bars_set_current())
    }

    /// Whether replacing the current arrangement would lose something kept nowhere else.
    pub(crate) fn bars_replacing_loses_work(&self) -> bool {
        SavedCommandBarRules::needs_confirm_before_replace(
            &self.bars_set_current(),
            &self.bars_saved(),
        )
    }

    fn bars_changed(&self) {
        self.hub.model(Model::Bars);
        self.hub.model(Model::BarPage);
    }

    /// Puts an arrangement in force. One that equals what ships is stored as nothing at all, so
    /// somebody who dragged a button and dragged it back is on the default again.
    pub(crate) fn bars_set(&self, set: CommandBarSet) -> AppResult<()> {
        let mut clean = CommandBarRules::clean(Some(&set));
        clean.revision = CommandBarDefaults::REVISION;
        {
            let mut bars = lock(&self.extra.bars);
            if CommandBarRules::same(&clean, &bars.current) {
                return Ok(());
            }
            let is_default = CommandBarRules::is_default(&clean);
            bars.current = if is_default {
                CommandBarDefaults::default_set().clone()
            } else {
                clean
            };
            lock(&self.settings).command_bars = (!is_default).then(|| bars.current.clone());
        }
        self.save_settings_now()?;
        self.bars_changed();
        Ok(())
    }

    /// The settings file's `commandBars` changed under us (a hand edit, a page write).
    pub(crate) fn reload_bars(&self) {
        let stored = lock(&self.settings).command_bars.clone();
        lock(&self.extra.bars).current = CommandBarRules::clean(stored.as_ref());
        self.bars_changed();
    }

    pub(crate) fn bars_reset(&self) -> AppResult<()> {
        self.bars_set(CommandBarDefaults::default_set().clone())
    }

    pub(crate) fn bars_name_taken(&self, name: &str, except: Option<&str>) -> bool {
        self.bars_saved().iter().any(|s| {
            s.name.eq_ignore_ascii_case(name)
                && !except.is_some_and(|e| e.eq_ignore_ascii_case(&s.name))
        })
    }

    /// Saves the current arrangement under a name, replacing one already there.
    pub(crate) fn bars_save_as(&self, name: &str) -> AppResult<()> {
        if let Some(problem) =
            SavedCommandBarRules::validate(name, |n| self.bars_name_taken(n, Some(n)) && false)
        {
            return Err(AppError::Invalid(problem));
        }
        let current = self.bars_set_current();
        {
            let mut s = lock(&self.settings);
            let upserted = SavedCommandBarRules::upsert(
                &s.saved_command_bars,
                SavedCommandBarSet::new(name, current),
            );
            s.saved_command_bars = upserted;
        }
        self.save_settings_now()?;
        self.bars_changed();
        Ok(())
    }

    /// Recalls a saved arrangement.
    pub(crate) fn bars_apply(&self, name: &str) -> AppResult<()> {
        let saved = self.bars_saved();
        let Some(found) = SavedCommandBarRules::find(&saved, name) else {
            return Err(AppError::Invalid(format!("no saved arrangement '{name}'")));
        };
        self.bars_set(found.set.clone())
    }

    pub(crate) fn bars_rename(&self, old: &str, new: &str) -> AppResult<()> {
        let renamed = {
            let s = lock(&self.settings);
            SavedCommandBarRules::rename(&s.saved_command_bars, old, new)
        };
        let Some(renamed) = renamed else {
            return Err(AppError::Invalid(format!(
                "cannot rename '{old}' to '{new}'"
            )));
        };
        lock(&self.settings).saved_command_bars = renamed;
        self.save_settings_now()?;
        self.bars_changed();
        Ok(())
    }

    pub(crate) fn bars_remove(&self, name: &str) -> AppResult<()> {
        {
            let mut s = lock(&self.settings);
            let kept = SavedCommandBarRules::remove(&s.saved_command_bars, name);
            s.saved_command_bars = kept;
        }
        self.save_settings_now()?;
        self.bars_changed();
        Ok(())
    }

    /// Replaces one bar's items from tokens (`-`, `@list`, command ids) in the harness's spelling.
    pub(crate) fn bars_set_tokens(
        &self,
        target: &str,
        tokens: &str,
        show_text: Option<bool>,
    ) -> AppResult<()> {
        let target = target_of(target)
            .ok_or_else(|| AppError::Invalid(format!("no such bar '{target}'")))?;
        let mut set = self.bars_set_current();
        let items = items_from_tokens(tokens);
        match target.kind {
            BarTargetKind::MenuBar => {
                return Err(AppError::Invalid("use bars_set_menu for a menu".into()));
            }
            BarTargetKind::TitleBar => {
                set.title_bar = ToolbarSpec {
                    items,
                    show_text: show_text.unwrap_or(set.title_bar.show_text),
                };
            }
            BarTargetKind::Row => {
                while set.rows.len() <= target.row {
                    set.rows.push(ToolbarSpec::new(vec![]));
                }
                let keep = set.rows[target.row].show_text;
                set.rows[target.row] = ToolbarSpec {
                    items,
                    show_text: show_text.unwrap_or(keep),
                };
            }
        }
        self.bars_set(set)
    }

    /// Adds (or replaces, by name) a top-level menu.
    pub(crate) fn bars_set_menu(&self, caption: &str, tokens: &str) -> AppResult<()> {
        let mut set = self.bars_set_current();
        let menu = BarItem::menu(caption, items_from_tokens(tokens));
        match set.menus.iter().position(|m| {
            AccessKeys::strip(m.caption.as_deref().unwrap_or("")) == AccessKeys::strip(caption)
        }) {
            Some(at) => set.menus[at] = menu,
            None => set.menus.push(menu),
        }
        self.bars_set(set)
    }

    // ---- what a bar shows ----

    pub(crate) fn list_count(&self, list_id: &str) -> usize {
        match list_id {
            CommandBarLists::WORKSPACES => self.saved_workspaces().len(),
            CommandBarLists::SAVED_SEARCHES => self.saved_searches().len(),
            CommandBarLists::BOOKMARKS => self.bookmark_list().len(),
            CommandBarLists::NEW_FILE_TYPES => self.new_file_types().len(),
            CommandBarLists::CUSTOM_COMMANDS => self.custom_command_ids().len(),
            CommandBarLists::COMMAND_BARS => self.bars_saved().len(),
            CommandBarLists::DRIVES => self.drive_list().len(),
            // Nothing to list while a walk holds the history; it says what it did when it finishes.
            CommandBarLists::UNDO_STEPS => {
                crate::app::try_lock(&self.undo).map_or(0, |u| u.cursor())
            }
            CommandBarLists::REDO_STEPS => crate::app::try_lock(&self.undo)
                .map_or(0, |undo| undo.entries().len() - undo.cursor()),
            _ => 0,
        }
    }

    /// The rows a list expands to.
    pub(crate) fn list_rows(&self, list_id: &str) -> Vec<Value> {
        match list_id {
            CommandBarLists::WORKSPACES => self
                .saved_workspaces()
                .iter()
                .map(|w| {
                    json!({
                        "id": w.name,
                        "name": w.name,
                        "detail": bb_core::rules::SavedWorkspaceRules::shape_text(&w.layout),
                    })
                })
                .collect(),
            CommandBarLists::SAVED_SEARCHES => self
                .saved_searches()
                .iter()
                .map(|s| json!({ "id": s.name, "name": s.name, "detail": s.query }))
                .collect(),
            CommandBarLists::BOOKMARKS => self
                .bookmark_list()
                .iter()
                .map(|b| json!({ "id": b.display_path, "name": b.display_path }))
                .collect(),
            CommandBarLists::NEW_FILE_TYPES => self
                .new_file_types()
                .iter()
                .map(|t| json!({ "id": t.extension, "name": t.label, "detail": t.extension }))
                .collect(),
            CommandBarLists::CUSTOM_COMMANDS => self
                .custom_command_ids()
                .iter()
                .filter_map(|id| {
                    self.custom_command(id)
                        .map(|c| json!({ "id": id, "name": c.name }))
                })
                .collect(),
            CommandBarLists::COMMAND_BARS => {
                let current = self.bars_current_name();
                self.bars_saved()
                    .iter()
                    .map(|s| {
                        json!({
                            "id": s.name,
                            "name": s.name,
                            "checked": current.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&s.name)),
                        })
                    })
                    .collect()
            }
            CommandBarLists::DRIVES => self
                .drive_list()
                .iter()
                .map(|(root, label)| json!({ "id": root, "name": root, "detail": label }))
                .collect(),
            CommandBarLists::UNDO_STEPS => {
                let Some(undo) = crate::app::try_lock(&self.undo) else {
                    return Vec::new();
                };
                let cursor = undo.cursor();
                undo.entries()[..cursor]
                    .iter()
                    .rev()
                    .enumerate()
                    .map(|(i, e)| json!({ "id": i + 1, "name": e.record().description() }))
                    .collect()
            }
            CommandBarLists::REDO_STEPS => {
                let Some(undo) = crate::app::try_lock(&self.undo) else {
                    return Vec::new();
                };
                let cursor = undo.cursor();
                undo.entries()[cursor..]
                    .iter()
                    .enumerate()
                    .map(|(i, e)| json!({ "id": i + 1, "name": e.record().description() }))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// The headings of the menu bar, access keys assigned.
    pub(crate) fn bars_menu_bar_json(&self) -> Value {
        let set = self.bars_set_current();
        let captions: Vec<String> = set
            .menus
            .iter()
            .map(|m| m.caption.clone().unwrap_or_default())
            .collect();
        let keyed = AccessKeys::assign(&captions);
        Value::Array(
            keyed
                .iter()
                .map(|c| json!({ "caption": c, "name": AccessKeys::strip(c) }))
                .collect(),
        )
    }

    /// One top-level menu as it would open right now. `which` is its name (access key ignored) or
    /// its position.
    pub(crate) fn bars_menu_json(&self, which: &str) -> AppResult<Value> {
        let set = self.bars_set_current();
        let menu = match which.parse::<usize>() {
            Ok(i) => set.menus.get(i),
            Err(_) => set.menus.iter().find(|m| {
                AccessKeys::strip(m.caption.as_deref().unwrap_or("")).eq_ignore_ascii_case(which)
            }),
        }
        .ok_or_else(|| AppError::Invalid(format!("no menu '{which}'")))?;
        let ctx = self.command_ctx();
        let rows = CommandBarRules::present(
            menu.children(),
            &Source {
                inner: self,
                ctx: &ctx,
            },
            true,
            false,
        );
        Ok(json!({
            "caption": menu.caption,
            "rows": rows.iter().map(row_json).collect::<Vec<_>>(),
        }))
    }

    /// A toolbar as it shows right now. `which` is `title` or `rowN`.
    pub(crate) fn bars_toolbar_json(&self, which: &str) -> AppResult<Value> {
        let target = target_of(which)
            .filter(|t| t.kind != BarTargetKind::MenuBar)
            .ok_or_else(|| AppError::Invalid(format!("no toolbar '{which}'")))?;
        let set = self.bars_set_current();
        let spec = match target.kind {
            BarTargetKind::TitleBar => Some(&set.title_bar),
            _ => set.rows.get(target.row),
        }
        .ok_or_else(|| AppError::Invalid(format!("no toolbar '{which}'")))?;
        let ctx = self.command_ctx();
        let rows = CommandBarRules::present(
            &spec.items,
            &Source {
                inner: self,
                ctx: &ctx,
            },
            false,
            spec.show_text,
        );
        Ok(json!({
            "showText": spec.show_text,
            "tokens": tokens_of(&spec.items),
            "rows": rows.iter().map(row_json).collect::<Vec<_>>(),
        }))
    }

    /// The arrangement in force, in the harness's spelling.
    pub(crate) fn bars_state_json(&self) -> Value {
        let set = self.bars_set_current();
        json!({
            "isDefault": CommandBarRules::is_default(&set),
            "name": self.bars_current_name(),
            "replacingLosesWork": self.bars_replacing_loses_work(),
            "menus": set.menus.iter().map(|m| json!({
                "caption": m.caption,
                "tokens": tokens_of(m.children()),
            })).collect::<Vec<_>>(),
            "title": { "tokens": tokens_of(&set.title_bar.items), "showText": set.title_bar.show_text },
            "rows": set.rows.iter().map(|r| json!({
                "tokens": tokens_of(&r.items), "showText": r.show_text,
            })).collect::<Vec<_>>(),
            "saved": self.bars_saved().iter().map(|s| json!({
                "name": s.name,
                "shape": SavedCommandBarRules::shape_text(&s.set),
            })).collect::<Vec<_>>(),
            "json": serde_json::to_value(&set).unwrap_or(Value::Null),
        })
    }

    /// Replaces the arrangement with one given as JSON (the settings file's shape).
    pub(crate) fn bars_set_json(&self, text: &str) -> AppResult<()> {
        let set: CommandBarSet =
            serde_json::from_str(text).map_err(|e| AppError::Invalid(e.to_string()))?;
        self.bars_set(set)
    }

    // ---- the Settings page's editor ----

    fn bar_row_name(&self, item: &BarItem) -> (String, String, String, Option<String>) {
        let name = CommandBarCaptions::describe(item);
        if item.is_menu() {
            let count = item.children().iter().filter(|i| !i.is_separator()).count();
            return (
                name,
                format!(
                    "submenu \u{00b7} {count} item{}",
                    if count == 1 { "" } else { "s" }
                ),
                String::new(),
                Some("Icon.ChevronDown".into()),
            );
        }
        if item.is_list() {
            let list = item.id.as_deref().and_then(CommandBarLists::find);
            return (
                name,
                if list.is_some() {
                    "list"
                } else {
                    "not in this version"
                }
                .to_string(),
                String::new(),
                Some(list.map_or("Icon.Warning", |l| l.icon).to_string()),
            );
        }
        if item.is_command()
            && let Some(id) = item.id.as_deref()
        {
            let (known, icon) = self.describe_bar_command(id);
            let named = if item.caption.as_deref().is_some_and(|c| !c.is_empty()) {
                name
            } else {
                known.clone().unwrap_or(name)
            };
            return (
                named,
                if known.is_none() {
                    "not in this version".to_string()
                } else if item.show_text == Some(true) {
                    "with its name".to_string()
                } else {
                    String::new()
                },
                self.keymap_gesture(id),
                Some(icon.unwrap_or_else(|| "Icon.Warning".to_string())),
            );
        }
        (
            name,
            if item.is_separator() {
                String::new()
            } else {
                "not in this version".to_string()
            },
            String::new(),
            None,
        )
    }

    fn describe_bar_command(&self, id: &str) -> (Option<String>, Option<String>) {
        if let Some(command) = CommandCatalog::find(id) {
            return (
                Some(
                    CommandBarCaptions::for_id(id)
                        .unwrap_or(command.name)
                        .to_string(),
                ),
                command.icon.map(str::to_string),
            );
        }
        match self.custom_command(id) {
            Some(c) => (
                Some(c.name),
                Some(
                    if c.run_elevated {
                        "Icon.Shield"
                    } else {
                        "Icon.CustomCommand"
                    }
                    .to_string(),
                ),
            ),
            None => (None, None),
        }
    }

    pub(crate) fn barpage_json(&self) -> Value {
        let set = self.bars_set_current();
        let mut page = lock(&self.extra.barpage);
        // The place may have gone: a row removed, a submenu deleted from a menu elsewhere.
        if matches!(page.target.kind, BarTargetKind::Row) && page.target.row >= set.rows.len() {
            page.target = BarTarget::MENU_BAR;
            page.path.clear();
        }
        while !page.path.is_empty() && !CommandBarEdit::exists(&set, page.target, &page.path) {
            page.path.pop();
        }
        let items = CommandBarEdit::items_at(&set, page.target, &page.path);
        if !items.is_empty() {
            page.selected_row = page.selected_row.min(items.len() - 1);
        } else {
            page.selected_row = 0;
        }
        let selected = items.get(page.selected_row);

        let rows: Vec<Value> = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let (name, detail, gesture, icon) = self.bar_row_name(item);
                json!({
                    "index": i,
                    "name": name,
                    "detail": detail,
                    "gesture": gesture,
                    "icon": icon,
                    "separator": item.is_separator(),
                    "menu": item.is_menu(),
                    "list": item.is_list(),
                    "showText": item.show_text == Some(true),
                    "selected": i == page.selected_row,
                })
            })
            .collect();

        let mut targets = vec![
            target_json(&set, BarTarget::MENU_BAR),
            target_json(&set, BarTarget::TITLE_BAR),
        ];
        for i in 0..set.rows.len() {
            targets.push(target_json(&set, BarTarget::for_row(i)));
        }

        let placed = CommandBarRules::placements(&set);
        let filter = page.filter.trim().to_string();
        let mut choices: Vec<Value> = Vec::new();
        let mut heading: Option<String> = None;
        let mut push = |category: &str, choice: Value, choices: &mut Vec<Value>| {
            if heading.as_deref() != Some(category) {
                choices.push(json!({ "heading": true, "name": category }));
                heading = Some(category.to_string());
            }
            choices.push(choice);
        };
        let mut commands: Vec<crate::keys::CommandRow> = CommandCatalog::all()
            .iter()
            .map(|c| {
                (
                    c.id.to_string(),
                    c.name.to_string(),
                    c.category.to_string(),
                    c.icon.map(str::to_string),
                    c.aliases.iter().map(|a| a.to_string()).collect(),
                )
            })
            .collect();
        for id in self.custom_command_ids() {
            if let Some(c) = self.custom_command(&id) {
                commands.push((
                    id,
                    c.name,
                    "Custom commands".into(),
                    Some(
                        if c.run_elevated {
                            "Icon.Shield"
                        } else {
                            "Icon.CustomCommand"
                        }
                        .into(),
                    ),
                    vec![],
                ));
            }
        }
        for (id, name, category, icon, aliases) in &commands {
            let count = placed.get(id).copied().unwrap_or(0);
            if page.unplaced_only && count > 0 {
                continue;
            }
            let alias_refs: Vec<&str> = aliases.iter().map(String::as_str).collect();
            if !filter.is_empty()
                && !CommandMatcher::matches(&filter, name, &alias_refs, category, "").is_match()
            {
                continue;
            }
            push(
                category,
                json!({
                    "heading": false, "kind": "command", "id": id, "name": name, "icon": icon,
                    "placed": count,
                    "placedText": match count { 0 => String::new(), 1 => "placed".to_string(), n => format!("placed \u{00d7}{n}") },
                }),
                &mut choices,
            );
        }
        // Lists are not commands and cannot be "unplaced" in the sense that filter means.
        if !page.unplaced_only {
            for list in CommandBarLists::all() {
                if !filter.is_empty()
                    && !CommandMatcher::matches(&filter, list.name, &[], "Lists", "").is_match()
                {
                    continue;
                }
                let count = CommandBarRules::flatten(&set)
                    .iter()
                    .filter(|i| i.is_list() && i.id.as_deref() == Some(list.id))
                    .count();
                push(
                    "Lists",
                    json!({
                        "heading": false, "kind": "list", "id": list.id, "name": list.name,
                        "icon": list.icon, "placed": count,
                    }),
                    &mut choices,
                );
            }
        }

        let is_toolbar = page.target.kind != BarTargetKind::MenuBar;
        let can_add = is_toolbar || !page.path.is_empty();
        let caption_editable = selected.is_some_and(|i| !i.is_separator() && !i.is_list());
        let current_name = self.bars_current_name();
        let status = if CommandBarRules::is_default(&set) {
            "The arrangement BertBrowser ships with".to_string()
        } else if let Some(name) = &current_name {
            format!("Saved as \"{name}\"")
        } else {
            "Your own arrangement, not saved under a name".to_string()
        };
        json!({
            "targets": targets,
            "target": target_json(&set, page.target),
            "path": page.path,
            "location": CommandBarEdit::trail(&set, page.target, &page.path).join("  \u{203a}  "),
            "status": status,
            "isDefault": CommandBarRules::is_default(&set),
            "rows": rows,
            "selectedRow": page.selected_row,
            "caption": if caption_editable { selected.and_then(|i| i.caption.clone()).unwrap_or_default() } else { String::new() },
            "captionHint": match selected {
                Some(i) if i.is_menu() => "A menu's name. Put & before the letter Alt should pick it by.".to_string(),
                Some(i) if i.is_command() => format!("Leave empty to call it \"{}\".", CommandBarCaptions::describe(&BarItem { caption: None, ..i.clone() })),
                _ => String::new(),
            },
            "canEditCaption": caption_editable,
            "canUp": !page.path.is_empty(),
            "canAdd": can_add,
            "isToolbar": is_toolbar,
            "addSubmenuLabel": if !is_toolbar && page.path.is_empty() { "Menu" } else { "Submenu" },
            "showsText": CommandBarEdit::shows_text(&set, page.target),
            "filter": page.filter,
            "unplacedOnly": page.unplaced_only,
            "choices": choices,
            "saved": self.bars_saved().iter().map(|s| json!({
                "name": s.name,
                "shape": SavedCommandBarRules::shape_text(&s.set),
                "current": current_name.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&s.name)),
            })).collect::<Vec<_>>(),
            "selectedSaved": page.selected_saved,
        })
    }

    /// Resolves a row given by index or by name against the page's current rows.
    fn barpage_row(&self, arg: &str) -> AppResult<usize> {
        if let Ok(i) = arg.parse::<usize>() {
            return Ok(i);
        }
        let set = self.bars_set_current();
        let page = lock(&self.extra.barpage);
        let items = CommandBarEdit::items_at(&set, page.target, &page.path);
        let wanted = AccessKeys::strip(arg);
        for (i, item) in items.iter().enumerate() {
            let (name, ..) = self.bar_row_name(item);
            if name.eq_ignore_ascii_case(&wanted)
                || AccessKeys::strip(&name).eq_ignore_ascii_case(&wanted)
            {
                return Ok(i);
            }
        }
        Err(AppError::Invalid(format!("no row '{arg}' here")))
    }

    fn barpage_commit(
        &self,
        edited: std::borrow::Cow<'_, CommandBarSet>,
        select: Option<usize>,
    ) -> AppResult<()> {
        let owned = edited.into_owned();
        if let Some(select) = select {
            lock(&self.extra.barpage).selected_row = select;
        }
        self.bars_set(owned)?;
        self.hub.model(Model::BarPage);
        Ok(())
    }

    /// One verb of the editor, with its argument. Rows are given by index or by name.
    pub(crate) fn barpage_do(&self, verb: &str, arg: &str) -> AppResult<()> {
        let set = self.bars_set_current();
        let (target, path, selected) = {
            let page = lock(&self.extra.barpage);
            (page.target, page.path.clone(), page.selected_row)
        };
        let changed = || self.hub.model(Model::BarPage);
        match verb {
            "select" => {
                let t =
                    target_of(arg).ok_or_else(|| AppError::Invalid(format!("no bar '{arg}'")))?;
                let mut page = lock(&self.extra.barpage);
                page.target = t;
                page.path.clear();
                page.selected_row = 0;
                drop(page);
                changed();
                Ok(())
            }
            "pick" => {
                let i = self.barpage_row(arg)?;
                lock(&self.extra.barpage).selected_row = i;
                changed();
                Ok(())
            }
            "open" => {
                // A top-level menu opens by name from the menu bar; a submenu by its row.
                let i = self.barpage_row(arg)?;
                let items = CommandBarEdit::items_at(&set, target, &path);
                if !items.get(i).is_some_and(BarItem::is_menu) {
                    return Err(AppError::Invalid(format!("'{arg}' is not a menu")));
                }
                let mut page = lock(&self.extra.barpage);
                page.path.push(i);
                page.selected_row = 0;
                drop(page);
                changed();
                Ok(())
            }
            "up" => {
                let mut page = lock(&self.extra.barpage);
                if let Some(came_from) = page.path.pop() {
                    page.selected_row = came_from;
                }
                drop(page);
                changed();
                Ok(())
            }
            "add" | "add-separator" | "add-menu" => {
                let item = match verb {
                    "add-separator" => BarItem::separator(),
                    "add-menu" => BarItem::menu("New menu", vec![]),
                    _ if arg.starts_with('@') => BarItem::list(&arg[1..]),
                    _ if arg == "-" => BarItem::separator(),
                    _ => {
                        // A choice by id or by name.
                        let id = if CommandCatalog::is_known(arg)
                            || self.custom_command(arg).is_some()
                        {
                            arg.to_string()
                        } else {
                            CommandCatalog::all()
                                .iter()
                                .find(|c| c.name.eq_ignore_ascii_case(arg))
                                .map(|c| c.id.to_string())
                                .or_else(|| {
                                    CommandBarLists::all()
                                        .iter()
                                        .find(|l| l.name.eq_ignore_ascii_case(arg))
                                        .map(|l| format!("@{}", l.id))
                                })
                                .ok_or_else(|| {
                                    AppError::Invalid(format!(
                                        "nothing called '{arg}' can be added"
                                    ))
                                })?
                        };
                        match id.strip_prefix('@') {
                            Some(list) => BarItem::list(list),
                            None => BarItem::command(&id),
                        }
                    }
                };
                let count = CommandBarEdit::items_at(&set, target, &path).len();
                let at = if count == 0 {
                    0
                } else {
                    (selected + 1).min(count)
                };
                let edited = CommandBarEdit::insert(&set, target, &path, at, item);
                self.barpage_commit(edited, Some(at))
            }
            "remove" => {
                let i = if arg.is_empty() {
                    selected
                } else {
                    self.barpage_row(arg)?
                };
                let edited = CommandBarEdit::remove(&set, target, &path, i);
                self.barpage_commit(edited, Some(i))
            }
            "nudge" => {
                // `arg` is "<row> up|down" or "up|down" for the selected row.
                let (row, dir) = match arg.rsplit_once(' ') {
                    Some((row, dir)) => (row.trim(), dir.trim()),
                    None => ("", arg),
                };
                let from = if row.is_empty() {
                    selected
                } else {
                    self.barpage_row(row)?
                };
                let to = match dir {
                    "up" if from > 0 => from - 1,
                    "down" => from + 1,
                    _ => return Ok(()),
                };
                if to >= CommandBarEdit::items_at(&set, target, &path).len() {
                    return Ok(());
                }
                let edited = CommandBarEdit::move_item(&set, target, &path, from, to);
                self.barpage_commit(edited, Some(to))
            }
            "move" => {
                let (from, to) = arg
                    .split_once(' ')
                    .and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)))
                    .ok_or_else(|| AppError::Invalid("move takes '<from> <to>'".into()))?;
                let edited = CommandBarEdit::move_item(&set, target, &path, from, to);
                self.barpage_commit(edited, Some(to))
            }
            "name" => {
                let items = CommandBarEdit::items_at(&set, target, &path);
                let Some(item) = items.get(selected) else {
                    return Ok(());
                };
                if item.is_separator() || item.is_list() {
                    return Ok(());
                }
                let caption = arg.trim();
                if item.is_menu() && caption.is_empty() {
                    return Ok(());
                }
                let wanted = (!caption.is_empty()).then(|| caption.to_string());
                if wanted == item.caption {
                    return Ok(());
                }
                let edited = CommandBarEdit::replace(
                    &set,
                    target,
                    &path,
                    selected,
                    BarItem {
                        caption: wanted,
                        ..item.clone()
                    },
                );
                self.barpage_commit(edited, Some(selected))
            }
            "row-show-text" => {
                let items = CommandBarEdit::items_at(&set, target, &path);
                let Some(item) = items.get(selected) else {
                    return Ok(());
                };
                let on = matches!(arg, "on" | "true" | "1");
                let edited = CommandBarEdit::replace(
                    &set,
                    target,
                    &path,
                    selected,
                    BarItem {
                        show_text: on.then_some(true),
                        ..item.clone()
                    },
                );
                self.barpage_commit(edited, Some(selected))
            }
            "show-text" => {
                let on = matches!(arg, "on" | "true" | "1");
                let edited = CommandBarEdit::with_show_text(&set, target, on);
                self.barpage_commit(edited, None)
            }
            "add-row" => {
                let edited = CommandBarEdit::add_row(&set);
                let row = edited.rows.len() - 1;
                self.bars_set(edited)?;
                let mut page = lock(&self.extra.barpage);
                page.target = BarTarget::for_row(row);
                page.path.clear();
                page.selected_row = 0;
                drop(page);
                changed();
                Ok(())
            }
            "remove-row" => {
                if target.kind != BarTargetKind::Row {
                    return Ok(());
                }
                let edited = CommandBarEdit::remove_row(&set, target.row);
                self.barpage_commit(edited, None)
            }
            "filter" => {
                lock(&self.extra.barpage).filter = arg.to_string();
                changed();
                Ok(())
            }
            "unplaced" => {
                lock(&self.extra.barpage).unplaced_only = matches!(arg, "on" | "true" | "1");
                changed();
                Ok(())
            }
            "reset" => {
                lock(&self.extra.barpage).path.clear();
                self.bars_reset()
            }
            "apply-saved" => {
                lock(&self.extra.barpage).path.clear();
                self.bars_apply(arg)
            }
            "remove-saved" => self.bars_remove(arg),
            "select-saved" => {
                lock(&self.extra.barpage).selected_saved = Some(arg.to_string());
                changed();
                Ok(())
            }
            "save" => self.bars_save_as(arg),
            "rename-saved" => {
                let (old, new) = arg.split_once(" to ").ok_or_else(|| {
                    AppError::Invalid("rename-saved takes '<old> to <new>'".into())
                })?;
                self.bars_rename(old.trim(), new.trim())
            }
            other => Err(AppError::Invalid(format!("unknown bars verb '{other}'"))),
        }
    }
}
