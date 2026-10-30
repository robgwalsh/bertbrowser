// Port of tests/BertBrowser.Core.Tests/CommandBarEditTests.cs.
use std::borrow::Cow;

use bb_core::commandbars::{
    AccessKeys, BarItem, BarTarget, CommandBarEdit, CommandBarRules, CommandBarSet, ToolbarSpec,
};

fn c(id: &str) -> BarItem {
    BarItem::command(id)
}

/// File[ a, Sub[ b, c ], d ] · Edit[ e ] — title bar: t1 t2 — one row: r1.
fn set() -> CommandBarSet {
    CommandBarSet {
        revision: 1,
        menus: vec![
            BarItem::menu(
                "&File",
                vec![c("a"), BarItem::menu("Sub", vec![c("b"), c("c")]), c("d")],
            ),
            BarItem::menu("&Edit", vec![c("e")]),
        ],
        title_bar: ToolbarSpec::new(vec![c("t1"), c("t2")]),
        rows: vec![ToolbarSpec::new(vec![c("r1")])],
    }
}

fn ids(items: &[BarItem]) -> String {
    items
        .iter()
        .map(|i| {
            if i.is_separator() {
                "|".to_string()
            } else {
                i.id.clone()
                    .unwrap_or_else(|| AccessKeys::strip(i.caption.as_deref().unwrap_or("?")))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `Assert.Same`: the edit handed the arrangement back untouched.
fn unchanged(result: Cow<'_, CommandBarSet>) -> bool {
    matches!(result, Cow::Borrowed(_))
}

#[test]
fn a_place_is_a_bar_and_a_path_down_through_its_submenus() {
    let set = set();

    assert_eq!(
        "File Edit",
        ids(CommandBarEdit::items_at(&set, BarTarget::MENU_BAR, &[]))
    );
    assert_eq!(
        "a Sub d",
        ids(CommandBarEdit::items_at(&set, BarTarget::MENU_BAR, &[0]))
    );
    assert_eq!(
        "b c",
        ids(CommandBarEdit::items_at(&set, BarTarget::MENU_BAR, &[0, 1]))
    );
    assert_eq!(
        "t1 t2",
        ids(CommandBarEdit::items_at(&set, BarTarget::TITLE_BAR, &[]))
    );
    assert_eq!(
        "r1",
        ids(CommandBarEdit::items_at(&set, BarTarget::for_row(0), &[]))
    );

    assert_eq!(
        vec!["Menu bar", "File", "Sub"],
        CommandBarEdit::trail(&set, BarTarget::MENU_BAR, &[0, 1])
    );
    assert_eq!(
        vec!["Toolbar row 1"],
        CommandBarEdit::trail(&set, BarTarget::for_row(0), &[])
    );
}

#[test]
fn a_place_that_is_not_there_is_empty_and_an_edit_aimed_at_it_changes_nothing() {
    let set = set();

    // Through a command, past the end, and a row that does not exist.
    assert!(CommandBarEdit::items_at(&set, BarTarget::MENU_BAR, &[0, 0]).is_empty());
    assert!(!CommandBarEdit::exists(&set, BarTarget::MENU_BAR, &[7]));
    assert!(!CommandBarEdit::exists(&set, BarTarget::for_row(3), &[]));

    assert!(unchanged(CommandBarEdit::insert(
        &set,
        BarTarget::MENU_BAR,
        &[0, 0],
        0,
        c("x")
    )));
    assert!(unchanged(CommandBarEdit::insert(
        &set,
        BarTarget::for_row(3),
        &[],
        0,
        c("x")
    )));
    assert!(unchanged(CommandBarEdit::remove(
        &set,
        BarTarget::TITLE_BAR,
        &[],
        9
    )));
    assert!(unchanged(CommandBarEdit::move_item(
        &set,
        BarTarget::TITLE_BAR,
        &[],
        9,
        0
    )));
}

#[test]
fn an_insert_lands_where_asked_and_leaves_every_other_bar_alone() {
    let set = set();

    let edited =
        CommandBarEdit::insert(&set, BarTarget::MENU_BAR, &[0, 1], 1, BarItem::separator());

    assert_eq!(
        "b | c",
        ids(CommandBarEdit::items_at(
            &edited,
            BarTarget::MENU_BAR,
            &[0, 1]
        ))
    );
    assert_eq!(
        "a Sub d",
        ids(CommandBarEdit::items_at(&edited, BarTarget::MENU_BAR, &[0]))
    );
    assert_eq!(set.menus[1], edited.menus[1]);
    assert_eq!(set.title_bar, edited.title_bar);
    // Past the end is the end.
    assert_eq!(
        "t1 t2 x",
        ids(CommandBarEdit::items_at(
            &CommandBarEdit::insert(&set, BarTarget::TITLE_BAR, &[], 99, c("x")),
            BarTarget::TITLE_BAR,
            &[]
        ))
    );
}

#[test]
fn the_menu_bar_itself_takes_menus_only() {
    let set = set();

    assert!(unchanged(CommandBarEdit::insert(
        &set,
        BarTarget::MENU_BAR,
        &[],
        0,
        c("x")
    )));
    assert!(unchanged(CommandBarEdit::insert(
        &set,
        BarTarget::MENU_BAR,
        &[],
        0,
        BarItem::separator()
    )));

    let edited = CommandBarEdit::insert(
        &set,
        BarTarget::MENU_BAR,
        &[],
        1,
        BarItem::menu("View", vec![]),
    );
    assert_eq!("File View Edit", ids(&edited.menus));
    // A toolbar takes anything, a submenu included.
    assert_eq!(
        "Drop t1 t2",
        ids(&CommandBarEdit::insert(
            &set,
            BarTarget::TITLE_BAR,
            &[],
            0,
            BarItem::menu("Drop", vec![])
        )
        .title_bar
        .items)
    );
}

#[test]
fn a_submenu_cannot_be_added_past_the_depth_limit() {
    let mut set = set();
    let mut path: Vec<usize> = Vec::new();
    for depth in 1..=CommandBarRules::MAX_DEPTH {
        set = CommandBarEdit::insert(
            &set,
            BarTarget::TITLE_BAR,
            &path,
            0,
            BarItem::menu(&format!("m{depth}"), vec![]),
        )
        .into_owned();
        path.push(0);
    }

    assert!(unchanged(CommandBarEdit::insert(
        &set,
        BarTarget::TITLE_BAR,
        &path,
        0,
        BarItem::menu("too deep", vec![])
    )));
    assert!(!unchanged(CommandBarEdit::insert(
        &set,
        BarTarget::TITLE_BAR,
        &path,
        0,
        c("fine")
    )));
    assert!(CommandBarRules::same(
        &set,
        &CommandBarRules::clean(Some(&set))
    ));
}

#[test]
fn move_remove_and_replace_act_on_one_item() {
    let set = set();

    assert_eq!(
        "d a Sub",
        ids(CommandBarEdit::items_at(
            &CommandBarEdit::move_item(&set, BarTarget::MENU_BAR, &[0], 2, 0),
            BarTarget::MENU_BAR,
            &[0]
        ))
    );
    assert_eq!(
        "Sub d a",
        ids(CommandBarEdit::items_at(
            &CommandBarEdit::move_item(&set, BarTarget::MENU_BAR, &[0], 0, 2),
            BarTarget::MENU_BAR,
            &[0]
        ))
    );
    assert_eq!(
        "a d",
        ids(CommandBarEdit::items_at(
            &CommandBarEdit::remove(&set, BarTarget::MENU_BAR, &[0], 1),
            BarTarget::MENU_BAR,
            &[0]
        ))
    );

    let renamed = CommandBarEdit::replace(
        &set,
        BarTarget::MENU_BAR,
        &[],
        1,
        BarItem {
            caption: Some("E&dit".into()),
            ..set.menus[1].clone()
        },
    );
    assert_eq!(Some("E&dit"), renamed.menus[1].caption.as_deref());
    assert_eq!("e", ids(renamed.menus[1].children()));
}

#[test]
fn rows_are_added_empty_and_removed_whole() {
    let set = set();
    let added = CommandBarEdit::add_row(&set);

    assert_eq!(2, added.rows.len());
    assert!(added.rows[1].items.is_empty());
    let one = CommandBarEdit::remove_row(&added, 1);
    assert_eq!(1, one.rows.len());
    assert_eq!("r1", ids(&one.rows[0].items));
    assert!(CommandBarEdit::remove_row(&set, 0).rows.is_empty());
    assert!(unchanged(CommandBarEdit::remove_row(&set, 5)));
}

#[test]
fn labels_are_a_toolbar_s_own_setting() {
    let set = set();
    let labelled = CommandBarEdit::with_show_text(&set, BarTarget::for_row(0), true);

    assert!(CommandBarEdit::shows_text(&labelled, BarTarget::for_row(0)));
    assert!(!CommandBarEdit::shows_text(&labelled, BarTarget::TITLE_BAR));
    assert!(unchanged(CommandBarEdit::with_show_text(
        &set,
        BarTarget::MENU_BAR,
        true
    )));
}
