//! The operations UI's models and verbs: the queue (reordered, cancelled, running included), what a clash
//! asks, the right-drag menu, rename preview, archives (extract, compress, edits that undo), shortcuts, the
//! undo history and the right-click menu's composition rules.

mod common;

use bb_app::OpId;
use common::{Fixture, eventually};
use serde_json::{Value, json};

fn query(f: &Fixture, what: &str, arg: &str) -> Value {
    serde_json::from_str(&f.app.ops_query(what, arg).unwrap()).unwrap()
}

fn run(f: &Fixture, verb: &str, arg: &str) -> Value {
    serde_json::from_str(&f.app.ops_do(verb, arg).unwrap()).unwrap()
}

fn op_of(v: &Value) -> OpId {
    OpId(v["op"].as_u64().expect("an operation was started"))
}

fn write(f: &Fixture, relative: &str, bytes: usize) -> String {
    let full = f.root.join(relative);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, vec![b'z'; bytes]).unwrap();
    full.to_string_lossy().into_owned()
}

// ---- the queue ----

#[test]
fn a_posed_queue_lists_the_running_job_first_and_reorders_only_what_waits() {
    let f = Fixture::new();
    run(&f, "pose-transfer", "queued|C:\\Archive");
    let q = query(&f, "queue", "");
    assert_eq!(true, q["active"]);
    assert_eq!(4, q["jobs"].as_array().unwrap().len());
    assert_eq!("running", q["jobs"][0]["state"]);
    assert_eq!(false, q["jobs"][0]["canUp"], "the running job never moves");
    assert_eq!(true, q["jobs"][3]["canUp"]);
    assert_eq!(false, q["jobs"][3]["canDown"]);

    // The last job twice up, past the one that waits ahead of it and no further.
    run(&f, "queue-move", r#"{"id":4,"delta":-1}"#);
    run(&f, "queue-move", r#"{"id":4,"delta":-1}"#);
    run(&f, "queue-move", r#"{"id":4,"delta":-1}"#);
    let q = query(&f, "queue", "");
    let ids: Vec<i64> = q["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["id"].as_i64().unwrap())
        .collect();
    assert_eq!(vec![1, 4, 2, 3], ids);

    run(&f, "queue-cancel", "4");
    let q = query(&f, "queue", "");
    assert_eq!("cancelled", q["jobs"][1]["state"]);
    assert_eq!(
        4,
        q["jobs"].as_array().unwrap().len(),
        "a cancelled job stays listed"
    );

    run(&f, "pose-transfer", "off");
    assert_eq!(false, query(&f, "queue", "")["active"]);
}

#[test]
fn pausing_changes_what_the_strip_says_and_stops_the_rates() {
    let f = Fixture::new();
    run(&f, "pose-transfer", "sized|C:\\Archive");
    let before = query(&f, "queue", "");
    assert!(
        before["view"]["headline"]
            .as_str()
            .unwrap()
            .starts_with("Copying 2 of 3")
    );
    assert!(!before["view"]["rateText"].as_str().unwrap().is_empty());

    run(&f, "queue-pause", "");
    let paused = query(&f, "queue", "");
    assert_eq!(true, paused["paused"]);
    assert_eq!(true, paused["canResume"]);
    assert_eq!(false, paused["canPause"]);
    assert!(
        paused["view"]["headline"]
            .as_str()
            .unwrap()
            .starts_with("Paused")
    );
    assert_eq!(
        "", paused["view"]["rateText"],
        "nothing is moving, so there is no rate"
    );
    run(&f, "queue-resume", "");
    run(&f, "pose-transfer", "off");
}

#[test]
fn a_running_transfer_can_be_cancelled_and_leaves_nothing_behind() {
    let f = Fixture::new();
    let dest = write(&f, "Out/keep.txt", 1);
    let out = std::path::Path::new(&dest).parent().unwrap().to_path_buf();
    f.app.op_pause(true);
    let op = f
        .app
        .op_copy(
            vec![f.path("photo.jpg"), f.path("archive.zip")],
            &out.to_string_lossy(),
        )
        .unwrap();
    // It has started (and is held at its first file by the pause) once the queue shows it running.
    assert!(eventually(10_000, || {
        query(&f, "queue", "")["jobs"][0]["state"] == "running"
    }));
    assert_eq!(true, query(&f, "queue", "")["canCancel"]);
    run(&f, "cancel-running", "");
    let (_, message) = f.wait_op(op);
    assert!(message.to_lowercase().contains("cancel"), "{message}");
    assert!(!out.join("photo.jpg").exists() || !out.join("archive.zip").exists());
    assert!(
        !f.app.is_paused(),
        "cancelling lets a paused run go, so it can notice"
    );
}

#[test]
fn a_waiting_job_that_is_cancelled_never_runs() {
    let f = Fixture::new();
    let out = f.root.join("Out");
    std::fs::create_dir_all(&out).unwrap();
    f.app.op_pause(true);
    let first = f
        .app
        .op_copy(vec![f.path("photo.jpg")], &out.to_string_lossy())
        .unwrap();
    let second = f
        .app
        .op_copy(vec![f.path("budget.xlsx")], &out.to_string_lossy())
        .unwrap();
    assert!(eventually(10_000, || {
        query(&f, "queue", "")["jobs"][0]["state"] == "running"
    }));
    f.app.op_cancel(second).unwrap();
    f.app.op_pause(false);
    f.wait_op(first);
    let (ok, message) = f.wait_op(second);
    assert!(!ok);
    assert_eq!("cancelled", message);
    assert!(out.join("photo.jpg").exists());
    assert!(!out.join("budget.xlsx").exists());
}

// ---- clashes ----

#[test]
fn a_clash_names_each_row_by_its_path_in_the_drop_and_defaults_identical_files_to_skip() {
    let f = Fixture::new();
    write(&f, "photos/2024/a.jpg", 11);
    write(&f, "photos/2024/c.jpg", 5);
    write(&f, "Backup/photos/2024/a.jpg", 14);
    write(&f, "Backup/photos/2024/b.jpg", 7);
    write(&f, "photos/2024/b.jpg", 7);
    let same = std::fs::metadata(f.path("photos/2024/b.jpg"))
        .unwrap()
        .modified()
        .unwrap();
    std::fs::File::options()
        .write(true)
        .open(f.path("Backup/photos/2024/b.jpg"))
        .unwrap()
        .set_modified(same)
        .unwrap();
    let model = run(
        &f,
        "h:conflict-plan",
        &json!({"verb":"copy","sources":[f.path("photos")],"destination":f.path("Backup")})
            .to_string(),
    );
    let rows = model["rows"].as_array().unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(vec!["photos\\2024\\a.jpg", "photos\\2024\\b.jpg"], names);
    assert_eq!("keep-both", rows[0]["default"]);
    assert_eq!("skip", rows[1]["default"]);
    assert_eq!(true, rows[1]["identical"]);
    assert_eq!(true, model["merged"]);
    assert!(
        model["title"]
            .as_str()
            .unwrap()
            .starts_with("Merging photos into Backup")
    );
}

#[test]
fn clashes_are_answered_by_label_and_a_cancel_abandons_the_transfer() {
    let f = Fixture::new();
    write(&f, "A/notes.txt", 3);
    write(&f, "B/notes.txt", 9);
    run(
        &f,
        "h:conflict",
        r#"{"mode":"named","answers":{"notes.txt":"replace"}}"#,
    );
    let op = f
        .app
        .op_move(vec![f.path("A/notes.txt")], &f.path("B"))
        .unwrap();
    let (_, _) = f.wait_op(op);
    assert!(
        !f.root.join("A/notes.txt").exists(),
        "replaced: the incoming copy took the name"
    );
    assert_eq!(3, std::fs::metadata(f.path("B/notes.txt")).unwrap().len());

    write(&f, "A/notes.txt", 4);
    run(&f, "h:conflict", r#"{"mode":"cancel"}"#);
    let op = f
        .app
        .op_move(vec![f.path("A/notes.txt")], &f.path("B"))
        .unwrap();
    let (ok, message) = f.wait_op(op);
    assert!(!ok);
    assert_eq!("cancelled", message);
    assert!(f.root.join("A/notes.txt").exists());
    assert_eq!(2, query(&f, "conflicts", "")["asked"]);
}

// ---- right-drag and drops ----

#[test]
fn the_right_drag_menu_greys_a_move_that_has_nothing_to_do() {
    let f = Fixture::new();
    let items = query(
        &f,
        "right-drop-menu",
        &json!({"sources":[f.path("notes.txt")],"destination":f.root.to_string_lossy()})
            .to_string(),
    );
    let verbs: Vec<(&str, bool)> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["verb"].as_str().unwrap(), e["enabled"].as_bool().unwrap()))
        .collect();
    assert_eq!(
        vec![("move", false), ("copy", true), ("shortcut", true)],
        verbs
    );
    assert_eq!("Create shortcut here", items[2]["label"]);
}

#[test]
fn an_in_app_drop_reports_nothing_and_a_foreign_one_reports_the_verb_it_did() {
    let f = Fixture::new();
    let ours = query(
        &f,
        "drop-decide",
        r#"{"origin":"in-app","control":false,"shift":false,"allowed":3}"#,
    );
    assert_eq!("move", ours["verb"]);
    assert_eq!(0, ours["report"]);
    let foreign = query(
        &f,
        "drop-decide",
        r#"{"origin":"external","control":false,"shift":false,"allowed":3}"#,
    );
    assert_eq!("copy", foreign["verb"]);
    assert_eq!(1, foreign["report"]);
    assert_eq!(
        "remove",
        query(
            &f,
            "dragout-decide",
            r#"{"handledInApp":false,"returned":2}"#
        ),
        "a foreign move with nothing written back is read as a move"
    );
    assert_eq!(
        "nothing",
        query(
            &f,
            "dragout-decide",
            r#"{"handledInApp":true,"returned":2}"#
        ),
        "our own pipeline already moved the items"
    );
}

// ---- rename ----

#[test]
fn the_rename_preview_says_what_each_item_becomes_and_warns_about_lost_extensions() {
    let f = Fixture::new();
    let paths = json!([f.path("photo.jpg"), f.path("budget.xlsx")]);
    let simple = query(
        &f,
        "rename-preview",
        &json!({"paths":paths,"rule":null,"pattern":"Holiday"}).to_string(),
    );
    assert_eq!(true, simple["hasWork"]);
    assert_eq!("Holiday 1.jpg", simple["rows"][0]["newName"]);

    let rule = json!({"template": "Photo {n:00}", "counterStart": 1, "counterStep": 1});
    let p = query(
        &f,
        "rename-preview",
        &json!({"paths":paths,"rule":rule,"pattern":""}).to_string(),
    );
    assert!(
        p["hint"]
            .as_str()
            .unwrap()
            .contains("lose their extensions")
    );
    assert_eq!("Photo 01", p["rows"][0]["newName"]);
}

// ---- archives ----

#[test]
fn compress_then_extract_round_trips_and_neither_enters_the_undo_history() {
    let f = Fixture::new();
    let target = f.path("Bundle.zip");
    let op = op_of(&run(
        &f,
        "compress",
        &json!({"sources":[f.path("notes.txt"), f.path("photo.jpg")],"target":target,"format":"zip","level":"normal"}).to_string(),
    ));
    let (ok, message) = f.wait_op(op);
    assert!(ok, "{message}");
    assert!(std::path::Path::new(&target).exists());
    assert!(!f.app.can_undo(), "making an archive is additive");

    let out = f.root.join("Unpacked");
    std::fs::create_dir_all(&out).unwrap();
    let archive_tab = f.app.open_tab(None, &target, true).unwrap();
    f.settle();
    let op = op_of(&run(
        &f,
        "extract",
        &json!({"tab":archive_tab.0,"names":[],"destination":out.to_string_lossy(),"conflict":"keep-both"}).to_string(),
    ));
    let (ok, message) = f.wait_op(op);
    assert!(ok, "{message}");
    assert_eq!(100, std::fs::metadata(out.join("notes.txt")).unwrap().len());
    assert!(!f.app.can_undo());
}

#[test]
fn deleting_inside_an_archive_rewrites_it_and_the_undo_history_puts_it_back() {
    let f = Fixture::new();
    let target = f.path("Bundle.zip");
    let op = op_of(&run(
        &f,
        "compress",
        &json!({"sources":[f.path("notes.txt"), f.path("report.md")],"target":target,"format":"zip","level":"store"}).to_string(),
    ));
    f.wait_op(op);
    let inside = format!("{target}\\notes.txt");
    let op = f
        .app
        .op_delete(vec![inside.clone()], bb_core::delete::DeleteMode::Recycle)
        .unwrap();
    let (ok, message) = f.wait_op(op);
    assert!(ok, "{message}");
    assert!(f.app.can_undo());
    let tab = f.app.open_tab(None, &target, true).unwrap();
    f.settle();
    let names: Vec<String> = f
        .app
        .listing(tab)
        .unwrap()
        .rows()
        .iter()
        .map(|r| r.name.clone())
        .collect();
    assert_eq!(vec!["report.md"], names);

    let undo = f.app.undo_step(1).unwrap();
    let (ok, message) = f.wait_op(undo);
    assert!(ok, "{message}");
    f.settle();
    let names: Vec<String> = f
        .app
        .listing(tab)
        .unwrap()
        .rows()
        .iter()
        .map(|r| r.name.clone())
        .collect();
    assert_eq!(2, names.len(), "the open tab reloads: {names:?}");
}

// ---- shortcuts and the history ----

#[test]
fn shortcuts_are_real_links_named_the_way_explorer_names_them() {
    let f = Fixture::new();
    let out = f.root.join("Pictures");
    // Made first and waited for: a folder that appears in the watched root during the run is merged in by
    // the watcher a debounce later, and that reload ends by writing "N items" over the sentence checked below.
    std::fs::create_dir_all(&out).unwrap();
    let start = std::time::Instant::now();
    while !f.names().iter().any(|n| n == "Pictures") {
        assert!(
            start.elapsed().as_secs() < 5,
            "the watcher never listed Pictures"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    f.settle();
    let op = op_of(&run(
        &f,
        "shortcut",
        &json!({"sources":[f.path("notes.txt")],"directory":out.to_string_lossy()}).to_string(),
    ));
    let (ok, message) = f.wait_op(op);
    assert!(ok, "{message}");
    let link = out.join("notes.txt - Shortcut.lnk");
    assert!(link.exists());
    assert!(
        std::fs::metadata(&link).unwrap().len() > 100,
        "an IShellLink wrote it"
    );
    assert!(!f.app.can_undo(), "additive: the undo slot is left alone");
    assert!(f.status().contains("Created"), "{}", f.status());
}

#[test]
fn the_history_counts_what_the_budget_released_and_walks_to_an_entry() {
    let f = Fixture::new();
    for (from, to) in [
        ("notes.txt", "a.txt"),
        ("report.md", "b.md"),
        ("budget.xlsx", "c.xlsx"),
    ] {
        let op = f.app.op_rename(vec![f.path(from)], to).unwrap();
        f.wait_op(op);
    }
    let h = query(&f, "history", "");
    assert_eq!(3, h["count"]);
    assert_eq!(3, h["cursor"]);
    assert_eq!(0, h["released"]);
    assert_eq!(
        1, h["rows"][0]["steps"],
        "the newest entry is one step back"
    );
    assert_eq!(3, h["rows"][2]["steps"]);

    run(&f, "undo-budget", r#"{"steps":2}"#);
    let h = query(&f, "history", "");
    assert_eq!(2, h["count"]);
    assert_eq!(1, h["released"]);
    assert!(h["releasedText"].as_str().unwrap().contains("older action"));

    let op = f.app.undo_step(2).unwrap();
    f.wait_op(op);
    let h = query(&f, "history", "");
    assert_eq!(0, h["cursor"]);
    assert_eq!(false, h["rows"][0]["isDone"]);
    assert_eq!(
        2, h["rows"][0]["steps"],
        "redoing to the newest takes both undone entries"
    );
}

// ---- the right-click menu ----

#[test]
fn the_menu_layout_orders_everything_and_a_tidy_pass_removes_touching_separators() {
    let f = Fixture::new();
    let arranged = query(
        &f,
        "menu-arrange",
        &json!({"builtIns":["cut","copy","paste","properties"],"commands":[],"shell":["clsid:{abc}"]}).to_string(),
    );
    let kinds: Vec<String> = arranged
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            format!(
                "{}:{}",
                p["kind"].as_str().unwrap(),
                p["id"].as_str().unwrap()
            )
        })
        .collect();
    let pos = |needle: &str| {
        kinds
            .iter()
            .position(|k| k == needle)
            .unwrap_or_else(|| panic!("{needle} in {kinds:?}"))
    };
    assert!(pos("builtin:cut") < pos("builtin:paste"));
    assert!(
        pos("shell:clsid:{abc}") > pos("builtin:paste"),
        "an unplaced extension lands at @more"
    );
    assert!(pos("shell:clsid:{abc}") < pos("builtin:properties"));

    let visible = query(
        &f,
        "menu-tidy",
        r#"{"slots":["separator","separator","item","separator","hidden","separator","item","separator"]}"#,
    );
    assert_eq!(
        json!([false, false, true, true, false, false, true, false]),
        visible
    );
}

#[test]
fn registrations_become_a_catalog_and_the_pages_ticks_decide_what_is_shown() {
    let f = Fixture::new();
    let catalog = query(
        &f,
        "shell-catalog",
        &json!({
            "handlers": [{"family":"*","name":"7-Zip","clsid":"{23170F69-40C1-278A-1000-000100020000}","className":"7-Zip Shell Extension"}],
            "verbs": [{"family":"Directory","verb":"git_shell","displayName":"Open Git Bash here","command":"git-bash.exe"}]
        })
        .to_string(),
    );
    let ids: Vec<&str> = catalog
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        vec![
            "verb:git_shell",
            "clsid:{23170f69-40c1-278a-1000-000100020000}"
        ],
        ids
    );

    f.app
        .settings_set("hiddenShellExtensions", r#"["verb:git_shell"]"#)
        .unwrap();
    let shown = query(&f, "shell-visible", &json!({"ids": ids}).to_string());
    assert_eq!(
        json!(["clsid:{23170f69-40c1-278a-1000-000100020000}"]),
        shown
    );
    f.app.settings_set("showShellExtensions", "false").unwrap();
    assert_eq!(
        json!([]),
        query(&f, "shell-visible", &json!({"ids": ids}).to_string())
    );
}

// ---- the product's questions ----

/// An app over the interactive seams: no probes, and a clash waits for the host to answer it.
fn interactive() -> (
    bb_app::App,
    std::path::PathBuf,
    std::path::PathBuf,
    std::sync::Arc<std::sync::Mutex<Vec<bb_app::AppEvent>>>,
) {
    let data = bb_test_support::unique_temp_path("bb-ops-data", "");
    let root = bb_test_support::unique_temp_path("bb-ops-tree", "");
    std::fs::create_dir_all(root.join("A")).unwrap();
    std::fs::create_dir_all(root.join("B")).unwrap();
    std::fs::write(root.join("A/n.txt"), "incoming").unwrap();
    std::fs::write(root.join("B/n.txt"), "existing!!").unwrap();
    let mut config = bb_app::AppConfig::new(&data);
    config.index_declined = true;
    config.watch_debounce_ms = 40;
    config.settings_debounce_ms = 0;
    let app = bb_app::App::new(config).unwrap();
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    {
        let events = events.clone();
        app.set_event_sink(Some(std::sync::Arc::new(move |e| {
            events.lock().unwrap().push(e)
        })));
    }
    app.initialize(&root.to_string_lossy()).unwrap();
    (app, root, data, events)
}

fn asked(events: &std::sync::Mutex<Vec<bb_app::AppEvent>>) -> bool {
    events.lock().unwrap().iter().any(|e| {
        matches!(
            e,
            bb_app::AppEvent::ModelChanged {
                model: bb_app::Model::Conflicts
            }
        )
    })
}

#[test]
fn a_clash_parks_the_transfer_until_the_host_answers_it() {
    let (app, root, data, events) = interactive();
    let op = app
        .op_move(
            vec![root.join("A/n.txt").to_string_lossy().into_owned()],
            &root.join("B").to_string_lossy(),
        )
        .unwrap();
    assert!(
        eventually(15_000, || asked(&events)),
        "the host was never asked"
    );
    let pending: Value = serde_json::from_str(&app.ops_query("conflicts", "").unwrap()).unwrap();
    let row = &pending["pending"]["rows"][0];
    assert_eq!("n.txt", row["name"]);
    assert!(
        matches!(app.op_state(op), Some(bb_app::OpState::Running)),
        "the transfer is waiting, not finished"
    );

    app.ops_do(
        "conflict-answer",
        &json!({"answers": {row["groupKey"].as_str().unwrap(): "replace"}}).to_string(),
    )
    .unwrap();
    assert!(eventually(15_000, || matches!(
        app.op_state(op),
        Some(bb_app::OpState::Done { .. })
    )));
    assert!(
        !root.join("A/n.txt").exists(),
        "Replace: the incoming copy took the name"
    );
    assert_eq!(8, std::fs::metadata(root.join("B/n.txt")).unwrap().len());
    let again: Value = serde_json::from_str(&app.ops_query("conflicts", "").unwrap()).unwrap();
    assert!(
        again["pending"].is_null(),
        "the question is gone once answered"
    );
    app.shutdown();
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(data);
}

#[test]
fn closing_the_clash_dialog_cancels_the_transfer_rather_than_leaving_it_waiting() {
    let (app, root, data, events) = interactive();
    let op = app
        .op_move(
            vec![root.join("A/n.txt").to_string_lossy().into_owned()],
            &root.join("B").to_string_lossy(),
        )
        .unwrap();
    assert!(eventually(15_000, || asked(&events)));
    app.ops_do("conflict-answer", r#"{"cancel":true}"#).unwrap();
    assert!(eventually(15_000, || matches!(
        app.op_state(op),
        Some(bb_app::OpState::Done { ok: false, .. })
    )));
    assert!(root.join("A/n.txt").exists(), "nothing was moved");
    app.shutdown();
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(data);
}
