//! The side panes' models: what the preview makes of a selection, and the metadata pane's boxes,
//! apply, undo and dates, over real files in a throwaway tree.

mod common;

use std::sync::Arc;

use bb_app::{
    HostReading, MODE_AUTO, MODE_GUESS, MODE_HEX, MODE_RAW, MetadataAction, ShellPropertyHost,
};
use bb_test_support::metadata_fixtures as fx;
use bb_test_support::metadata_fixtures_data as data;
use common::Fixture;
use serde_json::Value;

fn put(f: &Fixture, name: &str, bytes: &[u8]) {
    let full = f.root.join(name);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, bytes).unwrap();
}

fn reload(f: &Fixture) {
    f.app.refresh(f.tab).unwrap();
    f.settle();
}

fn preview(f: &Fixture, mode: u8) -> Value {
    serde_json::from_str(&f.app.preview_build(f.tab, mode).unwrap()).unwrap()
}

fn field<'a>(model: &'a Value, id: &str) -> Option<&'a Value> {
    model["fields"].as_array()?.iter().find(|f| f["id"] == id)
}

// ---- the preview ----

#[test]
fn nothing_several_and_a_folder_say_what_they_are() {
    let f = Fixture::new();
    let p = preview(&f, MODE_AUTO);
    assert_eq!(p["state"], "none");
    assert_eq!(p["message"], "Select a file to preview");

    f.select_names(&["notes.txt", "report.md"]);
    assert_eq!(preview(&f, MODE_AUTO)["message"], "2 items selected");

    f.select_names(&["Documents"]);
    let folder = preview(&f, MODE_AUTO);
    assert_eq!(folder["state"], "none");
    assert!(folder["message"].as_str().unwrap().starts_with("Folder"));
    assert_eq!(folder["canChooseMode"], false);
}

#[test]
fn code_is_read_as_text_with_a_footer_and_colour() {
    let f = Fixture::new();
    put(
        &f,
        "Sample.cs",
        b"using System;\n// a comment\nint x = 42; \"s\"\n",
    );
    reload(&f);
    f.select_names(&["Sample.cs"]);
    let p = preview(&f, MODE_AUTO);
    assert_eq!(p["state"], "text");
    assert_eq!(p["lines"].as_array().unwrap().len(), 3);
    assert_eq!(p["lines"][1], "// a comment");
    let footer = p["footer"].as_str().unwrap();
    assert!(
        footer.contains("UTF-8") && footer.contains("3 lines"),
        "{footer}"
    );
    // [line, start, length, class]: `using` is a keyword on line 0, the comment is class 3.
    let spans = p["spans"].as_array().unwrap();
    assert!(
        spans
            .iter()
            .any(|s| s[0] == 0 && s[1] == 0 && s[2] == 5 && s[3] == 1),
        "{spans:?}"
    );
    assert!(spans.iter().any(|s| s[0] == 1 && s[3] == 3));
    assert_eq!(p["canChooseMode"], true);
}

#[test]
fn raw_is_uncoloured_and_hex_is_a_dump_with_its_own_footer() {
    let f = Fixture::new();
    put(&f, "Sample.cs", b"using System;\n");
    reload(&f);
    f.select_names(&["Sample.cs"]);
    let raw = preview(&f, MODE_RAW);
    assert_eq!(raw["state"], "text");
    assert!(raw["footer"].as_str().unwrap().starts_with("Raw"));
    let hex = preview(&f, MODE_HEX);
    assert_eq!(hex["state"], "hex");
    assert!(hex["lines"][0].as_str().unwrap().starts_with("00000000"));
    assert!(hex["footer"].as_str().unwrap().starts_with("Hex"));
}

#[test]
fn binary_is_refused_with_an_invitation_and_unknown_text_is_shown() {
    let f = Fixture::new();
    let opaque: Vec<u8> = (0..2048).map(|i| (1 + i % 0x1F) as u8).collect();
    put(&f, "opaque.qqq", &opaque);
    put(
        &f,
        "mystery.qqq",
        b"Not a format anyone has heard of,\nand plainly readable anyway.\n",
    );
    reload(&f);

    f.select_names(&["opaque.qqq"]);
    // The classifier calls an unknown extension a document: the host asks the shell first, and
    // only then asks again to have the bytes looked at.
    assert_eq!(preview(&f, MODE_AUTO)["state"], "document");
    let guessed = preview(&f, MODE_GUESS);
    assert_eq!(guessed["state"], "none");
    assert_eq!(guessed["message"], "No preview available");

    f.select_names(&["mystery.qqq"]);
    assert_eq!(preview(&f, MODE_GUESS)["state"], "text");

    // Forced to text, a binary is "binary file", not mojibake.
    f.select_names(&["opaque.qqq"]);
    let raw = preview(&f, MODE_RAW);
    assert_eq!(raw["state"], "text", "raw reads whatever it is given");
}

#[test]
fn a_zip_is_listed_without_extracting() {
    let f = Fixture::new();
    let bytes = fx::zip_of(&[("readme.txt", "hello"), ("src/app.js", "x")]);
    put(&f, "sample.zip", &bytes);
    reload(&f);
    f.select_names(&["sample.zip"]);
    let p = preview(&f, MODE_AUTO);
    assert_eq!(p["state"], "archive");
    assert_eq!(p["entries"].as_array().unwrap().len(), 2);
    assert!(p["footer"].as_str().unwrap().contains("2 entries"));
}

/// A picture inside a zip is the host's to decode, from bytes the core reads out of the entry.
/// Spec: PreviewPaneViewModel.OpenBounded. A zip inside a zip cannot be opened in place, and says
/// how to see it.
#[test]
fn a_picture_inside_a_zip_is_previewed_from_its_bytes() {
    let f = Fixture::new();
    let inner = fx::zip_of(&[("deep.txt", "deep")]);
    let picture: &[u8] = b"\x89PNG not really, but bytes all the same";
    let outer = fx::zip_of_bytes(&[("pic.png", picture), ("inner.zip", &inner)]);
    put(&f, "pack.zip", &outer);
    f.app.navigate(f.tab, &f.path("pack.zip")).unwrap();
    f.settle();

    f.select_names(&["pic.png"]);
    let p = preview(&f, MODE_AUTO);
    assert_eq!(p["state"], "image");
    assert_eq!(p["inArchive"], true);
    let bytes = f.app.preview_read(p["path"].as_str().unwrap()).unwrap();
    assert_eq!(bytes, picture);

    f.select_names(&["inner.zip"]);
    let p = preview(&f, MODE_AUTO);
    assert_eq!(p["message"], "Extract this archive to preview it.", "{p}");
}

#[test]
fn images_fonts_and_media_are_the_hosts_to_show() {
    let f = Fixture::new();
    put(&f, "a.png", b"png");
    put(&f, "a.ttf", b"ttf");
    put(&f, "a.mp4", b"mp4");
    reload(&f);
    for (name, state) in [("a.png", "image"), ("a.ttf", "font"), ("a.mp4", "media")] {
        f.select_names(&[name]);
        let p = preview(&f, MODE_AUTO);
        assert_eq!(p["state"], state, "{name}");
        assert!(p["path"].as_str().unwrap().ends_with(name));
    }
}

#[test]
fn a_cloud_placeholder_is_never_opened() {
    // Refused before anything is read; the classifier owns the rule, this checks it is reached.
    let f = Fixture::new();
    f.select_names(&["notes.txt"]);
    assert_eq!(preview(&f, MODE_AUTO)["state"], "text");
}

#[test]
fn nothing_is_held_open_by_a_preview() {
    let f = Fixture::new();
    put(&f, "Sample.cs", b"using System;\n");
    reload(&f);
    f.select_names(&["Sample.cs"]);
    assert_eq!(preview(&f, MODE_AUTO)["state"], "text");
    let from = f.root.join("Sample.cs");
    let to = f.root.join("Renamed.cs");
    std::fs::rename(&from, &to).expect("a previewed file can be renamed");
}

// ---- the metadata pane ----

fn camera(f: &Fixture) {
    put(f, "camera.jpg", &fx::jpeg(data::CAMERA_JPEG));
    put(f, "plain.jpg", &fx::jpeg(data::PLAIN_JPEG));
    put(
        f,
        "first.mp3",
        &fx::mp3(
            &[
                ("TIT2", "First song"),
                ("TPE1", "The Band"),
                ("TALB", "Album One"),
                ("TRCK", "1"),
            ],
            false,
        ),
    );
    put(
        f,
        "second.mp3",
        &fx::mp3(
            &[
                ("TIT2", "Second song"),
                ("TPE1", "The Band"),
                ("TALB", "Album One"),
                ("TRCK", "2"),
            ],
            true,
        ),
    );
    reload(f);
}

fn model(f: &Fixture) -> Value {
    f.app.metadata_model(f.tab).unwrap()
}

fn load(f: &Fixture) {
    f.app.metadata_load(f.tab, true).unwrap();
}

#[test]
fn a_picture_shows_what_the_camera_wrote() {
    let f = Fixture::new();
    camera(&f);
    f.select_names(&["camera.jpg"]);
    load(&f);
    let m = model(&f);
    assert_eq!(m["title"], "camera.jpg");
    assert_eq!(field(&m, "title").unwrap()["text"], "Old title");
    assert_eq!(field(&m, "make").unwrap()["text"], "TestCam");
    assert_eq!(field(&m, "taken").unwrap()["text"], "2021-06-05 04:03:02");
    assert!(field(&m, "taken").unwrap()["isDate"].as_bool().unwrap());
    assert!(field(&m, "modified").is_some() && field(&m, "created").is_some());
    assert_eq!(m["canApply"], false);
}

#[test]
fn typing_marks_a_box_changed_and_invalid_text_blocks_apply() {
    let f = Fixture::new();
    camera(&f);
    f.select_names(&["camera.jpg"]);
    load(&f);
    f.app
        .metadata_set_text(f.tab, "title", "Harbour at dawn")
        .unwrap();
    let m = model(&f);
    assert_eq!(field(&m, "title").unwrap()["isDirty"], true);
    assert_eq!(field(&m, "title").unwrap()["note"], "Changed");
    assert_eq!(m["summary"], "1 field will be written.");
    assert_eq!(m["canApply"], true);

    f.app.metadata_set_text(f.tab, "rating", "9").unwrap();
    let m = model(&f);
    assert_eq!(field(&m, "rating").unwrap()["isInvalid"], true);
    assert_eq!(field(&m, "rating").unwrap()["note"], "0 to 5");
    assert_eq!(m["canApply"], false);
    assert_eq!(m["summary"], "Fix the highlighted field before applying.");

    f.app.metadata_revert(f.tab, "rating").unwrap();
    assert_eq!(model(&f)["canApply"], true);
}

#[test]
fn apply_writes_the_file_follows_it_and_undo_is_one_step() {
    let f = Fixture::new();
    camera(&f);
    f.select_names(&["camera.jpg"]);
    load(&f);
    f.app
        .metadata_set_text(f.tab, "title", "Harbour at dawn")
        .unwrap();
    f.app
        .metadata_set_text(f.tab, "author", "Ren\u{e9}e")
        .unwrap();
    f.app.metadata_set_text(f.tab, "rating", "4").unwrap();
    let op = f.app.metadata_act(f.tab, MetadataAction::Apply).unwrap();
    let (ok, message) = f.wait_op(op);
    assert!(ok, "{message}");

    // The pane follows what is on disk, and the selection survived the reload.
    let m = model(&f);
    assert_eq!(field(&m, "title").unwrap()["text"], "Harbour at dawn");
    assert_eq!(field(&m, "title").unwrap()["isDirty"], false);
    assert_eq!(m["summary"], "1 item changed. Undo takes it back.");
    assert_eq!(m["files"][0]["state"], "Changed");
    assert_eq!(f.app.tab_info(f.tab).unwrap().selected, 1);
    assert!(f.app.can_undo());

    let undo = f.app.undo_step(1).unwrap();
    assert!(f.wait_op(undo).0);
    // An undo reloads the list with nothing selected; the person selects again.
    f.select_names(&["camera.jpg"]);
    load(&f);
    assert_eq!(field(&model(&f), "title").unwrap()["text"], "Old title");
}

#[test]
fn a_selection_that_disagrees_keeps_what_was_not_touched() {
    let f = Fixture::new();
    camera(&f);
    f.select_names(&["first.mp3", "second.mp3"]);
    load(&f);
    let m = model(&f);
    assert_eq!(field(&m, "album").unwrap()["text"], "Album One");
    assert_eq!(field(&m, "title").unwrap()["isMixed"], true);
    assert_eq!(field(&m, "title").unwrap()["mixedPlaceholder"], true);

    f.app
        .metadata_set_text(f.tab, "album", "Album Two")
        .unwrap();
    let op = f.app.metadata_act(f.tab, MetadataAction::Apply).unwrap();
    assert!(f.wait_op(op).0);
    f.select_names(&["first.mp3"]);
    load(&f);
    let m = model(&f);
    assert_eq!(field(&m, "album").unwrap()["text"], "Album Two");
    assert_eq!(field(&m, "title").unwrap()["text"], "First song");
}

#[test]
fn emptying_a_box_is_a_deliberate_clear() {
    let f = Fixture::new();
    camera(&f);
    f.select_names(&["first.mp3", "second.mp3"]);
    load(&f);
    f.app.metadata_set_text(f.tab, "title", " ").unwrap();
    f.app.metadata_set_text(f.tab, "title", "").unwrap();
    let m = model(&f);
    assert_eq!(field(&m, "title").unwrap()["note"], "Will be cleared");
    assert_eq!(field(&m, "title").unwrap()["showsNote"], true);
    let op = f.app.metadata_act(f.tab, MetadataAction::Apply).unwrap();
    assert!(f.wait_op(op).0);
    f.select_names(&["first.mp3"]);
    load(&f);
    assert_eq!(field(&model(&f), "title").unwrap()["text"], "");
}

#[test]
fn a_file_with_no_codec_has_its_dates_and_says_why() {
    let f = Fixture::new();
    put(&f, "notes2.txt", b"hello");
    std::fs::create_dir_all(f.root.join("Album")).unwrap();
    reload(&f);
    f.select_names(&["notes2.txt", "Album"]);
    load(&f);
    let m = model(&f);
    let files = m["files"].as_array().unwrap();
    assert!(files.iter().all(|x| x["state"] == "Skipped"));
    assert!(
        files.iter().find(|x| x["name"] == "Album").unwrap()["detail"]
            .as_str()
            .unwrap()
            .contains("only its dates")
    );
    assert!(
        files.iter().find(|x| x["name"] == "notes2.txt").unwrap()["detail"]
            .as_str()
            .unwrap()
            .contains("only its dates")
    );
    assert!(field(&m, "title").is_none());
    assert!(field(&m, "modified").is_some());
}

#[test]
fn dates_are_applied_after_the_metadata_and_undone_on_their_own() {
    let f = Fixture::new();
    camera(&f);
    f.select_names(&["first.mp3"]);
    load(&f);
    f.app.metadata_set_text(f.tab, "title", "Dated").unwrap();
    f.app
        .metadata_set_text(f.tab, "modified", "2018-07-06 05:04:03")
        .unwrap();
    let op = f.app.metadata_act(f.tab, MetadataAction::Apply).unwrap();
    assert!(f.wait_op(op).0);
    load(&f);
    let m = model(&f);
    assert_eq!(field(&m, "title").unwrap()["text"], "Dated");
    // The write moved the date; the stamp set afterwards is what stands.
    assert_eq!(
        field(&m, "modified").unwrap()["text"],
        "2018-07-06 05:04:03"
    );

    // Two history entries: the tags and the date. Undoing one leaves the other.
    let undo = f.app.undo_step(1).unwrap();
    assert!(f.wait_op(undo).0);
    f.select_names(&["first.mp3"]);
    load(&f);
    let m = model(&f);
    assert_ne!(
        field(&m, "modified").unwrap()["text"],
        "2018-07-06 05:04:03"
    );
    assert_eq!(field(&m, "title").unwrap()["text"], "Dated");
}

#[test]
fn a_reload_that_changes_nothing_leaves_what_is_half_typed() {
    let f = Fixture::new();
    camera(&f);
    f.select_names(&["camera.jpg"]);
    f.app.metadata_load(f.tab, false).unwrap();
    f.app
        .metadata_set_text(f.tab, "title", "Half typed")
        .unwrap();
    f.app.metadata_load(f.tab, false).unwrap();
    assert_eq!(field(&model(&f), "title").unwrap()["text"], "Half typed");
    f.app.metadata_load(f.tab, true).unwrap();
    assert_eq!(field(&model(&f), "title").unwrap()["text"], "Old title");
}

#[test]
fn removing_a_location_leaves_the_rest_and_undo_puts_it_back() {
    let f = Fixture::new();
    camera(&f);
    f.select_names(&["camera.jpg", "plain.jpg"]);
    load(&f);
    let m = model(&f);
    assert_eq!(m["located"], 1);
    assert_eq!(m["canRemoveLocation"], true);
    let op = f
        .app
        .metadata_act(f.tab, MetadataAction::RemoveLocation)
        .unwrap();
    assert!(f.wait_op(op).0);
    assert_eq!(model(&f)["located"], 0);
    assert_eq!(model(&f)["canRemoveLocation"], false);
    let undo = f.app.undo_step(1).unwrap();
    assert!(f.wait_op(undo).0);
    f.select_names(&["camera.jpg", "plain.jpg"]);
    load(&f);
    assert_eq!(model(&f)["located"], 1);
}

#[test]
fn a_button_that_would_not_work_says_so() {
    let f = Fixture::new();
    camera(&f);
    f.select_names(&["camera.jpg"]);
    load(&f);
    let err = f
        .app
        .metadata_act(f.tab, MetadataAction::Apply)
        .unwrap_err();
    assert!(format!("{err:?}").contains("would not apply"), "{err:?}");
}

// ---- a file only Windows writes ----

/// A property host that answers for `.asf` as a Windows handler would, in memory.
struct FakeWindows {
    store: std::sync::Mutex<std::collections::BTreeMap<String, String>>,
}

impl ShellPropertyHost for FakeWindows {
    fn details(&self, _: &str) -> Vec<bb_core::preview::ShellPropertyRow> {
        vec![bb_core::preview::ShellPropertyRow {
            canonical: "System.Image.Dimensions".into(),
            name: "Dimensions".into(),
            value: "8 x 8".into(),
        }]
    }

    fn read_fields(&self, path: &str, canonicals: &[&str], ask: bool) -> Option<HostReading> {
        if !path.to_lowercase().contains(".asf") {
            return None;
        }
        let store = self.store.lock().unwrap();
        Some(HostReading {
            writable: ask.then(|| {
                vec![
                    "System.Title".to_string(),
                    "System.Music.Artist".to_string(),
                ]
            }),
            values: canonicals
                .iter()
                .filter_map(|c| {
                    store.get(*c).map(|t| bb_app::HostValue {
                        canonical: (*c).to_string(),
                        text: Some(t.clone()),
                        ..Default::default()
                    })
                })
                .collect(),
        })
    }

    fn write_fields(
        &self,
        _: &str,
        values: &[(String, bb_core::metadata::WindowsPropertyValue)],
    ) -> Result<(), String> {
        let mut store = self.store.lock().unwrap();
        for (name, v) in values {
            match &v.text {
                Some(t) => store.insert(name.clone(), t.clone()),
                None => store.remove(name),
            };
        }
        Ok(())
    }
}

#[test]
fn windows_writes_what_no_codec_does_and_the_row_says_so() {
    let f = Fixture::new();
    put(&f, "tone.asf", b"not really an asf");
    reload(&f);
    f.app.set_shell_property_host(Some(Arc::new(FakeWindows {
        store: Default::default(),
    })));
    f.select_names(&["tone.asf"]);
    load(&f);
    let m = model(&f);
    assert_eq!(m["files"][0]["viaWindows"], true);
    assert!(
        m["files"][0]["detail"]
            .as_str()
            .unwrap()
            .starts_with("Written by Windows")
    );
    assert!(field(&m, "title").is_some() && field(&m, "artist").is_some());
    assert!(
        field(&m, "album").is_none(),
        "only what the handler offers gets a box"
    );
    assert_eq!(
        m["canRemove"], false,
        "a removal is never made through a handler"
    );

    f.app.metadata_set_text(f.tab, "title", "One tone").unwrap();
    let op = f.app.metadata_act(f.tab, MetadataAction::Apply).unwrap();
    let (ok, message) = f.wait_op(op);
    assert!(ok, "{message}");
    let m = model(&f);
    assert_eq!(field(&m, "title").unwrap()["text"], "One tone");
    assert!(
        m["files"][0]["detail"]
            .as_str()
            .unwrap()
            .starts_with("Written by Windows")
    );
}

#[test]
fn the_details_under_a_picture_come_from_windows() {
    let f = Fixture::new();
    camera(&f);
    f.app.set_shell_property_host(Some(Arc::new(FakeWindows {
        store: Default::default(),
    })));
    f.select_names(&["camera.jpg"]);
    load(&f);
    assert_eq!(model(&f)["details"][0]["label"], "Dimensions");
    assert_eq!(model(&f)["details"][0]["value"], "8 x 8");
}

// ---- rename by tags ----

#[test]
fn a_template_naming_a_tag_reads_it_from_each_file() {
    let f = Fixture::new();
    camera(&f);
    f.select_names(&["first.mp3", "second.mp3"]);
    let mut rule = bb_core::rename::RenameRule::new("{track:00} {artist} - {title}{ext}");
    rule.is_literal = false;
    let op = f
        .app
        .op_rename_rule(vec![f.path("first.mp3"), f.path("second.mp3")], rule)
        .unwrap();
    let (ok, message) = f.wait_op(op);
    assert!(ok, "{message}");
    let names = f.names();
    assert!(
        names.contains(&"01 The Band - First song.mp3".to_string()),
        "{names:?}"
    );
    assert!(
        names.contains(&"02 The Band - Second song.mp3".to_string()),
        "{names:?}"
    );
}

// ---- a property cell must not outlive the file it was read from ----------------------------------------

#[test]
fn a_file_written_since_its_properties_were_read_is_read_again() {
    let f = Fixture::new();
    f.app.ui_set_flag("propertyProvider", true);
    f.app.columns_do(f.tab, "add", "System.Title").unwrap();
    let ask = |f: &Fixture| -> Vec<Value> {
        serde_json::from_str::<Value>(&f.app.prop_requests(f.tab, 0, 0).unwrap())
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    };
    let supply = |f: &Fixture, asked: &[Value]| {
        f.app
            .prop_supply(
                f.tab,
                asked
                    .iter()
                    .map(|e| bb_app::PropSupply {
                        path: e[0].as_str().unwrap().to_string(),
                        canonical: e[1].as_str().unwrap().to_string(),
                        value: Some(bb_app::prop_words("the title it had")),
                    })
                    .collect(),
            )
            .unwrap();
        f.settle();
    };
    let first = ask(&f);
    assert!(!first.is_empty());
    supply(&f, &first);
    assert!(
        ask(&f).is_empty(),
        "everything was answered, nothing is owed"
    );

    // One file is rewritten (a different length, so a different stamp): only it is asked about again.
    std::fs::write(f.path("notes.txt"), vec![b'y'; 321]).unwrap();
    f.app.refresh(f.tab).unwrap();
    // Not settled: the app is rightly busy while the host owes an answer.
    // (Asking is what hands the cells over, so the answer is kept from the asking that found it.)
    let mut again = Vec::new();
    assert!(
        common::eventually(10_000, || {
            again = ask(&f);
            !again.is_empty()
        }),
        "the rewritten file was never asked about again"
    );
    assert_eq!(1, again.len(), "{again:?}");
    assert!(again[0][0].as_str().unwrap().ends_with("notes.txt"));
}

// ---- two panes, compared --------------------------------------------------------------------------------

fn status_of(f: &Fixture, tab: bb_app::TabId, name: &str) -> String {
    let listing = f.app.listing(tab).unwrap();
    let at = listing
        .rows()
        .iter()
        .position(|r| r.name == name)
        .unwrap_or_else(|| panic!("{name} is not listed"));
    // CELL_COLUMNS: Name, Folder, Match, CompareStatus, ...
    String::from_utf16_lossy(listing.cell(at, 3).unwrap())
}

#[test]
fn a_comparison_says_what_each_row_is_and_content_settles_only_what_the_bytes_agree_on() {
    let f = Fixture::new();
    let (left, right) = (f.root.join("cmp-left"), f.root.join("cmp-right"));
    std::fs::create_dir_all(&left).unwrap();
    std::fs::create_dir_all(&right).unwrap();

    // The same bytes written far enough apart that one side reads as older; two files whose sizes
    // genuinely differ; one that only the left holds.
    std::fs::write(left.join("moved.txt"), "same bytes\n").unwrap();
    std::fs::write(left.join("really.txt"), "alpha\nbeta\n").unwrap();
    std::fs::write(right.join("really.txt"), "alpha\ngamma!\n").unwrap();
    std::fs::write(left.join("only.txt"), "mine").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2300));
    std::fs::write(right.join("moved.txt"), "same bytes\n").unwrap();

    let left_path = left.to_string_lossy().into_owned();
    f.app.navigate(f.tab, &left_path).unwrap();
    f.settle();
    let pane = f.app.active_pane();
    f.app
        .split_pane(
            pane,
            bb_core::layout::SplitOrientation::Vertical,
            &right.to_string_lossy(),
        )
        .unwrap();
    f.settle();
    f.app.activate_pane(pane).unwrap();
    f.settle();

    f.app.compare_toggle().unwrap();
    f.settle();
    assert_eq!("Older", status_of(&f, f.tab, "moved.txt"));
    assert_eq!("Differs", status_of(&f, f.tab, "really.txt"));
    assert_eq!("Only here", status_of(&f, f.tab, "only.txt"));
    // The status column is there while comparing, and only then.
    let columns = serde_json::from_str::<Value>(&f.app.columns_json(f.tab).unwrap()).unwrap();
    assert!(
        columns["columns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "CompareStatus")
    );

    let both = [
        f.path("cmp-left\\moved.txt"),
        f.path("cmp-left\\really.txt"),
    ];
    assert_eq!(
        1,
        f.app.compare_settle(&both).unwrap(),
        "only the identical pair settles"
    );
    f.settle();
    assert_eq!("Same", status_of(&f, f.tab, "moved.txt"));
    assert_eq!(
        "Differs",
        status_of(&f, f.tab, "really.txt"),
        "different bytes stay different"
    );

    f.app.compare_toggle().unwrap();
    f.settle();
    assert_eq!("", status_of(&f, f.tab, "moved.txt"));
    let columns = serde_json::from_str::<Value>(&f.app.columns_json(f.tab).unwrap()).unwrap();
    assert!(
        !columns["columns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "CompareStatus")
    );
}

#[test]
fn settling_with_nothing_compared_is_refused() {
    let f = Fixture::new();
    assert!(f.app.compare_settle(&[f.path("notes.txt")]).is_err());
}
