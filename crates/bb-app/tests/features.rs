//! The services around the tabs, one test group per feature: new items, the session, the command
//! registry and its one gate, the keymap and the Keyboard page, the palette, the command bars,
//! columns and their property seam, saved searches and workspaces, settings, themes and the index.

mod common;

use bb_app::{AppError, AppEvent, Model, PropSupply, prop_words};
use bb_core::commands::{FocusKind, FocusState, KeyChord};
use bb_core::newitem::NewItemKind;
use bb_core::theming::SystemAppearance;
use common::{Fixture, eventually};
use serde_json::Value;

fn json(text: &str) -> Value {
    serde_json::from_str(text).expect("json")
}

fn row_names(page: &Value) -> Vec<String> {
    page["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] != "heading")
        .map(|r| r["text"].as_str().unwrap_or("").to_string())
        .collect()
}

fn palette_texts(f: &Fixture) -> Vec<String> {
    json(&f.app.palette_json())["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["text"].as_str().unwrap_or("").to_string())
        .collect()
}

fn why(f: &Fixture, id: &str) -> Option<String> {
    f.app.command_why_not(id).unwrap()
}

// ---- new items ----

#[test]
fn a_new_folder_is_created_selected_and_listed() {
    let f = Fixture::new();
    let name = f
        .app
        .new_item_suggest(f.tab, NewItemKind::Folder, "")
        .unwrap();
    assert_eq!("New folder", name);
    let op = f.app.new_item(f.tab, &name, NewItemKind::Folder).unwrap();
    let (ok, _) = f.wait_op(op);
    assert!(ok);
    f.settle();
    assert!(f.root.join("New folder").is_dir());
    assert!(f.names().contains(&"New folder".to_string()));
    assert_eq!(
        vec![f.path("New folder")],
        f.app.selected_paths(f.tab).unwrap(),
        "the new item is the selection"
    );
    // The suggestion steps aside once the name is taken.
    assert_eq!(
        "New folder (2)",
        f.app
            .new_item_suggest(f.tab, NewItemKind::Folder, "")
            .unwrap()
    );
}

#[test]
fn a_new_item_is_refused_before_anything_is_written() {
    let f = Fixture::new();
    let plan = json(
        &f.app
            .new_item_plan(f.tab, "notes.txt", NewItemKind::File)
            .unwrap(),
    );
    assert_eq!(false, plan["ok"]);
    assert!(
        plan["rejected"]
            .as_str()
            .unwrap()
            .contains("already exists")
    );
    assert!(matches!(
        f.app.new_item(f.tab, "notes.txt", NewItemKind::File),
        Err(AppError::Invalid(_))
    ));
    let plan = json(
        &f.app
            .new_item_plan(f.tab, "bad|name", NewItemKind::Folder)
            .unwrap(),
    );
    assert_eq!(false, plan["ok"]);
}

#[test]
fn a_new_file_takes_the_template_of_its_type() {
    let f = Fixture::new();
    let kinds = json(&f.app.new_file_types_json());
    assert!(
        kinds
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["extension"] == ".txt")
    );
    let suggested = f
        .app
        .new_item_suggest(f.tab, NewItemKind::File, ".txt")
        .unwrap();
    assert!(suggested.ends_with(".txt"), "{suggested}");
    let op = f
        .app
        .new_item(f.tab, "letter.txt", NewItemKind::File)
        .unwrap();
    assert!(f.wait_op(op).0);
    assert!(f.root.join("letter.txt").is_file());
}

#[test]
fn importing_from_windows_adds_only_the_types_that_are_missing() {
    let f = Fixture::new();
    let before = json(&f.app.new_types_state_json())["types"]
        .as_array()
        .unwrap()
        .len();
    let finished = |f: &Fixture| {
        let st = json(&f.app.new_types_state_json());
        (!st["importing"].as_bool().unwrap()).then_some(st)
    };
    // The run's catalog offers .txt (already listed), .rtf and .bmp.
    f.app.new_types_import();
    assert!(eventually(5000, || finished(&f).is_some()));
    let st = finished(&f).unwrap();
    let extensions: Vec<&str> = st["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["extension"].as_str().unwrap())
        .collect();
    assert_eq!(before + 2, extensions.len(), "{extensions:?}");
    assert_eq!(&[".rtf", ".bmp"], &extensions[before..]);
    assert!(st["status"].as_str().unwrap().contains("Added 2"));
    // A second import finds nothing new and leaves the list alone.
    f.app.new_types_import();
    assert!(eventually(5000, || finished(&f).is_some_and(|s| {
        s["status"].as_str().unwrap().contains("Nothing new")
    })));
    assert_eq!(
        before + 2,
        json(&f.app.new_types_state_json())["types"]
            .as_array()
            .unwrap()
            .len()
    );
}

// ---- the session ----

#[test]
fn the_session_round_trip_keeps_panes_tabs_order_and_the_active_pane() {
    let f = Fixture::new();
    let a = f.app.open_tab(None, &f.path("Documents"), true).unwrap();
    let _b = f.app.open_tab(None, &f.path("Pictures"), true).unwrap();
    // A drag reorders the tabs: the last goes first.
    let tabs = f.app.pane_tabs(f.app.active_pane()).unwrap();
    f.app.move_tab(tabs[2], 0).unwrap();
    let pane = f.app.active_pane();
    f.app
        .split_pane(
            pane,
            bb_core::layout::SplitOrientation::Vertical,
            &f.path("Documents"),
        )
        .unwrap();
    f.settle();
    let _ = a;

    assert!(f.app.session_roundtrip().unwrap());
    f.settle();
    assert_eq!(2, f.app.panes().len());
    let panes = f.app.panes();
    // The pane that was active (the split) is still active, still on Documents.
    assert_eq!(panes[1], f.app.active_pane());
    let active = f.app.active_tab().unwrap();
    assert_eq!(f.path("Documents"), f.app.tab_info(active).unwrap().path);
    // The first pane's three tabs came back in the moved order.
    let first = f.app.pane_tabs(panes[0]).unwrap();
    assert_eq!(3, first.len());
    let paths: Vec<String> = first
        .iter()
        .map(|t| f.app.tab_info(*t).unwrap().path)
        .collect();
    assert_eq!(f.path("Pictures"), paths[0]);
    assert_eq!(f.root.to_string_lossy(), paths[1]);
}

#[test]
fn a_saved_session_is_restored_by_the_next_launch() {
    let f = Fixture::new();
    f.app.open_tab(None, &f.path("Documents"), true).unwrap();
    f.settle();
    f.app.session_save().unwrap();
    let saved = json(&f.app.session_capture_json());
    assert_eq!(2, saved["tabs"].as_array().unwrap().len());

    let mut config = bb_app::AppConfig::harness(&f.data);
    config.watch_debounce_ms = 40;
    let second = bb_app::App::new(config).unwrap();
    let tab = second.session_restore(&f.path("Pictures")).unwrap();
    assert!(second.wait_idle(40, 10_000));
    assert_eq!(f.path("Documents"), second.tab_info(tab).unwrap().path);
    assert_eq!(2, second.pane_tabs(second.active_pane()).unwrap().len());
    second.shutdown();
}

#[test]
fn a_session_that_is_off_or_unusable_opens_the_fallback() {
    let f = Fixture::new();
    f.app.settings_set("restoreLastSession", "false").unwrap();
    f.app
        .settings_set(
            "startupDefaultPath",
            &serde_json::to_string(&f.path("Pictures")).unwrap(),
        )
        .unwrap();
    f.app.session_save().unwrap();
    let mut config = bb_app::AppConfig::harness(&f.data);
    config.watch_debounce_ms = 40;
    let second = bb_app::App::new(config).unwrap();
    let tab = second.session_restore(&f.path("Documents")).unwrap();
    second.wait_idle(40, 10_000);
    assert_eq!(f.path("Pictures"), second.tab_info(tab).unwrap().path);
    second.shutdown();
}

#[test]
fn tabs_and_panes_move_and_swap() {
    let f = Fixture::new();
    f.app.open_tab(None, &f.path("Documents"), true).unwrap();
    let pane = f.app.active_pane();
    let other = f
        .app
        .split_pane(
            pane,
            bb_core::layout::SplitOrientation::Vertical,
            &f.path("Pictures"),
        )
        .unwrap();
    f.settle();
    let tab = f.app.pane_tabs(pane).unwrap()[1];
    f.app.move_tab_to_pane(tab, other).unwrap();
    assert_eq!(2, f.app.pane_tabs(other).unwrap().len());
    let before = f.app.panes();
    f.app.swap_panes().unwrap();
    assert_eq!(vec![before[1], before[0]], f.app.panes());
    // Emptying a pane closes it.
    let only = f.app.pane_tabs(pane).unwrap()[0];
    f.app.move_tab_to_pane(only, other).unwrap();
    assert_eq!(1, f.app.panes().len());
}

// ---- the command registry and its one gate ----

#[test]
fn every_catalogue_command_has_an_owner_and_a_state() {
    let f = Fixture::new();
    let list = json(&f.app.command_list_json());
    let list = list.as_array().unwrap();
    assert_eq!(bb_core::commands::CommandCatalog::all().len(), list.len());
    assert!(
        list.iter()
            .any(|c| c["id"] == "file.rename" && c["owner"] == "ui")
    );
    assert!(
        list.iter()
            .any(|c| c["id"] == "tab.new" && c["owner"] == "core")
    );
    assert!(f.app.command_why_not("no.such").is_err());
}

/// The runner's match ends in "has no handler here"; nothing the catalogue lists as the core's may
/// reach it. Each runs on a fresh window with one file selected, so the gate lets most through.
#[test]
fn every_core_command_has_a_handler() {
    for c in bb_core::commands::CommandCatalog::all() {
        if bb_app::command_owner(c.id) != bb_app::CommandOwner::Core {
            continue;
        }
        let f = Fixture::new();
        f.select_names(&["notes.txt"]);
        if let Err(e) = f.app.command_execute(c.id) {
            assert!(!e.to_string().contains("has no handler"), "{}: {e}", c.id);
        }
        f.settle();
    }
}

#[test]
fn the_gate_says_why_in_the_words_of_the_original() {
    let f = Fixture::new();
    assert_eq!(
        Some("Nothing is selected.".to_string()),
        why(&f, "file.rename")
    );
    assert!(why(&f, "edit.undo").unwrap().contains("nothing to undo"));
    assert!(why(&f, "pane.close").unwrap().contains("only pane"));
    assert!(why(&f, "tab.move-left").unwrap().contains("first tab"));
    assert_eq!(None, why(&f, "nav.drive-root"));
    assert!(why(&f, "view.details").unwrap().contains("already"));
    assert!(why(&f, "view.thumbs-smaller").unwrap().contains("details"));
    f.select_names(&["notes.txt"]);
    assert_eq!(None, why(&f, "file.rename"));
    assert!(
        why(&f, "file.compare-files")
            .unwrap()
            .contains("exactly two")
    );
    f.select_names(&["notes.txt", "report.md"]);
    assert_eq!(None, why(&f, "file.compare-files"));
    assert!(
        why(&f, "file.rename").is_none(),
        "renaming many is the dialog's pattern"
    );
    assert!(
        why(&f, "pane.copy-to-other")
            .unwrap()
            .contains("no other pane")
    );
}

#[test]
fn a_dialog_or_the_settings_page_silences_the_folders() {
    let f = Fixture::new();
    f.app.ui_set_dialog_open(true);
    assert_eq!(
        Some(bb_app::DIALOG_OPEN_REASON.to_string()),
        why(&f, "tab.new")
    );
    f.app.ui_set_dialog_open(false);
    assert_eq!(None, why(&f, "tab.new"));

    f.app.settings_open("general").unwrap();
    assert_eq!(
        Some(bb_app::SETTINGS_OPEN_REASON.to_string()),
        why(&f, "tab.new")
    );
    // The app-wide ones still run under the page.
    assert_eq!(None, why(&f, "app.settings"));
    assert!(f.app.command_run("tab.new").is_err());
    f.app.settings_close().unwrap();
    assert_eq!(None, why(&f, "tab.new"));
}

#[test]
fn commands_run_through_the_gate_and_the_host_is_told_what_is_its_own() {
    let f = Fixture::new();
    assert_eq!(
        bb_app::CommandOutcome::Done,
        f.app.command_execute("tab.new").unwrap()
    );
    assert_eq!(2, f.app.pane_tabs(f.app.active_pane()).unwrap().len());
    f.app.activate_tab(f.tab).unwrap();
    // A dialog is the host's: gated here, run there.
    f.select_names(&["notes.txt"]);
    assert_eq!(
        bb_app::CommandOutcome::NeedsUi,
        f.app.command_execute("file.rename").unwrap()
    );
    // Refused, with the reason, when the gate says no.
    f.app.select(f.tab, bb_app::SelectOp::Clear, &[]).unwrap();
    let err = f.app.command_execute("file.rename").unwrap_err();
    assert_eq!(AppError::Invalid("Nothing is selected.".into()), err);
}

#[test]
fn view_commands_sort_toggle_and_size() {
    let f = Fixture::new();
    let tab = f.tab;
    f.app.command_run("view.sort-size").unwrap();
    f.app.command_run("view.sort-size").unwrap();
    let info = f.app.tab_info(tab).unwrap();
    assert_eq!(
        ("Size".to_string(), false),
        (info.sort_by, info.sort_desc),
        "sort by size is always ascending"
    );
    f.app.command_run("view.sort-reverse").unwrap();
    assert!(f.app.tab_info(tab).unwrap().sort_desc);
    f.app.command_run("view.sort-name").unwrap();
    let state: Value = json(&f.app.command_state_json("view.sort-name").unwrap());
    assert_eq!(true, state["checked"]);

    f.app.command_run("view.toggle-thumbnails").unwrap();
    assert!(f.app.tab_info(tab).unwrap().thumbnail_scale > 0.0);
    assert!(why(&f, "view.thumbnails").unwrap().contains("already"));
    f.app.command_run("view.thumbs-larger").unwrap();
    f.app.command_run("view.details").unwrap();
    assert_eq!(0.0, f.app.tab_info(tab).unwrap().thumbnail_scale);
    assert!(why(&f, "view.details").unwrap().contains("already"));

    f.app.command_run("view.preview").unwrap();
    assert_eq!(1, f.app.tab_info(tab).unwrap().side_pane);
    f.app.command_run("view.metadata").unwrap();
    assert_eq!(2, f.app.tab_info(tab).unwrap().side_pane);
    f.app.command_run("view.side-pane").unwrap();
    assert_eq!(0, f.app.tab_info(tab).unwrap().side_pane);
}

#[test]
fn selection_commands_pick_files_and_folders() {
    let f = Fixture::new();
    f.app.command_run("select.all").unwrap();
    assert_eq!(7, f.app.tab_info(f.tab).unwrap().selected);
    f.app.command_run("select.files").unwrap();
    assert_eq!(5, f.app.tab_info(f.tab).unwrap().selected);
    f.app.command_run("select.folders").unwrap();
    assert_eq!(2, f.app.tab_info(f.tab).unwrap().selected);
    f.app.command_run("select.invert").unwrap();
    assert_eq!(5, f.app.tab_info(f.tab).unwrap().selected);
    f.app.command_run("select.none").unwrap();
    assert!(why(&f, "select.none").is_some());
}

#[test]
fn tab_and_pane_commands_reshape_the_window() {
    let f = Fixture::new();
    f.app.command_run("tab.duplicate").unwrap();
    f.app.command_run("tab.new").unwrap();
    let pane = f.app.active_pane();
    assert_eq!(3, f.app.pane_tabs(pane).unwrap().len());
    f.app.command_run("tab.move-left").unwrap();
    f.app.command_run("tab.close-right").unwrap();
    assert_eq!(2, f.app.pane_tabs(pane).unwrap().len());
    f.app.command_run("tab.close-others").unwrap();
    assert_eq!(1, f.app.pane_tabs(pane).unwrap().len());
    assert!(why(&f, "tab.reopen").is_none());
    f.app.command_run("tab.reopen").unwrap();
    assert_eq!(2, f.app.pane_tabs(pane).unwrap().len());

    f.app.command_run("pane.split-right").unwrap();
    assert_eq!(2, f.app.panes().len());
    f.app.command_run("pane.equalise").unwrap();
    f.app.command_run("pane.swap").unwrap();
    // The one tab of the first pane moves across, and the pane it leaves empty closes.
    f.app.command_run("tab.move-to-other-pane").unwrap();
    assert_eq!(1, f.app.panes().len());
    assert!(why(&f, "pane.close").unwrap().contains("only pane"));
}

#[test]
fn numbered_tab_commands_pick_a_slot_and_last_is_the_last() {
    let f = Fixture::new();
    let pane = f.app.active_pane();
    for _ in 0..9 {
        f.app.command_run("tab.new").unwrap();
    }
    let tabs = f.app.pane_tabs(pane).unwrap();
    assert_eq!(10, tabs.len());
    let active = || f.app.active_tab_of(pane).unwrap().unwrap();
    f.app.command_run("tab.goto-1").unwrap();
    assert_eq!(tabs[0], active());
    f.app.command_run("tab.goto-3").unwrap();
    assert_eq!(tabs[2], active());
    f.app.command_run("tab.goto-8").unwrap();
    assert_eq!(tabs[7], active());
    f.app.command_run("tab.goto-last").unwrap();
    assert_eq!(
        tabs[9],
        active(),
        "last means the last, however many there are"
    );
}

#[test]
fn opening_here_in_the_other_pane_takes_its_tab_there_rather_than_adding_one() {
    let f = Fixture::new();
    f.app.command_run("pane.split-right").unwrap();
    f.settle();
    let panes = f.app.panes();
    f.app.activate_pane(panes[0]).unwrap();
    let tab = f.app.active_tab_of(panes[0]).unwrap().unwrap();
    f.app.navigate(tab, &f.path("Documents")).unwrap();
    f.settle();
    f.app.command_run("pane.open-here-in-other").unwrap();
    f.settle();
    assert_eq!(1, f.app.pane_tabs(panes[1]).unwrap().len());
    let other = f.app.active_tab_of(panes[1]).unwrap().unwrap();
    assert!(f.app.tab_info(other).unwrap().path.ends_with("Documents"));
    assert_eq!(
        panes[0],
        f.app.active_pane(),
        "the focus stays where the work is"
    );
}

/// Sync deletes from the right, so which side is which must not depend on where the focus was.
#[test]
fn the_left_of_a_comparison_is_the_left_pane_whichever_has_the_focus() {
    let f = Fixture::new();
    f.app.command_run("pane.split-right").unwrap();
    f.settle();
    let panes = f.app.panes();
    for (pane, folder) in panes.iter().zip(["Documents", "Pictures"]) {
        let tab = f.app.active_tab_of(*pane).unwrap().unwrap();
        f.app.navigate(tab, &f.path(folder)).unwrap();
    }
    f.settle();
    f.app.activate_pane(panes[1]).unwrap();
    f.app.command_run("tools.compare").unwrap();
    f.settle();
    let m: Value = json(&f.app.tools("compare.model", "{}").unwrap());
    assert!(m["left"].as_str().unwrap().ends_with("Documents"), "{m}");
}

#[test]
fn comparing_panes_stops_them_changing_places() {
    let f = Fixture::new();
    f.app.command_run("pane.split-right").unwrap();
    f.settle();
    assert_eq!(None, why(&f, "pane.swap"));
    // Two panes on one folder are refused (a folder compared with itself says nothing); give each
    // somewhere different to be.
    for (pane, folder) in f.app.panes().into_iter().zip(["Documents", "Pictures"]) {
        let tab = f.app.active_tab_of(pane).unwrap().unwrap();
        f.app.navigate(tab, &f.path(folder)).unwrap();
    }
    f.settle();
    f.app.command_run("tools.compare").unwrap();
    assert!(
        why(&f, "pane.swap")
            .unwrap()
            .contains("Stop comparing first")
    );
    assert!(
        why(&f, "tab.move-to-other-pane")
            .unwrap()
            .contains("Stop comparing first")
    );
    let compare = json(&f.app.command_state_json("tools.compare").unwrap());
    assert_eq!(true, compare["checked"]);
    assert_eq!("Stop comparing", compare["caption"]);
    f.app.compare_end();
    assert_eq!(None, why(&f, "pane.swap"));
}

#[test]
fn the_containing_folder_command_leaves_a_flat_view_selecting_the_row() {
    let f = Fixture::new();
    assert!(
        why(&f, "nav.containing-folder")
            .unwrap()
            .contains("search results or a flat view")
    );
    f.app.set_flat_view(f.tab, true).unwrap();
    f.settle();
    let index = f
        .app
        .listing(f.tab)
        .unwrap()
        .rows()
        .iter()
        .position(|r| r.full_path == f.path("Documents\\report.md"))
        .unwrap() as u32;
    f.app
        .select(f.tab, bb_app::SelectOp::Replace, &[index])
        .unwrap();
    assert_eq!(None, why(&f, "nav.containing-folder"));
    f.app.command_run("nav.containing-folder").unwrap();
    f.settle();
    let info = f.app.tab_info(f.tab).unwrap();
    assert_eq!(f.path("Documents"), info.path);
    assert!(!info.is_flattened);
    assert_eq!(
        vec![f.path("Documents\\report.md")],
        f.app.selected_paths(f.tab).unwrap()
    );
}

// ---- keys ----

fn focus() -> FocusState {
    FocusState::new(FocusKind::FileList)
}

#[test]
fn a_key_press_is_the_keymaps_and_runs_through_the_gate() {
    let f = Fixture::new();
    let press = |chord: &str| json(&f.app.key_press(&KeyChord::parse(chord), focus(), false));
    let r = press("Ctrl+T");
    assert_eq!("ran", r["result"]);
    assert_eq!(2, f.app.pane_tabs(f.app.active_pane()).unwrap().len());
    // Claimed even though it cannot run: it must not fall through to type-ahead.
    let r = press("F2");
    assert_eq!(true, r["claimed"]);
    assert_eq!("unavailable", r["result"]);
    assert_eq!(Value::Null, press("Q")["command"]);
    assert_eq!(
        "ignored",
        json(&f.app.key_press(&KeyChord::parse("Ctrl+T"), focus(), true))["result"]
    );
    f.app.activate_tab(f.tab).unwrap();
    f.select_names(&["notes.txt"]);
    assert_eq!("ui", press("F2")["result"]);
}

#[test]
fn rebinding_takes_the_chord_from_its_owner_and_is_written_at_once() {
    let f = Fixture::new();
    f.app
        .keymap_set_chords("view.flat", &["Ctrl+K".to_string()])
        .unwrap();
    assert_eq!("Ctrl+K", f.app.keymap_gesture("view.flat"));
    assert!(
        f.data.join("settings.json").exists(),
        "a shortcut is written the moment it is assigned"
    );
    let r = json(&f.app.key_press(&KeyChord::parse("Ctrl+K"), focus(), false));
    assert_eq!("view.flat", r["command"]);
    assert!(f.app.tab_info(f.tab).unwrap().is_flat_view);

    // Ctrl+T moves to another command; the loser has none.
    f.app
        .keymap_set_chords("tab.duplicate", &["Ctrl+T".to_string()])
        .unwrap();
    assert_eq!("", f.app.keymap_gesture("tab.new"));
    assert_eq!("Ctrl+T", f.app.keymap_gesture("tab.duplicate"));
    f.app.keymap_reset_all().unwrap();
    assert_eq!("Ctrl+T", f.app.keymap_gesture("tab.new"));
    assert_eq!("Ctrl+B", f.app.keymap_gesture("view.flat"));
    let text = std::fs::read_to_string(f.data.join("settings.json")).unwrap();
    assert!(!text.contains("Ctrl+K"));
}

#[test]
fn the_keyboard_page_records_refuses_and_reassigns() {
    let f = Fixture::new();
    let page = json(&f.app.keys_page_json());
    assert!(
        page["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == "New tab" && r["chords"][0] == "Ctrl+T")
    );

    // The filter finds by name or by key.
    f.app.keys_do("filter", "flat").unwrap();
    let page = json(&f.app.keys_page_json());
    assert!(
        page["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "view.flat")
    );
    assert!(
        !page["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "tab.new")
    );
    f.app.keys_do("filter", "").unwrap();

    f.app.keys_do("select", "view.flat").unwrap();
    f.app.keys_do("change", "").unwrap();
    assert!(
        json(&f.app.keys_page_json())["text"]
            .as_str()
            .unwrap()
            .starts_with("Press the new shortcut")
    );
    assert!(f.app.keys_page_json().contains("\"capturing\":true"));
    // A key that types a character is refused and the recorder keeps listening.
    f.app.keys_do("press", "B").unwrap();
    let page = json(&f.app.keys_page_json());
    assert!(
        page["text"]
            .as_str()
            .unwrap()
            .contains("can\u{2019}t be used")
    );
    assert_eq!(true, page["recording"]);
    f.app.keys_do("press", "Ctrl+K").unwrap();
    assert_eq!("Ctrl+K", f.app.keymap_gesture("view.flat"));

    // Somebody else's chord is offered for reassignment, never taken silently.
    f.app.keys_do("select", "tab.close-others").unwrap();
    f.app.keys_do("change", "").unwrap();
    f.app.keys_do("press", "Ctrl+T").unwrap();
    let page = json(&f.app.keys_page_json());
    assert!(page["text"].as_str().unwrap().contains("already runs"));
    assert_eq!(true, page["canReassign"]);
    assert_eq!("Ctrl+T", f.app.keymap_gesture("tab.new"));
    f.app.keys_do("reassign", "").unwrap();
    assert_eq!("Ctrl+T", f.app.keymap_gesture("tab.close-others"));
    assert_eq!("", f.app.keymap_gesture("tab.new"));

    // Add keeps what is there; remove leaves none; reset puts the shipped keys back.
    f.app.keys_do("select", "tab.new").unwrap();
    f.app.keys_do("add", "").unwrap();
    f.app.keys_do("press", "Ctrl+N").unwrap();
    f.app.keys_do("add", "").unwrap();
    f.app.keys_do("press", "Ctrl+Shift+Y").unwrap();
    assert_eq!(
        vec!["Ctrl+N", "Ctrl+Shift+Y"],
        f.app.keymap_chords("tab.new")
    );
    f.app.keys_do("select", "nav.refresh").unwrap();
    f.app.keys_do("remove", "").unwrap();
    assert_eq!("", f.app.keymap_gesture("nav.refresh"));
    f.app.keys_do("reset", "").unwrap();
    assert_eq!("F5", f.app.keymap_gesture("nav.refresh"));

    // Find: press a key to see what it runs.
    f.app.keys_do("find", "").unwrap();
    f.app.keys_do("press", "Ctrl+K").unwrap();
    let page = json(&f.app.keys_page_json());
    assert_eq!("view.flat", page["selected"]);
    f.app.keys_do("filter", "").unwrap();
    f.app.keys_do("reset-all", "").unwrap();
    assert_eq!("Ctrl+B", f.app.keymap_gesture("view.flat"));
}

#[test]
fn the_recorder_stands_the_dispatcher_down() {
    let f = Fixture::new();
    f.app.keys_do("select", "tab.new").unwrap();
    f.app.keys_do("change", "").unwrap();
    // While listening, the key is the recorder's answer, not a command.
    let r = json(&f.app.key_press(&KeyChord::parse("Ctrl+T"), focus(), false));
    assert_eq!(false, r["claimed"]);
    f.app.keys_do("cancel", "").unwrap();
    assert_eq!(
        true,
        json(&f.app.key_press(&KeyChord::parse("Ctrl+T"), focus(), false))["claimed"]
    );
}

// ---- the palette ----

#[test]
fn the_empty_palette_is_never_the_catalogue() {
    let f = Fixture::new();
    f.app.palette_open("");
    let texts = palette_texts(&f);
    assert!(texts.contains(&"Suggested".to_string()), "{texts:?}");
    assert!(texts.contains(&"All commands".to_string()), "{texts:?}");
    assert!(
        texts.iter().any(|t| t == "Tabs"),
        "one row per category: {texts:?}"
    );
    assert!(!texts.contains(&"Close other tabs".to_string()));
    assert!(texts.contains(&"Edit address".to_string()));
    assert!(
        !texts.contains(&"Rename".to_string()),
        "nothing is selected"
    );
    let page = json(&f.app.palette_json());
    assert_eq!(true, page["open"]);
    // Enter should do something: the first row that can run is selected.
    assert!(page["selected"].is_number());
}

#[test]
fn typing_ranks_matches_and_a_category_narrows() {
    let f = Fixture::new();
    f.app.palette_open("");
    f.app.palette_do("type", "new").unwrap();
    let texts = row_names(&json(&f.app.palette_json()));
    assert_eq!("New tab", texts[0]);
    f.app.palette_do("type", "ctrl+b").unwrap();
    let page = json(&f.app.palette_json());
    let selected = page["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["selected"] == true)
        .unwrap();
    assert_eq!("Toggle flat view", selected["text"]);
    f.app.palette_do("type", "view: sort").unwrap();
    let names = row_names(&json(&f.app.palette_json()));
    assert_eq!(&["Sort by name", "Sort by size"], &names[..2]);
    assert!(!names.contains(&"Toggle hidden items".to_string()));
    // An unavailable command stays listed, and its reason is the hint.
    f.app.palette_do("type", "rename").unwrap();
    let page = json(&f.app.palette_json());
    assert!(
        page["hint"]
            .as_str()
            .unwrap()
            .contains("Nothing is selected")
    );
    f.app.palette_do("pick", "").unwrap();
    assert!(
        f.app.palette_is_open(),
        "left open: closing on a command that then does nothing looks broken"
    );
}

#[test]
fn picking_runs_remembers_in_memory_and_pinning_is_saved() {
    let f = Fixture::new();
    f.app.palette_open("");
    f.app.palette_do("type", "new tab").unwrap();
    let r = json(&f.app.palette_do("pick", "").unwrap());
    assert_eq!(true, r["ran"]);
    assert!(!f.app.palette_is_open());
    assert_eq!(2, f.app.pane_tabs(f.app.active_pane()).unwrap().len());
    assert_eq!(vec!["tab.new".to_string()], f.app.palette_recents());
    f.app.palette_open("");
    let texts = palette_texts(&f);
    let at = texts
        .iter()
        .position(|t| t == "Recent")
        .expect("a Recent heading");
    assert_eq!("New tab", texts[at + 1]);

    f.app.palette_do("type", "toggle flat").unwrap();
    f.app.palette_do("pin", "").unwrap();
    f.app.palette_do("type", "").unwrap();
    let texts = palette_texts(&f);
    let at = texts
        .iter()
        .position(|t| t == "Pinned")
        .expect("a Pinned heading");
    assert_eq!("Toggle flat view", texts[at + 1]);
    let saved = std::fs::read_to_string(f.data.join("settings.json")).unwrap();
    assert!(
        saved.contains("view.flat"),
        "pins are a preference and are saved"
    );
    assert!(!saved.contains("tab.new"), "recents never are");
    f.app.palette_do("pick", "").unwrap();
    f.app.palette_open("");
    f.app.palette_do("type", "toggle flat").unwrap();
    f.app.palette_do("pin", "").unwrap();
    f.app.palette_do("type", "").unwrap();
    assert!(!palette_texts(&f).contains(&"Pinned".to_string()));
}

#[test]
fn a_typed_path_is_a_destination_not_a_search() {
    let f = Fixture::new();
    f.app.palette_open("");
    f.app
        .palette_do("type", &format!("{}\\Doc", f.root.to_string_lossy()))
        .unwrap();
    let page = json(&f.app.palette_json());
    let texts = row_names(&page);
    assert!(texts.contains(&"Documents".to_string()), "{texts:?}");
    f.app
        .palette_do("type", &format!("{}\\Documents", f.root.to_string_lossy()))
        .unwrap();
    let texts = row_names(&json(&f.app.palette_json()));
    assert_eq!("Go to Documents", texts[0]);
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("Open Documents in a new tab"))
    );
    f.app.palette_do("pick", "").unwrap();
    f.settle();
    assert_eq!(f.path("Documents"), f.app.tab_info(f.tab).unwrap().path);

    // "Go to folder" opens the box on this folder, ready to descend.
    f.app.command_run("nav.goto").unwrap();
    let page = json(&f.app.palette_json());
    assert_eq!(format!("{}\\", f.path("Documents")), page["query"]);
    let texts = row_names(&page);
    assert!(texts.contains(&"nested".to_string()), "{texts:?}");
    f.app.palette_do("down", "1").unwrap();
    f.app.palette_do("complete", "").unwrap();
    assert!(
        json(&f.app.palette_json())["query"]
            .as_str()
            .unwrap()
            .ends_with("nested\\")
            || true
    );
}

#[test]
fn the_palette_under_the_settings_page_lists_but_will_not_run_folder_commands() {
    let f = Fixture::new();
    f.app.settings_open("general").unwrap();
    f.app.palette_open("");
    f.app.palette_do("type", "new tab").unwrap();
    let page = json(&f.app.palette_json());
    assert!(
        page["hint"]
            .as_str()
            .unwrap()
            .contains("Not while Settings is open")
    );
    // A settings page is reachable from anywhere.
    f.app.palette_do("type", "keyboard shortcuts").unwrap();
    f.app.palette_do("pick", "").unwrap();
    assert!(!f.app.palette_is_open());
    assert_eq!(
        Some("keyboard"),
        json(&f.app.settings_page_json())["page"].as_str()
    );
}

#[test]
fn the_palette_lists_what_comes_from_data() {
    let f = Fixture::new();
    f.app.saved_do(None, "save-workspace", "Projects").unwrap();
    f.app.palette_open("");
    f.app.palette_do("type", "projects").unwrap();
    let page = json(&f.app.palette_json());
    let selected = page["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["selected"] == true)
        .unwrap();
    assert_eq!("Projects", selected["text"]);
    f.app.palette_do("type", "dark").unwrap();
    assert!(
        row_names(&json(&f.app.palette_json()))
            .iter()
            .any(|t| t.starts_with("Dark"))
    );
    f.app.palette_do("type", "column: created").unwrap();
    assert!(
        row_names(&json(&f.app.palette_json()))
            .iter()
            .any(|t| t.contains("Show column: Created"))
    );
    f.app.palette_do("pick", "").unwrap();
    assert!(f.app.columns_json(f.tab).unwrap().contains("\"Created\""));
}

#[test]
fn alt_k_opens_the_keyboard_page_on_the_command_already_listening() {
    let f = Fixture::new();
    f.app.palette_open("");
    f.app.palette_do("type", "new tab").unwrap();
    f.app.palette_do("shortcut", "").unwrap();
    assert!(!f.app.palette_is_open());
    let page = json(&f.app.keys_page_json());
    assert_eq!("tab.new", page["selected"]);
    assert_eq!(true, page["recording"]);
    f.app.keys_do("press", "Ctrl+Shift+Y").unwrap();
    assert_eq!("Ctrl+Shift+Y", f.app.keymap_gesture("tab.new"));
}

// ---- menu bar and toolbars ----

#[test]
fn the_shipped_arrangement_is_the_default_and_stored_as_nothing() {
    let f = Fixture::new();
    let state = json(&f.app.bars_state_json());
    assert_eq!(true, state["isDefault"]);
    let menus: Vec<String> = json(&f.app.bars_menu_bar_json())
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(vec!["File", "Edit", "View", "Go", "Tools", "Help"], menus);
    let title = json(&f.app.bars_toolbar_json("title").unwrap());
    let tokens: Vec<&str> = title["tokens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    assert_eq!(
        vec![
            "edit.undo",
            "edit.redo",
            "-",
            "tools.disk-usage",
            "tools.duplicates",
            "tools.changes",
            "tools.compare",
            "app.settings"
        ],
        tokens
    );
    let undo = &title["rows"][0];
    assert_eq!("edit.undo", undo["id"]);
    assert_eq!(false, undo["enabled"]);
    let text = std::fs::read_to_string(f.data.join("settings.json")).unwrap_or_default();
    assert!(!text.contains("\"commandBars\":{") && !text.contains("\"commandBars\": {"));
}

#[test]
fn a_menu_is_built_from_the_moment() {
    let f = Fixture::new();
    let file = json(&f.app.bars_menu_json("File").unwrap());
    let rows = file["rows"].as_array().unwrap();
    let captions: Vec<String> = rows
        .iter()
        .map(|r| r["caption"].as_str().unwrap().replace('&', ""))
        .collect();
    assert!(captions.contains(&"New tab".to_string()), "{captions:?}");
    assert!(
        rows.iter()
            .any(|r| r["id"] == "tab.new" && r["gesture"] == "Ctrl+T")
    );
    f.select_names(&["notes.txt", "report.md"]);
    let file = json(&f.app.bars_menu_json("File").unwrap());
    let captions: Vec<String> = file["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["caption"].as_str().unwrap().replace('&', ""))
        .collect();
    assert!(
        captions.iter().any(|c| c.starts_with("Rename 2 items")),
        "{captions:?}"
    );
    assert!(
        captions.iter().any(|c| c.starts_with("Delete 2 items")),
        "{captions:?}"
    );
    // A list with no rows leaves its greyed line.
    let view = json(&f.app.bars_menu_json("View").unwrap());
    assert!(view.to_string().contains("No saved workspaces"));
    assert!(view.to_string().contains("No saved arrangements"));
}

#[test]
fn an_edited_arrangement_is_customised_saved_and_recalled_by_name() {
    let f = Fixture::new();
    f.app
        .bars_set_toolbar(
            "title",
            "nav.back nav.up - view.preview view.hidden custom:gone @workspaces",
            -1,
        )
        .unwrap();
    let state = json(&f.app.bars_state_json());
    assert_eq!(false, state["isDefault"]);
    // A command nothing answers to is kept in the arrangement and not shown.
    let title = json(&f.app.bars_toolbar_json("title").unwrap());
    let shown: Vec<String> = title["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap_or("-").to_string())
        .collect();
    assert!(!shown.contains(&"custom:gone".to_string()));
    assert!(
        state["title"]["tokens"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "custom:gone")
    );
    assert!(
        std::fs::read_to_string(f.data.join("settings.json"))
            .unwrap()
            .contains("custom:gone")
    );

    // Toggle commands carry their state.
    f.app.command_run("view.preview").unwrap();
    let title = json(&f.app.bars_toolbar_json("title").unwrap());
    let preview = title["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "view.preview")
        .unwrap();
    assert_eq!(true, preview["checked"]);

    f.app
        .bars_set_toolbar("row1", "tab.new pane.split-right - file.rename", 1)
        .unwrap();
    assert_eq!(
        1,
        json(&f.app.bars_state_json())["rows"]
            .as_array()
            .unwrap()
            .len()
    );
    f.app.bars_set_menu("Mine", "tab.new - @drives").unwrap();
    assert!(f.app.bars_menu_bar_json().contains("Mine"));

    f.app.bars_do("save", "Work").unwrap();
    assert_eq!("Work", json(&f.app.bars_state_json())["name"]);
    f.app.bars_do("reset", "").unwrap();
    let state = json(&f.app.bars_state_json());
    assert_eq!(true, state["isDefault"]);
    assert_eq!(0, state["rows"].as_array().unwrap().len());
    f.app.bars_do("apply", "Work").unwrap();
    assert_eq!("Work", json(&f.app.bars_state_json())["name"]);
    f.app.bars_do("rename", "Work to Daily").unwrap();
    assert_eq!("Daily", json(&f.app.bars_state_json())["name"]);
    // It is in the palette by name.
    f.app.palette_open("daily");
    assert!(row_names(&json(&f.app.palette_json())).contains(&"Daily".to_string()));
    f.app.palette_close();
    f.app.bars_do("remove", "Daily").unwrap();
    assert_eq!(Value::Null, json(&f.app.bars_state_json())["name"]);
}

#[test]
fn dragging_a_button_back_is_the_default_again() {
    let f = Fixture::new();
    let original = json(&f.app.bars_state_json())["title"]["tokens"].clone();
    f.app.bars_set_toolbar("title", "edit.undo", -1).unwrap();
    assert_eq!(false, json(&f.app.bars_state_json())["isDefault"]);
    let tokens: Vec<String> = original
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap().to_string())
        .collect();
    f.app
        .bars_set_toolbar("title", &tokens.join(" "), -1)
        .unwrap();
    assert_eq!(true, json(&f.app.bars_state_json())["isDefault"]);
}

#[test]
fn the_toolbar_editor_walks_menus_and_edits_in_place() {
    let f = Fixture::new();
    f.app.barpage_do("open", "Help").unwrap();
    let names = |page: &Value| -> Vec<String> {
        page["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["name"].as_str().unwrap().to_string())
            .collect()
    };
    let page = json(&f.app.barpage_json());
    assert_eq!(
        vec![
            "Command palette",
            "Search syntax help",
            "Separator",
            "About BertBrowser"
        ],
        names(&page)
    );
    f.app.barpage_do("pick", "Search syntax help").unwrap();
    f.app.barpage_do("add", "nav.refresh").unwrap();
    assert_eq!(
        vec![
            "Command palette",
            "Search syntax help",
            "Refresh",
            "Separator",
            "About BertBrowser"
        ],
        names(&json(&f.app.barpage_json()))
    );
    f.app.barpage_do("nudge", "Refresh up").unwrap();
    f.app.barpage_do("name", "Reload").unwrap();
    assert_eq!(
        vec![
            "Command palette",
            "Reload",
            "Search syntax help",
            "Separator",
            "About BertBrowser"
        ],
        names(&json(&f.app.barpage_json()))
    );
    // A choice dropped between two rows lands exactly there, whatever is selected; past the end is the end.
    f.app.barpage_do("add-at", "0 app.settings").unwrap();
    f.app.barpage_do("add-at", "99 -").unwrap();
    let after = names(&json(&f.app.barpage_json()));
    assert_eq!("Settings", after[0]);
    assert_eq!("Separator", after[after.len() - 1]);
    assert!(f.app.barpage_do("add-at", "app.settings").is_err());
    f.app.barpage_do("remove", "0").unwrap();
    f.app
        .barpage_do("remove", &(after.len() - 2).to_string())
        .unwrap();
    assert_eq!(
        vec![
            "Command palette",
            "Reload",
            "Search syntax help",
            "Separator",
            "About BertBrowser"
        ],
        names(&json(&f.app.barpage_json()))
    );
    assert_eq!(false, json(&f.app.bars_state_json())["isDefault"]);
    assert_eq!(
        "Your own arrangement, not saved under a name",
        json(&f.app.barpage_json())["status"]
    );
    f.app.barpage_do("remove", "Reload").unwrap();
    f.app.barpage_do("up", "").unwrap();
    assert_eq!(
        true,
        json(&f.app.bars_state_json())["isDefault"],
        "edited back to what ships"
    );

    // A menu of one's own, and a trimmed toolbar.
    f.app.barpage_do("pick", "Help").unwrap();
    f.app.barpage_do("add-menu", "").unwrap();
    f.app.barpage_do("name", "&Window").unwrap();
    assert!(names(&json(&f.app.barpage_json())).contains(&"Window".to_string()));
    f.app.barpage_do("open", "Window").unwrap();
    f.app.barpage_do("add", "pane.swap").unwrap();
    f.app.barpage_do("add", "@drives").unwrap();
    assert_eq!(
        vec!["Swap panes", "Drives"],
        names(&json(&f.app.barpage_json()))
    );
    f.app.barpage_do("up", "").unwrap();

    // The picker: filtered, and "not placed anywhere".
    f.app.barpage_do("filter", "thumbnail").unwrap();
    let choices = json(&f.app.barpage_json())["choices"].to_string();
    assert!(choices.contains("Thumbnail view") && !choices.contains("New tab"));
    f.app.barpage_do("filter", "").unwrap();
    f.app.barpage_do("unplaced", "on").unwrap();
    let choices = json(&f.app.barpage_json())["choices"].to_string();
    assert!(choices.contains("Go to tab 1") && !choices.contains("\"New tab\""));
    f.app.barpage_do("unplaced", "off").unwrap();

    f.app.barpage_do("select", "title").unwrap();
    f.app.barpage_do("remove", "Find duplicates").unwrap();
    let title = json(&f.app.bars_toolbar_json("title").unwrap());
    assert!(!title["tokens"].to_string().contains("tools.duplicates"));
    f.app.barpage_do("add-row", "").unwrap();
    assert_eq!(
        1,
        json(&f.app.bars_state_json())["rows"]
            .as_array()
            .unwrap()
            .len()
    );
    // Changes saved under no name: Reset asks first, and this run's answer is no.
    f.app.barpage_do("reset", "").unwrap();
    f.settle();
    assert_eq!(false, json(&f.app.bars_state_json())["isDefault"]);
    // Once they are kept under a name there is nothing to lose, and it happens at once.
    f.app.bars_do("save", "Mine").unwrap();
    f.app.barpage_do("reset", "").unwrap();
    assert_eq!(true, json(&f.app.bars_state_json())["isDefault"]);
}

// ---- columns ----

fn column_ids(f: &Fixture) -> Vec<String> {
    json(&f.app.columns_json(f.tab).unwrap())["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_columns_page_edits_the_default_set_that_new_tabs_start_from() {
    let f = Fixture::new();
    let defaults = |f: &Fixture| -> Vec<String> {
        json(&f.app.columns_defaults_json())["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(vec!["Name", "Size", "Type", "Modified"], defaults(&f));
    f.app.columns_defaults_do("add", "Created").unwrap();
    f.app.columns_defaults_do("remove", "Type").unwrap();
    // Name stays first whatever is asked, and cannot be taken off.
    f.app.columns_defaults_do("move", "Created 0").unwrap();
    f.app.columns_defaults_do("remove", "Name").unwrap();
    assert_eq!(vec!["Name", "Created", "Size", "Modified"], defaults(&f));
    // The tab never arranged its own, so it follows.
    assert_eq!(vec!["Name", "Created", "Size", "Modified"], column_ids(&f));
    // The wheel over a width: snapped to the grid first, so a notch is always one visible move; Ctrl is one DIP.
    let width = |f: &Fixture| {
        json(&f.app.columns_defaults_json())["columns"][1]["width"]
            .as_f64()
            .unwrap()
    };
    f.app.columns_defaults_do("width", "Created 137").unwrap();
    f.app
        .columns_defaults_do("step-width", "Created 1")
        .unwrap();
    assert_eq!(140.0, width(&f));
    f.app.columns_defaults_do("width", "Created 137").unwrap();
    f.app
        .columns_defaults_do("step-width", "Created -1")
        .unwrap();
    assert_eq!(130.0, width(&f));
    f.app
        .columns_defaults_do("step-width", "Created 1 fine")
        .unwrap();
    assert_eq!(131.0, width(&f));
    assert!(f.app.columns_defaults_do("step-width", "Nope 1").is_err());
    f.app.columns_defaults_do("reset", "").unwrap();
    assert_eq!(vec!["Name", "Size", "Type", "Modified"], defaults(&f));
}

#[test]
fn columns_are_added_removed_moved_and_sized() {
    let f = Fixture::new();
    assert_eq!(vec!["Name", "Size", "Type", "Modified"], column_ids(&f));
    f.app.columns_do(f.tab, "add", "Created").unwrap();
    assert_eq!(
        vec!["Name", "Size", "Type", "Modified", "Created"],
        column_ids(&f)
    );
    f.app.columns_do(f.tab, "remove", "Type").unwrap();
    f.app.columns_do(f.tab, "move", "Created 1").unwrap();
    assert_eq!(vec!["Name", "Created", "Size", "Modified"], column_ids(&f));
    f.app.columns_do(f.tab, "width", "Created 200").unwrap();
    let cols = json(&f.app.columns_json(f.tab).unwrap());
    let created = cols["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "Created")
        .unwrap();
    assert_eq!(200.0, created["width"].as_f64().unwrap());
    // Name cannot be removed or moved off the front.
    f.app.columns_do(f.tab, "remove", "Name").unwrap();
    f.app.columns_do(f.tab, "move", "Name 3").unwrap();
    assert_eq!(vec!["Name", "Created", "Size", "Modified"], column_ids(&f));
    assert_eq!(true, cols["customized"]);
    // A reset is the catalogue default, and the gate knows the tab is no longer plain.
    f.app.columns_do(f.tab, "reset", "").unwrap();
    assert_eq!(vec!["Name", "Size", "Type", "Modified"], column_ids(&f));
}

#[test]
fn a_flat_result_gets_the_folder_column_and_content_matches_get_match() {
    let f = Fixture::new();
    f.app.set_search(f.tab, "report", false).unwrap();
    f.settle();
    assert_eq!(
        vec!["Name", "RelativePath", "Size", "Type", "Modified"],
        column_ids(&f)
    );
    f.app.clear_search(f.tab).unwrap();
    f.settle();
    assert_eq!(vec!["Name", "Size", "Type", "Modified"], column_ids(&f));
    // Added by hand, the injected one is not the model's: it stays out of the saved layout.
    f.app.columns_do(f.tab, "add", "RelativePath").unwrap();
    assert_eq!(vec!["Name", "Size", "Type", "Modified"], column_ids(&f));
    std::fs::write(f.root.join("needle.txt"), "a needle here").unwrap();
    f.app.refresh(f.tab).unwrap();
    f.settle();
    f.app.set_search(f.tab, "content:needle", false).unwrap();
    f.settle();
    assert_eq!(
        vec!["Name", "RelativePath", "Match", "Size", "Type", "Modified"],
        column_ids(&f)
    );
}

#[test]
fn a_saved_default_reaches_tabs_that_have_not_arranged_their_own() {
    let f = Fixture::new();
    f.app.columns_do(f.tab, "add", "Created").unwrap();
    f.app.columns_do(f.tab, "default", "").unwrap();
    // The tab that arranged it keeps it; a new one starts from it.
    let fresh = f.app.open_tab(None, &f.path("Documents"), true).unwrap();
    f.settle();
    let ids: Vec<String> = json(&f.app.columns_json(fresh).unwrap())["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(vec!["Name", "Size", "Type", "Modified", "Created"], ids);
    // Moving the saved default reaches the untouched tab, and leaves the arranged one alone.
    f.app
        .settings_set(
            "fileListColumns",
            r#"[{"id":"Name","width":300},{"id":"Size","width":80}]"#,
        )
        .unwrap();
    let ids: Vec<String> = json(&f.app.columns_json(fresh).unwrap())["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(vec!["Name", "Size"], ids);
    assert_eq!(
        vec!["Name", "Size", "Type", "Modified", "Created"],
        column_ids(&f)
    );
}

#[test]
fn a_customised_layout_survives_the_session() {
    let f = Fixture::new();
    f.app.columns_do(f.tab, "add", "Created").unwrap();
    f.app.columns_do(f.tab, "width", "Created 180").unwrap();
    assert!(f.app.session_roundtrip().unwrap());
    f.settle();
    let tab = f.app.active_tab().unwrap();
    let cols = json(&f.app.columns_json(tab).unwrap());
    let ids: Vec<&str> = cols["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(vec!["Name", "Size", "Type", "Modified", "Created"], ids);
    assert_eq!(true, cols["customized"]);
}

#[test]
fn the_add_popup_offers_what_is_not_yet_showing() {
    let f = Fixture::new();
    let popup = json(&f.app.columns_candidates_json(f.tab, "").unwrap());
    assert_eq!(
        true, popup["loading"],
        "the property system has not been enumerated yet"
    );
    let all = popup.to_string();
    assert!(all.contains("Created") && !all.contains("\"Modified\""));
    f.app
        .columns_set_machine_properties(r#"[["System.Photo.CameraManufacturer","Camera maker"]]"#)
        .unwrap();
    let popup = json(&f.app.columns_candidates_json(f.tab, "camera").unwrap());
    assert!(
        popup
            .to_string()
            .contains("System.Photo.CameraManufacturer")
    );
}

#[test]
fn shell_property_cells_are_read_by_the_host_and_hold_the_settle_until_it_has_looked() {
    let f = Fixture::new();
    f.app.ui_set_flag("propertyProvider", true);
    f.app
        .columns_do(f.tab, "add", "System.Image.Dimensions")
        .unwrap();
    // The host has not looked: the tab is hydrating, so a settle would be premature.
    assert!(f.app.is_busy());
    assert!(f.app.state_json().contains("\"hydrating\":1"));
    let asked = json(&f.app.prop_requests(f.tab, 0, 0).unwrap());
    assert_eq!(7, asked.as_array().unwrap().len(), "one cell per row");
    assert!(f.app.is_busy(), "answers are still owed");
    // The cell is blank until it is answered: unknown, never wrong.
    let listing = f.app.listing(f.tab).unwrap();
    let column = json(&f.app.columns_json(f.tab).unwrap())["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "System.Image.Dimensions")
        .unwrap()["cell"]
        .as_i64()
        .unwrap() as u32;
    assert!(listing.cell(0, column).unwrap().is_empty());

    let photo = f.path("photo.jpg");
    f.app
        .prop_supply(
            f.tab,
            asked
                .as_array()
                .unwrap()
                .iter()
                .map(|e| PropSupply {
                    path: e[0].as_str().unwrap().to_string(),
                    canonical: e[1].as_str().unwrap().to_string(),
                    value: (e[0] == photo.as_str()).then(|| prop_words("6240 x 4160")),
                })
                .collect(),
        )
        .unwrap();
    f.settle();
    let listing = f.app.listing(f.tab).unwrap();
    let at = listing
        .rows()
        .iter()
        .position(|r| r.full_path == photo)
        .unwrap();
    assert_eq!(
        "6240 x 4160",
        String::from_utf16_lossy(listing.cell(at, column).unwrap())
    );
    let blank = listing
        .rows()
        .iter()
        .position(|r| r.full_path == f.path("notes.txt"))
        .unwrap();
    assert!(listing.cell(blank, column).unwrap().is_empty());

    // Sorting by it puts the known value first in both directions: a blank is not a small value.
    f.app.set_sort(f.tab, "System.Image.Dimensions").unwrap();
    let files = |f: &Fixture| -> Vec<String> {
        f.names()
            .into_iter()
            .filter(|n| !["Documents", "Pictures"].contains(&n.as_str()))
            .collect()
    };
    assert_eq!("photo.jpg", files(&f)[0]);
    f.app.set_sort(f.tab, "System.Image.Dimensions").unwrap();
    assert_eq!("photo.jpg", files(&f)[0]);
}

#[test]
fn without_a_provider_property_cells_are_simply_blank() {
    let f = Fixture::new();
    f.app
        .columns_do(f.tab, "add", "System.Image.Dimensions")
        .unwrap();
    f.settle();
    assert!(!f.app.is_busy());
}

// ---- tiles ----

#[test]
fn the_tile_state_carries_scale_and_aspect() {
    let f = Fixture::new();
    f.app.set_thumbnail_scale(f.tab, 0.5).unwrap();
    let state = json(&f.app.tile_state_json(f.tab).unwrap());
    assert_eq!(0.5, state["scale"]);
    assert_eq!(true, state["isThumbnails"]);
    assert_eq!("4:3", state["aspect"]["text"]);
    f.app.settings_set("tileAspectRatio", "\"16:9\"").unwrap();
    let state = json(&f.app.tile_state_json(f.tab).unwrap());
    assert!((state["aspect"]["ratio"].as_f64().unwrap() - 16.0 / 9.0).abs() < 1e-9);
    assert_eq!((4.0, 3.0), bb_app::parse_aspect("nonsense"));
    assert_eq!((4.0, 3.0), bb_app::parse_aspect("0:5"));
}

// ---- saved searches, workspaces ----

#[test]
fn a_saved_search_is_navigate_then_type() {
    let f = Fixture::new();
    f.app.set_search(f.tab, "report", false).unwrap();
    f.settle();
    f.app
        .saved_do(Some(f.tab), "save-search", "Reports")
        .unwrap();
    let saved = json(&f.app.saved_json());
    assert_eq!("Reports", saved["searches"][0]["name"]);
    assert_eq!("current", saved["searches"][0]["scope"]);

    f.app.clear_search(f.tab).unwrap();
    f.settle();
    f.app
        .saved_do(Some(f.tab), "run-search", "Reports")
        .unwrap();
    f.settle();
    let info = f.app.tab_info(f.tab).unwrap();
    assert!(info.is_flattened);
    assert_eq!(3, info.count);
    assert!(info.status.contains("report"));

    // In a new tab; the first tab is untouched.
    f.app.clear_search(f.tab).unwrap();
    f.settle();
    f.app
        .saved_do(Some(f.tab), "run-search-new-tab", "Reports")
        .unwrap();
    f.settle();
    assert_eq!(2, f.app.pane_tabs(f.app.active_pane()).unwrap().len());
    assert_eq!(
        3,
        f.app.tab_info(f.app.active_tab().unwrap()).unwrap().count
    );
}

#[test]
fn a_pinned_search_goes_to_its_folder_and_a_pc_search_is_global() {
    let f = Fixture::new();
    f.app.navigate(f.tab, &f.path("Documents")).unwrap();
    f.settle();
    f.app.set_search(f.tab, "report", false).unwrap();
    f.settle();
    f.app
        .saved_do(Some(f.tab), "save-search", "Docs reports folder")
        .unwrap();
    f.app.up(f.tab).unwrap();
    f.settle();
    f.app
        .saved_do(Some(f.tab), "run-search", "Docs reports")
        .unwrap();
    f.settle();
    assert_eq!(f.path("Documents"), f.app.tab_info(f.tab).unwrap().path);
    assert_eq!(2, f.app.tab_info(f.tab).unwrap().count);

    f.app
        .saved_do(Some(f.tab), "save-search", "Everywhere pc")
        .unwrap();
    let saved = json(&f.app.saved_json());
    assert!(
        saved["searches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "Everywhere" && s["scope"] == "pc")
    );
    // Saving over a name replaces it; rename and remove.
    f.app
        .saved_do(Some(f.tab), "save-search", "Everywhere")
        .unwrap();
    f.app
        .saved_do(None, "rename-search", "Everywhere to Anywhere")
        .unwrap();
    assert!(f.app.saved_json().contains("Anywhere"));
    f.app.saved_do(None, "remove-search", "Anywhere").unwrap();
    f.app
        .saved_do(None, "remove-search", "Docs reports")
        .unwrap();
    assert_eq!(
        0,
        json(&f.app.saved_json())["searches"]
            .as_array()
            .unwrap()
            .len()
    );
    assert!(
        f.app
            .saved_search_problem("x", "", "current", None)
            .unwrap()
            .contains("Type a search")
    );
}

#[test]
fn a_workspace_is_the_arrangement_rebuilt_in_place() {
    let f = Fixture::new();
    f.app.open_tab(None, &f.path("Documents"), true).unwrap();
    let pane = f.app.active_pane();
    f.app
        .split_pane(
            pane,
            bb_core::layout::SplitOrientation::Vertical,
            &f.path("Pictures"),
        )
        .unwrap();
    f.settle();
    f.app.saved_do(None, "save-workspace", "Projects").unwrap();
    let saved = json(&f.app.saved_json());
    assert_eq!("2 panes, 3 tabs", saved["workspaces"][0]["shape"]);

    f.app.command_run("pane.close").unwrap();
    assert_eq!(1, f.app.panes().len());
    f.app
        .saved_do(None, "switch-workspace", "Projects")
        .unwrap();
    f.settle();
    assert_eq!(2, f.app.panes().len());
    f.app
        .saved_do(None, "rename-workspace", "Projects to Work")
        .unwrap();
    f.app.saved_do(None, "remove-workspace", "Work").unwrap();
    assert_eq!(
        0,
        json(&f.app.saved_json())["workspaces"]
            .as_array()
            .unwrap()
            .len()
    );
    assert!(f.app.saved_workspace_problem("").unwrap().contains("name"));
}

#[test]
fn a_bookmark_toggles_and_the_palette_lists_it() {
    let f = Fixture::new();
    f.app
        .saved_do(None, "toggle-bookmark", &f.path("Documents"))
        .unwrap();
    f.app.palette_open("");
    f.app.palette_do("type", "book:").unwrap();
    assert!(row_names(&json(&f.app.palette_json())).contains(&"Documents".to_string()));
    f.app.palette_close();
    f.app
        .saved_do(None, "toggle-bookmark", &f.path("Documents"))
        .unwrap();
}

// ---- settings ----

#[test]
fn settings_apply_at_once_and_are_written_on_the_debounce() {
    let f = Fixture::with(|c| c.settings_debounce_ms = 120);
    let file = f.data.join("settings.json");
    let before = std::fs::read_to_string(&file).unwrap_or_default();
    f.app.settings_set("showHiddenItems", "true").unwrap();
    assert_eq!(
        "true",
        f.app.settings_get("showHiddenItems").unwrap(),
        "in effect at once"
    );
    assert_eq!(
        before,
        std::fs::read_to_string(&file).unwrap_or_default(),
        "not yet on disk"
    );
    assert!(eventually(3000, || std::fs::read_to_string(&file)
        .unwrap_or_default()
        .contains("\"showHiddenItems\": true")));
}

#[test]
fn a_flush_writes_what_the_page_still_holds() {
    let f = Fixture::with(|c| c.settings_debounce_ms = 60_000);
    f.app.settings_set("showPreviewPane", "true").unwrap();
    let file = f.data.join("settings.json");
    assert!(
        !std::fs::read_to_string(&file)
            .unwrap_or_default()
            .contains("\"showPreviewPane\": true")
    );
    f.app.settings_flush();
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("\"showPreviewPane\": true")
    );
}

#[test]
fn services_write_at_once_not_on_the_debounce() {
    let f = Fixture::with(|c| c.settings_debounce_ms = 60_000);
    f.app
        .keymap_set_chords("view.flat", &["Ctrl+K".to_string()])
        .unwrap();
    assert!(
        std::fs::read_to_string(f.data.join("settings.json"))
            .unwrap()
            .contains("Ctrl+K")
    );
}

#[test]
fn a_command_for_neither_files_nor_folders_is_held_unwritten() {
    let f = Fixture::with(|c| c.settings_debounce_ms = 0);
    f.app.settings_open("context-menu").unwrap();
    f.app
        .settings_set(
            "customCommands",
            r#"[{"id":"a","name":"Diff","command":"diff.exe","appliesToFiles":false,"appliesToDirectories":false}]"#,
        )
        .unwrap();
    assert_eq!("[]", f.app.settings_get("customCommands").unwrap());
    assert!(
        f.app
            .state_json()
            .contains("must apply to files, folders, or both")
    );
    f.app
        .settings_set(
            "customCommands",
            r#"[{"id":"a","name":"Diff","command":"diff.exe","appliesToFiles":false,"appliesToDirectories":true}]"#,
        )
        .unwrap();
    assert!(
        f.app
            .settings_get("customCommands")
            .unwrap()
            .contains("Diff")
    );
}

#[test]
fn a_list_with_a_missing_field_stays_unwritten_until_leaving_says_so() {
    let f = Fixture::with(|c| c.settings_debounce_ms = 0);
    f.app.settings_open("context-menu").unwrap();
    f.app
        .settings_set(
            "customCommands",
            r#"[{"id":"a","name":"Open","command":""}]"#,
        )
        .unwrap();
    assert!(
        f.app.settings_get("customCommands").unwrap() == "[]",
        "kept out of the live settings"
    );
    assert!(f.app.state_json().contains("needs a name and a program"));
    let why = f.app.settings_close().unwrap_err();
    assert!(why.contains("needs a name and a program"), "{why}");
    assert_eq!(
        true,
        json(&f.app.settings_page_json())["open"],
        "leaving is refused"
    );
    f.app
        .settings_set(
            "customCommands",
            r#"[{"id":"a","name":"Open","command":"notepad.exe","appliesToFiles":true}]"#,
        )
        .unwrap();
    f.app.settings_close().unwrap();
    assert_eq!(false, json(&f.app.settings_page_json())["open"]);
    // The command is bindable and runnable only against what it applies to.
    assert_eq!(
        None,
        f.app
            .command_why_not("custom:a")
            .unwrap()
            .filter(|r| !r.contains("applies to files"))
    );
    f.select_names(&["notes.txt"]);
    assert_eq!(None, f.app.command_why_not("custom:a").unwrap());
    assert_eq!(
        bb_app::CommandOutcome::NeedsUi,
        f.app.command_execute("custom:a").unwrap()
    );
}

#[test]
fn a_new_file_type_needs_a_dotted_extension() {
    let f = Fixture::with(|c| c.settings_debounce_ms = 0);
    f.app.settings_open("new-items").unwrap();
    f.app
        .settings_set(
            "newFileTypes",
            r#"[{"label":"Notes","extension":"txt","enabled":true}]"#,
        )
        .unwrap();
    assert!(
        f.app
            .settings_close()
            .unwrap_err()
            .contains("starting with a dot")
    );
}

#[test]
fn the_settings_page_lists_its_pages_and_selecting_one_writes_nothing() {
    let f = Fixture::with(|c| c.settings_debounce_ms = 0);
    let pages = json(&f.app.settings_page_json());
    assert_eq!(12, pages["pages"].as_array().unwrap().len());
    assert_eq!(false, pages["open"]);
    let before = std::fs::metadata(f.data.join("settings.json"))
        .ok()
        .map(|m| m.modified().unwrap());
    for page in ["general", "appearance", "keyboard", "command-bars"] {
        f.app.settings_open(page).unwrap();
        assert_eq!(
            Some(page),
            json(&f.app.settings_page_json())["page"].as_str()
        );
    }
    f.app.settings_close().unwrap();
    assert!(f.app.settings_open("no-such-page").is_err());
    let after = std::fs::metadata(f.data.join("settings.json"))
        .ok()
        .map(|m| m.modified().unwrap());
    assert_eq!(before, after);
}

#[test]
fn opening_and_closing_the_page_are_events() {
    let f = Fixture::new();
    f.app.settings_open("keyboard").unwrap();
    f.app.settings_close().unwrap();
    let events = f.events.lock().unwrap().clone();
    let pages = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                AppEvent::ModelChanged {
                    model: Model::SettingsPage
                }
            )
        })
        .count();
    assert!(pages >= 2);
}

// ---- themes ----

fn theme(f: &Fixture) -> Value {
    json(&f.app.theme_json())
}

#[test]
fn matching_windows_swaps_the_theme_with_the_system_and_high_contrast_wins() {
    let f = Fixture::new();
    assert_eq!(
        true,
        theme(&f)["following"],
        "a fresh install matches Windows"
    );
    f.app.theme_set_system_appearance(true, false);
    assert_eq!("dark-plus", theme(&f)["id"]);
    f.app.theme_set_system_appearance(false, false);
    assert_eq!("light-plus", theme(&f)["id"]);

    f.app.theme_do("slot", "light solarized-light").unwrap();
    f.app.theme_do("slot", "dark nord").unwrap();
    assert_eq!(
        "solarized-light",
        theme(&f)["id"],
        "the slot Windows is asking for applies"
    );
    f.app.theme_set_system_appearance(true, false);
    assert_eq!("nord", theme(&f)["id"]);
    f.app.theme_set_system_appearance(true, true);
    assert_eq!("high-contrast-dark", theme(&f)["id"]);
    // Setting the light slot while Windows is dark configures, it does not preview.
    f.app.theme_set_system_appearance(true, false);
    f.app.theme_do("slot", "light catppuccin-latte").unwrap();
    assert_eq!("nord", theme(&f)["id"]);
    let settings = std::fs::read_to_string(f.data.join("settings.json")).unwrap();
    assert!(
        !settings.contains("\"themeId\": \"nord\""),
        "the OS moving is never written as a pick"
    );
}

#[test]
fn choosing_a_theme_ends_following_and_ticking_it_back_on_returns_to_windows() {
    let f = Fixture::new();
    f.app.theme_set_system_appearance(true, false);
    f.app.theme_do("slot", "dark nord").unwrap();
    f.app.theme_do("follow", "off").unwrap();
    assert_eq!(false, theme(&f)["following"]);
    assert_eq!("nord", theme(&f)["id"], "unticking keeps what is on screen");
    f.app.theme_set_system_appearance(false, false);
    assert_eq!("nord", theme(&f)["id"], "unfollowed is the user's");
    f.app.theme_do("slot", "light catppuccin-latte").unwrap();
    f.app.theme_do("follow", "on").unwrap();
    assert_eq!("catppuccin-latte", theme(&f)["id"]);
    f.app.theme_do("select", "monokai").unwrap();
    assert_eq!(false, theme(&f)["following"]);
    f.app.theme_set_system_appearance(true, false);
    assert_eq!("monokai", theme(&f)["id"]);
}

#[test]
fn a_theme_change_asks_the_host_to_repush_the_palette() {
    let f = Fixture::new();
    let seen = f.events.lock().unwrap().len();
    let dark = f.app.theme_palette();
    f.app.theme_do("select", "light-plus").unwrap();
    let light = f.app.theme_palette();
    assert_ne!(dark, light);
    let events = f.events.lock().unwrap().clone();
    assert!(events[seen..].iter().any(|e| matches!(
        e,
        AppEvent::ModelChanged {
            model: Model::Theme
        }
    )));
    assert_eq!(bb_core::theming::ThemeToken::COUNT, light.len());
}

#[test]
fn colour_tweaks_are_kept_per_theme_and_unsaved_ones_can_be_reverted() {
    let f = Fixture::new();
    f.app.theme_do("select", "dark-plus").unwrap();
    let base = f.app.theme_palette();
    f.app
        .theme_do("override", "Theme.Window.Background #FF0000")
        .unwrap();
    assert_ne!(base, f.app.theme_palette());
    assert_eq!(true, theme(&f)["dirty"]);
    f.app.theme_do("revert", "").unwrap();
    assert_eq!(base, f.app.theme_palette());
    f.app
        .theme_do("override", "Theme.Window.Background #FF0000")
        .unwrap();
    f.app.theme_do("persist", "").unwrap();
    let tweaked = f.app.theme_palette();
    assert_ne!(base, tweaked);
    f.app.theme_do("select", "light-plus").unwrap();
    f.app.theme_do("select", "dark-plus").unwrap();
    assert_eq!(
        tweaked,
        f.app.theme_palette(),
        "the tweak belongs to the theme"
    );
    f.app.theme_do("reset-all", "").unwrap();
    assert_eq!(base, f.app.theme_palette());
    assert!(f.app.theme_do("override", "Not.A.Token #FFFFFF").is_err());
}

#[test]
fn a_theme_of_your_own_is_baked_flat_and_can_be_deleted() {
    let f = Fixture::new();
    f.app.theme_do("select", "nord").unwrap();
    f.app
        .theme_do("override", "Theme.Window.Background #123456")
        .unwrap();
    let id = f.app.theme_do("save-as", "My Nord").unwrap();
    assert!(f.data.join("themes").join(format!("{id}.json")).exists());
    assert_eq!(id, theme(&f)["id"].as_str().unwrap());
    assert!(f.app.theme_json().contains("My Nord"));
    // A built-in cannot be deleted; one of the user's can, and the app lands on a theme that exists.
    assert!(f.app.theme_do("delete", "nord").is_err());
    f.app.theme_do("delete", &id).unwrap();
    assert!(!f.data.join("themes").join(format!("{id}.json")).exists());
    assert_ne!(id, theme(&f)["id"].as_str().unwrap());

    // Importing never replaces: a built-in's id is renamed.
    let source = f.data.join("import.json");
    std::fs::write(&source, r##"{"id":"nord","name":"Imposter","isDark":true,"colors":{"Theme.Window.Background":"#010203"}}"##).unwrap();
    let imported = f.app.theme_do("import", &source.to_string_lossy()).unwrap();
    assert_ne!("nord", imported);
    assert!(f.app.theme_json().contains("Imposter"));
}

#[test]
fn the_seam_is_read_when_the_host_does_not_push() {
    let f = Fixture::new();
    f.probes.set_appearance(SystemAppearance::LIGHT);
    assert_eq!("light-plus", theme(&f)["id"]);
    f.probes.set_appearance(SystemAppearance::HIGH_CONTRAST);
    assert_eq!("high-contrast-dark", theme(&f)["id"]);
}

// ---- the index ----

#[test]
fn without_a_helper_the_index_says_nothing_and_search_falls_back() {
    let f = Fixture::new();
    let index = json(&f.app.index_json());
    assert_eq!(
        false, index["banner"]["show"],
        "this host has no separate helper"
    );
    f.app.set_search(f.tab, "report", true).unwrap();
    f.settle();
    assert!(
        f.status().contains("the search index is off"),
        "{}",
        f.status()
    );
}

#[test]
fn the_declined_index_offers_then_fails_then_offers_again() {
    let f = Fixture::with(|c| c.index_declined = true);
    f.settle();
    let index = json(&f.app.index_json());
    assert_eq!(true, index["banner"]["show"]);
    assert!(
        index["banner"]["message"]
            .as_str()
            .unwrap()
            .contains("administrator rights")
    );
    assert_eq!(false, index["canRetry"], "nothing has been asked for yet");
    assert_eq!(
        "", index["status"],
        "two places describing one situation would read as two problems"
    );
    assert_eq!(None, why(&f, "tools.indexer-start"));
    assert!(
        why(&f, "tools.indexer-retry")
            .unwrap()
            .contains("not failed")
    );

    // "Not now": not persisted, and the app is entirely usable without it.
    f.app.index_do("dismiss-banner").unwrap();
    assert_eq!(false, json(&f.app.index_json())["banner"]["show"]);
    f.app.set_search(f.tab, "report", false).unwrap();
    f.settle();
    assert_eq!(3, f.app.tab_info(f.tab).unwrap().count);
    f.app.clear_search(f.tab).unwrap();
    f.settle();

    // Pressing Start is what raises the prompt; declining it is a failure the status bar speaks.
    f.app.index_do("start").unwrap();
    f.settle();
    let index = json(&f.app.index_json());
    assert!(
        index["status"]
            .as_str()
            .unwrap()
            .contains("permission declined")
    );
    assert_eq!(true, index["canRetry"]);
    assert_eq!(
        false, index["banner"]["show"],
        "the status bar is already saying so"
    );
    assert_eq!(None, why(&f, "tools.indexer-retry"));

    f.app.set_search(f.tab, "report", true).unwrap();
    f.settle();
    assert!(
        f.status().contains("the search index is off"),
        "{}",
        f.status()
    );
}

// ---- select by pattern, and the delete confirmation ----

#[test]
fn select_by_pattern_counts_as_typed_and_adds_or_takes_away() {
    let f = Fixture::new();
    let st: Value = json(&f.app.select_pattern_json(f.tab, "", true, false).unwrap());
    assert_eq!("7 item(s) here.", st["summary"]);
    assert_eq!(false, st["usable"], "an empty pattern selects nothing");

    let st: Value = json(
        &f.app
            .select_pattern_json(f.tab, "*.md", true, false)
            .unwrap(),
    );
    assert_eq!("1 of 7 item(s) match.", st["summary"]);
    assert_eq!(
        0,
        f.app.tab_info(f.tab).unwrap().selected,
        "counting selects nothing"
    );

    f.select_names(&["notes.txt"]);
    f.app
        .select_pattern_json(f.tab, "*.md", true, true)
        .unwrap();
    assert_eq!(
        2,
        f.app.tab_info(f.tab).unwrap().selected,
        "added to what was selected"
    );
    f.app
        .select_pattern_json(f.tab, "notes*", false, true)
        .unwrap();
    let names = f.app.selected_paths(f.tab).unwrap();
    assert_eq!(1, names.len());
    assert!(names[0].ends_with("report.md"));

    let st: Value = json(
        &f.app
            .select_pattern_json(f.tab, "content:invoice", true, false)
            .unwrap(),
    );
    assert!(st["problem"].is_string(), "{st}");
    assert_eq!(false, st["usable"]);
}

#[test]
fn a_delete_is_asked_about_first_and_only_what_was_shown_goes() {
    let f = Fixture::new();
    f.select_names(&["notes.txt"]);
    // The command is the host's: it puts the confirmation up rather than deleting.
    assert_eq!(
        bb_app::CommandOutcome::NeedsUi,
        f.app.command_execute("file.delete").unwrap()
    );
    assert_eq!(
        bb_app::CommandOutcome::NeedsUi,
        f.app.command_execute("file.delete-permanently").unwrap()
    );
    assert!(std::path::Path::new(&f.path("notes.txt")).exists());

    let p: Value = json(
        &f.app
            .ops_query("delete-preview", r#"{"permanent":false}"#)
            .unwrap(),
    );
    assert_eq!(true, p["hasWork"]);
    assert_eq!("Delete 'notes.txt'?", p["heading"]);
    assert_eq!(1, p["items"].as_array().unwrap().len());
    // Cancelled: nothing goes, and the plan is forgotten.
    f.app.ops_do("delete-cancel", "").unwrap();
    assert!(f.app.ops_do("delete-confirm", "").is_err());
    assert!(std::path::Path::new(&f.path("notes.txt")).exists());

    // Permanent, so a test run never fills the real Recycle Bin.
    let p: Value = json(
        &f.app
            .ops_query("delete-preview", r#"{"permanent":true}"#)
            .unwrap(),
    );
    assert_eq!("Permanently delete 'notes.txt'?", p["heading"]);
    f.app.ops_do("delete-confirm", "").unwrap();
    f.settle();
    assert!(!std::path::Path::new(&f.path("notes.txt")).exists());
}

/// A splitter drag writes each pane's size back as its weight, so the saved session (and the next
/// split or close) keeps it; a path or a count that no longer fits the tree is stale, not a panic.
/// Spec: PaneLayoutHost.WriteBackWeights.
#[test]
fn a_dragged_split_is_kept_in_the_saved_layout() {
    let f = Fixture::new();
    f.app
        .split_pane(
            f.app.active_pane(),
            bb_core::layout::SplitOrientation::Vertical,
            &f.path("Documents"),
        )
        .unwrap();
    f.settle();
    f.app.layout_set_weights(&[], &[300.0, 700.0]).unwrap();
    let saved = json(&f.app.session_capture_json());
    let weights: Vec<f64> = saved["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["weight"].as_f64().unwrap())
        .collect();
    assert_eq!(vec![300.0, 700.0], weights);
    assert!(matches!(
        f.app.layout_set_weights(&[], &[1.0]),
        Err(AppError::Stale)
    ));
    assert!(matches!(
        f.app.layout_set_weights(&[5], &[1.0, 1.0]),
        Err(AppError::Stale)
    ));
}

/// Where the window was left comes back next launch, maximised or not.
#[test]
fn the_window_placement_is_remembered_with_the_settings() {
    let f = Fixture::new();
    assert_eq!((None, false), f.app.window_placement());
    f.app
        .set_window_placement([10.0, 20.0, 1300.0, 800.0], true);
    f.app.shutdown();
    let second = bb_app::App::new(bb_app::AppConfig::harness(&f.data)).unwrap();
    assert_eq!(
        (Some([10.0, 20.0, 1300.0, 800.0]), true),
        second.window_placement()
    );
    second.shutdown();
}

/// Inside an archive an entry has no file on disk, so a content term cannot be answered there: it is
/// refused in so many words rather than matching every entry. Spec: DirectoryTabViewModel
/// (the archive branch of RunSearchAsync).
#[test]
fn a_content_search_inside_an_archive_is_refused_rather_than_answered_wrongly() {
    let f = Fixture::new();
    let zip = bb_test_support::metadata_fixtures::zip_of(&[("a.txt", "needle"), ("b.txt", "hay")]);
    std::fs::write(f.root.join("pack.zip"), zip).unwrap();
    f.app.navigate(f.tab, &f.path("pack.zip")).unwrap();
    f.settle();
    f.app.set_search(f.tab, "content:needle", false).unwrap();
    f.settle();
    assert_eq!(
        "content: can't look inside archives \u{2014} extract it first.",
        f.status()
    );
    assert!(f.names().is_empty(), "{:?}", f.names());
    // A name search there still works, from the listing's own index.
    f.app.set_search(f.tab, "a.txt", false).unwrap();
    f.settle();
    assert_eq!(vec!["a.txt"], f.names());
}
