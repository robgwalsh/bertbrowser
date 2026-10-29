//! The command palette's model. It holds no rule of its own about what to list or in what order:
//! it gathers what can be run, asks `PaletteRules`, and answers with rows. **The empty state is
//! never the catalogue**: pinned, then this session's recents, then what suits the selection, then
//! one row per category. Recents live in memory only (a list of what was run is a history, and a
//! persisted one would owe the opt-in switch every history feature here has); pins are a preference
//! and are saved. Spec: CommandPaletteViewModel, MainWindow.Palette, PaletteMemory.

use std::sync::Arc;

use bb_core::columns::{ColumnCatalog, ColumnLayoutRules};
use bb_core::commands::{
    CommandCatalog, CommandProminence, GoToRules, PaletteEntry, PaletteRow, PaletteRowKind,
    PaletteRules, PaletteState, PaletteSuggestions,
};
use bb_core::data::SavedSearchScope;
use bb_core::layout::SplitOrientation;
use bb_core::rules::SavedWorkspaceRules;
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::commands::{CommandOutcome, SETTINGS_OPEN_REASON};
use crate::events::Model;
use crate::ids::{AppError, AppResult};
use crate::settings_svc::SETTINGS_PAGES;

/// How many folders a typed path lists under it. A screenful: past that, typing another letter
/// narrows faster than reading does.
const MAX_PLACES: usize = 12;
const MAX_RECENTS: usize = 20;

/// What choosing an entry does.
#[derive(Clone, Debug)]
pub(crate) enum PaletteAction {
    Command(String),
    Bars(String),
    Workspace(String),
    Search(String),
    Bookmark(String),
    NewFile(String),
    Tab(usize),
    Go(String),
    OpenInTab(String),
    OpenInPane(String),
    Reveal(String),
    Column(String, bool),
    Theme(String),
    SettingsPage(String),
    Nothing,
}

#[derive(Clone, Debug)]
pub(crate) struct PaletteItem {
    pub entry: PaletteEntry,
    pub action: PaletteAction,
    /// Whether a keyboard shortcut can be given to it.
    pub bindable: bool,
    /// Whether running it goes into "Recent".
    pub remembered: bool,
    /// For a place: the text that carries on from it.
    pub completion: Option<String>,
}

#[derive(Clone)]
pub(crate) struct PaletteRowModel {
    pub row: PaletteRow,
    pub item: Option<PaletteItem>,
    pub pinned: bool,
    pub detail: String,
}

#[derive(Default)]
pub(crate) struct PaletteModel {
    pub open: bool,
    pub query: String,
    pub items: Vec<PaletteItem>,
    pub suggested: Vec<String>,
    pub rows: Vec<PaletteRowModel>,
    pub selected: Option<usize>,
    pub hint: String,
    pub empty: bool,
}

impl PaletteRowModel {
    fn is_heading(&self) -> bool {
        self.row.kind == PaletteRowKind::Heading
    }

    fn is_category(&self) -> bool {
        self.row.kind == PaletteRowKind::Category
    }

    fn is_available(&self) -> bool {
        self.row
            .entry
            .as_ref()
            .is_none_or(PaletteEntry::is_available)
    }

    fn can_pin(&self) -> bool {
        self.row.kind == PaletteRowKind::Entry && self.item.as_ref().is_some_and(|i| i.remembered)
    }

    fn can_bind(&self) -> bool {
        self.row.kind == PaletteRowKind::Entry && self.item.as_ref().is_some_and(|i| i.bindable)
    }
}

fn leaf_name(path: &str) -> String {
    let trimmed = path.trim_end_matches('\\');
    match trimmed.rsplit('\\').next() {
        Some(leaf) if !leaf.is_empty() => leaf.to_string(),
        _ => path.to_string(),
    }
}

fn slashed(path: &str) -> String {
    if path.ends_with('\\') {
        path.to_string()
    } else {
        format!("{path}\\")
    }
}

fn is_remote(path: &str) -> bool {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    if path.starts_with("\\\\") {
        return true;
    }
    let b = path.as_bytes();
    if b.len() >= 2 && b[1] == b':' {
        let root: Vec<u16> = format!("{}:\\", &path[..1])
            .encode_utf16()
            .chain([0])
            .collect();
        // DRIVE_REMOTE == 4. Asking the drive's type touches no network.
        return unsafe { GetDriveTypeW(root.as_ptr()) } == 4;
    }
    false
}

fn place(
    name: &str,
    detail: &str,
    icon: &str,
    blocked: Option<&str>,
    action: PaletteAction,
    completion: Option<String>,
) -> PaletteItem {
    let mut entry = PaletteEntry::new(&format!("goto:{name}"), name, "Go to");
    entry.icon = Some(icon.to_string());
    entry.unavailable = blocked.map(str::to_string);
    entry.detail = detail.to_string();
    PaletteItem {
        entry,
        action,
        bindable: false,
        // A place somebody typed their way to is not a command to offer again unasked.
        remembered: false,
        completion,
    }
}

impl Inner {
    pub(crate) fn palette_is_open(&self) -> bool {
        lock(&self.extra.palette).open
    }

    // ---- what it can run ----

    fn build_palette_items(&self) -> (Vec<PaletteItem>, Vec<String>) {
        let ctx = self.command_ctx();
        let blocked: Option<String> = ctx
            .is_settings_open()
            .then(|| SETTINGS_OPEN_REASON.to_string());
        let blocked = blocked.as_deref();
        let mut items = Vec::new();

        for command in CommandCatalog::all() {
            // The palette is not offered as a row in itself.
            if command.id == "app.palette" {
                continue;
            }
            let Some(state) = self.command_state_in(&ctx, command.id) else {
                continue;
            };
            let mut entry = PaletteEntry::new(command.id, command.name, command.category);
            entry.icon = command.icon.map(str::to_string);
            entry.gesture = state.gesture.clone();
            entry.aliases = command.aliases.iter().map(|a| a.to_string()).collect();
            entry.prominence = command.prominence;
            entry.unavailable = state.why_not;
            items.push(PaletteItem {
                entry,
                action: PaletteAction::Command(command.id.to_string()),
                bindable: true,
                remembered: true,
                completion: None,
            });
        }

        // Not blocked by the settings page: recalling an arrangement is what its own page is for.
        let in_force = self.bars_current_name();
        for saved in self.bars_saved() {
            let mut entry = PaletteEntry::new(
                &format!("bars:{}", saved.name),
                &saved.name,
                "Menus and toolbars",
            );
            entry.icon = Some("Icon.Reorder".into());
            entry.aliases = vec![
                "switch menus".into(),
                "toolbar arrangement".into(),
                "menu bar".into(),
            ];
            entry.detail = if in_force
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(&saved.name))
            {
                "in use".to_string()
            } else {
                bb_core::commandbars::SavedCommandBarRules::shape_text(&saved.set)
            };
            items.push(PaletteItem {
                entry,
                action: PaletteAction::Bars(saved.name.clone()),
                bindable: false,
                remembered: true,
                completion: None,
            });
        }

        for workspace in self.saved_workspaces() {
            let mut entry = PaletteEntry::new(
                &format!("workspace:{}", workspace.name),
                &workspace.name,
                "Workspaces",
            );
            entry.icon = Some("Icon.SplitRight".into());
            entry.aliases = vec!["switch workspace".into(), "layout".into(), "tab set".into()];
            entry.unavailable = blocked.map(str::to_string);
            entry.detail = SavedWorkspaceRules::shape_text(&workspace.layout);
            items.push(PaletteItem {
                entry,
                action: PaletteAction::Workspace(workspace.name.clone()),
                bindable: false,
                remembered: true,
                completion: None,
            });
        }

        for search in self.saved_searches() {
            let mut entry = PaletteEntry::new(
                &format!("search:{}", search.name),
                &search.name,
                "Saved searches",
            );
            entry.icon = Some("Icon.Search".into());
            entry.aliases = vec!["run search".into()];
            entry.unavailable = blocked.map(str::to_string);
            entry.detail = match search.scope {
                SavedSearchScope::ThisPc => "This PC".to_string(),
                SavedSearchScope::CurrentFolder => "Current folder".to_string(),
                SavedSearchScope::Folder => search.scope_path.clone().unwrap_or_default(),
            };
            items.push(PaletteItem {
                entry,
                action: PaletteAction::Search(search.name.clone()),
                bindable: false,
                remembered: true,
                completion: None,
            });
        }

        for bookmark in self.bookmark_list() {
            let name = leaf_name(&bookmark.display_path);
            let mut entry = PaletteEntry::new(
                &format!("bookmark:{}", bookmark.display_path),
                &name,
                "Bookmarks",
            );
            entry.icon = Some("Icon.Bookmark".into());
            entry.aliases = vec!["favourite".into(), "favorite".into(), "go to".into()];
            entry.unavailable = blocked.map(str::to_string);
            entry.detail = bookmark.display_path.clone();
            items.push(PaletteItem {
                entry,
                action: PaletteAction::Bookmark(bookmark.display_path.clone()),
                bindable: false,
                remembered: true,
                completion: None,
            });
        }

        let custom_ids = self.custom_command_ids();
        for id in custom_ids {
            if let Some(state) = self.command_state_in(&ctx, &id)
                && let Some(def) = self.custom_command(&id)
            {
                let mut entry = PaletteEntry::new(&id, &def.name, "Custom commands");
                entry.icon = state.icon.clone();
                entry.gesture = state.gesture.clone();
                entry.unavailable = blocked.map(str::to_string).or(state.why_not);
                entry.detail = std::path::Path::new(&def.command)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                items.push(PaletteItem {
                    entry,
                    action: PaletteAction::Command(id.clone()),
                    bindable: true,
                    remembered: true,
                    completion: None,
                });
            }
        }

        let new_blocked = blocked
            .map(str::to_string)
            .or_else(|| self.registry_why_not(&ctx, "file.new-file"));
        for template in self.new_file_types() {
            let mut entry = PaletteEntry::new(
                &format!("new:{}|{}", template.label, template.extension),
                &format!("New {}", template.label),
                "New",
            );
            entry.icon = Some("Icon.NewFile".into());
            entry.aliases = vec!["create file".into(), template.extension.clone()];
            entry.unavailable = new_blocked.clone();
            entry.detail = template.extension.clone();
            items.push(PaletteItem {
                entry,
                action: PaletteAction::NewFile(template.extension.clone()),
                bindable: false,
                remembered: true,
                completion: None,
            });
        }

        let mut number = 0;
        for (path, title) in self.all_tab_titles() {
            number += 1;
            let mut entry = PaletteEntry::new(&format!("tab:{number}"), &title, "Open tabs");
            entry.icon = Some("Icon.Tab".into());
            entry.aliases = vec!["switch to tab".into()];
            entry.unavailable = blocked.map(str::to_string);
            entry.detail = path;
            items.push(PaletteItem {
                entry,
                action: PaletteAction::Tab(number),
                bindable: false,
                // The third tab today is a different tab tomorrow.
                remembered: false,
                completion: None,
            });
        }

        for (root, label) in self.drive_list() {
            let mut entry = PaletteEntry::new(&format!("drive:{root}"), &root, "Drives");
            entry.icon = Some("Icon.Drive".into());
            entry.aliases = vec!["go to drive".into(), "volume".into()];
            entry.unavailable = blocked.map(str::to_string);
            entry.detail = label;
            items.push(PaletteItem {
                entry,
                action: PaletteAction::Go(root.clone()),
                bindable: false,
                remembered: true,
                completion: Some(root),
            });
        }

        // Every built-in and curated column, each as "show" or "hide" for this tab.
        let showing: std::collections::HashSet<String> =
            ColumnLayoutRules::normalize(self.active_tab_columns().as_deref())
                .into_iter()
                .map(|c| bb_paths::fold_upper(&c.id))
                .collect();
        for spec in ColumnCatalog::built_ins()
            .iter()
            .chain(ColumnCatalog::curated())
        {
            // Folder and Match come and go with the listing, and Name cannot be turned off.
            if ColumnCatalog::is_injected(&spec.id) || spec.id == ColumnCatalog::NAME {
                continue;
            }
            let on = showing.contains(&bb_paths::fold_upper(&spec.id));
            let mut entry = PaletteEntry::new(
                &format!("column:{}", spec.id),
                &format!(
                    "{} column: {}",
                    if on { "Hide" } else { "Show" },
                    spec.header
                ),
                "Columns",
            );
            entry.icon = Some("Icon.Columns".into());
            entry.aliases = vec![spec.header.clone()];
            entry.prominence = CommandProminence::Rare;
            entry.unavailable = blocked.map(str::to_string);
            items.push(PaletteItem {
                entry,
                action: PaletteAction::Column(spec.id.clone(), !on),
                bindable: false,
                remembered: true,
                completion: None,
            });
        }

        let current_theme = self.theme_current_id();
        for theme in self.theme_available() {
            let mut entry =
                PaletteEntry::new(&format!("theme:{}", theme.id), &theme.name, "Themes");
            entry.icon = Some("Icon.Appearance".into());
            entry.aliases = vec![
                "colour theme".into(),
                "color theme".into(),
                "switch theme".into(),
            ];
            if bb_core::theming::ids_equal(&theme.id, &current_theme) {
                entry.detail = "in use".into();
            }
            items.push(PaletteItem {
                entry,
                action: PaletteAction::Theme(theme.id.clone()),
                bindable: false,
                remembered: true,
                completion: None,
            });
        }

        for (id, name, icon) in SETTINGS_PAGES {
            let mut entry = PaletteEntry::new(&format!("settings:{id}"), name, "Settings");
            entry.icon = Some(icon.to_string());
            entry.aliases = vec![
                "settings page".into(),
                "options".into(),
                "preferences".into(),
            ];
            items.push(PaletteItem {
                entry,
                action: PaletteAction::SettingsPage(id.to_string()),
                bindable: false,
                remembered: true,
                completion: None,
            });
        }

        let suggested = PaletteSuggestions::for_state(ctx.selected_count(), ctx.is_settings_open())
            .iter()
            .map(|s| s.to_string())
            .collect();
        (items, suggested)
    }

    /// What a typed path offers, or None when the text is not a path and is therefore a search for
    /// a command. Runs on a keystroke, so it only ever asks about local drives.
    fn palette_places(&self, query: &str, blocked: Option<&str>) -> Option<Vec<PaletteItem>> {
        let home = std::env::var("USERPROFILE").unwrap_or_default();
        let target = GoToRules::parse(query, |name| std::env::var(name).ok(), &home)?;
        let mut places = Vec::new();
        let remote = is_remote(&target.path);

        if remote || std::path::Path::new(&target.path).is_dir() {
            let path = target.path.clone();
            let name = leaf_name(&path);
            places.push(place(
                &format!("Go to {name}"),
                &path,
                "Icon.Folder",
                blocked,
                PaletteAction::Go(path.clone()),
                Some(slashed(&path)),
            ));
            places.push(place(
                &format!("Open {name} in a new tab"),
                &path,
                "Icon.OpenInNewTab",
                blocked,
                PaletteAction::OpenInTab(path.clone()),
                None,
            ));
            places.push(place(
                &format!("Open {name} in a new pane"),
                &path,
                "Icon.OpenInNewPane",
                blocked,
                PaletteAction::OpenInPane(path.clone()),
                None,
            ));
        } else if std::path::Path::new(&target.path).is_file() {
            let file = target.path.clone();
            let name = std::path::Path::new(&file)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            places.push(place(
                &format!("Show {name} in its folder"),
                &file,
                "Icon.MoveToFolder",
                blocked,
                PaletteAction::Reveal(file.clone()),
                None,
            ));
        }

        if !remote {
            places.extend(self.folders_under(
                &target.folder,
                &target.prefix,
                &target.path,
                blocked,
            ));
        }
        if places.is_empty() {
            let mut entry = PaletteEntry::new("goto:none", &target.path, "Go to");
            entry.icon = Some("Icon.Folder".into());
            entry.unavailable = Some("There is no such folder.".into());
            places.push(PaletteItem {
                entry,
                action: PaletteAction::Nothing,
                bindable: false,
                remembered: false,
                completion: None,
            });
        }
        Some(places)
    }

    fn folders_under(
        &self,
        folder: &str,
        prefix: &str,
        typed: &str,
        blocked: Option<&str>,
    ) -> Vec<PaletteItem> {
        let show_hidden = lock(&self.settings).show_hidden_items;
        let Ok(read) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        let prefix_lower = prefix.to_lowercase();
        let mut folders: Vec<String> = read
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .starts_with(&prefix_lower)
            })
            .map(|e| e.path().to_string_lossy().into_owned())
            .filter(|p| show_hidden || !is_hidden_dir(p))
            // The folder that was typed in full is already the first row.
            .filter(|p| !p.eq_ignore_ascii_case(typed))
            .collect();
        folders.sort_by_key(|p| p.to_lowercase());
        folders.truncate(MAX_PLACES);
        folders
            .into_iter()
            .map(|p| {
                place(
                    &leaf_name(&p),
                    "",
                    "Icon.Folder",
                    blocked,
                    PaletteAction::Go(p.clone()),
                    Some(slashed(&p)),
                )
            })
            .collect()
    }

    // ---- the list ----

    fn palette_rebuild(&self, model: &mut PaletteModel, keep_selected: Option<&str>) {
        let pins = lock(&self.settings).palette_pins.clone();
        let blocked = self.palette_blocked().then_some(SETTINGS_OPEN_REASON);
        let mut rows: Vec<PaletteRowModel> = Vec::new();

        if let Some(places) = self.palette_places(&model.query, blocked) {
            rows.push(PaletteRowModel {
                row: PaletteRow {
                    kind: PaletteRowKind::Heading,
                    text: "Go to".into(),
                    entry: None,
                    highlights: Vec::new(),
                    token: String::new(),
                    count: 0,
                },
                item: None,
                pinned: false,
                detail: String::new(),
            });
            for item in places {
                let detail = if let Some(why) = &item.entry.unavailable {
                    why.clone()
                } else {
                    item.entry.detail.clone()
                };
                rows.push(PaletteRowModel {
                    row: PaletteRow {
                        kind: PaletteRowKind::Entry,
                        text: item.entry.name.clone(),
                        entry: Some(item.entry.clone()),
                        highlights: Vec::new(),
                        token: String::new(),
                        count: 0,
                    },
                    item: Some(item),
                    pinned: false,
                    detail,
                });
            }
        } else {
            let recents = lock(&self.extra.recents).clone();
            let state = PaletteState {
                pins: pins.clone(),
                recents,
                suggested: model.suggested.clone(),
            };
            let entries: Vec<PaletteEntry> = model.items.iter().map(|i| i.entry.clone()).collect();
            let (category, _) = PaletteRules::split_category(
                model.query.trim(),
                &PaletteRules::categories(&entries),
            );
            let by_id: std::collections::HashMap<&str, &PaletteItem> = model
                .items
                .iter()
                .map(|i| (i.entry.id.as_str(), i))
                .collect();
            for row in PaletteRules::build(&model.query, &entries, &state) {
                let item = row
                    .entry
                    .as_ref()
                    .and_then(|e| by_id.get(e.id.as_str()).map(|i| (*i).clone()));
                let pinned = row.entry.as_ref().is_some_and(|e| pins.contains(&e.id));
                let detail = match (&row.kind, &row.entry) {
                    (PaletteRowKind::Category, _) => {
                        if row.count == 1 {
                            "1 command".to_string()
                        } else {
                            format!("{} commands", bb_core::format::thousands(row.count as i64))
                        }
                    }
                    (PaletteRowKind::Entry, Some(e)) if !e.is_available() => {
                        e.unavailable.clone().unwrap_or_default()
                    }
                    (PaletteRowKind::Entry, Some(e)) if !e.detail.is_empty() => e.detail.clone(),
                    // Inside one category every row would say the same thing.
                    (PaletteRowKind::Entry, Some(e)) if category.is_none() => e.category.clone(),
                    _ => String::new(),
                };
                rows.push(PaletteRowModel {
                    row,
                    item,
                    pinned,
                    detail,
                });
            }
        }

        model.empty = !rows.iter().any(|r| !r.is_heading());
        // Enter should do something: start on the first row that can run. With something typed
        // that is the best match unless it is unavailable, in which case the best match stays
        // selected so its reason is what the hint strip shows.
        let kept = keep_selected.and_then(|id| {
            rows.iter()
                .position(|r| r.row.entry.as_ref().is_some_and(|e| e.id == id))
        });
        model.selected = kept
            .or_else(|| {
                if model.query.trim().is_empty() {
                    rows.iter()
                        .position(|r| !r.is_heading() && r.is_available())
                } else {
                    None
                }
            })
            .or_else(|| rows.iter().position(|r| !r.is_heading()));
        model.rows = rows;
        Self::palette_update_hint(model);
    }

    fn palette_blocked(&self) -> bool {
        lock(&self.extra.ui).settings_page.is_some()
    }

    fn palette_update_hint(model: &mut PaletteModel) {
        model.hint = match model.selected.and_then(|i| model.rows.get(i)) {
            None => "Nothing matches. Try another word for it, or clear the box to browse by category."
                .to_string(),
            Some(row) if row.is_category() => "Enter or Tab lists this category".to_string(),
            Some(row) if !row.is_available() => format!("Unavailable \u{2014} {}", row.detail),
            Some(row) if row.item.as_ref().is_some_and(|i| i.completion.is_some()) => {
                "Enter goes there \u{00b7} Tab carries on into it \u{00b7} Esc closes".to_string()
            }
            Some(row) if row.can_bind() => {
                "Enter runs \u{00b7} Alt+P pins \u{00b7} Alt+K changes its shortcut \u{00b7} Esc closes"
                    .to_string()
            }
            Some(row) if row.can_pin() => {
                "Enter runs \u{00b7} Alt+P pins \u{00b7} Esc closes".to_string()
            }
            Some(_) => "Enter runs \u{00b7} Esc closes".to_string(),
        };
    }

    // ---- verbs ----

    pub(crate) fn palette_open(&self, seed: &str) {
        let (items, suggested) = self.build_palette_items();
        {
            let mut model = lock(&self.extra.palette);
            model.open = true;
            model.items = items;
            model.suggested = suggested;
            model.query = seed.to_string();
            let mut taken = std::mem::take(&mut *model);
            self.palette_rebuild(&mut taken, None);
            *model = taken;
        }
        self.hub.model(Model::Palette);
    }

    pub(crate) fn palette_close(&self) {
        {
            let mut model = lock(&self.extra.palette);
            if !model.open {
                return;
            }
            model.open = false;
            model.rows.clear();
            model.items.clear();
            model.query.clear();
            model.selected = None;
        }
        self.hub.model(Model::Palette);
    }

    pub(crate) fn palette_toggle(&self) {
        if self.palette_is_open() {
            self.palette_close();
        } else {
            self.palette_open("");
        }
    }

    pub(crate) fn palette_set_query(&self, text: &str) {
        {
            let mut model = lock(&self.extra.palette);
            if !model.open {
                return;
            }
            model.query = text.to_string();
            let mut taken = std::mem::take(&mut *model);
            self.palette_rebuild(&mut taken, None);
            *model = taken;
        }
        self.hub.model(Model::Palette);
    }

    /// Moves the selection by `delta` choosable rows, stepping over headings and wrapping.
    pub(crate) fn palette_move(&self, delta: i64) {
        {
            let mut model = lock(&self.extra.palette);
            let choosable: Vec<usize> = model
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| !r.is_heading())
                .map(|(i, _)| i)
                .collect();
            if choosable.is_empty() {
                return;
            }
            let at = model
                .selected
                .and_then(|s| choosable.iter().position(|c| *c == s));
            let n = choosable.len() as i64;
            let next = match at {
                None if delta > 0 => 0,
                None => choosable.len() - 1,
                Some(at) => ((at as i64 + delta) % n + n) as usize % choosable.len(),
            };
            model.selected = Some(choosable[next]);
            let mut taken = std::mem::take(&mut *model);
            Self::palette_update_hint(&mut taken);
            *model = taken;
        }
        self.hub.model(Model::Palette);
    }

    pub(crate) fn palette_edge(&self, last: bool) {
        {
            let mut model = lock(&self.extra.palette);
            let choosable: Vec<usize> = model
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| !r.is_heading())
                .map(|(i, _)| i)
                .collect();
            let Some(pick) = (if last {
                choosable.last()
            } else {
                choosable.first()
            })
            .copied() else {
                return;
            };
            model.selected = Some(pick);
            let mut taken = std::mem::take(&mut *model);
            Self::palette_update_hint(&mut taken);
            *model = taken;
        }
        self.hub.model(Model::Palette);
    }

    pub(crate) fn palette_select(&self, row: usize) {
        {
            let mut model = lock(&self.extra.palette);
            if model.rows.get(row).is_none_or(PaletteRowModel::is_heading) {
                return;
            }
            model.selected = Some(row);
            let mut taken = std::mem::take(&mut *model);
            Self::palette_update_hint(&mut taken);
            *model = taken;
        }
        self.hub.model(Model::Palette);
    }

    /// Tab: carries on typing from the selected row, into a category or down into a folder,
    /// instead of choosing it.
    pub(crate) fn palette_complete(&self) {
        let text = {
            let model = lock(&self.extra.palette);
            model
                .selected
                .and_then(|i| model.rows.get(i))
                .and_then(|row| {
                    if row.is_category() {
                        Some(format!("{} ", row.row.token))
                    } else {
                        row.item.as_ref().and_then(|i| i.completion.clone())
                    }
                })
        };
        if let Some(text) = text {
            self.palette_set_query(&text);
        }
    }

    /// Enter: runs the selected entry, or narrows to the selected category.
    pub(crate) fn palette_activate(self: &Arc<Self>) -> AppResult<Value> {
        let picked = {
            let model = lock(&self.extra.palette);
            model.selected.and_then(|i| model.rows.get(i).cloned())
        };
        let Some(row) = picked else {
            return Ok(json!({ "ran": false }));
        };
        if row.is_category() {
            self.palette_set_query(&format!("{} ", row.row.token));
            return Ok(json!({ "ran": false, "narrowed": true }));
        }
        let Some(item) = row.item.clone() else {
            return Ok(json!({ "ran": false }));
        };
        // Left open, with the reason in the hint strip: closing on a command that then does
        // nothing would look exactly like the command being broken.
        if !row.is_available() {
            return Ok(json!({ "ran": false, "unavailable": row.detail }));
        }
        if item.remembered {
            let mut recents = lock(&self.extra.recents);
            recents.retain(|r| *r != item.entry.id);
            recents.insert(0, item.entry.id.clone());
            recents.truncate(MAX_RECENTS);
        }
        self.palette_close();
        let outcome = self.run_palette_action(&item.action)?;
        Ok(match outcome {
            Some((id, CommandOutcome::NeedsUi)) => {
                json!({ "ran": true, "ui": true, "command": id })
            }
            Some((id, CommandOutcome::Done)) => json!({ "ran": true, "command": id }),
            None => json!({ "ran": true }),
        })
    }

    fn run_palette_action(
        self: &Arc<Self>,
        action: &PaletteAction,
    ) -> AppResult<Option<(String, CommandOutcome)>> {
        let tab = lock(&self.shell).active_tab_id();
        let pane = lock(&self.shell).active_pane;
        match action {
            PaletteAction::Command(id) => {
                return self.run_command(id).map(|o| Some((id.clone(), o)));
            }
            PaletteAction::Bars(name) => {
                self.bars_apply(name)?;
            }
            PaletteAction::Workspace(name) => self.workspace_switch(name)?,
            PaletteAction::Search(name) => self.search_run_saved(name, false)?,
            PaletteAction::Bookmark(path) => {
                if let Some(tab) = tab {
                    self.navigate(tab, path)?;
                }
            }
            PaletteAction::NewFile(ext) => {
                if let Some(tab) = tab {
                    let name =
                        self.new_item_suggest(tab, bb_core::newitem::NewItemKind::File, ext)?;
                    self.op_new_item(tab, &name, bb_core::newitem::NewItemKind::File)?;
                }
            }
            PaletteAction::Tab(number) => {
                let target = {
                    let shell = lock(&self.shell);
                    let mut n = 0;
                    let mut found = None;
                    for leaf in shell.layout.leaves() {
                        let pane = *shell.layout.value(leaf);
                        if let Some(p) = shell.panes.get(&pane) {
                            for t in &p.tabs {
                                n += 1;
                                if n == *number {
                                    found = Some((pane, *t));
                                }
                            }
                        }
                    }
                    found
                };
                if let Some((pane, tab)) = target {
                    self.activate_pane(pane)?;
                    self.activate_tab(tab)?;
                }
            }
            PaletteAction::Go(path) => {
                if let Some(tab) = tab {
                    self.navigate(tab, path)?;
                }
            }
            PaletteAction::OpenInTab(path) => {
                self.open_tab(pane, path, true)?;
            }
            PaletteAction::OpenInPane(path) => {
                self.split_pane(pane, SplitOrientation::Vertical, path)?;
            }
            PaletteAction::Reveal(file) => {
                if let Some(tab) = tab {
                    self.reveal_file(tab, file)?;
                }
            }
            PaletteAction::Column(id, on) => {
                if let Some(tab) = tab {
                    self.columns_toggle(tab, id, *on)?;
                }
            }
            PaletteAction::Theme(id) => self.theme_select(id),
            PaletteAction::SettingsPage(page) => self.settings_open_page(page)?,
            PaletteAction::Nothing => {}
        }
        Ok(None)
    }

    pub(crate) fn palette_toggle_pin(&self) {
        let picked = {
            let model = lock(&self.extra.palette);
            model.selected.and_then(|i| model.rows.get(i).cloned())
        };
        let Some(row) = picked.filter(PaletteRowModel::can_pin) else {
            return;
        };
        let Some(entry) = row.row.entry else { return };
        {
            let mut settings = lock(&self.settings);
            if let Some(at) = settings.palette_pins.iter().position(|p| *p == entry.id) {
                settings.palette_pins.remove(at);
            } else {
                settings.palette_pins.push(entry.id.clone());
            }
        }
        let _ = self.save_settings_now();
        {
            let mut model = lock(&self.extra.palette);
            let mut taken = std::mem::take(&mut *model);
            self.palette_rebuild(&mut taken, Some(&entry.id));
            *model = taken;
        }
        self.hub.model(Model::Palette);
    }

    /// Alt+K: closes the palette and opens the Keyboard page on this command, listening.
    pub(crate) fn palette_change_shortcut(&self) -> AppResult<()> {
        let picked = {
            let model = lock(&self.extra.palette);
            model.selected.and_then(|i| model.rows.get(i).cloned())
        };
        let Some(row) = picked.filter(PaletteRowModel::can_bind) else {
            return Err(AppError::Invalid(
                "this entry has no shortcut to change".into(),
            ));
        };
        let id = row.row.entry.map(|e| e.id).unwrap_or_default();
        self.palette_close();
        self.key_change_shortcut(&id)
    }

    pub(crate) fn palette_json(&self) -> Value {
        let model = lock(&self.extra.palette);
        let rows: Vec<Value> = model
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let kind = match r.row.kind {
                    PaletteRowKind::Heading => "heading",
                    PaletteRowKind::Entry => "entry",
                    PaletteRowKind::Category => "category",
                };
                let entry = r.row.entry.as_ref();
                json!({
                    "index": i,
                    "kind": kind,
                    "text": r.row.text,
                    "id": entry.map(|e| e.id.clone()),
                    "icon": entry.and_then(|e| e.icon.clone()),
                    "gesture": entry.map(|e| e.gesture.clone()).unwrap_or_default(),
                    "detail": r.detail,
                    "available": r.is_available(),
                    "pinned": r.pinned,
                    "canPin": r.can_pin(),
                    "canBind": r.can_bind(),
                    "token": r.row.token,
                    "count": r.row.count,
                    "highlights": r.row.highlights.iter().map(|h| json!([h.start, h.length])).collect::<Vec<_>>(),
                    "selected": model.selected == Some(i),
                })
            })
            .collect();
        json!({
            "open": model.open,
            "query": model.query,
            "hint": model.hint,
            "empty": model.empty,
            "selected": model.selected,
            "rows": rows,
        })
    }

    pub(crate) fn palette_recents(&self) -> Vec<String> {
        lock(&self.extra.recents).clone()
    }
}

fn is_hidden_dir(path: &str) -> bool {
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, GetFileAttributesW};
    let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
    let attrs = unsafe { GetFileAttributesW(wide.as_ptr()) };
    attrs != u32::MAX && attrs & FILE_ATTRIBUTE_HIDDEN != 0
}
