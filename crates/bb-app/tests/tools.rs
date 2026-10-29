//! The tool windows' state: checksums, the file compare, disk usage, the change timeline, and the
//! folder comparison's banner, filter, refusals, ending and sync.

mod common;

use bb_app::TabId;
use bb_core::delete::DeleteMode;
use bb_core::layout::SplitOrientation;
use common::Fixture;
use serde_json::{Value, json};

fn call(f: &Fixture, verb: &str, args: Value) -> Value {
    let text = f
        .app
        .tools(verb, &args.to_string())
        .unwrap_or_else(|e| panic!("{verb}: {e}"));
    // A checksum verb that reads files starts a run off the thread: wait for it, as the window does
    // for its event, and answer with the model it left.
    if matches!(
        verb,
        "checksum.hash"
            | "checksum.verify"
            | "checksum.algorithms"
            | "checksum.expected"
            | "checksum.rehash"
    ) {
        f.settle();
        let model = f.app.tools("checksum.model", "{}").unwrap();
        return serde_json::from_str(&model).unwrap();
    }
    // So does comparing two files.
    if matches!(
        verb,
        "filecompare.open" | "filecompare.options" | "filecompare.swap"
    ) {
        f.settle();
        let model = f.app.tools("filecompare.model", "{}").unwrap();
        return serde_json::from_str(&model).unwrap();
    }
    serde_json::from_str(&text).unwrap()
}

fn call_err(f: &Fixture, verb: &str, args: Value) -> String {
    f.app
        .tools(verb, &args.to_string())
        .expect_err("should have been refused")
        .to_string()
}

fn row<'a>(model: &'a Value, name: &str) -> &'a Value {
    model["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("no row {name}"))
}

// ---- checksums ----------------------------------------------------------------------------------------

#[test]
fn a_file_is_hashed_and_each_digest_is_shown_in_its_own_case() {
    let f = Fixture::new();
    std::fs::write(f.root.join("abc.txt"), "abc").unwrap();
    let path = f.path("abc.txt");
    let m = call(
        &f,
        "checksum.hash",
        json!({"paths": [path], "algorithms": ["Sha256", "Crc32", "Md5"]}),
    );
    let r = row(&m, "abc.txt");
    // Digests are uppercase inside; the case they are shown in is ChecksumAlgorithms' one decision.
    assert_eq!(
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        r["computed"]["Sha256"].as_str().unwrap().to_lowercase()
    );
    let shown: Vec<(&str, &str)> = r["digests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["algorithm"].as_str().unwrap(),
                d["digest"].as_str().unwrap(),
            )
        })
        .collect();
    let sha = shown.iter().find(|(a, _)| *a == "Sha256").unwrap().1;
    assert_eq!(sha, sha.to_lowercase(), "sha256sum files are lower case");
    let crc = shown.iter().find(|(a, _)| *a == "Crc32").unwrap().1;
    assert_eq!(
        "352441C2", crc,
        "CRC-32 is shown the way an .sfv writes it, big-endian and upper case"
    );
}

#[test]
fn ticking_another_algorithm_reads_only_what_is_missing_and_a_paste_ticks_its_own() {
    let f = Fixture::new();
    std::fs::write(f.root.join("abc.txt"), "abc").unwrap();
    let m = call(
        &f,
        "checksum.hash",
        json!({"paths": [f.path("abc.txt")], "algorithms": ["Sha256"]}),
    );
    assert_eq!(1, row(&m, "abc.txt")["computed"].as_object().unwrap().len());

    // A pasted MD5 names the algorithm being worked in: it is ticked and computed.
    let m = call(
        &f,
        "checksum.expected",
        json!({"text": "900150983CD24FB0D6963F7D28E17F72  abc.txt"}),
    );
    assert_eq!("\u{2713} Matches", m["pasteVerdict"]);
    assert!(m["pasteMatched"].as_bool().unwrap());
    let ticked: Vec<&str> = m["algorithms"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["selected"] == true)
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(vec!["Md5", "Sha256"], ticked);

    let m = call(&f, "checksum.expected", json!({"text": "not a checksum"}));
    assert_eq!("That does not look like a checksum.", m["pasteVerdict"]);
    let m = call(
        &f,
        "checksum.expected",
        json!({"text": "00000000000000000000000000000000"}),
    );
    assert_eq!("\u{2717} Does not match", m["pasteVerdict"]);
}

#[test]
fn a_checksum_file_verifies_each_name_and_never_follows_one_out_of_its_folder() {
    let f = Fixture::new();
    std::fs::write(f.root.join("a.txt"), "abc").unwrap();
    std::fs::write(f.root.join("b.txt"), "def").unwrap();
    std::fs::write(f.root.join("c.txt"), "ghi").unwrap();
    let sums = f.path("sums.sha256");
    call(
        &f,
        "checksum.write-file",
        json!({"path": sums, "algorithm": "Sha256",
               "files": [f.path("a.txt"), f.path("b.txt"), f.path("c.txt")]}),
    );
    let written = std::fs::read_to_string(&sums).unwrap();
    assert!(
        written.contains("ba7816bf8f01cfea"),
        "written lower case: {written}"
    );
    // A hostile line is a visible row, not a path that gets opened.
    std::fs::write(
        &sums,
        format!("{written}{}  ..\\..\\Windows\\win.ini\n", "0".repeat(64)),
    )
    .unwrap();
    std::fs::write(f.root.join("b.txt"), "changed").unwrap();
    std::fs::remove_file(f.root.join("c.txt")).unwrap();

    let m = call(&f, "checksum.verify", json!({"path": sums}));
    assert_eq!("Ok", row(&m, "a.txt")["verifyState"]);
    assert_eq!("Mismatch", row(&m, "b.txt")["verifyState"]);
    assert_eq!("Missing", row(&m, "c.txt")["verifyState"]);
    assert_eq!(
        "Refused",
        row(&m, "..\\..\\Windows\\win.ini")["verifyState"]
    );
    assert!(m["verifySummary"].as_str().unwrap().contains("mismatched"));
    // Verify checks against the file's own algorithm; the ticks describe the other mode.
    let m = call(&f, "checksum.algorithms", json!({"algorithms": ["Md5"]}));
    let on: Vec<&str> = m["algorithms"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["selected"] == true)
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(vec!["Sha256"], on);
}

#[test]
fn saving_lists_names_relative_to_the_checksum_file_and_says_what_it_left_out() {
    let f = Fixture::new();
    let m = call(
        &f,
        "checksum.hash",
        json!({"paths": [f.path("notes.txt"), f.path("Documents\\report.md")], "algorithms": ["Sha1"]}),
    );
    assert!(m["canSave"].as_bool().unwrap());
    let target = f.path("Documents\\files.sha1");
    let r = call(
        &f,
        "checksum.save",
        json!({"path": target, "algorithm": "Sha1"}),
    );
    assert!(r["ok"].as_bool().unwrap());
    assert!(
        r["note"]
            .as_str()
            .unwrap()
            .contains("1 file(s) are not under this folder")
    );
    let text = std::fs::read_to_string(&target).unwrap();
    assert!(
        text.contains("report.md") && !text.contains("Documents"),
        "{text}"
    );
    assert!(!text.contains("notes.txt"));
}

// ---- comparing two files ------------------------------------------------------------------------------

#[test]
fn a_pair_of_text_files_is_diffed_and_a_pair_of_binaries_is_dumped() {
    let f = Fixture::new();
    std::fs::write(f.root.join("one.txt"), "a\nb\nc\n").unwrap();
    std::fs::write(f.root.join("two.txt"), "a\nB\nc\n").unwrap();
    let m = call(
        &f,
        "filecompare.open",
        json!({"left": f.path("one.txt"), "right": f.path("two.txt")}),
    );
    assert_eq!("text", m["mode"]);
    assert_eq!("1 difference", m["counter"]);
    let m = call(&f, "filecompare.options", json!({"ignoreCase": true}));
    assert_eq!(
        "",
        m["counter"].as_str().unwrap().replace('\u{2014}', ""),
        "case ignored, nothing differs"
    );

    let bytes: Vec<u8> = (0..4096u32).map(|i| (i % 253) as u8 | 0x80).collect();
    std::fs::write(f.root.join("x.bin"), &bytes).unwrap();
    let mut other = bytes.clone();
    other[3000] ^= 1;
    std::fs::write(f.root.join("y.bin"), other).unwrap();
    let m = call(
        &f,
        "filecompare.open",
        json!({"left": f.path("x.bin"), "right": f.path("y.bin")}),
    );
    assert_eq!("binary", m["mode"]);
    assert!(m["canForceText"].as_bool().unwrap());
    assert!(
        m["verdict"]
            .as_str()
            .unwrap()
            .contains("first difference at byte 3,000")
    );
}

// ---- disk usage ---------------------------------------------------------------------------------------

#[test]
fn disk_usage_reads_its_folder_off_the_ui_thread_and_a_later_read_wins() {
    let f = Fixture::new();
    let root = f.root.to_string_lossy().to_string();
    let docs = f.root.join("Documents").to_string_lossy().to_string();
    let first = call(&f, "diskusage.load", json!({ "path": root }));
    assert_eq!(
        root, first["path"],
        "the folder asked for is the one being read"
    );
    call(&f, "diskusage.load", json!({ "path": docs }));
    f.settle();
    let st = call(&f, "diskusage.state", json!({}));
    assert!(!st["loading"].as_bool().unwrap());
    assert_eq!(docs, st["path"]);
    assert_eq!(docs, st["model"]["root"], "the superseded read never lands");
    assert!(
        !call_err(
            &f,
            "diskusage.load",
            json!({ "path": f.root.join("nope").to_string_lossy() })
        )
        .is_empty(),
        "a folder that is not there is refused, not read"
    );
}

#[test]
fn a_folder_nobody_measured_reads_not_measured_and_gets_no_share() {
    let f = Fixture::new();
    let m = call(
        &f,
        "diskusage.model",
        json!({"path": f.root.to_string_lossy()}),
    );
    let kids = m["children"].as_array().unwrap();
    let documents = kids.iter().find(|c| c["name"] == "Documents").unwrap();
    assert_eq!("Not measured", documents["sizeText"], "never 0 B");
    assert!(documents["unknown"].as_bool().unwrap());
    assert_eq!(0.0, documents["fraction"].as_f64().unwrap());
    // Files come from the enumeration, so their sizes are real with nothing indexed.
    let zip = kids.iter().find(|c| c["name"] == "archive.zip").unwrap();
    assert_eq!("19.5 KB", zip["sizeText"]);
    assert_eq!(1.0, zip["fraction"].as_f64().unwrap());
    assert!(
        m["hasMessage"].as_bool().unwrap(),
        "the banner says the totals are missing"
    );
    assert!(!m["fills"].as_array().unwrap().is_empty());

    let tiles = call(
        &f,
        "treemap.layout",
        json!({"weights": [3.0, 0.0, 1.0], "width": 80.0, "height": 40.0}),
    );
    assert_eq!(
        2,
        tiles.as_array().unwrap().len(),
        "an unmeasured item has no area"
    );
}

// ---- the change timeline ------------------------------------------------------------------------------

#[test]
fn recording_is_off_until_asked_for_and_turning_it_off_wipes_the_log() {
    let f = Fixture::new();
    let m = call(
        &f,
        "changes.open",
        json!({"path": f.root.to_string_lossy()}),
    );
    assert_eq!("RecordingOff", m["availability"]);
    assert!(m["canOpenSettings"].as_bool().unwrap());
    assert!(m["rows"].as_array().unwrap().is_empty());

    let seeded = call(
        &f,
        "changes.seed",
        json!({"root": f.root.to_string_lossy()}),
    );
    assert!(seeded["rows"].as_i64().unwrap() >= 8);
    let m = call(&f, "changes.model", json!({}));
    assert_ne!("RecordingOff", m["availability"]);
    let rows = m["rows"].as_array().unwrap();
    assert!(!rows.is_empty());
    let log = rows.iter().find(|r| r["name"] == "app.log").unwrap();
    assert_eq!("\u{d7}5", log["countText"], "five writes are one row");
    let renamed = rows.iter().find(|r| r["kind"] == "Renamed").unwrap();
    assert_eq!("was installer.part", renamed["renamedFrom"]);
    assert!(
        rows.iter().all(|r| r["name"] != "stale.txt"),
        "past retention is pruned"
    );
    assert!(
        rows.iter().all(|r| r["name"] != "index"),
        "hidden entries wait for the box"
    );
    let m = call(&f, "changes.set", json!({"hidden": true}));
    assert!(
        m["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == "index")
    );
    let m = call(
        &f,
        "changes.set",
        json!({"deleted": false, "hidden": false}),
    );
    assert!(
        m["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["kind"] != "Deleted")
    );

    // The switch is the settings page's; off deletes what was recorded.
    f.app.settings_set("recordFileChanges", "false").unwrap();
    let m = call(&f, "changes.model", json!({}));
    assert_eq!("RecordingOff", m["availability"]);
    assert!(m["rows"].as_array().unwrap().is_empty());
    f.app.settings_set("recordFileChanges", "true").unwrap();
    let m = call(&f, "changes.set", json!({"deleted": true}));
    assert!(
        m["rows"].as_array().unwrap().is_empty(),
        "the wipe was real"
    );
}

#[test]
fn clearing_the_history_now_empties_it_without_turning_recording_off() {
    let f = Fixture::new();
    call(
        &f,
        "changes.seed",
        json!({"root": f.root.to_string_lossy()}),
    );
    call(
        &f,
        "changes.open",
        json!({"path": f.root.to_string_lossy()}),
    );
    assert!(
        !call(&f, "changes.model", json!({}))["rows"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    call(&f, "changes.clear", json!({}));
    let m = call(&f, "changes.model", json!({}));
    assert!(m["rows"].as_array().unwrap().is_empty());
    assert!(m["recording"].as_bool().unwrap());
}

// ---- comparing two folders ----------------------------------------------------------------------------

struct Pair {
    left: TabId,
    right: TabId,
    pane: bb_app::PaneId,
    right_pane: bb_app::PaneId,
}

fn write(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// left\ and right\ side by side: shared (equal), only-left, only-right, and a folder that differs
/// deep inside.
fn two_panes(f: &Fixture) -> Pair {
    let (l, r) = (f.root.join("l"), f.root.join("r"));
    write(&l.join("shared.txt"), "same");
    write(&r.join("shared.txt"), "same");
    write(&l.join("only-left.txt"), "mine");
    write(&r.join("only-right.txt"), "theirs");
    write(&l.join("deep\\changed.txt"), "short");
    write(&r.join("deep\\changed.txt"), "a good deal longer");
    f.app.navigate(f.tab, &l.to_string_lossy()).unwrap();
    f.settle();
    let pane = f.app.active_pane();
    let right_pane = f
        .app
        .split_pane(pane, SplitOrientation::Vertical, &r.to_string_lossy())
        .unwrap();
    f.settle();
    f.app.activate_pane(pane).unwrap();
    f.settle();
    let right = f
        .app
        .active_tab_of(right_pane)
        .unwrap()
        .expect("the right tab");
    Pair {
        left: f.tab,
        right,
        pane,
        right_pane,
    }
}

fn compare(f: &Fixture) -> Value {
    f.app.compare_toggle().unwrap();
    f.settle();
    call(f, "compare.model", json!({}))
}

fn names(f: &Fixture, tab: TabId) -> Vec<String> {
    f.app
        .listing(tab)
        .unwrap()
        .rows()
        .iter()
        .map(|r| r.name.clone())
        .collect()
}

#[test]
fn the_banner_counts_what_the_sync_would_do_and_says_when_nothing_differs() {
    let f = Fixture::new();
    two_panes(&f);
    let m = call(&f, "compare.model", json!({}));
    assert!(
        !m["active"].as_bool().unwrap(),
        "no banner until there is a comparison"
    );
    let m = compare(&f);
    assert!(m["active"].as_bool().unwrap());
    assert_eq!(
        "1 only on the left \u{b7} 1 only on the right \u{b7} 1 differ",
        m["summary"]
    );
    assert!(m["canSync"].as_bool().unwrap());

    // The folders are made to match; a rescan says so.
    std::fs::copy(
        f.root.join("l\\only-left.txt"),
        f.root.join("r\\only-left.txt"),
    )
    .unwrap();
    std::fs::remove_file(f.root.join("r\\only-right.txt")).unwrap();
    std::fs::copy(
        f.root.join("l\\deep\\changed.txt"),
        f.root.join("r\\deep\\changed.txt"),
    )
    .unwrap();
    f.app.tools("compare.rescan", "{}").unwrap();
    f.settle();
    assert_eq!(
        "These folders match.",
        call(&f, "compare.model", json!({}))["summary"]
    );
}

#[test]
fn differences_only_hides_what_the_comparison_has_nothing_to_say_about_on_both_sides() {
    let f = Fixture::new();
    let pair = two_panes(&f);
    compare(&f);
    assert!(names(&f, pair.left).contains(&"shared.txt".to_string()));
    call(&f, "compare.filter", json!({"on": true}));
    f.settle();
    for tab in [pair.left, pair.right] {
        assert!(
            !names(&f, tab).contains(&"shared.txt".to_string()),
            "{:?}",
            names(&f, tab)
        );
        assert!(names(&f, tab).contains(&"deep".to_string()));
    }
    call(&f, "compare.filter", json!({"on": false}));
    f.settle();
    assert!(
        names(&f, pair.left).contains(&"shared.txt".to_string()),
        "nothing is lost by filtering"
    );
    assert!(names(&f, pair.right).contains(&"shared.txt".to_string()));
}

#[test]
fn a_comparison_ends_when_a_pane_leaves_its_folder_goes_flat_or_closes() {
    let f = Fixture::new();
    let pair = two_panes(&f);
    compare(&f);
    // Walking into a subfolder both sides have keeps it.
    f.app.navigate(pair.left, &f.path("l\\deep")).unwrap();
    f.settle();
    assert!(
        call(&f, "compare.model", json!({}))["active"]
            .as_bool()
            .unwrap()
    );
    // Leaving the compared folder ends it.
    f.app
        .navigate(pair.left, &f.root.to_string_lossy())
        .unwrap();
    f.settle();
    assert!(
        !call(&f, "compare.model", json!({}))["active"]
            .as_bool()
            .unwrap()
    );
    f.app.navigate(pair.left, &f.path("l")).unwrap();
    f.settle();
    compare(&f);
    f.app.set_flat_view(pair.left, true).unwrap();
    f.settle();
    assert!(
        !call(&f, "compare.model", json!({}))["active"]
            .as_bool()
            .unwrap()
    );
    f.app.set_flat_view(pair.left, false).unwrap();
    f.settle();

    compare(&f);
    f.app.close_pane(pair.right_pane).unwrap();
    f.settle();
    assert!(
        !call(&f, "compare.model", json!({}))["active"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn a_comparison_is_refused_with_the_reason_said_and_nothing_started() {
    let f = Fixture::new();
    // One pane.
    let again = f.app.compare_toggle().unwrap_err().to_string();
    assert!(again.contains("needs two panes side by side"), "{again}");
    assert!(
        f.probes
            .notices()
            .iter()
            .any(|n| n.contains("needs two panes")),
        "the reason was said"
    );

    let pair = two_panes(&f);
    // Three panes.
    let third = f
        .app
        .split_pane(pair.pane, SplitOrientation::Vertical, &f.path("l"))
        .unwrap();
    f.settle();
    let three = f.app.compare_toggle().unwrap_err().to_string();
    assert!(three.contains("exactly two panes"), "{three}");
    f.app.close_pane(third).unwrap();
    f.settle();

    // One folder inside the other.
    f.app
        .navigate(pair.left, &f.root.to_string_lossy())
        .unwrap();
    f.settle();
    let nested = f.app.compare_toggle().unwrap_err().to_string();
    assert!(nested.contains("neither inside the other"), "{nested}");
    assert!(
        !call(&f, "compare.model", json!({}))["active"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn a_sync_is_additive_until_deletes_are_ticked_undoes_whole_and_rescans() {
    let f = Fixture::new();
    let pair = two_panes(&f);
    compare(&f);
    let preview = call(&f, "compare.sync-preview", json!({"withDeletes": false}));
    let kinds: Vec<&str> = preview["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        vec!["Copy", "Replace"],
        kinds,
        "nothing is removed unless asked"
    );
    assert_eq!("copy 1 \u{b7} replace 1 \u{2014} 9 B", preview["summary"]);

    let op = call(&f, "compare.sync", json!({"withDeletes": false}))["op"]
        .as_u64()
        .unwrap();
    let (ok, message) = f.wait_op(bb_app::OpId(op));
    assert!(ok, "{message}");
    assert!(f.root.join("r\\only-left.txt").exists());
    assert!(
        f.root.join("r\\only-right.txt").exists(),
        "right-only is untouched"
    );
    assert_eq!(
        "short",
        std::fs::read_to_string(f.root.join("r\\deep\\changed.txt")).unwrap()
    );
    // The comparison rescanned itself after the run.
    assert_eq!(
        "1 only on the right",
        call(&f, "compare.model", json!({}))["summary"]
    );

    let undo = f.app.undo_step(1).unwrap();
    assert!(f.wait_op(undo).0);
    assert!(!f.root.join("r\\only-left.txt").exists());
    assert_eq!(
        "a good deal longer",
        std::fs::read_to_string(f.root.join("r\\deep\\changed.txt")).unwrap(),
        "what was replaced is back"
    );
    assert_eq!(
        "1 only on the left \u{b7} 1 only on the right \u{b7} 1 differ",
        call(&f, "compare.model", json!({}))["summary"]
    );

    // With the destructive half ticked the right side ends up matching the left.
    let op = call(&f, "compare.sync", json!({"withDeletes": true}))["op"]
        .as_u64()
        .unwrap();
    assert!(f.wait_op(bb_app::OpId(op)).0);
    assert!(!f.root.join("r\\only-right.txt").exists());
    assert_eq!(
        "These folders match.",
        call(&f, "compare.model", json!({}))["summary"]
    );
    let undo = f.app.undo_step(1).unwrap();
    assert!(f.wait_op(undo).0);
    assert!(
        f.root.join("r\\only-right.txt").exists(),
        "the deletion goes back with the copies"
    );
    let _ = (pair.left, pair.right);
}

#[test]
fn an_unticked_row_is_not_synced_and_there_is_nothing_to_run_without_work() {
    let f = Fixture::new();
    two_panes(&f);
    compare(&f);
    let p = call(
        &f,
        "compare.sync-preview",
        json!({"withDeletes": false, "ticked": ["ONLY-LEFT.TXT"]}),
    );
    assert_eq!("copy 1 \u{2014} 4 B", p["summary"]);
    let p = call(
        &f,
        "compare.sync-preview",
        json!({"withDeletes": false, "ticked": []}),
    );
    assert_eq!("Nothing selected", p["summary"]);
    assert!(!p["canRun"].as_bool().unwrap());
    let err = call_err(
        &f,
        "compare.sync",
        json!({"withDeletes": false, "ticked": []}),
    );
    assert!(err.contains("nothing for a sync to do"), "{err}");
    f.app
        .op_delete(vec![f.path("r\\shared.txt")], DeleteMode::Permanent)
        .unwrap();
    f.settle();
}

#[test]
fn a_verdict_about_a_file_that_has_changed_since_the_scan_reads_not_compared() {
    let f = Fixture::new();
    let pair = two_panes(&f);
    compare(&f);
    assert_eq!("Same", status_of(&f, pair.left, "shared.txt"));
    // Written behind the comparison's back (not through the app, so nothing rescans).
    std::thread::sleep(std::time::Duration::from_millis(30));
    write(&f.root.join("l\\shared.txt"), "now it is longer");
    f.app.refresh(pair.left).unwrap();
    f.settle();
    assert_eq!("Not compared", status_of(&f, pair.left, "shared.txt"));
    assert!(
        call(&f, "compare.model", json!({}))["stale"]
            .as_bool()
            .unwrap()
    );
}

fn status_of(f: &Fixture, tab: TabId, name: &str) -> String {
    let listing = f.app.listing(tab).unwrap();
    let at = listing.rows().iter().position(|r| r.name == name).unwrap();
    String::from_utf16_lossy(listing.cell(at, 3).unwrap())
}

#[test]
fn an_unknown_verb_is_refused() {
    let f = Fixture::new();
    assert!(call_err(&f, "nonsense", json!({})).contains("unknown tool verb"));
}

// ---- the duplicate finder ----

#[test]
fn a_duplicate_scan_runs_off_the_ui_thread_and_never_lets_every_copy_go() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.path("twins")).unwrap();
    for name in ["twins/a.bin", "twins/b.bin", "twins/c.bin"] {
        std::fs::write(f.path(name), vec![7u8; 4096]).unwrap();
    }
    let root = f.path("twins");
    let loaded = call(&f, "duplicates.load", json!({ "root": root }));
    assert_eq!(false, loaded["hasScanned"]);
    assert_eq!(
        "Press Scan to look for identical files.",
        loaded["emptyMessage"]
    );
    call(&f, "duplicates.scan", json!({ "root": root, "minSize": 1 }));
    f.settle();
    let m = call(&f, "duplicates.model", json!({}));
    assert_eq!(false, m["scanning"]);
    let groups = m["groups"].as_array().unwrap();
    assert_eq!(1, groups.len(), "{m}");
    let paths: Vec<String> = groups[0]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["path"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(3, paths.len());

    // Every copy ticked: the group contributes nothing.
    let all = call(&f, "duplicates.removable", json!({ "paths": paths }));
    assert!(all["paths"].as_array().unwrap().is_empty());
    let two = call(&f, "duplicates.removable", json!({ "paths": paths[..2] }));
    assert_eq!(2, two["paths"].as_array().unwrap().len());

    // Once two have gone the group is gone, and the last copy cannot be offered up again.
    let after = call(&f, "duplicates.prune", json!({ "paths": paths[..2] }));
    assert!(after["groups"].as_array().unwrap().is_empty());
    let last = call(&f, "duplicates.removable", json!({ "paths": paths[2..] }));
    assert!(last["paths"].as_array().unwrap().is_empty());
}

// ---- editing what is saved ----

#[test]
fn a_saved_search_is_edited_under_a_new_name_and_a_workspace_renamed() {
    let f = Fixture::new();
    let save = |name: &str, query: &str, previous: Option<&str>| {
        call(
            &f,
            "search.save",
            json!({ "name": name, "query": query, "scope": "current", "previous": previous }),
        )
    };
    save("Reports", "report", None);
    save("Notes", "notes", None);
    let seed = call(&f, "search.edit-seed", json!({ "name": "Reports" }));
    assert_eq!("report", seed["query"]);
    // An edit may keep its own name, but not take another's.
    let mine = call(
        &f,
        "search.save-check",
        json!({ "name": "Reports", "query": "report", "scope": "current", "previous": "Reports" }),
    );
    assert!(mine["problem"].is_null(), "{mine}");
    assert_eq!(false, mine["replacing"]);
    let theirs = call(
        &f,
        "search.save-check",
        json!({ "name": "Notes", "query": "report", "scope": "current", "previous": "Reports" }),
    );
    assert!(theirs["problem"].is_string(), "{theirs}");
    save("Markdown", "*.md", Some("Reports"));
    let saved: Value = serde_json::from_str(&f.app.saved_json()).unwrap();
    let names: Vec<String> = saved["searches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        names.contains(&"Markdown".to_string()) && !names.contains(&"Reports".to_string()),
        "{names:?}"
    );

    f.app.saved_do(None, "save-workspace", "Desk").unwrap();
    f.app.saved_do(None, "save-workspace", "Laptop").unwrap();
    let taken = call(
        &f,
        "workspace.rename-check",
        json!({ "old": "Desk", "name": "Laptop" }),
    );
    assert!(taken["problem"].is_string());
    let fine = call(
        &f,
        "workspace.rename-check",
        json!({ "old": "Desk", "name": "desk" }),
    );
    assert!(fine["problem"].is_null(), "{fine}");
}

// ---- portable devices ---------------------------------------------------------------------------------

#[test]
fn portable_devices_are_read_off_the_thread_and_open_in_explorer() {
    let f = Fixture::new();
    let first = call(&f, "devices.list", json!({}));
    assert!(
        !first["loaded"].as_bool().unwrap() || first["devices"].as_array().unwrap().len() == 1,
        "the first ask starts the read rather than waiting on it"
    );
    f.settle();
    let m = call(&f, "devices.list", json!({}));
    assert!(m["loaded"].as_bool().unwrap());
    assert_eq!("Pixel 8", m["devices"][0]["name"]);
    let path = m["devices"][0]["path"].as_str().unwrap().to_string();
    call(&f, "devices.open", json!({ "path": path }));
    let launched = f.probes.launches();
    assert_eq!(1, launched.len());
    assert!(
        launched[0]
            .0
            .to_ascii_lowercase()
            .ends_with("\\explorer.exe"),
        "{launched:?}"
    );
}
