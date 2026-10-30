//! Commands: the one gate (`why_not_now`) and the one runner. A key, a menu row, a toolbar button and
//! the palette all go through these, so the menu greying an item is a courtesy and this is the
//! guard. Every catalogue id has an owner: **Core** commands are carried out here; **Ui** commands
//! (anything that puts up a dialog, plays media, moves focus or touches the clipboard) are gated
//! here and carried out by the host, which is told so (`CommandOutcome::NeedsUi`) rather than being
//! left to wonder. Spec: Core/Services/Commands, MainWindow.Commands, MainWindow.CommandBars
//! (WhyNotNow).

use std::sync::Arc;

use bb_core::archives::ArchiveFormats;
use bb_core::commands::{
    CommandCatalog, CommandContext, FileVerb, FileVerbRules, FileVerbSnapshot, FileVerbState,
    FocusState, KeyChord, Keymap, KeymapRules,
};
use bb_core::layout::SplitOrientation;
use bb_core::transfer::TransferVerb;
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::ids::{AppError, AppResult, PaneId, TabId};
use crate::tabs::{SelectOp, parent_path};

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

pub(crate) fn build_keymap(overrides: Option<&bb_core::commands::Overrides>) -> Keymap {
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

impl Inner {
    pub(crate) fn command_ctx(&self) -> Ctx {
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
        let selected_paths: Vec<String>;
        {
            let t = lock(&cell);
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
                    paths.push(row.full_path.clone());
                    if ctx.selected == 1 {
                        if row.is_directory {
                            ctx.single_folder = Some(row.full_path.clone());
                        } else {
                            ctx.single_is_archive = ArchiveFormats::is_archive_name(&row.name);
                            let lower = row.name.to_lowercase();
                            // The launcher knows the registry; without one the well-known list.
                            ctx.single_runnable = self
                                .seams
                                .launcher
                                .can_run_elevated(&row.full_path)
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
                ctx.single_runnable = false;
            }
            selected_paths = paths;
        }
        ctx.all_bookmarked = !selected_paths.is_empty()
            && selected_paths
                .iter()
                .all(|p| self.is_bookmarked(p).unwrap_or(false));
        ctx
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

    /// The registry's answer for the command itself.
    pub(crate) fn registry_why_not(&self, ctx: &Ctx, id: &str) -> Option<String> {
        if !ctx.has_tab {
            return Some("There is no tab.".to_string());
        }
        let s = |t: &str| Some(t.to_string());
        let no_rows =
            || (ctx.row_count == 0).then(|| "There is nothing here to select.".to_string());
        let no_selection = || (ctx.selected == 0).then(|| "Nothing is selected.".to_string());
        let multi_tab =
            || (ctx.tabs_in_pane < 2).then(|| "This is the only tab in the pane.".to_string());
        let no_other = || ctx.other.is_none().then(|| NO_OTHER_PANE.to_string());
        let while_comparing = || ctx.comparing.then(|| WHILE_COMPARING.to_string());
        let no_preview = || (ctx.side_pane != 1).then(|| "The preview pane is closed.".to_string());
        let only_pane = || (ctx.pane_count < 2).then(|| "This is the only pane.".to_string());
        let comparing_only =
            || (!ctx.comparing).then(|| "Only while two panes are being compared.".to_string());
        let transferring =
            || (!ctx.transferring).then(|| "Nothing is being transferred.".to_string());
        if let Some(rest) = id.strip_prefix("custom:") {
            let _ = rest;
            return self.custom_command_reason(ctx, id);
        }
        match id {
            "nav.back" => (!ctx.can_back).then(|| "There is nowhere to go back to.".to_string()),
            "nav.forward" => {
                (!ctx.can_forward).then(|| "There is nowhere to go forward to.".to_string())
            }
            "nav.up" => (ctx.path.is_empty() || parent_path(&ctx.path).is_none())
                .then(|| "This is already the top.".to_string()),
            "nav.drive-root" => match drive_root(&ctx.path) {
                Some(root)
                    if !root
                        .trim_end_matches('\\')
                        .eq_ignore_ascii_case(ctx.path.trim_end_matches('\\')) =>
                {
                    None
                }
                _ => s("This is already the top of the drive."),
            },
            "nav.containing-folder" => {
                if !ctx.is_flattened {
                    s(
                        "Only in search results or a flat view \u{2014} elsewhere the folder is the one showing.",
                    )
                } else if ctx.in_archive {
                    s("Not available inside an archive.")
                } else if ctx.selected == 1 {
                    None
                } else {
                    s("Select one item.")
                }
            }
            "tab.close-others" | "tab.next" | "tab.previous" => multi_tab(),
            "tab.reopen" => (ctx.closed_in_pane == 0)
                .then(|| "No tab has been closed in this pane.".to_string()),
            "tab.move-to-new-pane" => (ctx.tabs_in_pane < 2).then(|| {
                "This is the pane's only tab \u{2014} it already has a pane to itself.".to_string()
            }),
            "tab.close-right" => (ctx.tab_index + 1 >= ctx.tabs_in_pane)
                .then(|| "There are no tabs to the right.".to_string()),
            "tab.move-left" => {
                (ctx.tab_index == 0).then(|| "This is already the first tab.".to_string())
            }
            "tab.move-right" => (ctx.tab_index + 1 >= ctx.tabs_in_pane)
                .then(|| "This is already the last tab.".to_string()),
            "tab.move-to-other-pane" => no_other().or_else(while_comparing),
            "pane.close" | "pane.next" | "pane.previous" => only_pane(),
            "pane.swap" => no_other().or_else(while_comparing),
            "pane.equalise" => no_other(),
            "pane.open-here-in-other" => ctx
                .path
                .is_empty()
                .then(|| "There is no folder here to open.".to_string()),
            "pane.open-selected-in-other" => ctx
                .single_folder
                .is_none()
                .then(|| "Select one folder.".to_string()),
            "pane.copy-to-other" => self.send_to_other_reason(ctx, TransferVerb::Copy),
            "pane.move-to-other" => self.send_to_other_reason(ctx, TransferVerb::Move),
            "select.all" | "select.invert" | "select.pattern" | "select.files"
            | "select.folders" => no_rows(),
            "select.none" | "select.unpattern" => no_selection(),
            "file.open" => self.verb_reason(ctx, FileVerb::Open),
            "file.run-as-admin" => self.verb_reason(ctx, FileVerb::RunAsAdmin),
            "file.open-new-tab" => self.verb_reason(ctx, FileVerb::OpenInNewTab),
            "file.open-pane-right" | "file.open-pane-below" => {
                self.verb_reason(ctx, FileVerb::OpenInNewPane)
            }
            "file.open-terminal" => self.verb_reason(ctx, FileVerb::OpenInTerminal),
            "file.open-vscode" => self.verb_reason(ctx, FileVerb::OpenInVSCode),
            "file.new-folder" | "file.new-file" => self.verb_reason(ctx, FileVerb::New),
            "file.rename" => self.verb_reason(ctx, FileVerb::Rename),
            "file.delete" => self.verb_reason(ctx, FileVerb::Delete),
            "file.delete-permanently" => self.verb_reason(
                ctx,
                if ctx.in_archive {
                    FileVerb::Delete
                } else {
                    FileVerb::DeletePermanently
                },
            ),
            "file.properties" => self.verb_reason(ctx, FileVerb::Properties),
            "file.compress" => self.verb_reason(ctx, FileVerb::Compress),
            "file.extract-here" => self.verb_reason(ctx, FileVerb::ExtractHere),
            "file.extract-to" => self.verb_reason(ctx, FileVerb::ExtractTo),
            "file.unlock-archive" => self.verb_reason(ctx, FileVerb::UnlockArchive),
            "file.bookmark" => self.verb_reason(ctx, FileVerb::Bookmark),
            "file.checksum" => self.verb_reason(ctx, FileVerb::Checksum),
            "file.verify-checksums" => self.verb_reason(ctx, FileVerb::VerifyChecksums),
            "file.compare-files" => self.verb_reason(ctx, FileVerb::CompareFiles),
            "file.settle-by-content" => self.verb_reason(ctx, FileVerb::SettleByContent),
            "file.create-shortcut" => self
                .verb_reason(ctx, FileVerb::Copy)
                .or_else(|| self.verb_reason(ctx, FileVerb::New)),
            "file.copy-to" => self.verb_reason(ctx, FileVerb::Copy),
            "file.move-to" => self.verb_reason(ctx, FileVerb::Cut),
            "edit.cut" => self.verb_reason(ctx, FileVerb::Cut),
            "edit.copy" => self.verb_reason(ctx, FileVerb::Copy),
            "edit.paste" => self.verb_reason(ctx, FileVerb::Paste),
            "edit.copy-path" => self.verb_reason(ctx, FileVerb::CopyPath),
            "edit.copy-name" => self.verb_reason(ctx, FileVerb::CopyName),
            "edit.copy-folder-path" => ctx
                .path
                .is_empty()
                .then(|| "There is no folder here.".to_string()),
            "edit.paste-shortcut" => self
                .verb_reason(ctx, FileVerb::Paste)
                .or_else(|| self.verb_reason(ctx, FileVerb::New)),
            "edit.undo" => {
                if ctx.transferring {
                    s("An operation is running.")
                } else if ctx.undo.is_none() {
                    s("There is nothing to undo.")
                } else {
                    None
                }
            }
            "edit.redo" => {
                if ctx.transferring {
                    s("An operation is running.")
                } else if ctx.redo.is_none() {
                    s("There is nothing to redo.")
                } else {
                    None
                }
            }
            "edit.clear-undo-history" => {
                (ctx.undo_entries == 0).then(|| "The undo history is already empty.".to_string())
            }
            "view.details" => (ctx.thumbnail_scale <= 0.0)
                .then(|| "This is already the details view.".to_string()),
            "view.thumbnails" => (ctx.thumbnail_scale > 0.0)
                .then(|| "This is already the thumbnail view.".to_string()),
            "view.thumbs-larger" => (ctx.thumbnail_scale >= 1.0)
                .then(|| "The thumbnails are as large as they go.".to_string()),
            "view.thumbs-smaller" => (ctx.thumbnail_scale <= 0.0).then(|| {
                "This is the details view \u{2014} there are no thumbnails to shrink.".to_string()
            }),
            "view.reset-columns" => (!ctx.columns_customized)
                .then(|| "These are already the default columns.".to_string()),
            "search.clear" => ctx
                .search_text
                .is_empty()
                .then(|| "No search is showing.".to_string()),
            "search.stop" => (!ctx.search_running).then(|| "No search is running.".to_string()),
            "search.save" => ctx
                .search_text
                .trim()
                .is_empty()
                .then(|| "There is no search to save.".to_string()),
            "tools.compare-differences" => comparing_only(),
            // Not while a scan is reading: a second one would race it into the same result.
            "tools.compare-rescan" => (!ctx.comparing || ctx.compare_scanning)
                .then(|| "Only while two panes are being compared.".to_string()),
            // A sync is planned from the verdicts, so only from a whole answer that has arrived.
            "tools.compare-sync" => {
                (!ctx.comparing || ctx.compare_scanning || !ctx.compare_can_sync)
                    .then(|| "Only once a comparison has found something to sync.".to_string())
            }
            "tools.indexer-dismiss" => {
                (!ctx.banner_shown).then(|| "The banner is not showing.".to_string())
            }
            "tools.indexer-start" => (!ctx.banner_can_start).then(|| {
                "The search index is already running, or cannot be started here.".to_string()
            }),
            "tools.indexer-retry" => {
                (!ctx.index_can_retry).then(|| "Indexing has not failed.".to_string())
            }
            "tools.transfer-cancel" | "tools.transfer-details" => transferring(),
            "tools.transfer-pause" => {
                if !ctx.transferring {
                    s("Nothing is being transferred.")
                } else if ctx.paused {
                    s("The transfer is already paused.")
                } else {
                    None
                }
            }
            "tools.transfer-resume" => (!ctx.paused).then(|| "No transfer is paused.".to_string()),
            "preview.mode-auto"
            | "preview.mode-raw"
            | "preview.mode-hex"
            | "preview.wrap"
            | "preview.fit"
            | "preview.fit-width"
            | "preview.play-next"
            | "preview.auto-advance"
            | "preview.loop"
            | "preview.autoplay" => no_preview(),
            "preview.play" | "preview.fullscreen" => no_preview().or_else(|| {
                (!ctx.preview_has_media).then(|| "Nothing playable is being previewed.".to_string())
            }),
            "app.bookmark-folder" => {
                if ctx.path.is_empty() {
                    s("There is no folder here to bookmark.")
                } else if ctx.in_archive {
                    s("Not available inside an archive.")
                } else {
                    None
                }
            }
            _ => None,
        }
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

    fn run_core(self: &Arc<Self>, id: &str, tab: TabId, pane: PaneId) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let path = lock(&cell).path.clone();
        // Where Windows says they are today: OneDrive's folder backup and a relocated Downloads
        // move them away from the profile.
        use bb_core::fs::KnownFolders;
        let profile = KnownFolders::profile().or_else(|| std::env::var("USERPROFILE").ok());
        match id {
            "nav.back" => self.back(tab),
            "nav.forward" => self.forward(tab),
            "nav.up" => self.up(tab),
            "nav.refresh" => self.refresh(tab),
            "nav.refresh-all" => {
                self.refresh_all();
                Ok(())
            }
            "nav.goto" => {
                let seed = if path.is_empty() || ArchivePathCheck::in_archive(&path) {
                    String::new()
                } else if path.ends_with('\\') {
                    path
                } else {
                    format!("{path}\\")
                };
                self.palette_open(&seed);
                Ok(())
            }
            "nav.drive-root" => self.goto_known_folder(tab, drive_root(&path)),
            "nav.home" => self.goto_known_folder(tab, profile.clone()),
            "nav.desktop" => self.goto_known_folder(tab, KnownFolders::desktop()),
            "nav.documents" => self.goto_known_folder(tab, KnownFolders::documents()),
            "nav.downloads" => self.goto_known_folder(tab, KnownFolders::downloads()),
            "nav.containing-folder" => self.containing_folder(tab),
            "tab.new" | "tab.duplicate" => self.open_tab(pane, &path, true).map(|_| ()),
            "tab.close" => {
                let last = {
                    let shell = lock(&self.shell);
                    shell.layout.leaves().len() == 1
                        && shell.panes.get(&pane).is_some_and(|p| p.tabs.len() == 1)
                };
                if last { Ok(()) } else { self.close_tab(tab) }
            }
            "tab.close-others" => self.close_other_tabs(tab),
            "tab.reopen" => {
                let closed = lock(&self.shell)
                    .panes
                    .get_mut(&pane)
                    .and_then(|p| p.closed_paths.pop());
                match closed {
                    Some(path) => self.open_tab(pane, &path, true).map(|_| ()),
                    None => Err(AppError::Invalid("no closed tab to reopen".into())),
                }
            }
            "tab.next" => self.step_tab(pane, 1),
            "tab.previous" => self.step_tab(pane, -1),
            // The last tab however many there are; a numbered slot past the end lands there too.
            "tab.goto-last" => self.goto_tab(pane, usize::MAX),
            _ if id.starts_with("tab.goto-") => match id["tab.goto-".len()..].parse::<usize>() {
                Ok(slot @ 1..=8) => self.goto_tab(pane, slot - 1),
                _ => Err(AppError::Invalid(format!("'{id}' has no handler here"))),
            },
            "tab.move-to-new-pane" => self.move_tab_to_new_pane(tab).map(|_| ()),
            "tab.close-right" => {
                let later: Vec<TabId> = lock(&self.shell)
                    .panes
                    .get(&pane)
                    .map(|p| {
                        let at = p.tabs.iter().position(|t| *t == tab).unwrap_or(0);
                        p.tabs.iter().skip(at + 1).copied().collect()
                    })
                    .unwrap_or_default();
                for t in later {
                    self.close_tab(t)?;
                }
                Ok(())
            }
            "tab.move-left" | "tab.move-right" => {
                let at = lock(&self.shell)
                    .panes
                    .get(&pane)
                    .and_then(|p| p.tabs.iter().position(|t| *t == tab))
                    .unwrap_or(0);
                let to = if id == "tab.move-left" {
                    at.saturating_sub(1)
                } else {
                    at + 1
                };
                self.move_tab(tab, to)
            }
            "tab.move-to-other-pane" => {
                let other = self.other_pane(pane).ok_or(AppError::Stale)?;
                self.move_tab_to_pane(tab, other)
            }
            "pane.split-right" => self
                .split_pane(pane, SplitOrientation::Vertical, &path)
                .map(|_| ()),
            "pane.split-below" => self
                .split_pane(pane, SplitOrientation::Horizontal, &path)
                .map(|_| ()),
            "pane.close" => self.close_pane(pane),
            "pane.next" => {
                self.step_pane(1);
                Ok(())
            }
            "pane.previous" => {
                self.step_pane(-1);
                Ok(())
            }
            "pane.swap" => self.swap_panes(),
            "pane.equalise" => {
                self.equalise_panes();
                Ok(())
            }
            "pane.open-here-in-other" => self.open_in_other_pane(pane, &path),
            "pane.open-selected-in-other" => {
                let folder = self
                    .selected_paths(tab)?
                    .into_iter()
                    .next()
                    .ok_or(AppError::Stale)?;
                self.open_in_other_pane(pane, &folder)
            }
            "pane.copy-to-other" | "pane.move-to-other" => {
                let other = self.other_pane(pane).ok_or(AppError::Stale)?;
                let target = self
                    .panes_active_path(other)
                    .ok_or_else(|| AppError::Invalid("the other pane has no folder".into()))?;
                let paths = self.selected_paths(tab)?;
                let verb = if id == "pane.copy-to-other" {
                    TransferVerb::Copy
                } else {
                    TransferVerb::Move
                };
                self.op_transfer(paths, target, verb).map(|_| ())
            }
            "select.all" => self.select(tab, SelectOp::All, &[]),
            "select.none" => self.select(tab, SelectOp::Clear, &[]),
            "select.invert" => self.select(tab, SelectOp::Invert, &[]),
            "select.files" | "select.folders" => {
                let want_dirs = id == "select.folders";
                let indices: Vec<u32> = lock(&cell)
                    .listing
                    .rows()
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.is_directory == want_dirs)
                    .map(|(i, _)| i as u32)
                    .collect();
                self.select(tab, SelectOp::Replace, &indices)
            }
            "file.open" => {
                let first = self.selected_indices(tab)?.first().copied();
                match first {
                    Some(i) => self.open_item(tab, i),
                    None => Ok(()),
                }
            }
            "file.open-new-tab" => {
                let folders = self.selected_folders(tab, 10)?;
                // In the background, beside this one, as a middle-click does.
                for folder in &folders {
                    self.open_tab(pane, folder, false)?;
                }
                Ok(())
            }
            "file.open-pane-right" | "file.open-pane-below" => {
                let folder = self.selected_folders(tab, 1)?.into_iter().next();
                match folder {
                    Some(f) => {
                        let o = if id.ends_with("right") {
                            SplitOrientation::Vertical
                        } else {
                            SplitOrientation::Horizontal
                        };
                        self.split_pane(pane, o, &f).map(|_| ())
                    }
                    None => Ok(()),
                }
            }
            "file.bookmark" => {
                let paths = self.selected_paths(tab)?;
                self.toggle_bookmarks(tab, &paths)
            }
            "edit.undo" => self.op_undo_step(1).map(|_| ()),
            "edit.redo" => self.op_undo_step(-1).map(|_| ()),
            "view.preview" | "view.metadata" => {
                let target = if id == "view.preview" { 1 } else { 2 };
                let current = lock(&cell).side_pane;
                self.set_side_pane(tab, if current == target { 0 } else { target })
            }
            "view.side-pane" => {
                let current = lock(&cell).side_pane;
                self.set_side_pane(tab, (current + 1) % 3)
            }
            "view.flat" => {
                let on = !lock(&cell).is_flat_view;
                self.set_flat_view(tab, on)
            }
            "view.hidden" => {
                let now = !lock(&self.settings).show_hidden_items;
                self.settings_set("showHiddenItems", if now { "true" } else { "false" })
            }
            "view.details" => self.set_thumbs(tab, 0.0),
            "view.thumbnails" => {
                let last = *lock(&self.extra.last_thumbnail_scale);
                self.set_thumbs(tab, last)
            }
            "view.toggle-thumbnails" => {
                let on = lock(&cell).thumbnail_scale > 0.0;
                let last = *lock(&self.extra.last_thumbnail_scale);
                self.set_thumbs(tab, if on { 0.0 } else { last })
            }
            "view.thumbs-larger" => {
                let now = lock(&cell).thumbnail_scale;
                self.set_thumbs(tab, (now + THUMBNAIL_STEP).min(1.0))
            }
            "view.thumbs-smaller" => {
                let now = lock(&cell).thumbnail_scale;
                self.set_thumbs(tab, (now - THUMBNAIL_STEP).max(0.0))
            }
            "view.sort-name" => self.sort_ascending(tab, "Name"),
            "view.sort-size" => self.sort_ascending(tab, "Size"),
            "view.sort-type" => self.sort_ascending(tab, "Type"),
            "view.sort-modified" => self.sort_ascending(tab, "Modified"),
            "view.sort-created" => self.sort_ascending(tab, "Created"),
            "view.sort-extension" => self.sort_ascending(tab, "Extension"),
            "view.sort-reverse" => {
                let by = lock(&cell).sort_by.clone();
                self.set_sort(tab, &by)
            }
            "view.columns-default" => self.columns_use_for_new_tabs(tab),
            "view.reset-columns" => self.columns_reset(tab),
            "search.clear" => self.clear_search(tab),
            "search.stop" => self.stop_search(tab),
            "tools.compare" => self.compare_toggle(),
            "tools.indexer-dismiss" => {
                self.index_dismiss_banner();
                Ok(())
            }
            "tools.indexer-start" => self.index_start(),
            "tools.indexer-retry" => self.index_retry(),
            "tools.transfer-pause" => {
                self.set_paused(true);
                Ok(())
            }
            "tools.transfer-resume" => {
                self.set_paused(false);
                Ok(())
            }
            "tools.transfer-cancel" => {
                self.cancel_running();
                Ok(())
            }
            "preview.mode-auto" | "preview.mode-raw" | "preview.mode-hex" => {
                lock(&cell).preview_mode = match id {
                    "preview.mode-auto" => 0,
                    "preview.mode-raw" => 1,
                    _ => 2,
                };
                self.hub.emit(crate::events::AppEvent::TabChanged { tab });
                Ok(())
            }
            "preview.wrap" => {
                let mut t = lock(&cell);
                t.preview_wrap = !t.preview_wrap;
                drop(t);
                self.hub.emit(crate::events::AppEvent::TabChanged { tab });
                Ok(())
            }
            "preview.fit" => {
                let mut t = lock(&cell);
                t.preview_fit = !t.preview_fit;
                drop(t);
                self.hub.emit(crate::events::AppEvent::TabChanged { tab });
                Ok(())
            }
            "preview.auto-advance" | "preview.loop" | "preview.autoplay" => {
                let (key, other) = match id {
                    "preview.auto-advance" => ("previewAutoAdvance", Some("previewLoop")),
                    "preview.loop" => ("previewLoop", Some("previewAutoAdvance")),
                    _ => ("previewAutoPlay", None),
                };
                let now = self
                    .settings_value(key)
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                // At most one of auto-advance and loop is ever on.
                if !now && let Some(other) = other {
                    self.settings_set(other, "false")?;
                }
                self.settings_set(key, if now { "false" } else { "true" })
            }
            "app.palette" => {
                self.palette_toggle();
                Ok(())
            }
            "app.keyboard" => self.settings_open_page("keyboard"),
            "app.settings" => {
                if lock(&self.extra.ui).settings_page.is_some() {
                    Ok(())
                } else {
                    self.settings_open_page("general")
                }
            }
            "app.command-bars" => self.settings_open_page("command-bars"),
            "app.match-windows-theme" => {
                let on = !self.theme_is_following();
                self.theme_follow_set(on);
                Ok(())
            }
            "app.bookmark-folder" => self.toggle_bookmark(&path, true),
            _ => Err(AppError::Invalid(format!("'{id}' has no handler here"))),
        }
    }

    /// What Undo and Redo say they will do. Spec: ShellViewModel.Described.
    fn undo_caption(&self, command: &str, verb: &str, description: &str) -> String {
        let what = format!("{verb} {}", bb_core::undo::UndoText::as_object(description));
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
            .map(|r| r.full_path.clone())
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
