//! The public surface of the services around the tabs: new items, the session, the command
//! registry and the keymap, the palette, the command bars, columns and their property seam, saved
//! searches and workspaces, settings and their page, themes, the index and the host's window state.
//! Each is a thin front for an `Inner` verb. Models (a page, a list) come back as JSON text so the
//! host re-reads one thing on `ModelChanged`; the `*_do` entries take a verb and its arguments so a
//! page's whole vocabulary is one export.

use bb_core::columns::ColumnValue;
use bb_core::commands::{FocusState, KeyChord};
use bb_core::data::SavedSearchScope;
use bb_core::newitem::NewItemKind;
use bb_core::theming::SystemAppearance;
use serde_json::{Value, json};

use crate::app::{App, lock};
use crate::columns::PropSupply;
use crate::commands::CommandOutcome;
use crate::ids::{AppError, AppResult, OpId, PaneId, TabId};

fn text(v: &Value) -> String {
    v.to_string()
}

fn on(arg: &str) -> bool {
    matches!(
        arg.trim().to_lowercase().as_str(),
        "on" | "true" | "1" | "yes"
    )
}

impl App {
    // ---- new items ----

    /// The name the New dialog opens with: the type's default, stepped aside to "(2)" if taken.
    /// `extension` is the file type's (with its dot), ignored for a folder.
    pub fn new_item_suggest(
        &self,
        tab: TabId,
        kind: NewItemKind,
        extension: &str,
    ) -> AppResult<String> {
        self.inner.new_item_suggest(tab, kind, extension)
    }

    /// What creating `name` would produce, without changing anything, as JSON
    /// `{directory, name, target, ok, rejected}`.
    pub fn new_item_plan(&self, tab: TabId, name: &str, kind: NewItemKind) -> AppResult<String> {
        self.inner
            .new_item_plan_json(tab, name, kind)
            .map(|v| text(&v))
    }

    /// Creates a folder or file in the folder the tab shows, selecting it when its listing arrives.
    pub fn new_item(&self, tab: TabId, name: &str, kind: NewItemKind) -> AppResult<OpId> {
        self.inner.op_new_item(tab, name, kind)
    }

    /// The file types the New submenu offers: `[{label, extension, templatePath}]`.
    pub fn new_file_types_json(&self) -> String {
        text(&Value::Array(
            self.inner
                .new_file_types()
                .iter()
                .map(|t| json!({ "label": t.label, "extension": t.extension, "templatePath": t.template_path }))
                .collect(),
        ))
    }

    // ---- the session ----

    /// What closing the window would save, as the settings file spells it.
    pub fn session_capture_json(&self) -> String {
        serde_json::to_string(&self.inner.capture_layout()).unwrap_or_default()
    }

    /// Writes the arrangement into the settings now.
    pub fn session_save(&self) -> AppResult<()> {
        self.inner.session_save();
        self.inner.save_settings_now()
    }

    /// The launch rule: the saved session when asked for and usable, otherwise one tab at the
    /// startup folder (or `fallback`). Returns the active tab.
    pub fn session_restore(&self, fallback: &str) -> AppResult<TabId> {
        self.inner.session_restore(fallback)
    }

    /// Captures, prunes and rebuilds the arrangement in place. False when nothing was worth keeping.
    pub fn session_roundtrip(&self) -> AppResult<bool> {
        self.inner.session_roundtrip()
    }

    pub fn move_tab(&self, tab: TabId, index: usize) -> AppResult<()> {
        self.inner.move_tab(tab, index)
    }

    pub fn move_tab_to_pane(&self, tab: TabId, pane: PaneId) -> AppResult<()> {
        self.inner.move_tab_to_pane(tab, pane)
    }

    pub fn swap_panes(&self) -> AppResult<()> {
        self.inner.swap_panes()
    }

    pub fn equalise_panes(&self) {
        self.inner.equalise_panes();
    }

    // ---- commands ----

    /// Every command in the state a menu would draw it: JSON array of
    /// `{id, name, category, icon, gesture, context, owner, whyNot, checked, caption}`.
    pub fn command_list_json(&self) -> String {
        text(&Value::Array(
            self.inner
                .command_list()
                .iter()
                .map(|c| c.to_json())
                .collect(),
        ))
    }

    pub fn command_state_json(&self, id: &str) -> Option<String> {
        self.inner.command_state(id).map(|c| text(&c.to_json()))
    }

    /// A key press the way the window's one handler takes it; JSON
    /// `{claimed, command?, result?: "ran"|"ui"|"unavailable"|"ignored", reason?}`.
    pub fn key_press(&self, chord: &KeyChord, state: FocusState, repeat: bool) -> String {
        text(&self.inner.key_press(chord, state, repeat))
    }

    // ---- the keymap and its page ----

    /// Gives a command exactly these shortcuts (texts like `Ctrl+Shift+K`), taking each from
    /// whoever had it. Written at once.
    pub fn keymap_set_chords(&self, id: &str, chords: &[String]) -> AppResult<()> {
        let parsed: Vec<KeyChord> = chords
            .iter()
            .map(|c| {
                KeyChord::try_parse(c)
                    .ok_or_else(|| AppError::Invalid(format!("'{c}' is not a key")))
            })
            .collect::<Result<_, _>>()?;
        self.inner.keymap_set_chords(id, &parsed)
    }

    pub fn keymap_reset(&self, id: &str) -> AppResult<()> {
        self.inner.keymap_reset(id)
    }

    pub fn keymap_reset_all(&self) -> AppResult<()> {
        self.inner.keymap_reset_all()
    }

    /// The shortcut a menu prints beside the command; empty when it has none.
    pub fn keymap_gesture(&self, id: &str) -> String {
        self.inner.keymap_gesture(id)
    }

    pub fn keymap_chords(&self, id: &str) -> Vec<String> {
        self.inner.keymap_chords(id)
    }

    pub fn keys_page_json(&self) -> String {
        text(&self.inner.key_page_json())
    }

    /// One verb of the Keyboard page: `filter <text>`, `select <id>`, `change`, `add`, `find`,
    /// `cancel`, `press <chord>`, `reassign`, `remove`, `reset`, `reset-all`.
    pub fn keys_do(&self, verb: &str, arg: &str) -> AppResult<()> {
        let i = &self.inner;
        match verb {
            "filter" => i.key_set_filter(arg),
            "select" => i.key_select(arg),
            "change" => i.key_begin(false)?,
            "add" => i.key_begin(true)?,
            "find" => i.key_find(),
            "cancel" => i.key_cancel(),
            "press" => i.key_offer(arg)?,
            "reassign" => i.key_reassign()?,
            "remove" => i.key_remove()?,
            "reset" => i.key_reset_selected()?,
            "reset-all" => i.key_reset_all()?,
            other => return Err(AppError::Invalid(format!("unknown keys verb '{other}'"))),
        }
        Ok(())
    }

    // ---- the palette ----

    pub fn palette_open(&self, seed: &str) {
        self.inner.palette_open(seed);
    }

    pub fn palette_close(&self) {
        self.inner.palette_close();
    }

    pub fn palette_is_open(&self) -> bool {
        self.inner.palette_is_open()
    }

    /// `{open, query, hint, empty, selected, rows:[{index, kind, text, id, icon, gesture, detail,
    /// available, pinned, canPin, canBind, token, count, highlights, selected}]}`.
    pub fn palette_json(&self) -> String {
        text(&self.inner.palette_json())
    }

    /// What was run since the window opened, most recent first. In memory only.
    pub fn palette_recents(&self) -> Vec<String> {
        self.inner.palette_recents()
    }

    /// One verb of the palette: `type <text>`, `down [n]`, `up [n]`, `home`, `end`, `select <row>`,
    /// `pick` (Enter), `complete` (Tab), `pin`, `shortcut`, `close`. `pick` answers
    /// `{ran, ui?, command?, narrowed?, unavailable?}`.
    pub fn palette_do(&self, verb: &str, arg: &str) -> AppResult<String> {
        let i = &self.inner;
        let steps = || arg.trim().parse::<i64>().unwrap_or(1);
        match verb {
            "type" => i.palette_set_query(arg),
            "down" => i.palette_move(steps()),
            "up" => i.palette_move(-steps()),
            "home" => i.palette_edge(false),
            "end" => i.palette_edge(true),
            "select" => i.palette_select(arg.trim().parse().unwrap_or(0)),
            "complete" => i.palette_complete(),
            "pin" => i.palette_toggle_pin(),
            "shortcut" => i.palette_change_shortcut()?,
            "close" => i.palette_close(),
            "pick" => return i.palette_activate().map(|v| text(&v)),
            other => return Err(AppError::Invalid(format!("unknown palette verb '{other}'"))),
        }
        Ok("{}".to_string())
    }

    // ---- menu bar and toolbars ----

    /// `{isDefault, name, replacingLosesWork, menus, title, rows, saved, json}`.
    pub fn bars_state_json(&self) -> String {
        text(&self.inner.bars_state_json())
    }

    /// The menu bar's headings, access keys assigned.
    pub fn bars_menu_bar_json(&self) -> String {
        text(&self.inner.bars_menu_bar_json())
    }

    /// One top-level menu as it would open right now (`which`: its name or position).
    pub fn bars_menu_json(&self, which: &str) -> AppResult<String> {
        self.inner.bars_menu_json(which).map(|v| text(&v))
    }

    /// A toolbar (`title`, `row1`, ...) as it shows right now.
    pub fn bars_toolbar_json(&self, which: &str) -> AppResult<String> {
        self.inner.bars_toolbar_json(which).map(|v| text(&v))
    }

    /// The rows a list (`workspaces`, `drives`, `undo-steps`, ...) expands to.
    pub fn bars_list_rows_json(&self, list: &str) -> String {
        text(&Value::Array(self.inner.list_rows(list)))
    }

    pub fn bars_set_json(&self, json_text: &str) -> AppResult<()> {
        self.inner.bars_set_json(json_text)
    }

    /// Replaces a toolbar's items: tokens are command ids, `-` and `@list`. `show_text`: -1 keeps.
    pub fn bars_set_toolbar(&self, target: &str, tokens: &str, show_text: i32) -> AppResult<()> {
        self.inner.bars_set_tokens(
            target,
            tokens,
            match show_text {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            },
        )
    }

    pub fn bars_set_menu(&self, caption: &str, tokens: &str) -> AppResult<()> {
        self.inner.bars_set_menu(caption, tokens)
    }

    /// One verb of the command-bar service: `reset`, `save <name>`, `apply <name>`,
    /// `rename <old> to <new>`, `remove <name>`.
    pub fn bars_do(&self, verb: &str, arg: &str) -> AppResult<()> {
        let i = &self.inner;
        match verb {
            "reset" => i.bars_reset(),
            "save" => i.bars_save_as(arg),
            "apply" => i.bars_apply(arg),
            "remove" => i.bars_remove(arg),
            "rename" => {
                let (old, new) = arg
                    .split_once(" to ")
                    .ok_or_else(|| AppError::Invalid("rename takes '<old> to <new>'".into()))?;
                i.bars_rename(old.trim(), new.trim())
            }
            other => Err(AppError::Invalid(format!("unknown bars verb '{other}'"))),
        }
    }

    /// The Menus and toolbars page's editor model.
    pub fn barpage_json(&self) -> String {
        text(&self.inner.barpage_json())
    }

    /// One verb of that editor: `select <bar>`, `pick <row>`, `open <row>`, `up`, `add <id|name>`,
    /// `add-separator`, `add-menu`, `remove [row]`, `nudge [row] up|down`, `move <from> <to>`,
    /// `name <caption>`, `show-text on|off`, `row-show-text on|off`, `add-row`, `remove-row`,
    /// `filter <text>`, `unplaced on|off`, `reset`, `apply-saved <name>`, `remove-saved <name>`,
    /// `select-saved <name>`, `save <name>`, `rename-saved <old> to <new>`.
    pub fn barpage_do(&self, verb: &str, arg: &str) -> AppResult<()> {
        self.inner.barpage_do(verb, arg)
    }

    // ---- columns ----

    /// `{customized, columns:[{id, header, width, cell, rightAligned, sortable, injected,
    /// property, canonical}]}`.
    pub fn columns_json(&self, tab: TabId) -> AppResult<String> {
        self.inner.columns_json(tab).map(|v| text(&v))
    }

    /// One verb on a tab's columns: `add <id>`, `remove <id>`, `move <id> <index>`,
    /// `width <id> <dips>`, `reset`, `default` (make this the layout new tabs start from),
    /// `picked <id>,<id>...`, `order <id>,<id>...` (after a header drag).
    pub fn columns_do(&self, tab: TabId, verb: &str, arg: &str) -> AppResult<()> {
        let i = &self.inner;
        let mut parts = arg.split_whitespace();
        match verb {
            "add" => i.columns_toggle(tab, arg.trim(), true),
            "remove" => i.columns_toggle(tab, arg.trim(), false),
            "move" => {
                let id = parts.next().unwrap_or("");
                let at = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                i.columns_move(tab, id, at)
            }
            "width" => {
                let id = parts.next().unwrap_or("");
                let w = parts
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or_else(|| AppError::Invalid("width takes '<id> <dips>'".into()))?;
                i.columns_width(tab, id, w)
            }
            "reset" => i.columns_reset(tab),
            "default" => i.columns_use_for_new_tabs(tab),
            "picked" => {
                let chosen: Vec<String> = arg
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                i.columns_apply_picked(tab, &chosen)
            }
            "order" => {
                let live: Vec<String> = arg
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                i.columns_capture_order(tab, &live)
            }
            other => Err(AppError::Invalid(format!("unknown columns verb '{other}'"))),
        }
    }

    /// What the Add column popup offers for `search`.
    pub fn columns_candidates_json(&self, tab: TabId, search: &str) -> AppResult<String> {
        self.inner
            .columns_candidates_json(tab, search)
            .map(|v| text(&v))
    }

    /// The properties this machine reported, as a JSON array of `[canonical, display]`.
    pub fn columns_set_machine_properties(&self, json_text: &str) -> AppResult<()> {
        let list: Vec<(String, String)> =
            serde_json::from_str(json_text).map_err(|e| AppError::Invalid(e.to_string()))?;
        self.inner.columns_set_machine_properties(list);
        Ok(())
    }

    /// The shell-property cells the host should read for rows `from..from+count` (0: the first
    /// 256): JSON `[[path, canonical], ...]`. Each cell is handed out once.
    pub fn prop_requests(&self, tab: TabId, from: usize, count: usize) -> AppResult<String> {
        let list = self.inner.prop_requests(tab, from, count)?;
        Ok(text(&Value::Array(
            list.into_iter().map(|(p, c)| json!([p, c])).collect(),
        )))
    }

    /// The host's answers, as JSON `[{path, canonical, value?: {display, number?, date?}}]` where
    /// `date` is UTC FILETIME ticks and a missing `value` means the handler had none.
    pub fn prop_supply_json(&self, tab: TabId, json_text: &str) -> AppResult<()> {
        let list: Vec<Value> =
            serde_json::from_str(json_text).map_err(|e| AppError::Invalid(e.to_string()))?;
        let supplied = list
            .iter()
            .map(|e| {
                let value = e.get("value").filter(|v| !v.is_null()).map(|v| {
                    let display = v.get("display").and_then(Value::as_str).unwrap_or("");
                    if let Some(n) = v.get("number").and_then(Value::as_f64) {
                        ColumnValue::number(display, n)
                    } else if let Some(d) = v.get("date").and_then(Value::as_i64) {
                        ColumnValue::date(display, bb_core::FileTime(d))
                    } else {
                        ColumnValue::words(display)
                    }
                });
                PropSupply {
                    path: e
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    canonical: e
                        .get("canonical")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    value,
                }
            })
            .collect();
        self.inner.prop_supply(tab, supplied)
    }

    /// Typed form of the above, for tests and in-process hosts.
    pub fn prop_supply(&self, tab: TabId, supplied: Vec<PropSupply>) -> AppResult<()> {
        self.inner.prop_supply(tab, supplied)
    }

    // ---- tiles ----

    /// `{scale, isThumbnails, lastScale, aspect: {width, height, ratio}}`: the thumbnail view's
    /// state. The thumbnails themselves are the host's.
    pub fn tile_state_json(&self, tab: TabId) -> AppResult<String> {
        let cell = self.inner.tab(tab)?;
        let scale = lock(&cell).thumbnail_scale;
        let aspect = lock(&self.inner.settings).tile_aspect_ratio.clone();
        let (w, h) = parse_aspect(&aspect);
        Ok(text(&json!({
            "scale": scale,
            "isThumbnails": scale > 0.0,
            "lastScale": *lock(&self.inner.extra.last_thumbnail_scale),
            "aspect": { "width": w, "height": h, "ratio": w / h, "text": format!("{w}:{h}") },
        })))
    }

    // ---- saved searches, workspaces, bookmarks ----

    /// `{searches, workspaces, workspacesPlacement, savedSearchesPlacement}`.
    pub fn saved_json(&self) -> String {
        text(&self.inner.saved_json())
    }

    /// One verb: `save-search <name> [current|folder|pc]` on `tab`, `run-search <name>`,
    /// `run-search-new-tab <name>`, `rename-search <old> to <new>`, `remove-search <name>`,
    /// `save-workspace <name>`, `switch-workspace <name>`, `rename-workspace <old> to <new>`,
    /// `remove-workspace <name>`, `toggle-bookmark <path>`.
    pub fn saved_do(&self, tab: Option<TabId>, verb: &str, arg: &str) -> AppResult<()> {
        let i = &self.inner;
        let pair = |arg: &str| -> AppResult<(String, String)> {
            arg.split_once(" to ")
                .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
                .ok_or_else(|| AppError::Invalid("rename takes '<old> to <new>'".into()))
        };
        match verb {
            "save-search" => {
                let tab = tab.ok_or(AppError::Stale)?;
                // "<name> pc" / "<name> folder": the last word names the scope when it is one.
                let (name, scope) = match arg.rsplit_once(' ') {
                    Some((name, s)) if matches!(s, "pc" | "folder" | "current") => (name, s),
                    _ => (arg, "current"),
                };
                i.search_save_current(tab, name.trim(), scope)
            }
            "run-search" => i.search_run_saved(arg, false),
            "run-search-new-tab" => i.search_run_saved(arg, true),
            "rename-search" => {
                let (a, b) = pair(arg)?;
                i.search_rename(&a, &b)
            }
            "remove-search" => i.search_remove(arg),
            "save-workspace" => i.workspace_save(arg),
            "switch-workspace" => i.workspace_switch(arg),
            "rename-workspace" => {
                let (a, b) = pair(arg)?;
                i.workspace_rename(&a, &b)
            }
            "remove-workspace" => i.workspace_remove(arg),
            "toggle-bookmark" => i.toggle_bookmark(arg, std::path::Path::new(arg).is_dir()),
            other => Err(AppError::Invalid(format!("unknown saved verb '{other}'"))),
        }
    }

    /// The name a new saved search or workspace starts with.
    pub fn saved_default_name(&self, tab: TabId, workspace: bool) -> AppResult<String> {
        if workspace {
            Ok(self.inner.workspace_default_name())
        } else {
            self.inner.search_default_name(tab)
        }
    }

    /// The first reason a workspace cannot be saved under this name, or None.
    pub fn saved_workspace_problem(&self, name: &str) -> Option<String> {
        self.inner.workspace_problem(name)
    }

    /// The first reason a saved search cannot be saved, or None.
    pub fn saved_search_problem(
        &self,
        name: &str,
        query: &str,
        scope: &str,
        scope_path: Option<&str>,
    ) -> Option<String> {
        let scope = match scope {
            "folder" => SavedSearchScope::Folder,
            "pc" => SavedSearchScope::ThisPc,
            _ => SavedSearchScope::CurrentFolder,
        };
        self.inner
            .search_problem(name, query, scope, scope_path, None)
    }

    // ---- settings ----

    /// Writes what the page still holds. Called on Back and on window close.
    pub fn settings_flush(&self) {
        self.inner.flush_settings();
    }

    /// `{open, page, pages:[{id, name, icon}], problem}`.
    pub fn settings_page_json(&self) -> String {
        let page = lock(&self.inner.extra.ui).settings_page.clone();
        text(&json!({
            "open": page.is_some(),
            "page": page,
            "pages": crate::settings_svc::SETTINGS_PAGES.iter()
                .map(|(id, name, icon)| json!({ "id": id, "name": name, "icon": icon }))
                .collect::<Vec<_>>(),
            "problem": self.inner.settings_problem(),
        }))
    }

    /// Opens the Settings page on `page` (empty: General). Only app-wide shortcuts run meanwhile.
    pub fn settings_open(&self, page: &str) -> AppResult<()> {
        self.inner.settings_open_page(page)
    }

    /// Back: flushes, and refuses (with the reason) while a list is still unwritable.
    pub fn settings_close(&self) -> Result<(), String> {
        self.inner.settings_close_page()
    }

    // ---- themes ----

    /// `{id, name, isDark, following, lightSlot, darkSlot, appearance, available, overridden,
    /// dirty, issues}`.
    pub fn theme_json(&self) -> String {
        text(&self.inner.theme_json())
    }

    /// The host tells us what Windows looks like now.
    pub fn theme_set_system_appearance(&self, is_dark: bool, high_contrast: bool) {
        self.inner.theme_system_changed(SystemAppearance {
            is_dark,
            is_high_contrast: high_contrast,
        });
    }

    /// One verb: `select <id>`, `follow on|off`, `slot light|dark <id>`, `override <token> <#hex>`,
    /// `clear-override <token>`, `persist`, `revert`, `reset-all`, `save-as <name>`, `import <path>`,
    /// `delete <id>`. `save-as` and `import` answer the new id.
    pub fn theme_do(&self, verb: &str, arg: &str) -> AppResult<String> {
        let i = &self.inner;
        match verb {
            "select" => i.theme_select(arg),
            "follow" => i.theme_follow_set(on(arg)),
            "slot" => {
                let (which, id) = arg
                    .split_once(' ')
                    .ok_or_else(|| AppError::Invalid("slot takes 'light|dark <id>'".into()))?;
                i.theme_slot_set(which.trim().eq_ignore_ascii_case("dark"), id.trim())?;
            }
            "override" => {
                let (token, color) = arg
                    .split_once(' ')
                    .ok_or_else(|| AppError::Invalid("override takes '<token> <colour>'".into()))?;
                i.theme_override_set(token.trim(), Some(color.trim()))?;
            }
            "clear-override" => i.theme_override_set(arg.trim(), None)?,
            "persist" => i.theme_overrides_persist()?,
            "revert" => i.theme_overrides_revert(),
            "reset-all" => i.theme_overrides_reset_all()?,
            "save-as" => return i.theme_save_as(arg),
            "import" => return i.theme_import(arg),
            "delete" => i.theme_delete(arg)?,
            other => return Err(AppError::Invalid(format!("unknown theme verb '{other}'"))),
        }
        Ok(String::new())
    }

    // ---- the search index ----

    /// `{status, canRetry, canStart, presence, anyIndexed, building, banner:{show, message, canStart}}`.
    pub fn index_json(&self) -> String {
        text(&self.inner.index_json())
    }

    /// `dismiss-banner`, `start` (the one thing that raises the elevation prompt), `retry`.
    pub fn index_do(&self, verb: &str) -> AppResult<()> {
        match verb {
            "dismiss-banner" => {
                self.inner.index_dismiss_banner();
                Ok(())
            }
            "start" => self.inner.index_start(),
            "retry" => self.inner.index_retry(),
            other => Err(AppError::Invalid(format!("unknown index verb '{other}'"))),
        }
    }

    // ---- what the host tells us about the window ----

    /// `clipboardHasFiles`, `previewHasMedia`, `propertyProvider` (the host reads shell properties).
    pub fn ui_set_flag(&self, name: &str, value: bool) {
        self.inner.ui_set_flag(name, value);
    }

    /// A modal dialog opened or closed.
    pub fn ui_set_dialog_open(&self, open: bool) {
        self.inner.ui_set_dialog_open(open);
    }

    /// Starts or stops comparing the active pane with the next one. (The comparison engine itself is
    /// not part of this layer; the switch, its gates and its state are.)
    pub fn compare_toggle(&self) -> AppResult<()> {
        self.inner.compare_toggle()
    }

    pub fn compare_end(&self) {
        self.inner.compare_end();
    }

    /// Whether running the command was the host's to do: convenience for hosts that only care.
    pub fn command_needs_ui(outcome: CommandOutcome) -> bool {
        outcome == CommandOutcome::NeedsUi
    }
}

/// "w:h" as two positive numbers; anything else is 4:3, so hand-editing cannot break the view.
pub fn parse_aspect(text: &str) -> (f64, f64) {
    let parsed = text.split_once(':').and_then(|(w, h)| {
        let (w, h) = (w.trim().parse::<f64>().ok()?, h.trim().parse::<f64>().ok()?);
        (w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()).then_some((w, h))
    });
    parsed.unwrap_or((4.0, 3.0))
}
