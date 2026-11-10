//! Changes to an arrangement, each returning a new one. A place in it is a bar and a path of
//! indices down through its submenus — `(MenuBar, [0, 1])` is the second item of the first menu,
//! which had better be a submenu — so the editor's flat list is always "the items here".
//!
//! Nothing here fails for a place that does not exist: the arrangement can be replaced from a menu
//! or the palette while the editor is open on it, and an edit aimed at what is no longer there
//! should do nothing (`Cow::Borrowed`) rather than take the settings page down.
//! Spec: Core/Services/CommandBars/CommandBarEdit.cs.

use std::borrow::Cow;

use super::{AccessKeys, BarItem, CommandBarRules, CommandBarSet, ToolbarSpec};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BarTargetKind {
    /// The row of top-level menus.
    MenuBar,
    /// The title bar's toolbar.
    TitleBar,
    /// A toolbar row under the title bar, by index.
    Row,
}

/// Which bar of an arrangement is meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BarTarget {
    pub kind: BarTargetKind,
    pub row: usize,
}

impl BarTarget {
    pub const MENU_BAR: BarTarget = BarTarget {
        kind: BarTargetKind::MenuBar,
        row: 0,
    };
    pub const TITLE_BAR: BarTarget = BarTarget {
        kind: BarTargetKind::TitleBar,
        row: 0,
    };

    pub const fn for_row(row: usize) -> BarTarget {
        BarTarget {
            kind: BarTargetKind::Row,
            row,
        }
    }
}

pub struct CommandBarEdit;

impl CommandBarEdit {
    /// The items at a place, or empty when there is no such place.
    pub fn items_at<'a>(
        set: &'a CommandBarSet,
        target: BarTarget,
        path: &[usize],
    ) -> &'a [BarItem] {
        descend(root(set, target), path).unwrap_or(&[])
    }

    /// Whether the place exists: the bar is there, and every step of the path is a submenu.
    pub fn exists(set: &CommandBarSet, target: BarTarget, path: &[usize]) -> bool {
        descend(root(set, target), path).is_some()
    }

    /// The names on the way down: the bar's, then each submenu's.
    pub fn trail(set: &CommandBarSet, target: BarTarget, path: &[usize]) -> Vec<String> {
        let mut trail = vec![Self::name_of(target)];
        let mut items = root(set, target);
        for &index in path {
            let Some(item) = items.and_then(|items| items.get(index)) else {
                break;
            };
            if !item.is_menu() {
                break;
            }
            trail.push(AccessKeys::strip(item.caption.as_deref().unwrap_or("")));
            items = Some(item.children());
        }
        trail
    }

    pub fn name_of(target: BarTarget) -> String {
        match target.kind {
            BarTargetKind::MenuBar => "Menu bar".to_string(),
            BarTargetKind::TitleBar => "Title bar toolbar".to_string(),
            BarTargetKind::Row => format!("Toolbar row {}", target.row + 1),
        }
    }

    /// Whether an item may sit at a place. Only one rule: the menu bar itself holds menus and
    /// nothing else — a command there would be a heading that runs when clicked.
    pub fn accepts(target: BarTarget, path: &[usize], item: &BarItem) -> bool {
        target.kind != BarTargetKind::MenuBar || !path.is_empty() || item.is_menu()
    }

    pub fn insert<'a>(
        set: &'a CommandBarSet,
        target: BarTarget,
        path: &[usize],
        index: usize,
        item: BarItem,
    ) -> Cow<'a, CommandBarSet> {
        if !Self::exists(set, target, path) || !Self::accepts(target, path, &item) {
            return Cow::Borrowed(set);
        }
        if item.is_menu() && path.len() + 1 > CommandBarRules::MAX_DEPTH {
            return Cow::Borrowed(set);
        }

        let mut items = Self::items_at(set, target, path).to_vec();
        let at = index.min(items.len());
        items.insert(at, item);
        with_items_at(set, target, path, items)
    }

    pub fn remove<'a>(
        set: &'a CommandBarSet,
        target: BarTarget,
        path: &[usize],
        index: usize,
    ) -> Cow<'a, CommandBarSet> {
        let mut items = Self::items_at(set, target, path).to_vec();
        if index >= items.len() {
            return Cow::Borrowed(set);
        }
        items.remove(index);
        with_items_at(set, target, path, items)
    }

    /// Moves an item to the slot it should end up in, counted after it has been taken out.
    pub fn move_item<'a>(
        set: &'a CommandBarSet,
        target: BarTarget,
        path: &[usize],
        from: usize,
        to: usize,
    ) -> Cow<'a, CommandBarSet> {
        let mut items = Self::items_at(set, target, path).to_vec();
        if from >= items.len() {
            return Cow::Borrowed(set);
        }
        let item = items.remove(from);
        let at = to.min(items.len());
        items.insert(at, item);
        with_items_at(set, target, path, items)
    }

    pub fn replace<'a>(
        set: &'a CommandBarSet,
        target: BarTarget,
        path: &[usize],
        index: usize,
        item: BarItem,
    ) -> Cow<'a, CommandBarSet> {
        let mut items = Self::items_at(set, target, path).to_vec();
        if index >= items.len() {
            return Cow::Borrowed(set);
        }
        items[index] = item;
        with_items_at(set, target, path, items)
    }

    pub fn add_row(set: &CommandBarSet) -> CommandBarSet {
        let mut added = set.clone();
        added.rows.push(ToolbarSpec::new(vec![]));
        added
    }

    pub fn remove_row(set: &CommandBarSet, row: usize) -> Cow<'_, CommandBarSet> {
        if row >= set.rows.len() {
            return Cow::Borrowed(set);
        }
        let mut edited = set.clone();
        edited.rows.remove(row);
        Cow::Owned(edited)
    }

    /// Whether a toolbar's buttons carry their names. False for the menu bar, which has no buttons.
    pub fn shows_text(set: &CommandBarSet, target: BarTarget) -> bool {
        toolbar(set, target).is_some_and(|t| t.show_text)
    }

    pub fn with_show_text(
        set: &CommandBarSet,
        target: BarTarget,
        show_text: bool,
    ) -> Cow<'_, CommandBarSet> {
        let Some(current) = toolbar(set, target) else {
            return Cow::Borrowed(set);
        };
        let edited = ToolbarSpec {
            items: current.items.clone(),
            show_text,
        };
        Cow::Owned(with_toolbar(set, target, edited))
    }
}

fn root(set: &CommandBarSet, target: BarTarget) -> Option<&[BarItem]> {
    match target.kind {
        BarTargetKind::MenuBar => Some(&set.menus),
        _ => toolbar(set, target).map(|t| t.items.as_slice()),
    }
}

fn toolbar(set: &CommandBarSet, target: BarTarget) -> Option<&ToolbarSpec> {
    match target.kind {
        BarTargetKind::TitleBar => Some(&set.title_bar),
        BarTargetKind::Row => set.rows.get(target.row),
        BarTargetKind::MenuBar => None,
    }
}

fn descend<'a>(root: Option<&'a [BarItem]>, path: &[usize]) -> Option<&'a [BarItem]> {
    let mut items = root?;
    for &index in path {
        let item = items.get(index)?;
        if !item.is_menu() {
            return None;
        }
        items = item.children();
    }
    Some(items)
}

fn with_toolbar(set: &CommandBarSet, target: BarTarget, toolbar: ToolbarSpec) -> CommandBarSet {
    let mut edited = set.clone();
    match target.kind {
        BarTargetKind::TitleBar => edited.title_bar = toolbar,
        BarTargetKind::Row => {
            if let Some(row) = edited.rows.get_mut(target.row) {
                *row = toolbar;
            }
        }
        BarTargetKind::MenuBar => {}
    }
    edited
}

fn with_items_at<'a>(
    set: &'a CommandBarSet,
    target: BarTarget,
    path: &[usize],
    items: Vec<BarItem>,
) -> Cow<'a, CommandBarSet> {
    let Some(root) = root(set, target) else {
        return Cow::Borrowed(set);
    };
    let replaced = descend_replace(root, path, items);
    match target.kind {
        BarTargetKind::MenuBar => {
            let mut edited = set.clone();
            edited.menus = replaced;
            Cow::Owned(edited)
        }
        _ => {
            let show_text = toolbar(set, target).is_some_and(|t| t.show_text);
            Cow::Owned(with_toolbar(
                set,
                target,
                ToolbarSpec {
                    items: replaced,
                    show_text,
                },
            ))
        }
    }
}

fn descend_replace(here: &[BarItem], path: &[usize], items: Vec<BarItem>) -> Vec<BarItem> {
    let Some((&index, rest)) = path.split_first() else {
        return items;
    };
    let mut copy = here.to_vec();
    if let Some(item) = copy.get_mut(index)
        && item.is_menu()
    {
        let children = item.items.take().unwrap_or_default();
        item.items = Some(descend_replace(&children, rest, items));
    }
    copy
}
