//! Commands: the one gate (`why_not_now`) and the one runner. A key, a menu row, a toolbar button and
//! the palette all go through these, so the menu greying an item is a courtesy and this is the
//! guard. Every catalogue id has an owner: **Core** commands are carried out here; **Ui** commands
//! (anything that puts up a dialog, plays media, moves focus or touches the clipboard) are gated
//! here and carried out by the host, which is told so (`CommandOutcome::NeedsUi`) rather than being
//! left to wonder. Spec: Core/Services/Commands, MainWindow.Commands, MainWindow.CommandBars
//! (WhyNotNow).

pub(crate) mod bars;
pub(crate) mod keys;
pub(crate) mod palette;
mod reasons;
mod run;

use std::sync::Arc;

use bb_commands::commands::{
    CommandCatalog, CommandContext, FileVerb, FileVerbRules, FileVerbSnapshot, FileVerbState,
    FocusState, KeyChord, Keymap, KeymapRules,
};
use bb_core::archives::ArchiveFormats;
use bb_core::layout::SplitOrientation;
use bb_core::transfer::TransferVerb;
use serde_json::{Value, json};

use crate::app::{Inner, TabCell, lock};
use crate::ids::{AppError, AppResult, PaneId, TabId};

pub const SETTINGS_OPEN_REASON: &str = "Not while Settings is open.";
pub const DIALOG_OPEN_REASON: &str = "Not while a dialog is open.";
const NO_OTHER_PANE: &str = "There is no other pane \u{2014} split this one first.";
const WHILE_COMPARING: &str =
    "Stop comparing first \u{2014} the comparison is between these two panes as they stand.";
const THUMBNAIL_STEP: f64 = 0.15;

/// What a run did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandOutcome {
    /// It was carried out.
    Done,
    /// It is the host's to carry out (a dialog, a window, the clipboard, focus).
    NeedsUi,
}

/// Who carries a command out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandOwner {
    Core,
    Ui,
}

/// Ids the host carries out. Everything else in the catalogue is run here.
const UI_OWNED: &[&str] = &[
    "nav.address-bar",
    "pane.focus-tree",
    "pane.focus-list",
    "select.pattern",
    "select.unpattern",
    "file.run-as-admin",
    "file.open-terminal",
    "file.open-vscode",
    "file.edit-in-bertcut",
    "file.new-folder",
    "file.new-file",
    "file.rename",
    // A delete is only ever run from its confirmation, which shows what it comes to.
    "file.delete",
    "file.delete-permanently",
    "file.properties",
    "file.compress",
    "file.extract-here",
    "file.extract-to",
    "file.unlock-archive",
    "file.checksum",
    "file.verify-checksums",
    "file.compare-files",
    "file.settle-by-content",
    "file.create-shortcut",
    "file.copy-to",
    "file.move-to",
    "edit.cut",
    "edit.copy",
    "edit.paste",
    "edit.copy-path",
    "edit.copy-name",
    "edit.copy-folder-path",
    "edit.paste-shortcut",
    "edit.undo-history",
    "edit.clear-undo-history",
    "view.add-column",
    "search.folder",
    "search.pc",
    "search.save",
    "search.syntax",
    "tools.disk-usage",
    "tools.duplicates",
    "tools.changes",
    "tools.compare-rescan",
    "tools.compare-sync",
    "tools.compare-differences",
    "tools.transfer-cancel",
    "tools.transfer-details",
    "preview.fit-width",
    "preview.play-next",
    "preview.play",
    "preview.fullscreen",
    "app.customise-theme",
    "app.save-workspace",
    "app.save-command-bars",
    "app.about",
    "app.exit",
];

pub fn is_handled(id: &str) -> bool {
    CommandCatalog::is_known(id)
}

pub fn owner_of(id: &str) -> CommandOwner {
    if UI_OWNED.contains(&id) || id.starts_with("custom:") {
        CommandOwner::Ui
    } else {
        CommandOwner::Core
    }
}

pub(crate) fn build_keymap(overrides: Option<&bb_commands::commands::Overrides>) -> Keymap {
    KeymapRules::resolve(&KeymapRules::catalog_bindables(), overrides)
}

/// A command as a menu, a toolbar or the palette needs it.
#[derive(Clone, Debug)]
pub struct CommandState {
    pub id: String,
    pub name: String,
    pub category: String,
    pub icon: Option<String>,
    pub gesture: String,
    pub context: &'static str,
    pub owner: CommandOwner,
    /// The one gate's answer: why it cannot run now, or None.
    pub why_not: Option<String>,
    pub checked: bool,
    /// Words that depend on the moment ("Delete 3 items\u{2026}", "Stop comparing").
    pub caption: Option<String>,
    /// A file verb the right-click menu leaves out altogether rather than greying (Extract on
    /// something that is not an archive, Settle by content outside a comparison).
    pub hidden: bool,
}

impl CommandState {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "category": self.category,
            "icon": self.icon,
            "gesture": self.gesture,
            "context": self.context,
            "owner": if self.owner == CommandOwner::Core { "core" } else { "ui" },
            "whyNot": self.why_not,
            "checked": self.checked,
            "caption": self.caption,
            "hidden": self.hidden,
        })
    }
}

fn drive_root(path: &str) -> Option<String> {
    let b = path.as_bytes();
    if b.len() >= 2 && b[1] == b':' {
        return Some(format!("{}:\\", &path[..1]));
    }
    path.strip_prefix("\\\\").and_then(|rest| {
        let mut parts = rest.split('\\').filter(|p| !p.is_empty());
        let (server, share) = (parts.next()?, parts.next()?);
        Some(format!("\\\\{server}\\{share}\\"))
    })
}

/// Everything the gates need to know, read once so a pass of a hundred asks agrees with itself.
pub(crate) struct Ctx {
    has_tab: bool,
    path: String,
    in_archive: bool,
    archive_locked: bool,
    is_flattened: bool,
    is_flat_browse: bool,
    is_flat_view: bool,
    can_back: bool,
    can_forward: bool,
    row_count: usize,
    selected: usize,
    selected_files: usize,
    single_folder: Option<String>,
    single_is_archive: bool,
    single_is_video: bool,
    single_runnable: bool,
    all_bookmarked: bool,
    search_text: String,
    search_running: bool,
    thumbnail_scale: f64,
    sort_by: String,
    sort_desc: bool,
    columns_customized: bool,
    side_pane: u8,
    preview_mode: u8,
    preview_wrap: bool,
    preview_fit: bool,
    tabs_in_pane: usize,
    tab_index: usize,
    closed_in_pane: usize,
    pane_count: usize,
    other: Option<OtherPane>,
    comparing: bool,
    /// A comparison is reading its folders; it cannot be rescanned or synced until it has.
    compare_scanning: bool,
    /// The comparison's answer is one a sync may be planned from (whole, not refused or cut short).
    compare_can_sync: bool,
    differences_only: bool,
    undo: Option<String>,
    redo: Option<String>,
    undo_entries: usize,
    transferring: bool,
    paused: bool,
    settings_open: bool,
    dialogs: u32,
    show_hidden: bool,
    following_system: bool,
    banner_shown: bool,
    banner_can_start: bool,
    index_can_retry: bool,
    clipboard_has_files: bool,
    preview_has_media: bool,
    auto_advance: bool,
    loop_video: bool,
    auto_play: bool,
}

struct OtherPane {
    path: String,
    is_search_result: bool,
    in_archive: bool,
}

impl Ctx {
    pub(crate) fn in_archive_now(&self) -> bool {
        self.in_archive
    }

    pub(crate) fn is_settings_open(&self) -> bool {
        self.settings_open
    }

    pub(crate) fn selected_count(&self) -> usize {
        self.selected
    }

    /// What a command of the user's would act on: the selection (files, folders) or, with nothing
    /// selected, the folder being shown.
    pub(crate) fn target_counts(&self) -> (usize, usize) {
        if self.selected > 0 {
            (self.selected_files, self.selected - self.selected_files)
        } else if !self.path.is_empty() {
            (0, 1)
        } else {
            (0, 0)
        }
    }
}

/// What `command_ctx` reads off the shell's panes in one go.
struct PaneFacts {
    cell: Option<TabCell>,
    pane_count: usize,
    tabs_in_pane: usize,
    tab_index: usize,
    closed: usize,
    other: Option<OtherPane>,
}

impl Inner {
    pub(crate) fn command_ctx(&self) -> Ctx {
        let PaneFacts {
            cell,
            pane_count,
            tabs_in_pane,
            tab_index,
            closed,
            other,
        } = self.ctx_pane_facts();
        let (transferring, paused) = (lock(&self.queue.state).draining, self.gate.is_paused());
        let summary = lock(&self.undo_summary).clone();
        let ui = lock(&self.extra.ui);
        let (settings_open, dialogs, comparing, differences_only) = (
            ui.settings_page.is_some(),
            ui.dialogs,
            ui.compare.is_some(),
            ui.compare.as_ref().is_some_and(|c| c.differences_only),
        );
        let compare_scanning = ui.compare.as_ref().is_some_and(|c| c.comparing);
        let compare_can_sync = ui
            .compare
            .as_ref()
            .and_then(|c| c.outcome.as_ref())
            .is_some_and(|o| o.can_sync());
        let (clipboard_has_files, preview_has_media) =
            (ui.flag("clipboardHasFiles"), ui.flag("previewHasMedia"));
        drop(ui);
        let (show_hidden, auto_advance, loop_video, auto_play) = {
            let s = lock(&self.settings);
            (
                s.show_hidden_items,
                s.preview_auto_advance,
                s.preview_loop,
                s.preview_auto_play,
            )
        };
        let following_system = self.theme_is_following();
        let banner = self.index_banner();
        let index_can_retry = self.seams.index.can_retry();

        let mut ctx = Ctx {
            has_tab: cell.is_some(),
            path: String::new(),
            in_archive: false,
            archive_locked: false,
            is_flattened: false,
            is_flat_browse: false,
            is_flat_view: false,
            can_back: false,
            can_forward: false,
            row_count: 0,
            selected: 0,
            selected_files: 0,
            single_folder: None,
            single_is_archive: false,
            single_is_video: false,
            single_runnable: false,
            all_bookmarked: false,
            search_text: String::new(),
            search_running: false,
            thumbnail_scale: 0.0,
            sort_by: String::new(),
            sort_desc: false,
            columns_customized: false,
            side_pane: 0,
            preview_mode: 0,
            preview_wrap: false,
            preview_fit: false,
            tabs_in_pane,
            tab_index,
            closed_in_pane: closed,
            pane_count,
            other,
            comparing,
            compare_scanning,
            compare_can_sync,
            differences_only,
            // "Ctrl+Z: undo move 3 items to Documents": the caption a menu row and a toolbar button show.
            undo: summary
                .undo
                .map(|d| self.undo_caption("edit.undo", "undo", &d)),
            redo: summary
                .redo
                .map(|d| self.undo_caption("edit.redo", "redo", &d)),
            undo_entries: summary.entries,
            transferring,
            paused,
            settings_open,
            dialogs,
            show_hidden,
            following_system,
            banner_shown: banner.show,
            banner_can_start: banner.can_start,
            index_can_retry,
            clipboard_has_files,
            preview_has_media,
            auto_advance,
            loop_video,
            auto_play,
        };
        let Some(cell) = cell else { return ctx };
        self.fill_ctx_from_tab(&mut ctx, &cell);
        ctx
    }

    /// Where the active pane stands: its tab, how many tabs and panes there are, and what the other
    /// pane is showing.
    fn ctx_pane_facts(&self) -> PaneFacts {
        let (cell, pane_id, pane_count, tabs_in_pane, tab_index, closed, other_tab) = {
            let shell = lock(&self.shell);
            let pane_id = shell.active_pane;
            let pane = shell.panes.get(&pane_id);
            let active = pane.and_then(|p| p.active);
            let cell = active.and_then(|t| shell.tabs.get(&t).cloned());
            let index = pane
                .and_then(|p| active.and_then(|a| p.tabs.iter().position(|t| *t == a)))
                .unwrap_or(0);
            let other_tab = if shell.layout.leaves().len() > 1 {
                shell.leaf(pane_id).and_then(|n| {
                    let next = shell.layout.next_leaf(n, 1);
                    let other = *shell.layout.value(next);
                    shell
                        .panes
                        .get(&other)
                        .and_then(|p| p.active)
                        .and_then(|t| shell.tabs.get(&t).cloned())
                })
            } else {
                None
            };
            (
                cell,
                pane_id,
                shell.layout.leaves().len(),
                pane.map(|p| p.tabs.len()).unwrap_or(0),
                index,
                pane.map(|p| p.closed_paths.len()).unwrap_or(0),
                other_tab,
            )
        };
        let _ = pane_id;
        let other = other_tab.map(|c| {
            let t = lock(&c);
            OtherPane {
                path: t.path.clone(),
                is_search_result: t.is_flattened && !t.is_flat_browse,
                in_archive: ArchivePathCheck::in_archive(&t.path),
            }
        });
        PaneFacts {
            cell,
            pane_count,
            tabs_in_pane,
            tab_index,
            closed,
            other,
        }
    }

    /// The parts of the context that live on the tab itself: its place, listing, selection and view.
    fn fill_ctx_from_tab(&self, ctx: &mut Ctx, cell: &TabCell) {
        let selected_paths: Vec<String>;
        {
            let t = lock(cell);
            ctx.path = t.path.clone();
            ctx.in_archive = ArchivePathCheck::in_archive(&t.path);
            ctx.archive_locked = t.archive_locked;
            ctx.is_flattened = t.is_flattened;
            ctx.is_flat_browse = t.is_flat_browse;
            ctx.is_flat_view = t.is_flat_view;
            ctx.can_back = !t.back.is_empty();
            ctx.can_forward = !t.forward.is_empty();
            ctx.row_count = t.listing.len();
            ctx.search_text = t.active_search_text().to_string();
            ctx.search_running = t.is_search_running;
            ctx.thumbnail_scale = t.thumbnail_scale;
            ctx.sort_by = t.sort_by.clone();
            ctx.sort_desc = t.sort_desc;
            ctx.columns_customized = t.columns_customized;
            ctx.side_pane = t.side_pane;
            ctx.preview_mode = t.preview_mode;
            ctx.preview_wrap = t.preview_wrap;
            ctx.preview_fit = t.preview_fit;
            let mut paths = Vec::new();
            for row in t.listing.rows() {
                if t.selection.contains(&row.id) {
                    ctx.selected += 1;
                    if !row.is_directory {
                        ctx.selected_files += 1;
                    }
                    paths.push(row.full_path().into_owned());
                    if ctx.selected == 1 {
                        if row.is_directory {
                            ctx.single_folder = Some(row.full_path().into_owned());
                        } else {
                            ctx.single_is_archive = ArchiveFormats::is_archive_name(&row.name);
                            ctx.single_is_video =
                                bb_core::preview::PreviewClassifier::is_video(&row.name);
                            let lower = row.name.to_lowercase();
                            // The launcher knows the registry; without one the well-known list.
                            ctx.single_runnable = self
                                .seams
                                .launcher
                                .can_run_elevated(&row.full_path())
                                .unwrap_or_else(|| {
                                    [".exe", ".bat", ".cmd", ".msc"]
                                        .iter()
                                        .any(|e| lower.ends_with(e))
                                });
                        }
                    }
                }
            }
            if ctx.selected != 1 {
                ctx.single_folder = None;
                ctx.single_is_archive = false;
                ctx.single_is_video = false;
                ctx.single_runnable = false;
            }
            selected_paths = paths;
        }
        ctx.all_bookmarked = !selected_paths.is_empty()
            && selected_paths
                .iter()
                .all(|p| self.is_bookmarked(p).unwrap_or(false));
    }

    pub(crate) fn command_state_in(&self, ctx: &Ctx, id: &str) -> Option<CommandState> {
        let (name, category, icon, context) = match CommandCatalog::find(id) {
            Some(d) => (
                d.name.to_string(),
                d.category.to_string(),
                d.icon.map(str::to_string),
                match d.context {
                    CommandContext::App => "app",
                    CommandContext::Browser => "browser",
                    CommandContext::FileList => "file-list",
                },
            ),
            None => {
                let custom = self.custom_command(id)?;
                (
                    custom.name.clone(),
                    "Custom commands".to_string(),
                    Some(
                        if custom.run_elevated {
                            "Icon.Shield"
                        } else {
                            "Icon.CustomCommand"
                        }
                        .to_string(),
                    ),
                    "browser",
                )
            }
        };
        let gesture = lock(&self.keymap).gesture_text(id);
        let registry = self.registry_why_not(ctx, id);
        let why_not = self.gate(ctx, id, registry);
        Some(CommandState {
            id: id.to_string(),
            name,
            category,
            icon,
            gesture,
            context,
            owner: owner_of(id),
            why_not,
            checked: checked_in(ctx, id),
            caption: caption_in(ctx, id),
            hidden: file_verb_of(id).is_some_and(|v| !self.verb_state(ctx, v).visible),
        })
    }

    /// The registry's answer plus what only the window knows: a dialog is up, or Settings stands
    /// where the folders were.
    fn gate(&self, ctx: &Ctx, id: &str, registry: Option<String>) -> Option<String> {
        if ctx.dialogs > 0 {
            return Some(DIALOG_OPEN_REASON.to_string());
        }
        let context = CommandCatalog::find(id)
            .map(|d| d.context)
            .unwrap_or(CommandContext::Browser);
        if ctx.settings_open && context != CommandContext::App {
            return Some(SETTINGS_OPEN_REASON.to_string());
        }
        registry
    }

    /// The one gate: why the command cannot run now, or None. `Err` for an id nobody knows.
    pub(crate) fn why_not_now(&self, id: &str) -> AppResult<Option<String>> {
        if !CommandCatalog::is_known(id) && self.custom_command(id).is_none() {
            return Err(AppError::Invalid(format!("unknown command '{id}'")));
        }
        let ctx = self.command_ctx();
        let registry = self.registry_why_not(&ctx, id);
        Ok(self.gate(&ctx, id, registry))
    }

    /// Every command (catalogue order, then the user's own) in the state a menu would draw it.
    pub(crate) fn command_list(&self) -> Vec<CommandState> {
        let ctx = self.command_ctx();
        let mut ids: Vec<String> = CommandCatalog::all()
            .iter()
            .map(|c| c.id.to_string())
            .collect();
        ids.extend(self.custom_command_ids());
        ids.iter()
            .filter_map(|id| self.command_state_in(&ctx, id))
            .collect()
    }

    pub(crate) fn command_state(&self, id: &str) -> Option<CommandState> {
        let ctx = self.command_ctx();
        self.command_state_in(&ctx, id)
    }

    fn verb_snapshot<'a>(&self, ctx: &Ctx) -> FileVerbSnapshot<'a> {
        let mut s = FileVerbSnapshot::new(
            ctx.selected,
            ctx.selected_files,
            ctx.in_archive,
            ctx.is_flattened && !ctx.is_flat_browse,
            !ctx.path.is_empty(),
        );
        s.comparing = ctx.comparing;
        s.single_is_archive = ctx.single_is_archive;
        s.single_is_video = ctx.single_is_video;
        s.archive_locked = ctx.archive_locked;
        s.all_bookmarked = ctx.all_bookmarked;
        s
    }

    fn verb_state(&self, ctx: &Ctx, verb: FileVerb) -> FileVerbState {
        let clipboard = ctx.clipboard_has_files;
        let runnable = ctx.single_runnable;
        let has_files = move || clipboard;
        let can_run = move || runnable;
        let mut snapshot = self.verb_snapshot(ctx);
        snapshot.clipboard_has_files = Some(&has_files);
        snapshot.can_run_elevated = Some(&can_run);
        FileVerbRules::for_verb(verb, &snapshot)
    }

    fn verb_reason(&self, ctx: &Ctx, verb: FileVerb) -> Option<String> {
        let state = self.verb_state(ctx, verb);
        if state.enabled {
            None
        } else {
            Some(state.reason.unwrap_or_else(|| "Not available.".to_string()))
        }
    }

    fn send_to_other_reason(&self, ctx: &Ctx, verb: TransferVerb) -> Option<String> {
        if let Some(source) = self.verb_reason(
            ctx,
            if verb == TransferVerb::Move {
                FileVerb::Cut
            } else {
                FileVerb::Copy
            },
        ) {
            return Some(source);
        }
        let Some(other) = &ctx.other else {
            return Some(NO_OTHER_PANE.to_string());
        };
        if other.path.is_empty() || other.is_search_result {
            return Some("The other pane is not showing a folder.".to_string());
        }
        other
            .in_archive
            .then(|| "The other pane is inside an archive.".to_string())
    }

    // ---- running ----

    pub(crate) fn run_command(self: &Arc<Self>, id: &str) -> AppResult<CommandOutcome> {
        if let Some(reason) = self.why_not_now(id)? {
            return Err(AppError::Invalid(reason));
        }
        if owner_of(id) == CommandOwner::Ui {
            return Ok(CommandOutcome::NeedsUi);
        }
        let (tab, pane) = {
            let shell = lock(&self.shell);
            let tab = shell.active_tab_id().ok_or(AppError::Stale)?;
            (tab, shell.active_pane)
        };
        self.run_core(id, tab, pane)?;
        Ok(CommandOutcome::Done)
    }

    fn set_thumbs(&self, tab: TabId, scale: f64) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let current = lock(&cell).thumbnail_scale;
        if current > 0.0 {
            *lock(&self.extra.last_thumbnail_scale) = current;
        }
        if scale > 0.0 {
            *lock(&self.extra.last_thumbnail_scale) = scale;
        }
        self.set_thumbnail_scale(tab, scale)
    }

    /// Sorts ascending by a column whatever was in force, unlike a header click, which reverses on
    /// a repeat: "Sort by size" that sometimes sorted the other way would be unpredictable.
    pub(crate) fn sort_ascending(&self, tab: TabId, column: &str) -> AppResult<()> {
        self.set_sort(tab, column)?;
        let cell = self.tab(tab)?;
        let desc = lock(&cell).sort_desc;
        if desc {
            self.set_sort(tab, column)?;
        }
        Ok(())
    }

    fn open_in_other_pane(self: &Arc<Self>, pane: PaneId, path: &str) -> AppResult<()> {
        match self.other_pane(pane) {
            // The other pane's own tab goes there; focus stays here, where the work is.
            Some(other) => {
                let tab = lock(&self.shell).panes.get(&other).and_then(|p| p.active);
                match tab {
                    Some(tab) => self.navigate(tab, path),
                    None => self.open_tab(other, path, true).map(|_| ()),
                }
            }
            None => {
                self.split_pane(pane, SplitOrientation::Vertical, path)?;
                // The new pane takes the focus; the user asked for the other one, so hand it back.
                let _ = self.activate_pane(pane);
                Ok(())
            }
        }
    }

    fn goto_known_folder(self: &Arc<Self>, tab: TabId, path: Option<String>) -> AppResult<()> {
        match path.filter(|p| !p.is_empty()) {
            Some(path) => self.navigate(tab, &path),
            None => Ok(()),
        }
    }

    /// What Undo and Redo say they will do. Spec: ShellViewModel.Described.
    fn undo_caption(&self, command: &str, verb: &str, description: &str) -> String {
        let what = format!("{verb} {}", bb_undo::UndoText::as_object(description));
        match self.keymap_gesture(command) {
            g if g.is_empty() => {
                let mut c = what.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            }
            g => format!("{g}: {what}"),
        }
    }

    fn goto_tab(&self, pane: PaneId, slot: usize) -> AppResult<()> {
        let target = lock(&self.shell).panes.get(&pane).map(|p| {
            // A slot past the end lands on the last tab.
            p.tabs
                .get(slot.min(p.tabs.len().saturating_sub(1)))
                .copied()
        });
        match target.flatten() {
            Some(t) => self.activate_tab(t),
            None => Ok(()),
        }
    }

    fn selected_folders(&self, tab: TabId, cap: usize) -> AppResult<Vec<String>> {
        let cell = self.tab(tab)?;
        let t = lock(&cell);
        Ok(t.listing
            .rows()
            .iter()
            .filter(|r| r.is_directory && t.selection.contains(&r.id))
            .take(cap)
            .map(|r| r.full_path().into_owned())
            .collect())
    }

    pub(crate) fn panes_active_path(&self, pane: PaneId) -> Option<String> {
        let shell = lock(&self.shell);
        let tab = shell.panes.get(&pane)?.active?;
        let cell = shell.tabs.get(&tab)?;
        let path = lock(cell).path.clone();
        (!path.is_empty()).then_some(path)
    }

    /// Leaves a search result or a flat view for the folder the selected row is really in, with
    /// that row selected: "show me where this is".
    fn containing_folder(self: &Arc<Self>, tab: TabId) -> AppResult<()> {
        let selected = self
            .selected_paths(tab)?
            .into_iter()
            .next()
            .ok_or(AppError::Stale)?;
        // Both the search and the flat view have to go, or the folder would arrive already filtered
        // or flattened and the row would be shown among the same neighbours it had a moment ago.
        self.reveal_file(tab, &selected)
    }

    /// Runs a key press the way the window's one key handler does: asks the keymap whose it is
    /// given what has focus, closes the palette for anything but its own chord, and runs the
    /// command through the gate. A claimed chord is the command's even when it cannot run.
    pub(crate) fn key_press(
        self: &Arc<Self>,
        chord: &KeyChord,
        mut state: FocusState,
        repeat: bool,
    ) -> Value {
        {
            let ui = lock(&self.extra.ui);
            state.settings_open |= ui.settings_page.is_some();
            state.content_blocked |= ui.dialogs > 0;
        }
        state.palette_open |= self.palette_is_open();
        state.recording |= lock(&self.extra.keys).is_capturing();
        let Some(command) = self.dispatch_key(chord, &state) else {
            return json!({ "claimed": false });
        };
        if repeat && !KeymapRules::repeats_when_held(&command) {
            return json!({ "claimed": true, "command": command, "result": "ignored" });
        }
        if self.palette_is_open() && command != "app.palette" {
            self.palette_close();
        }
        match self.run_command(&command) {
            Ok(CommandOutcome::Done) => {
                json!({ "claimed": true, "command": command, "result": "ran" })
            }
            Ok(CommandOutcome::NeedsUi) => {
                json!({ "claimed": true, "command": command, "result": "ui" })
            }
            Err(e) => json!({
                "claimed": true, "command": command, "result": "unavailable", "reason": e.to_string()
            }),
        }
    }

    pub(crate) fn dispatch_key(&self, chord: &KeyChord, state: &FocusState) -> Option<String> {
        let keymap: Arc<Keymap> = lock(&self.keymap).clone();
        KeymapRules::dispatch(&keymap, chord, state)
    }

    pub(crate) fn rebuild_keymap(&self) {
        *lock(&self.keymap) = Arc::new(self.resolve_keymap());
        self.hub.model(crate::events::Model::Keymap);
    }
}

/// `checked` for a command that is a switch or one of a set.
fn checked_in(ctx: &Ctx, id: &str) -> bool {
    match id {
        "view.preview" => ctx.side_pane == 1,
        "view.metadata" => ctx.side_pane == 2,
        "view.flat" => ctx.is_flat_view,
        "view.hidden" => ctx.show_hidden,
        "view.details" => ctx.thumbnail_scale <= 0.0,
        "view.thumbnails" => ctx.thumbnail_scale > 0.0,
        "view.sort-name" => ctx.sort_by.eq_ignore_ascii_case("Name"),
        "view.sort-size" => ctx.sort_by.eq_ignore_ascii_case("Size"),
        "view.sort-type" => ctx.sort_by.eq_ignore_ascii_case("Type"),
        "view.sort-modified" => ctx.sort_by.eq_ignore_ascii_case("Modified"),
        "view.sort-created" => ctx.sort_by.eq_ignore_ascii_case("Created"),
        "view.sort-extension" => ctx.sort_by.eq_ignore_ascii_case("Extension"),
        "view.sort-reverse" => ctx.sort_desc,
        "tools.compare" => ctx.comparing,
        "tools.compare-differences" => ctx.differences_only,
        "app.match-windows-theme" => ctx.following_system,
        "preview.mode-auto" => ctx.side_pane == 1 && ctx.preview_mode == 0,
        "preview.mode-raw" => ctx.side_pane == 1 && ctx.preview_mode == 1,
        "preview.mode-hex" => ctx.side_pane == 1 && ctx.preview_mode == 2,
        "preview.wrap" => ctx.side_pane == 1 && ctx.preview_wrap,
        "preview.fit" => ctx.side_pane == 1 && !ctx.preview_fit,
        "preview.auto-advance" => ctx.side_pane == 1 && ctx.auto_advance,
        "preview.loop" => ctx.side_pane == 1 && ctx.loop_video,
        "preview.autoplay" => ctx.side_pane == 1 && ctx.auto_play,
        _ => false,
    }
}

/// Words that depend on the moment.
/// The file verb a command runs, for the commands whose caption and menu presence are the verb rule's.
fn file_verb_of(id: &str) -> Option<FileVerb> {
    Some(match id {
        "file.open" => FileVerb::Open,
        "file.run-as-admin" => FileVerb::RunAsAdmin,
        "file.open-new-tab" => FileVerb::OpenInNewTab,
        "file.open-terminal" => FileVerb::OpenInTerminal,
        "file.open-vscode" => FileVerb::OpenInVSCode,
        "file.edit-in-bertcut" => FileVerb::EditInBertCut,
        "file.rename" => FileVerb::Rename,
        "file.delete" => FileVerb::Delete,
        "file.properties" => FileVerb::Properties,
        "file.compress" => FileVerb::Compress,
        "file.extract-here" => FileVerb::ExtractHere,
        "file.extract-to" => FileVerb::ExtractTo,
        "file.unlock-archive" => FileVerb::UnlockArchive,
        "file.checksum" => FileVerb::Checksum,
        "file.verify-checksums" => FileVerb::VerifyChecksums,
        "file.compare-files" => FileVerb::CompareFiles,
        "file.settle-by-content" => FileVerb::SettleByContent,
        "file.bookmark" => FileVerb::Bookmark,
        "edit.cut" => FileVerb::Cut,
        "edit.copy" => FileVerb::Copy,
        "edit.paste" => FileVerb::Paste,
        "edit.copy-path" => FileVerb::CopyPath,
        "edit.copy-name" => FileVerb::CopyName,
        _ => return None,
    })
}

fn caption_in(ctx: &Ctx, id: &str) -> Option<String> {
    let snapshot = FileVerbSnapshot {
        comparing: ctx.comparing,
        all_bookmarked: ctx.all_bookmarked,
        single_is_archive: ctx.single_is_archive,
        single_is_video: ctx.single_is_video,
        ..FileVerbSnapshot::new(
            ctx.selected,
            ctx.selected_files,
            ctx.in_archive,
            ctx.is_flattened && !ctx.is_flat_browse,
            !ctx.path.is_empty(),
        )
    };
    // Every file verb says what it would do now, in the words of the rule the right-click menu
    // shows ("Delete 3 items..."). Spec: MainWindow.Commands Verb().
    if let Some(v) = file_verb_of(id) {
        return Some(FileVerbRules::for_verb(v, &snapshot).header);
    }
    match id {
        "edit.undo" => ctx.undo.clone(),
        "edit.redo" => ctx.redo.clone(),
        "tools.compare" => ctx.comparing.then(|| "Stop comparing".to_string()),
        _ => None,
    }
}

/// Whether a path names somewhere inside an archive.
pub(crate) struct ArchivePathCheck;

impl ArchivePathCheck {
    pub(crate) fn in_archive(path: &str) -> bool {
        !path.is_empty()
            && bb_core::archives::ArchivePath::looks_virtual(path)
            && bb_core::archives::ArchivePath::parse(path, |p| std::path::Path::new(p).is_file())
                .is_some()
    }
}
