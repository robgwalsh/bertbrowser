// Port of tests/BertBrowser.Core.Tests/CommandBarRulesTests.cs.
use std::collections::{HashMap, HashSet};

use bb_core::commandbars::{
    AccessKeys, BarCommandState, BarItem, BarItemKind, BarRow, BarRowKind, BarSource,
    CommandBarCaptions, CommandBarDefaults, CommandBarLists, CommandBarRules, CommandBarSet,
    ToolbarSpec,
};
use bb_core::commands::CommandCatalog;

/// A source that knows the catalogue, with whatever the test says is unavailable, ticked or listed.
#[derive(Default)]
struct Source {
    unavailable: HashMap<String, String>,
    checked: HashSet<String>,
    lists: HashSet<String>,
    live: HashMap<String, String>,
}

impl BarSource for Source {
    fn command(&self, id: &str) -> Option<BarCommandState> {
        CommandCatalog::find(id).map(|c| BarCommandState {
            name: c.name.to_string(),
            icon: c.icon.map(str::to_string),
            gesture: String::new(),
            unavailable: self.unavailable.get(id).cloned(),
            is_checked: self.checked.contains(id),
            live_caption: self.live.get(id).cloned(),
        })
    }

    fn list_has_rows(&self, list_id: &str) -> bool {
        self.lists.contains(list_id)
    }
}

fn strings(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn map(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn show(rows: &[BarRow]) -> String {
    rows.iter()
        .map(|r| match r.kind {
            BarRowKind::Separator => "|".to_string(),
            BarRowKind::Menu => {
                format!("{}[{}]", AccessKeys::strip(&r.caption), show(&r.children))
            }
            BarRowKind::List => format!("@{}", r.id.as_deref().unwrap_or("")),
            BarRowKind::Placeholder => format!("({})", r.caption),
            BarRowKind::Command => r.id.clone().unwrap_or_default(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn c(id: &str) -> BarItem {
    BarItem::command(id)
}

fn cc(id: &str, caption: &str) -> BarItem {
    BarItem::command_with_caption(id, caption)
}

fn menu(items: &[BarItem], source: &Source) -> Vec<BarRow> {
    CommandBarRules::present(items, source, true, false)
}

fn toolbar(items: &[BarItem], source: &Source) -> Vec<BarRow> {
    CommandBarRules::present(items, source, false, false)
}

fn single(rows: Vec<BarRow>) -> BarRow {
    assert_eq!(1, rows.len(), "{}", show(&rows));
    rows.into_iter().next().unwrap()
}

// --- The default ---

#[test]
fn every_command_and_list_in_the_default_exists() {
    let items = CommandBarRules::flatten(CommandBarDefaults::default_set());

    for i in items.iter().filter(|i| i.is_command()) {
        assert!(
            CommandCatalog::is_known(i.id.as_deref().unwrap()),
            "{:?}",
            i.id
        );
    }
    for i in items.iter().filter(|i| i.is_list()) {
        assert!(
            CommandBarLists::find(i.id.as_deref().unwrap()).is_some(),
            "{:?}",
            i.id
        );
    }
}

#[test]
fn the_default_has_no_separator_doubled_or_at_an_edge() {
    let default = CommandBarDefaults::default_set();
    let mut menus: Vec<&[BarItem]> = CommandBarRules::flatten(default)
        .into_iter()
        .filter(|i| i.is_menu())
        .map(BarItem::children)
        .collect();
    menus.push(&default.title_bar.items);

    for items in menus {
        assert!(!items[0].is_separator());
        assert!(!items[items.len() - 1].is_separator());
        assert!(
            !items
                .windows(2)
                .any(|pair| pair[0].is_separator() && pair[1].is_separator())
        );
    }
}

#[test]
fn top_level_access_keys_are_distinct_and_clear_of_shipped_alt_chords() {
    let keys: Vec<Option<char>> = CommandBarDefaults::default_set()
        .menus
        .iter()
        .map(|m| AccessKeys::key_of(m.caption.as_deref().unwrap()))
        .collect();

    assert!(!keys.contains(&None));
    let distinct: HashSet<Option<char>> = keys.iter().copied().collect();
    assert_eq!(keys.len(), distinct.len());

    // Alt+D, Alt+P, Alt+M today — and whatever is added later.
    let claimed: Vec<Option<char>> = CommandCatalog::all()
        .iter()
        .flat_map(|c| c.default_chords.iter())
        .map(ToString::to_string)
        .filter(|c| c.starts_with("Alt+") && c.len() == 5)
        .map(|c| c.chars().nth(4))
        .collect();
    assert!(
        keys.iter().all(|k| !claimed.contains(k)),
        "{keys:?} vs {claimed:?}"
    );
}

#[test]
fn the_default_toolbar_is_the_buttons_the_title_bar_always_had() {
    assert_eq!(
        "edit.undo edit.redo | tools.disk-usage tools.duplicates tools.changes tools.compare app.settings",
        show(&toolbar(
            &CommandBarDefaults::default_set().title_bar.items,
            &Source::default()
        ))
    );
    assert!(CommandBarDefaults::default_set().rows.is_empty());
}

#[test]
fn the_default_is_still_the_default_after_a_trip_through_the_settings_file() {
    let json = serde_json::to_string(CommandBarDefaults::default_set()).unwrap();
    let read: CommandBarSet = serde_json::from_str(&json).unwrap();
    let back = CommandBarRules::clean(Some(&read));

    assert!(CommandBarRules::is_default(&back));
    assert!(!json.contains("null"));
}

#[test]
fn every_short_caption_belongs_to_a_command() {
    for id in CommandBarCaptions::ids() {
        assert!(CommandCatalog::is_known(id), "{id}");
    }
}

// --- Clean ---

#[test]
fn nothing_stored_means_the_default() {
    assert_eq!(
        CommandBarDefaults::default_set(),
        &CommandBarRules::clean(None)
    );
}

#[test]
fn a_file_with_holes_in_it_is_made_walkable() {
    let holed: CommandBarSet = serde_json::from_str(
        r#"{"menus":[{"kind":"menu","caption":" &File ","items":[{"kind":"command"},null,{"kind":"command","id":" tab.new "},{}]},{"kind":"menu"},{"kind":"command","id":"tab.new"}]}"#,
    )
    .unwrap();

    let clean = CommandBarRules::clean(Some(&holed));

    assert_eq!(1, clean.menus.len());
    let file = &clean.menus[0];
    assert_eq!(Some("&File"), file.caption.as_deref());
    assert_eq!(1, file.children().len());
    assert_eq!(Some("tab.new"), file.children()[0].id.as_deref());
    assert!(clean.title_bar.items.is_empty());
    assert!(clean.rows.is_empty());
}

#[test]
fn a_command_or_kind_this_build_does_not_know_is_kept() {
    let set = CommandBarSet {
        revision: 9,
        menus: vec![BarItem::menu(
            "Later",
            vec![
                c("from.a-later-build"),
                BarItem {
                    kind: BarItemKind::Other("gauge".into()),
                    id: Some("disk".into()),
                    ..Default::default()
                },
            ],
        )],
        title_bar: ToolbarSpec::new(vec![]),
        rows: vec![],
    };

    let clean = CommandBarRules::clean(Some(&set));

    assert!(CommandBarRules::same(&set, &clean));
    assert_eq!(9, clean.revision);
}

#[test]
fn menus_stop_nesting_at_the_limit() {
    let deep = BarItem::menu(
        "1",
        vec![BarItem::menu(
            "2",
            vec![BarItem::menu(
                "3",
                vec![BarItem::menu(
                    "4",
                    vec![BarItem::menu("5", vec![c("tab.new")])],
                )],
            )],
        )],
    );

    let clean = CommandBarRules::clean(Some(&CommandBarSet {
        revision: 1,
        menus: vec![deep],
        title_bar: ToolbarSpec::new(vec![]),
        rows: vec![],
    }));

    assert_eq!(
        vec![Some("1"), Some("2"), Some("3"), Some("4")],
        CommandBarRules::flatten_items(&clean.menus)
            .iter()
            .map(|i| i.caption.as_deref())
            .collect::<Vec<_>>()
    );
}

// --- Same / Unplaced ---

#[test]
fn a_moved_item_a_new_caption_or_a_label_switch_is_a_different_arrangement() {
    let d = CommandBarDefaults::default_set();

    assert!(CommandBarRules::is_default(&CommandBarSet {
        revision: 99,
        ..d.clone()
    }));
    assert!(!CommandBarRules::is_default(&CommandBarSet {
        menus: d.menus.iter().rev().cloned().collect(),
        ..d.clone()
    }));
    assert!(!CommandBarRules::is_default(&CommandBarSet {
        title_bar: ToolbarSpec {
            show_text: true,
            ..d.title_bar.clone()
        },
        ..d.clone()
    }));
    assert!(!CommandBarRules::is_default(&CommandBarSet {
        rows: vec![ToolbarSpec::new(vec![])],
        ..d.clone()
    }));
    let mut recaptioned = d.clone();
    recaptioned.menus[0].caption = Some("F&ile".into());
    assert!(!CommandBarRules::is_default(&recaptioned));
}

#[test]
fn unplaced_lists_what_is_on_no_menu_and_no_toolbar() {
    let set = CommandBarSet {
        revision: 1,
        menus: vec![BarItem::menu(
            "A",
            vec![BarItem::menu("B", vec![c("tab.new")])],
        )],
        title_bar: ToolbarSpec::new(vec![c("tab.close")]),
        rows: vec![ToolbarSpec::new(vec![c("nav.up")])],
    };

    assert_eq!(
        vec!["nav.back", "custom:x"],
        CommandBarRules::unplaced(
            &set,
            ["tab.new", "nav.back", "tab.close", "nav.up", "custom:x"]
        )
    );
}

#[test]
fn the_default_leaves_out_only_what_the_palette_is_the_place_for() {
    let unplaced = CommandBarRules::unplaced(
        CommandBarDefaults::default_set(),
        CommandCatalog::all().iter().map(|c| c.id),
    );

    for id in unplaced {
        assert!(
            id.starts_with("tab.goto-")
                || id.starts_with("tools.indexer-")
                || matches!(
                    id.as_str(),
                    "view.side-pane"
                        | "view.toggle-thumbnails"
                        | "search.stop"
                        | "edit.clear-undo-history"
                        | "app.match-windows-theme"
                ),
            "{id}"
        );
    }
}

// --- Present ---

#[test]
fn an_unavailable_command_stays_and_says_why() {
    let source = Source {
        unavailable: map(&[("nav.back", "There is nowhere to go back to.")]),
        ..Default::default()
    };

    let row = single(menu(&[c("nav.back")], &source));

    assert!(!row.is_enabled);
    assert_eq!(
        Some("There is nowhere to go back to."),
        row.reason.as_deref()
    );
}

#[test]
fn what_nothing_answers_to_goes_and_takes_its_separators_with_it() {
    let rows = menu(
        &[
            BarItem::separator(),
            c("tab.new"),
            BarItem::separator(),
            c("custom:deleted"),
            BarItem::separator(),
            BarItem::list(CommandBarLists::DRIVES),
            BarItem::separator(),
            c("tab.close"),
            BarItem::separator(),
        ],
        &Source::default(),
    );

    assert_eq!("tab.new | tab.close", show(&rows));
}

#[test]
fn an_empty_list_leaves_its_line_or_nothing_and_an_empty_submenu_goes() {
    let items = [
        BarItem::menu(
            "Workspaces",
            vec![BarItem::list(CommandBarLists::WORKSPACES)],
        ),
        BarItem::menu(
            "Your commands",
            vec![BarItem::list(CommandBarLists::CUSTOM_COMMANDS)],
        ),
        BarItem::menu("Nothing", vec![BarItem::separator()]),
    ];

    assert_eq!(
        "Workspaces[(No saved workspaces)]",
        show(&menu(&items, &Source::default()))
    );
    assert_eq!(
        "Workspaces[@workspaces] Your commands[@custom-commands]",
        show(&menu(
            &items,
            &Source {
                lists: strings(&[
                    CommandBarLists::WORKSPACES,
                    CommandBarLists::CUSTOM_COMMANDS
                ]),
                ..Default::default()
            }
        ))
    );
}

#[test]
fn a_caption_is_the_spot_s_own_then_the_moment_s_then_the_menu_s_then_the_name() {
    let source = Source {
        live: map(&[("file.delete", "Delete 3 items\u{2026}")]),
        ..Default::default()
    };

    let rows = toolbar(
        &[
            cc("view.sort-name", "Name"),
            c("file.delete"),
            c("view.preview"),
            c("tab.new"),
        ],
        &source,
    );

    assert_eq!(
        vec!["Name", "Delete 3 items\u{2026}", "Preview pane", "New tab"],
        rows.iter().map(|r| r.caption.as_str()).collect::<Vec<_>>()
    );
    assert_eq!("Toggle preview pane", rows[2].name);
}

#[test]
fn an_empty_caption_for_the_moment_is_no_caption() {
    // What Undo describes when there is nothing to undo.
    let source = Source {
        live: map(&[("edit.undo", "")]),
        ..Default::default()
    };

    assert_eq!("Undo", single(toolbar(&[c("edit.undo")], &source)).caption);
}

#[test]
fn a_menu_gives_each_row_a_key_and_a_toolbar_gives_none() {
    let items = [
        c("edit.cut"),
        c("edit.copy"),
        c("edit.copy-path"),
        BarItem::menu("&Select", vec![c("select.all")]),
    ];
    let source = Source {
        checked: strings(&["edit.cut"]),
        ..Default::default()
    };

    let menu = menu(&items, &source);
    let toolbar = CommandBarRules::present(&items, &source, false, true);

    assert_eq!(
        vec!["&Cut", "C&opy", "Copy &as path", "&Select"],
        menu.iter().map(|r| r.caption.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        vec!["Cut", "Copy", "Copy as path", "Select"],
        toolbar
            .iter()
            .map(|r| r.caption.as_str())
            .collect::<Vec<_>>()
    );
    assert!(menu[0].is_checked);
    assert!(toolbar.iter().all(|r| r.show_text));
    // Under a toolbar's dropdown the rows are menu rows again.
    assert_eq!("&Select all", toolbar[3].children[0].caption);
}

#[test]
fn a_name_from_data_cannot_mark_a_key() {
    let source = Source {
        live: map(&[("tab.new", "R&D")]),
        ..Default::default()
    };

    let row = single(menu(&[c("tab.new")], &source));

    assert_eq!("&R&&D", row.caption);
    assert_eq!("_R&D", AccessKeys::to_underscored(&row.caption));
}

// --- Access keys ---

#[test]
fn a_marked_key_is_kept_and_the_rest_take_what_is_free() {
    assert_eq!(
        vec!["&Date modified", "D&ate created", "Da&te", "&Name", "&&"],
        AccessKeys::assign(&["Date modified", "D&ate created", "Date", "&Name", "&&"])
    );
    // Every letter gone: it goes without rather than share.
    assert_eq!(vec!["&A", "A"], AccessKeys::assign(&["A", "A"]));
}

#[test]
fn underscores_survive_the_translation_to_wpf() {
    assert_eq!("_File", AccessKeys::to_underscored("&File"));
    assert_eq!("my__file _x", AccessKeys::to_underscored("my_file &x"));
    assert_eq!("Cut & paste", AccessKeys::to_underscored("Cut && paste"));
    assert_eq!("Cut & paste", AccessKeys::strip("Cut && paste"));
}

// --- Toolbars ---

#[test]
fn undo_and_redo_carry_their_lists() {
    assert_eq!(
        Some(CommandBarLists::UNDO_STEPS),
        CommandBarRules::dropdown_for("edit.undo")
    );
    assert_eq!(
        Some(CommandBarLists::REDO_STEPS),
        CommandBarRules::dropdown_for("edit.redo")
    );
    assert_eq!(None, CommandBarRules::dropdown_for("edit.cut"));
}

#[test]
fn a_toolbar_shows_what_fits_and_leaves_room_for_the_overflow_button() {
    let widths = [30.0, 30.0, 30.0, 30.0];

    assert_eq!(4, CommandBarRules::fit(&widths, 120.0, 20.0));
    assert_eq!(3, CommandBarRules::fit(&widths, 119.0, 20.0));
    assert_eq!(2, CommandBarRules::fit(&widths, 100.0, 20.0));
    assert_eq!(0, CommandBarRules::fit(&widths, 10.0, 20.0));
    assert_eq!(0, CommandBarRules::fit(&[], 0.0, 20.0));
}
