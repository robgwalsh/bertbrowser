//! The runner for Core commands (`run_core`), one small method per catalogue area.

use std::sync::Arc;

use bb_core::fs::KnownFolders;
use bb_core::layout::SplitOrientation;
use bb_core::transfer::TransferVerb;

use super::{ArchivePathCheck, THUMBNAIL_STEP, drive_root};
use crate::app::{Inner, TabCell, lock};
use crate::browse::tabs::SelectOp;
use crate::ids::{AppError, AppResult, PaneId, TabId};

/// What one run acts on: the command, the active tab and pane, and where the tab is.
struct Run<'a> {
    id: &'a str,
    tab: TabId,
    pane: PaneId,
    cell: TabCell,
    path: String,
}

fn no_handler(id: &str) -> AppError {
    AppError::Invalid(format!("'{id}' has no handler here"))
}

impl Inner {
    pub(super) fn run_core(self: &Arc<Self>, id: &str, tab: TabId, pane: PaneId) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let path = lock(&cell).path.clone();
        let run = Run {
            id,
            tab,
            pane,
            cell,
            path,
        };
        match id.split_once('.').map(|(area, _)| area) {
            Some("nav") => self.run_nav(&run),
            Some("tab") => self.run_tab(&run),
            Some("pane") => self.run_pane(&run),
            Some("select") => self.run_select(&run),
            Some("file") => self.run_file(&run),
            Some("edit") => match id {
                "edit.undo" => self.op_undo_step(1).map(|_| ()),
                "edit.redo" => self.op_undo_step(-1).map(|_| ()),
                _ => Err(no_handler(id)),
            },
            Some("view") => self.run_view(&run),
            Some("search") => match id {
                "search.clear" => self.clear_search(tab),
                "search.stop" => self.stop_search(tab),
                _ => Err(no_handler(id)),
            },
            Some("tools") => self.run_tools(&run),
            Some("preview") => self.run_preview(&run),
            Some("app") => self.run_app(&run),
            _ => Err(no_handler(id)),
        }
    }

    fn run_nav(self: &Arc<Self>, run: &Run) -> AppResult<()> {
        let (tab, path) = (run.tab, &run.path);
        match run.id {
            "nav.back" => self.back(tab),
            "nav.forward" => self.forward(tab),
            "nav.up" => self.up(tab),
            "nav.refresh" => self.refresh(tab),
            "nav.refresh-all" => {
                self.refresh_all();
                Ok(())
            }
            "nav.goto" => {
                let seed = if path.is_empty() || ArchivePathCheck::in_archive(path) {
                    String::new()
                } else if path.ends_with('\\') {
                    path.clone()
                } else {
                    format!("{path}\\")
                };
                self.palette_open(&seed);
                Ok(())
            }
            "nav.drive-root" => self.goto_known_folder(tab, drive_root(path)),
            "nav.home" => {
                // Where Windows says they are today: OneDrive's folder backup and a relocated
                // Downloads move them away from the profile.
                let profile = KnownFolders::profile().or_else(|| std::env::var("USERPROFILE").ok());
                self.goto_known_folder(tab, profile)
            }
            "nav.desktop" => self.goto_known_folder(tab, KnownFolders::desktop()),
            "nav.documents" => self.goto_known_folder(tab, KnownFolders::documents()),
            "nav.downloads" => self.goto_known_folder(tab, KnownFolders::downloads()),
            "nav.containing-folder" => self.containing_folder(tab),
            id => Err(no_handler(id)),
        }
    }

    fn run_tab(self: &Arc<Self>, run: &Run) -> AppResult<()> {
        let (tab, pane) = (run.tab, run.pane);
        match run.id {
            "tab.new" | "tab.duplicate" => self.open_tab(pane, &run.path, true).map(|_| ()),
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
            id if id.starts_with("tab.goto-") => match id["tab.goto-".len()..].parse::<usize>() {
                Ok(slot @ 1..=8) => self.goto_tab(pane, slot - 1),
                _ => Err(no_handler(id)),
            },
            "tab.move-to-new-pane" => self.move_tab_to_new_pane(tab).map(|_| ()),
            "tab.close-right" => self.close_tabs_right_of(tab, pane),
            id @ ("tab.move-left" | "tab.move-right") => {
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
            id => Err(no_handler(id)),
        }
    }

    fn close_tabs_right_of(self: &Arc<Self>, tab: TabId, pane: PaneId) -> AppResult<()> {
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

    fn run_pane(self: &Arc<Self>, run: &Run) -> AppResult<()> {
        let (tab, pane, path) = (run.tab, run.pane, &run.path);
        match run.id {
            "pane.split-right" => self
                .split_pane(pane, SplitOrientation::Vertical, path)
                .map(|_| ()),
            "pane.split-below" => self
                .split_pane(pane, SplitOrientation::Horizontal, path)
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
            "pane.open-here-in-other" => self.open_in_other_pane(pane, path),
            "pane.open-selected-in-other" => {
                let folder = self
                    .selected_paths(tab)?
                    .into_iter()
                    .next()
                    .ok_or(AppError::Stale)?;
                self.open_in_other_pane(pane, &folder)
            }
            id @ ("pane.copy-to-other" | "pane.move-to-other") => {
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
            id => Err(no_handler(id)),
        }
    }

    fn run_select(self: &Arc<Self>, run: &Run) -> AppResult<()> {
        let tab = run.tab;
        match run.id {
            "select.all" => self.select(tab, SelectOp::All, &[]),
            "select.none" => self.select(tab, SelectOp::Clear, &[]),
            "select.invert" => self.select(tab, SelectOp::Invert, &[]),
            id @ ("select.files" | "select.folders") => {
                let want_dirs = id == "select.folders";
                let indices: Vec<u32> = lock(&run.cell)
                    .listing
                    .rows()
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.is_directory == want_dirs)
                    .map(|(i, _)| i as u32)
                    .collect();
                self.select(tab, SelectOp::Replace, &indices)
            }
            id => Err(no_handler(id)),
        }
    }

    fn run_file(self: &Arc<Self>, run: &Run) -> AppResult<()> {
        let (tab, pane) = (run.tab, run.pane);
        match run.id {
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
            id @ ("file.open-pane-right" | "file.open-pane-below") => {
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
            id => Err(no_handler(id)),
        }
    }

    fn run_view(self: &Arc<Self>, run: &Run) -> AppResult<()> {
        let (tab, cell) = (run.tab, &run.cell);
        match run.id {
            id @ ("view.preview" | "view.metadata") => {
                let target = if id == "view.preview" { 1 } else { 2 };
                let current = lock(cell).side_pane;
                self.set_side_pane(tab, if current == target { 0 } else { target })
            }
            "view.side-pane" => {
                let current = lock(cell).side_pane;
                self.set_side_pane(tab, (current + 1) % 3)
            }
            "view.flat" => {
                let on = !lock(cell).is_flat_view;
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
                let on = lock(cell).thumbnail_scale > 0.0;
                let last = *lock(&self.extra.last_thumbnail_scale);
                self.set_thumbs(tab, if on { 0.0 } else { last })
            }
            "view.thumbs-larger" => {
                let now = lock(cell).thumbnail_scale;
                self.set_thumbs(tab, (now + THUMBNAIL_STEP).min(1.0))
            }
            "view.thumbs-smaller" => {
                let now = lock(cell).thumbnail_scale;
                self.set_thumbs(tab, (now - THUMBNAIL_STEP).max(0.0))
            }
            "view.sort-name" => self.sort_ascending(tab, "Name"),
            "view.sort-size" => self.sort_ascending(tab, "Size"),
            "view.sort-type" => self.sort_ascending(tab, "Type"),
            "view.sort-modified" => self.sort_ascending(tab, "Modified"),
            "view.sort-created" => self.sort_ascending(tab, "Created"),
            "view.sort-extension" => self.sort_ascending(tab, "Extension"),
            "view.sort-reverse" => {
                let by = lock(cell).sort_by.clone();
                self.set_sort(tab, &by)
            }
            "view.columns-default" => self.columns_use_for_new_tabs(tab),
            "view.reset-columns" => self.columns_reset(tab),
            id => Err(no_handler(id)),
        }
    }

    fn run_tools(self: &Arc<Self>, run: &Run) -> AppResult<()> {
        match run.id {
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
            id => Err(no_handler(id)),
        }
    }

    fn run_preview(self: &Arc<Self>, run: &Run) -> AppResult<()> {
        let (tab, cell) = (run.tab, &run.cell);
        match run.id {
            id @ ("preview.mode-auto" | "preview.mode-raw" | "preview.mode-hex") => {
                lock(cell).preview_mode = match id {
                    "preview.mode-auto" => 0,
                    "preview.mode-raw" => 1,
                    _ => 2,
                };
                self.hub.emit(crate::events::AppEvent::TabChanged { tab });
                Ok(())
            }
            "preview.wrap" => {
                let mut t = lock(cell);
                t.preview_wrap = !t.preview_wrap;
                drop(t);
                self.hub.emit(crate::events::AppEvent::TabChanged { tab });
                Ok(())
            }
            "preview.fit" => {
                let mut t = lock(cell);
                t.preview_fit = !t.preview_fit;
                drop(t);
                self.hub.emit(crate::events::AppEvent::TabChanged { tab });
                Ok(())
            }
            id @ ("preview.auto-advance" | "preview.loop" | "preview.autoplay") => {
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
            id => Err(no_handler(id)),
        }
    }

    fn run_app(self: &Arc<Self>, run: &Run) -> AppResult<()> {
        match run.id {
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
            "app.bookmark-folder" => self.toggle_bookmark(&run.path, true),
            id => Err(no_handler(id)),
        }
    }
}
