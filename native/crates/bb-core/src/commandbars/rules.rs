//! The decisions behind the menu bar and the toolbars, kept pure: what an arrangement read back
//! from disk may contain, which commands it leaves out, and what a menu shows right now.
//! Spec: Core/Services/CommandBars/CommandBarRules.cs.

use std::collections::HashMap;

use super::{
    AccessKeys, BarItem, BarItemKind, CommandBarCaptions, CommandBarDefaults, CommandBarLists,
    CommandBarSet, ToolbarSpec,
};
use crate::shellmenu::{MenuSeparatorRules, MenuSlot};

/// A command as it stands this instant, as far as a menu or a toolbar needs to know.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BarCommandState {
    /// Its catalogue name (or a custom command's own), plain text.
    pub name: String,
    pub icon: Option<String>,
    /// Its shortcut in force, as text; empty when it has none.
    pub gesture: String,
    /// Why it cannot run now, or None when it can.
    pub unavailable: Option<String>,
    pub is_checked: bool,
    /// Words that depend on the moment — "Delete 3 items…", "Remove bookmark" — plain text, or
    /// None when the command reads the same always.
    pub live_caption: Option<String>,
}

/// What the rules ask of the running app in order to lay a bar out.
pub trait BarSource {
    /// None for an id nothing answers to — a command from a later build, or one of the user's own
    /// since deleted. Such an item is kept in the arrangement and simply not shown.
    fn command(&self, id: &str) -> Option<BarCommandState>;

    fn list_has_rows(&self, list_id: &str) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BarRowKind {
    Command,
    Separator,
    Menu,
    /// Stands where a list's rows go; the app fills them in.
    List,
    /// The greyed line an empty list leaves ("No saved workspaces").
    Placeholder,
}

/// One thing to draw. Only what shows is ever a row: hidden separators, unknown commands and
/// empty submenus have already gone.
#[derive(Clone, Debug, PartialEq)]
pub struct BarRow {
    pub kind: BarRowKind,
    pub id: Option<String>,
    /// With its access key marked (`AccessKeys`) on a menu; plain on a toolbar.
    pub caption: String,
    /// The command's full name, for a tooltip, whatever the caption was cut to.
    pub name: String,
    pub icon: Option<String>,
    pub gesture: String,
    pub is_enabled: bool,
    pub reason: Option<String>,
    pub is_checked: bool,
    pub show_text: bool,
    pub children: Vec<BarRow>,
}

impl BarRow {
    fn plain(kind: BarRowKind, id: Option<&str>, caption: &str, icon: Option<&str>) -> BarRow {
        BarRow {
            kind,
            id: id.map(str::to_string),
            caption: caption.to_string(),
            name: AccessKeys::strip(caption),
            icon: icon.map(str::to_string),
            gesture: String::new(),
            is_enabled: true,
            reason: None,
            is_checked: false,
            show_text: false,
            children: vec![],
        }
    }
}

pub struct CommandBarRules;

impl CommandBarRules {
    /// How deep submenus go. A menu under a menu under a menu under a menu is already further than
    /// anybody follows; this is here so a hand-edited file cannot recurse for ever.
    pub const MAX_DEPTH: usize = 4;

    /// The commands that carry a list of their own when they sit on a toolbar: Undo and Redo are
    /// split buttons, as they were when they were fixed there.
    pub fn dropdown_for(command_id: &str) -> Option<&'static str> {
        match command_id {
            "edit.undo" => Some(CommandBarLists::UNDO_STEPS),
            "edit.redo" => Some(CommandBarLists::REDO_STEPS),
            _ => None,
        }
    }

    // --- What an arrangement may hold ---

    /// An arrangement as read from the settings file, made safe to walk: nothing nested past
    /// `MAX_DEPTH`, no menu without a name and no command without an id. A command or a kind this
    /// build has never heard of is *kept* — it may be a later build's, and dropping it would lose
    /// it on the way back. Nothing stored means the default.
    pub fn clean(set: Option<&CommandBarSet>) -> CommandBarSet {
        let Some(set) = set else {
            return CommandBarDefaults::default_set().clone();
        };
        CommandBarSet {
            revision: set.revision,
            menus: clean_items(&set.menus, 1)
                .into_iter()
                .filter(BarItem::is_menu)
                .collect(),
            title_bar: clean_toolbar(&set.title_bar),
            rows: set.rows.iter().map(clean_toolbar).collect(),
        }
    }

    /// Whether two arrangements are the same arrangement, whatever revision each was made against.
    pub fn same(a: &CommandBarSet, b: &CommandBarSet) -> bool {
        same_items(&a.menus, &b.menus)
            && same_toolbar(&a.title_bar, &b.title_bar)
            && a.rows.len() == b.rows.len()
            && a.rows.iter().zip(&b.rows).all(|(x, y)| same_toolbar(x, y))
    }

    /// Whether this is the arrangement the app ships with — in which case nothing need be stored,
    /// and a later build's additions arrive by themselves.
    pub fn is_default(set: &CommandBarSet) -> bool {
        Self::same(set, CommandBarDefaults::default_set())
    }

    /// Every item of the arrangement, menus and toolbars alike, at any depth.
    pub fn flatten(set: &CommandBarSet) -> Vec<&BarItem> {
        let mut all = Self::flatten_items(&set.menus);
        all.extend(Self::flatten_items(&set.title_bar.items));
        for row in &set.rows {
            all.extend(Self::flatten_items(&row.items));
        }
        all
    }

    pub fn flatten_items(items: &[BarItem]) -> Vec<&BarItem> {
        let mut all = Vec::new();
        for item in items {
            all.push(item);
            all.extend(Self::flatten_items(item.children()));
        }
        all
    }

    /// How many times each command is placed, anywhere.
    pub fn placements(set: &CommandBarSet) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        for item in Self::flatten(set) {
            if item.is_command()
                && let Some(id) = &item.id
            {
                *counts.entry(id.clone()).or_insert(0) += 1;
            }
        }
        counts
    }

    /// The commands that are on no menu and no toolbar, in the order given — what the editor's
    /// "not placed anywhere" filter lists.
    pub fn unplaced<'a>(
        set: &CommandBarSet,
        command_ids: impl IntoIterator<Item = &'a str>,
    ) -> Vec<String> {
        let placed = Self::placements(set);
        command_ids
            .into_iter()
            .filter(|id| !placed.contains_key(*id))
            .map(str::to_string)
            .collect()
    }

    // --- What shows right now ---

    /// The rows one menu, or one toolbar, shows at this moment.
    ///
    /// A command is never hidden for being unavailable — it is greyed and says why, as in the
    /// palette — so a menu keeps its shape and people keep their place in it. What does go: a
    /// command nothing answers to, a list with no rows and no empty-state line, a submenu left
    /// with nothing in it, and then whichever separators that leaves doubled or at an edge.
    ///
    /// `access_keys`: menus mark a key on every caption; toolbars do not, or a letter typed at
    /// the file list could press a button. `show_text`: for a toolbar, whether its buttons carry
    /// captions by default.
    pub fn present<S: BarSource + ?Sized>(
        items: &[BarItem],
        source: &S,
        access_keys: bool,
        show_text: bool,
    ) -> Vec<BarRow> {
        present(items, source, access_keys, show_text, 1)
    }

    // --- Toolbar overflow ---

    /// How many of a toolbar's items fit, from the left. All of them when there is room; otherwise
    /// as many as leave space for the overflow button that will hold the rest.
    pub fn fit(widths: &[f64], available: f64, overflow_width: f64) -> usize {
        if widths.iter().sum::<f64>() <= available {
            return widths.len();
        }
        let mut used = overflow_width;
        let mut count = 0;
        for &width in widths {
            if used + width > available {
                break;
            }
            used += width;
            count += 1;
        }
        count
    }
}

fn clean_toolbar(toolbar: &ToolbarSpec) -> ToolbarSpec {
    ToolbarSpec {
        items: clean_items(&toolbar.items, 1),
        show_text: toolbar.show_text,
    }
}

fn clean_items(items: &[BarItem], depth: usize) -> Vec<BarItem> {
    let mut clean = Vec::new();
    for item in items {
        let caption = item
            .caption
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_string);
        match &item.kind {
            BarItemKind::Separator => clean.push(BarItem::separator()),
            BarItemKind::Menu => {
                if caption.is_none() || depth > CommandBarRules::MAX_DEPTH {
                    continue;
                }
                clean.push(BarItem {
                    kind: BarItemKind::Menu,
                    id: None,
                    caption,
                    items: Some(clean_items(item.children(), depth + 1)),
                    show_text: item.show_text,
                });
            }
            BarItemKind::Command | BarItemKind::List => {
                let Some(id) = item.id.as_deref().map(str::trim).filter(|i| !i.is_empty()) else {
                    continue;
                };
                clean.push(BarItem {
                    kind: item.kind.clone(),
                    id: Some(id.to_string()),
                    caption,
                    items: None,
                    show_text: item.show_text,
                });
            }
            BarItemKind::Other(kind) => {
                if kind.trim().is_empty() {
                    continue;
                }
                clean.push(item.clone());
            }
        }
    }
    clean
}

fn same_toolbar(a: &ToolbarSpec, b: &ToolbarSpec) -> bool {
    a.show_text == b.show_text && same_items(&a.items, &b.items)
}

fn same_items(a: &[BarItem], b: &[BarItem]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.kind == y.kind
                && x.id == y.id
                && x.caption == y.caption
                && x.show_text == y.show_text
                && same_items(x.children(), y.children())
        })
}

fn present<S: BarSource + ?Sized>(
    items: &[BarItem],
    source: &S,
    access_keys: bool,
    show_text: bool,
    depth: usize,
) -> Vec<BarRow> {
    let rows: Vec<Option<BarRow>> = items
        .iter()
        .map(|item| match &item.kind {
            BarItemKind::Separator => Some(BarRow::plain(BarRowKind::Separator, None, "", None)),
            BarItemKind::Command => present_command(item, source, access_keys, show_text),
            BarItemKind::List => present_list(item, source),
            BarItemKind::Menu if depth <= CommandBarRules::MAX_DEPTH => {
                present_menu(item, source, access_keys, show_text, depth)
            }
            _ => None,
        })
        .collect();

    let slots: Vec<MenuSlot> = rows
        .iter()
        .map(|r| MenuSlot {
            is_separator: r.as_ref().is_some_and(|r| r.kind == BarRowKind::Separator),
            is_visible: r.is_some(),
        })
        .collect();
    let visible = MenuSeparatorRules::apply(&slots);
    let shown: Vec<BarRow> = rows
        .into_iter()
        .zip(visible)
        .filter_map(|(row, visible)| if visible { row } else { None })
        .collect();

    if access_keys {
        with_access_keys(shown)
    } else {
        shown
    }
}

fn present_command<S: BarSource + ?Sized>(
    item: &BarItem,
    source: &S,
    access_keys: bool,
    show_text: bool,
) -> Option<BarRow> {
    let id = item.id.as_deref()?;
    let state = source.command(id)?;

    // The words somebody chose for this spot win; then what the command calls itself at this
    // moment; then the menu's shorter name for it; then its own.
    let mut caption = match &item.caption {
        Some(caption) => caption.clone(),
        None => AccessKeys::escape(
            live(&state)
                .or_else(|| CommandBarCaptions::for_id(id))
                .unwrap_or(&state.name),
        ),
    };
    if !access_keys {
        caption = AccessKeys::strip(&caption);
    }

    Some(BarRow {
        kind: BarRowKind::Command,
        id: Some(id.to_string()),
        caption,
        name: state.name.clone(),
        icon: state.icon.clone(),
        gesture: state.gesture.clone(),
        is_enabled: state.unavailable.is_none(),
        reason: state.unavailable.clone(),
        is_checked: state.is_checked,
        show_text: item.show_text.unwrap_or(show_text),
        children: vec![],
    })
}

/// A caption for the moment, when there is one. Empty is none: "nothing to undo" has no
/// description, and a blank menu row would be worse than the command's own name.
fn live(state: &BarCommandState) -> Option<&str> {
    state.live_caption.as_deref().filter(|l| !l.is_empty())
}

fn present_list<S: BarSource + ?Sized>(item: &BarItem, source: &S) -> Option<BarRow> {
    let list = CommandBarLists::find(item.id.as_deref()?)?;
    if source.list_has_rows(list.id) {
        return Some(BarRow::plain(
            BarRowKind::List,
            Some(list.id),
            &AccessKeys::escape(list.name),
            Some(list.icon),
        ));
    }
    let empty = list.empty_text?;
    Some(BarRow {
        is_enabled: false,
        ..BarRow::plain(
            BarRowKind::Placeholder,
            Some(list.id),
            &AccessKeys::escape(empty),
            Some(list.icon),
        )
    })
}

fn present_menu<S: BarSource + ?Sized>(
    item: &BarItem,
    source: &S,
    access_keys: bool,
    show_text: bool,
    depth: usize,
) -> Option<BarRow> {
    // Inside a dropdown every row is a menu row, whatever the toolbar it hangs from wanted.
    let children = present(item.children(), source, true, false, depth + 1);
    if children.is_empty() {
        return None;
    }

    let caption = item.caption.as_deref().unwrap_or("");
    let caption = if access_keys {
        caption.to_string()
    } else {
        AccessKeys::strip(caption)
    };
    Some(BarRow {
        children,
        show_text: item.show_text.unwrap_or(show_text),
        ..BarRow::plain(BarRowKind::Menu, None, &caption, None)
    })
}

/// Keys for the rows people choose between. A list's rows are data and a placeholder cannot be
/// chosen, so neither takes a letter from the rest.
fn with_access_keys(mut rows: Vec<BarRow>) -> Vec<BarRow> {
    let keyed: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| matches!(r.kind, BarRowKind::Command | BarRowKind::Menu))
        .map(|(i, _)| i)
        .collect();
    let captions: Vec<&str> = keyed.iter().map(|&i| rows[i].caption.as_str()).collect();
    let assigned = AccessKeys::assign(&captions);
    for (i, caption) in keyed.into_iter().zip(assigned) {
        rows[i].caption = caption;
    }
    rows
}
