// Port of tests/BertBrowser.Core.Tests/ShellMenuRulesTests.cs: how raw registrations become the
// rows a user can tick, and how a handler's menu is tidied. Nothing here opens a key: the app
// reads, this decides.
use std::collections::HashSet;

use bb_core::shellmenu::{
    Guid, ShellExtension, ShellExtensionKind, ShellHandlerRegistration, ShellMenuEntry,
    ShellMenuRules, ShellVerbRegistration,
};

type Entry = ShellMenuEntry<i32, ()>;

fn seven_zip() -> Guid {
    Guid::parse("23170F69-40C1-278A-1000-000100020000").unwrap()
}

fn tortoise() -> Guid {
    Guid::parse("30351346-7B7D-4FCC-81B4-1E394CA267EB").unwrap()
}

fn nothing_blocked() -> HashSet<Guid> {
    HashSet::new()
}

fn handler(family: &str, clsid: Guid) -> ShellHandlerRegistration {
    named_handler(family, clsid, "7-Zip", None)
}

fn named_handler(
    family: &str,
    clsid: Guid,
    name: &str,
    class_name: Option<&str>,
) -> ShellHandlerRegistration {
    ShellHandlerRegistration {
        family: family.to_string(),
        name: name.to_string(),
        clsid,
        class_name: class_name.map(str::to_string),
    }
}

fn verb(family: &str, verb: &str, name: Option<&str>) -> ShellVerbRegistration {
    ShellVerbRegistration {
        family: family.to_string(),
        verb: verb.to_string(),
        display_name: name.map(str::to_string),
        icon: None,
        command: Some("\"C:\\tool.exe\" \"%1\"".to_string()),
        ..Default::default()
    }
}

fn with_command(mut registration: ShellVerbRegistration, command: &str) -> ShellVerbRegistration {
    registration.command = Some(command.to_string());
    registration
}

fn names(rows: &[ShellExtension]) -> Vec<&str> {
    rows.iter().map(|r| r.name.as_str()).collect()
}

fn single<T>(rows: Vec<T>) -> T {
    assert_eq!(1, rows.len());
    rows.into_iter().next().unwrap()
}

// --- merging ---

#[test]
fn a_handler_registered_under_four_families_is_one_row() {
    let rows = ShellMenuRules::catalog(
        &[
            handler("*", seven_zip()),
            handler("Directory", seven_zip()),
            handler("Folder", seven_zip()),
            handler("Drive", seven_zip()),
        ],
        &[],
        &nothing_blocked(),
    );

    let row = single(rows);
    assert_eq!("clsid:{23170f69-40c1-278a-1000-000100020000}", row.id);
    assert_eq!("7-Zip", row.name);
    assert_eq!(ShellExtensionKind::Handler, row.kind);
    assert_eq!(Some(seven_zip()), row.clsid);
    assert_eq!(vec!["*", "Directory", "Folder", "Drive"], row.families);
}

#[test]
fn a_verb_registered_for_items_and_background_is_one_row_carrying_both() {
    let rows = ShellMenuRules::catalog(
        &[],
        &[
            with_command(
                verb("Directory", "git_shell", Some("Open Git Bash here")),
                "\"C:\\Git\\git-bash.exe\" \"--cd=%1\"",
            ),
            with_command(
                verb(
                    r"Directory\Background",
                    "git_shell",
                    Some("Open Git Bash here"),
                ),
                "\"C:\\Git\\git-bash.exe\" \"--cd=%v.\"",
            ),
        ],
        &nothing_blocked(),
    );

    let row = single(rows);
    assert_eq!("verb:git_shell", row.id);
    assert_eq!("Open Git Bash here", row.name);
    assert_eq!(2, row.verbs.len());
    assert_eq!(vec!["Directory", r"Directory\Background"], row.families);
}

#[test]
fn verbs_come_before_handlers_each_in_registration_order() {
    let rows = ShellMenuRules::catalog(
        &[
            named_handler("*", tortoise(), "TortoiseSVN", None),
            named_handler("*", seven_zip(), "7-Zip", None),
        ],
        &[
            verb("*", "b_verb", Some("B")),
            verb("*", "a_verb", Some("A")),
        ],
        &nothing_blocked(),
    );

    assert_eq!(vec!["B", "A", "TortoiseSVN", "7-Zip"], names(&rows));
}

// --- naming ---

#[test]
fn a_handler_named_by_its_clsid_takes_the_class_name() {
    let rows = ShellMenuRules::catalog(
        &[named_handler(
            "*",
            seven_zip(),
            &seven_zip().to_braced(),
            Some("7-Zip Shell Extension"),
        )],
        &[],
        &nothing_blocked(),
    );

    assert_eq!("7-Zip Shell Extension", single(rows).name);
}

#[test]
fn a_handler_with_no_name_anywhere_shows_its_clsid() {
    let rows = ShellMenuRules::catalog(
        &[named_handler("*", seven_zip(), "", None)],
        &[],
        &nothing_blocked(),
    );

    assert_eq!("{23170F69-40C1-278A-1000-000100020000}", single(rows).name);
}

/// A canonical verb like `print` is registered lowercase and named nowhere; Explorer shows it from
/// its own string table, and this at least does not show it lowercase.
#[test]
fn a_verb_with_no_display_name_is_named_by_its_verb() {
    for (name, expected) in [("Sublime", "Sublime"), ("print", "Print")] {
        let rows = ShellMenuRules::catalog(&[], &[verb("*", name, None)], &nothing_blocked());

        assert_eq!(expected, single(rows).name, "{name}");
    }
}

#[test]
fn a_verb_takes_its_name_from_whichever_family_has_one() {
    let rows = ShellMenuRules::catalog(
        &[],
        &[
            verb("Directory", "x", None),
            verb(r"Directory\Background", "x", Some("Do X here")),
        ],
        &nothing_blocked(),
    );

    assert_eq!("Do X here", single(rows).name);
}

// --- what is never offered ---

#[test]
fn a_blocked_handler_is_dropped() {
    let rows = ShellMenuRules::catalog(
        &[
            handler("*", seven_zip()),
            named_handler("*", tortoise(), "TortoiseSVN", None),
        ],
        &[],
        &HashSet::from([tortoise()]),
    );

    assert_eq!("7-Zip", single(rows).name);
}

/// Open With, as Windows registers it under `*\shellex\ContextMenuHandlers`. The app has its own
/// Open; Explorer's stock items must not come back in through the side door.
#[test]
fn windows_own_handler_for_a_verb_the_app_has_is_dropped() {
    let open_with = Guid::parse("09799AFB-AD67-11D1-ABCD-00C04FC30936").unwrap();

    let rows = ShellMenuRules::catalog(
        &[
            named_handler("*", open_with, "Open With", None),
            handler("*", seven_zip()),
        ],
        &[],
        &nothing_blocked(),
    );

    assert_eq!("7-Zip", single(rows).name);
}

#[test]
fn a_stock_verb_the_app_already_has_is_dropped() {
    for name in [
        "open",
        "Open",
        "explore",
        "runas",
        "cmd",
        "Powershell",
        "printto",
    ] {
        assert!(
            ShellMenuRules::catalog(
                &[],
                &[verb("*", name, Some("Whatever"))],
                &nothing_blocked()
            )
            .is_empty(),
            "{name}"
        );
    }
}

#[test]
fn a_file_types_own_verbs_stay() {
    for name in ["edit", "print"] {
        assert_eq!(
            1,
            ShellMenuRules::catalog(
                &[],
                &[verb("txtfile", name, Some("Edit"))],
                &nothing_blocked()
            )
            .len(),
            "{name}"
        );
    }
}

#[test]
fn an_extended_verb_is_dropped() {
    let registration = ShellVerbRegistration {
        extended: true,
        ..verb("*", "x", Some("X"))
    };
    assert!(ShellMenuRules::catalog(&[], &[registration], &nothing_blocked()).is_empty());
}

#[test]
fn a_legacy_disabled_verb_is_dropped() {
    let registration = ShellVerbRegistration {
        legacy_disable: true,
        ..verb("*", "x", Some("X"))
    };
    assert!(ShellMenuRules::catalog(&[], &[registration], &nothing_blocked()).is_empty());
}

#[test]
fn a_programmatic_only_verb_is_dropped() {
    let registration = ShellVerbRegistration {
        programmatic_only: true,
        ..verb("*", "x", Some("X"))
    };
    assert!(ShellMenuRules::catalog(&[], &[registration], &nothing_blocked()).is_empty());
}

#[test]
fn a_delegate_execute_verb_is_dropped_because_its_command_line_is_not_what_runs() {
    let registration = ShellVerbRegistration {
        has_delegate_execute: true,
        ..verb("*", "x", Some("X"))
    };
    assert!(ShellMenuRules::catalog(&[], &[registration], &nothing_blocked()).is_empty());
}

#[test]
fn a_cascading_verb_is_dropped() {
    let registration = ShellVerbRegistration {
        has_sub_commands: true,
        ..verb("*", "x", Some("X"))
    };
    assert!(ShellMenuRules::catalog(&[], &[registration], &nothing_blocked()).is_empty());
}

#[test]
fn a_verb_with_no_command_is_dropped() {
    let registration = ShellVerbRegistration {
        command: None,
        ..verb("*", "x", Some("X"))
    };
    assert!(ShellMenuRules::catalog(&[], &[registration], &nothing_blocked()).is_empty());
}

// --- the user's choice ---

#[test]
fn the_master_switch_off_hides_everything() {
    let catalog = ShellMenuRules::catalog(&[handler("*", seven_zip())], &[], &nothing_blocked());

    assert!(ShellMenuRules::visible(&catalog, false, Vec::<String>::new()).is_empty());
}

#[test]
fn a_hidden_id_is_left_out_case_insensitively() {
    let catalog = ShellMenuRules::catalog(
        &[
            handler("*", seven_zip()),
            named_handler("*", tortoise(), "TortoiseSVN", None),
        ],
        &[verb("*", "git_shell", Some("Git"))],
        &nothing_blocked(),
    );

    let visible = ShellMenuRules::visible(
        &catalog,
        true,
        [
            "VERB:GIT_SHELL".to_string(),
            ShellExtension::handler_id(tortoise()),
        ],
    );

    assert_eq!("7-Zip", single(visible).name);
}

#[test]
fn saving_keeps_hidden_ids_that_were_not_found_this_time() {
    let hidden = ShellMenuRules::hidden_after_save(
        ["verb:uninstalled", "verb:git_shell"],
        &[("verb:git_shell", true), ("clsid:{x}", false)],
    );

    assert_eq!(vec!["verb:uninstalled", "clsid:{x}"], hidden);
}

// --- menu text ---

#[test]
fn a_win32_menu_string_becomes_a_wpf_header() {
    for (win32, expected) in [
        (Some("&Add to archive..."), "_Add to archive..."),
        (Some("Tom && Jerry"), "Tom & Jerry"),
        (Some("snake_case"), "snake__case"),
        (Some("Extract\tCtrl+E"), "Extract"),
        (Some(""), ""),
        (None, ""),
    ] {
        assert_eq!(expected, ShellMenuRules::header(win32), "{win32:?}");
    }
}

// --- tidying a handler's menu ---

#[test]
fn separators_at_the_ends_and_doubled_up_are_collapsed() {
    let tidy = ShellMenuRules::tidy(vec![
        Entry::separator(),
        Entry::item("A", 1),
        Entry::separator(),
        Entry::separator(),
        Entry::item("B", 2),
        Entry::separator(),
    ]);

    assert_eq!(
        vec!["A", "", "B"],
        tidy.iter().map(|e| e.header.as_str()).collect::<Vec<_>>()
    );
    assert!(tidy[1].is_separator);
}

#[test]
fn an_empty_submenu_and_a_nameless_item_are_dropped() {
    let tidy = ShellMenuRules::tidy(vec![
        Entry::submenu("Empty", vec![]),
        Entry::submenu("Only separators", vec![Entry::separator()]),
        Entry::item("", 7),
        Entry::item("Kept", 8),
    ]);

    assert_eq!("Kept", single(tidy).header);
}

#[test]
fn a_submenu_is_tidied_inside() {
    let tidy = ShellMenuRules::tidy(vec![Entry::submenu(
        "7-Zip",
        vec![
            Entry::separator(),
            Entry::item("Extract", 1),
            Entry::separator(),
        ],
    )]);

    let submenu = single(tidy);
    assert_eq!("Extract", single(submenu.children).header);
}
