//! The menus-and-toolbars editor's verbs (`barpage_do`): one private verb enum, one small method per
//! verb. Rows are given by index or by name.

use std::sync::Arc;

use bb_core::commandbars::{
    BarItem, BarTarget, BarTargetKind, CommandBarEdit, CommandBarLists, CommandBarSet,
};
use bb_core::commands::CommandCatalog;

use super::target_of;
use crate::app::{Inner, lock};
use crate::events::Model;
use crate::ids::{AppError, AppResult};

verb_enum! {
    /// Every verb of the editor.
    BarVerb {
        Select = "select",
        Pick = "pick",
        Open = "open",
        Up = "up",
        Add = "add",
        AddAt = "add-at",
        AddSeparator = "add-separator",
        AddMenu = "add-menu",
        Remove = "remove",
        Nudge = "nudge",
        Move = "move",
        Name = "name",
        RowShowText = "row-show-text",
        ShowText = "show-text",
        AddRow = "add-row",
        RemoveRow = "remove-row",
        Filter = "filter",
        Unplaced = "unplaced",
        Reset = "reset",
        ApplySaved = "apply-saved",
        RemoveSaved = "remove-saved",
        SelectSaved = "select-saved",
        Save = "save",
        RenameSaved = "rename-saved",
    }
}

/// Where the editor stands when a verb arrives: the arrangement, the bar and submenu path being
/// edited, and the selected row.
struct PageState {
    set: CommandBarSet,
    target: BarTarget,
    path: Vec<usize>,
    selected: usize,
}

impl Inner {
    pub(crate) fn barpage_do(self: &Arc<Self>, verb: &str, arg: &str) -> AppResult<()> {
        let set = self.bars_set_current();
        let (target, path, selected) = {
            let page = lock(&self.extra.barpage);
            (page.target, page.path.clone(), page.selected_row)
        };
        let page = PageState {
            set,
            target,
            path,
            selected,
        };
        let verb: BarVerb = verb
            .parse()
            .map_err(|()| AppError::Invalid(format!("unknown bars verb '{verb}'")))?;
        match verb {
            BarVerb::Select => self.bp_select(arg),
            BarVerb::Pick => self.bp_pick(arg),
            BarVerb::Open => self.bp_open(&page, arg),
            BarVerb::Up => self.bp_up(),
            BarVerb::Add | BarVerb::AddAt | BarVerb::AddSeparator | BarVerb::AddMenu => {
                self.bp_add(&page, verb, arg)
            }
            BarVerb::Remove => self.bp_remove(&page, arg),
            BarVerb::Nudge => self.bp_nudge(&page, arg),
            BarVerb::Move => self.bp_move(&page, arg),
            BarVerb::Name => self.bp_name(&page, arg),
            BarVerb::RowShowText => self.bp_row_show_text(&page, arg),
            BarVerb::ShowText => {
                let on = matches!(arg, "on" | "true" | "1");
                let edited = CommandBarEdit::with_show_text(&page.set, page.target, on);
                self.barpage_commit(edited, None)
            }
            BarVerb::AddRow => self.bp_add_row(&page),
            BarVerb::RemoveRow => {
                if page.target.kind != BarTargetKind::Row {
                    return Ok(());
                }
                let edited = CommandBarEdit::remove_row(&page.set, page.target.row);
                self.barpage_commit(edited, None)
            }
            BarVerb::Filter => {
                lock(&self.extra.barpage).filter = arg.to_string();
                self.bp_changed();
                Ok(())
            }
            BarVerb::Unplaced => {
                lock(&self.extra.barpage).unplaced_only = matches!(arg, "on" | "true" | "1");
                self.bp_changed();
                Ok(())
            }
            BarVerb::Reset => self.bars_replace_asking(None),
            BarVerb::ApplySaved => self.bars_replace_asking(Some(arg.to_string())),
            BarVerb::RemoveSaved => self.bars_remove(arg),
            BarVerb::SelectSaved => {
                lock(&self.extra.barpage).selected_saved = Some(arg.to_string());
                self.bp_changed();
                Ok(())
            }
            BarVerb::Save => self.bars_save_as(arg),
            BarVerb::RenameSaved => {
                let (old, new) = arg.split_once(" to ").ok_or_else(|| {
                    AppError::Invalid("rename-saved takes '<old> to <new>'".into())
                })?;
                self.bars_rename(old.trim(), new.trim())
            }
        }
    }

    fn bp_changed(&self) {
        self.hub.model(Model::BarPage);
    }

    fn bp_select(&self, arg: &str) -> AppResult<()> {
        let t = target_of(arg).ok_or_else(|| AppError::Invalid(format!("no bar '{arg}'")))?;
        let mut page = lock(&self.extra.barpage);
        page.target = t;
        page.path.clear();
        page.selected_row = 0;
        drop(page);
        self.bp_changed();
        Ok(())
    }

    fn bp_pick(&self, arg: &str) -> AppResult<()> {
        let i = self.barpage_row(arg)?;
        lock(&self.extra.barpage).selected_row = i;
        self.bp_changed();
        Ok(())
    }

    fn bp_open(&self, page: &PageState, arg: &str) -> AppResult<()> {
        // A top-level menu opens by name from the menu bar; a submenu by its row.
        let i = self.barpage_row(arg)?;
        let items = CommandBarEdit::items_at(&page.set, page.target, &page.path);
        if !items.get(i).is_some_and(BarItem::is_menu) {
            return Err(AppError::Invalid(format!("'{arg}' is not a menu")));
        }
        let mut page = lock(&self.extra.barpage);
        page.path.push(i);
        page.selected_row = 0;
        drop(page);
        self.bp_changed();
        Ok(())
    }

    fn bp_up(&self) -> AppResult<()> {
        let mut page = lock(&self.extra.barpage);
        if let Some(came_from) = page.path.pop() {
            page.selected_row = came_from;
        }
        drop(page);
        self.bp_changed();
        Ok(())
    }

    /// `add`, `add-at`, `add-separator` and `add-menu`.
    fn bp_add(&self, page: &PageState, verb: BarVerb, arg: &str) -> AppResult<()> {
        let (set, target, path, selected) = (&page.set, page.target, &page.path, page.selected);
        // `add-at <index> <what>` puts it exactly there: a choice dropped between two rows.
        // Spec: SettingsView.xaml.cs (BarItemsList_Drop).
        let (place, arg) = if verb == BarVerb::AddAt {
            let (index, rest) = arg
                .split_once(' ')
                .ok_or_else(|| AppError::Invalid("add-at takes '<index> <what>'".into()))?;
            let index = index
                .parse::<usize>()
                .map_err(|_| AppError::Invalid("add-at takes '<index> <what>'".into()))?;
            (Some(index), rest.trim())
        } else {
            (None, arg)
        };
        let item = match verb {
            BarVerb::AddSeparator => BarItem::separator(),
            BarVerb::AddMenu => BarItem::menu("New menu", vec![]),
            _ if arg.starts_with('@') => BarItem::list(&arg[1..]),
            _ if arg == "-" => BarItem::separator(),
            _ => self.bp_choice(arg)?,
        };
        let count = CommandBarEdit::items_at(set, target, path).len();
        let at = match place {
            Some(index) => index.min(count),
            None if count == 0 => 0,
            None => (selected + 1).min(count),
        };
        let edited = CommandBarEdit::insert(set, target, path, at, item);
        self.barpage_commit(edited, Some(at))
    }

    /// A choice by id or by name.
    fn bp_choice(&self, arg: &str) -> AppResult<BarItem> {
        let id = if CommandCatalog::is_known(arg) || self.custom_command(arg).is_some() {
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
                .ok_or_else(|| AppError::Invalid(format!("nothing called '{arg}' can be added")))?
        };
        Ok(match id.strip_prefix('@') {
            Some(list) => BarItem::list(list),
            None => BarItem::command(&id),
        })
    }

    fn bp_remove(&self, page: &PageState, arg: &str) -> AppResult<()> {
        let i = if arg.is_empty() {
            page.selected
        } else {
            self.barpage_row(arg)?
        };
        let edited = CommandBarEdit::remove(&page.set, page.target, &page.path, i);
        self.barpage_commit(edited, Some(i))
    }

    fn bp_nudge(&self, page: &PageState, arg: &str) -> AppResult<()> {
        // `arg` is "<row> up|down" or "up|down" for the selected row.
        let (row, dir) = match arg.rsplit_once(' ') {
            Some((row, dir)) => (row.trim(), dir.trim()),
            None => ("", arg),
        };
        let from = if row.is_empty() {
            page.selected
        } else {
            self.barpage_row(row)?
        };
        let to = match dir {
            "up" if from > 0 => from - 1,
            "down" => from + 1,
            _ => return Ok(()),
        };
        if to >= CommandBarEdit::items_at(&page.set, page.target, &page.path).len() {
            return Ok(());
        }
        let edited = CommandBarEdit::move_item(&page.set, page.target, &page.path, from, to);
        self.barpage_commit(edited, Some(to))
    }

    fn bp_move(&self, page: &PageState, arg: &str) -> AppResult<()> {
        let (from, to) = arg
            .split_once(' ')
            .and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)))
            .ok_or_else(|| AppError::Invalid("move takes '<from> <to>'".into()))?;
        let edited = CommandBarEdit::move_item(&page.set, page.target, &page.path, from, to);
        self.barpage_commit(edited, Some(to))
    }

    fn bp_name(&self, page: &PageState, arg: &str) -> AppResult<()> {
        let (set, target, path, selected) = (&page.set, page.target, &page.path, page.selected);
        let items = CommandBarEdit::items_at(set, target, path);
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
            set,
            target,
            path,
            selected,
            BarItem {
                caption: wanted,
                ..item.clone()
            },
        );
        self.barpage_commit(edited, Some(selected))
    }

    fn bp_row_show_text(&self, page: &PageState, arg: &str) -> AppResult<()> {
        let (set, target, path, selected) = (&page.set, page.target, &page.path, page.selected);
        let items = CommandBarEdit::items_at(set, target, path);
        let Some(item) = items.get(selected) else {
            return Ok(());
        };
        let on = matches!(arg, "on" | "true" | "1");
        let edited = CommandBarEdit::replace(
            set,
            target,
            path,
            selected,
            BarItem {
                show_text: on.then_some(true),
                ..item.clone()
            },
        );
        self.barpage_commit(edited, Some(selected))
    }

    fn bp_add_row(&self, page: &PageState) -> AppResult<()> {
        let edited = CommandBarEdit::add_row(&page.set);
        let row = edited.rows.len() - 1;
        self.bars_set(edited)?;
        let mut page = lock(&self.extra.barpage);
        page.target = BarTarget::for_row(row);
        page.path.clear();
        page.selected_row = 0;
        drop(page);
        self.bp_changed();
        Ok(())
    }
}
