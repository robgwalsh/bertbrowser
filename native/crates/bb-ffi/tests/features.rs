//! The exports for the services around the tabs, driven exactly as the C++ side does: raw pointers,
//! views, owned strings, JSON models and a sink.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bb_test_support::unique_temp_path;
use bbcore::*;
use serde_json::Value;

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn bs(units: &[u16]) -> BbStr {
    BbStr {
        ptr: units.as_ptr(),
        len: units.len() as u32,
    }
}

unsafe fn take(s: BbOwnedStr) -> Option<String> {
    if s.ptr.is_null() {
        return None;
    }
    let t = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(s.ptr, s.len as usize) });
    unsafe { bb_str_free(s) };
    Some(t)
}

fn none() -> BbOwnedStr {
    BbOwnedStr {
        ptr: ptr::null_mut(),
        len: 0,
    }
}

fn last_error() -> String {
    let mut s = BbStr {
        ptr: ptr::null(),
        len: 0,
    };
    unsafe {
        bb_last_error(&mut s);
        String::from_utf16_lossy(std::slice::from_raw_parts(s.ptr, s.len as usize))
    }
}

struct Seen(Mutex<Vec<(BbEventKind, u64, u64)>>);

unsafe extern "C" fn sink(ctx: *mut c_void, evt: *mut BbEvent) {
    let seen = unsafe { &*(ctx as *const Seen) };
    let e = unsafe { &*evt };
    seen.0.lock().unwrap().push((e.kind, e.id, e.a));
    unsafe { bb_event_free(evt) };
}

struct Rig {
    app: *mut BbApp,
    tab: u64,
    tree: PathBuf,
    data: PathBuf,
    seen: Box<Seen>,
}

impl Rig {
    fn new() -> Rig {
        Rig::with(0, false)
    }

    fn with(settings_debounce_ms: u32, index_declined: bool) -> Rig {
        let tree = unique_temp_path("bb-ffi2-tree", "");
        let data = unique_temp_path("bb-ffi2-data", "");
        for (rel, len) in [
            ("Documents/report.md", 3),
            ("Pictures/a.png", 4),
            ("notes.txt", 100),
            ("photo.jpg", 5000),
        ] {
            let full = tree.join(rel);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, vec![b'x'; len]).unwrap();
        }
        let data_w = w(&data.to_string_lossy());
        let cfg = BbAppConfigEx {
            base: BbAppConfig {
                data_dir: bs(&data_w),
                fakes: BbFakes::BB_FAKES_HARNESS,
                search_debounce_ms: 0,
                watch_debounce_ms: 40,
                delete_staging_root: BbStr {
                    ptr: ptr::null(),
                    len: 0,
                },
                workers: 0,
            },
            settings_debounce_ms,
            index_declined,
        };
        let mut app: *mut BbApp = ptr::null_mut();
        assert_eq!(BbStatus::BB_OK, unsafe { bb_app_create_ex(&cfg, &mut app) });
        let seen = Box::new(Seen(Mutex::new(Vec::new())));
        unsafe { bb_app_set_event_sink(app, Some(sink), &*seen as *const Seen as *mut c_void) };
        let start = w(&tree.to_string_lossy());
        let mut tab = 0u64;
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_app_initialize(app, bs(&start), &mut tab)
        });
        let rig = Rig {
            app,
            tab,
            tree,
            data,
            seen,
        };
        rig.settle();
        rig
    }

    fn settle(&self) {
        let start = Instant::now();
        let mut quiet = None;
        while start.elapsed() < Duration::from_secs(15) {
            if unsafe { bb_app_is_busy(self.app) } {
                quiet = None;
            } else if quiet.get_or_insert_with(Instant::now).elapsed() > Duration::from_millis(40) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the app never went quiet");
    }

    fn path(&self, rel: &str) -> String {
        self.tree.join(rel).to_string_lossy().into_owned()
    }

    fn json(&self, f: unsafe extern "C" fn(*mut BbApp, *mut BbOwnedStr) -> BbStatus) -> Value {
        let mut out = none();
        assert_eq!(BbStatus::BB_OK, unsafe { f(self.app, &mut out) });
        serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap()
    }

    fn do2(
        &self,
        f: unsafe extern "C" fn(*mut BbApp, BbStr, BbStr) -> BbStatus,
        verb: &str,
        arg: &str,
    ) -> BbStatus {
        let (v, a) = (w(verb), w(arg));
        unsafe { f(self.app, bs(&v), bs(&a)) }
    }

    fn gate(&self, id: &str) -> Option<String> {
        let id = w(id);
        let mut out = none();
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_command_why_not(self.app, bs(&id), &mut out)
        });
        unsafe { take(out) }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        unsafe { bb_app_destroy(self.app) };
        let _ = std::fs::remove_dir_all(&self.tree);
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

fn focus() -> BbFocusState {
    BbFocusState {
        focus: BbFocusKind::BB_FOCUS_FILE_LIST,
        flags: 0,
    }
}

fn chord(modifiers: u32, key: &str, hold: &mut Vec<Vec<u16>>) -> BbKeyChord {
    hold.push(w(key));
    BbKeyChord {
        modifiers,
        key: bs(hold.last().unwrap()),
    }
}

#[test]
fn a_new_folder_through_the_c_surface() {
    let rig = Rig::new();
    let mut out = none();
    let empty = w("");
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_new_item_suggest(rig.app, rig.tab, 0, bs(&empty), &mut out)
    });
    let name = unsafe { take(out) }.unwrap();
    assert_eq!("New folder", name);
    let name_w = w(&name);
    let mut plan = none();
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_new_item_plan(rig.app, rig.tab, 0, bs(&name_w), &mut plan)
    });
    let plan: Value = serde_json::from_str(&unsafe { take(plan) }.unwrap()).unwrap();
    assert_eq!(true, plan["ok"]);
    let mut op = 0u64;
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_new_item(rig.app, rig.tab, 0, bs(&name_w), &mut op)
    });
    rig.settle();
    assert!(rig.tree.join("New folder").is_dir());
    let taken = w("notes.txt");
    assert_eq!(BbStatus::BB_ERR, unsafe {
        bb_new_item(rig.app, rig.tab, 1, bs(&taken), &mut op)
    });
    assert!(last_error().contains("refused"), "{}", last_error());
    let kinds = rig.json(bb_new_file_types);
    assert!(
        kinds
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["extension"] == ".txt")
    );
}

#[test]
fn the_session_round_trips_and_tabs_move() {
    let rig = Rig::new();
    let docs = w(&rig.path("Documents"));
    let mut second = 0u64;
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_tab_open(rig.app, 0, bs(&docs), true, &mut second)
    });
    assert_eq!(BbStatus::BB_OK, unsafe { bb_tab_move(rig.app, second, 0) });
    let mut rebuilt = false;
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_session_roundtrip(rig.app, &mut rebuilt)
    });
    assert!(rebuilt);
    rig.settle();
    let captured = rig.json(bb_session_capture);
    let tabs = captured["tabs"].as_array().unwrap();
    assert_eq!(2, tabs.len());
    assert_eq!(rig.path("Documents"), tabs[0]["path"]);
    assert_eq!(BbStatus::BB_OK, unsafe { bb_session_save(rig.app) });
    assert_eq!(BbStatus::BB_OK, unsafe { bb_pane_equalise(rig.app) });
    assert_eq!(
        BbStatus::BB_OK,
        unsafe { bb_pane_swap(rig.app) },
        "one pane: nothing to swap"
    );
}

#[test]
fn the_gate_the_list_and_the_outcome() {
    let rig = Rig::new();
    assert_eq!(
        Some("Nothing is selected.".to_string()),
        rig.gate("file.rename")
    );
    let list = rig.json(bb_command_list);
    assert!(list.as_array().unwrap().len() > 100);
    let id = w("file.rename");
    let mut ui = false;
    assert_eq!(BbStatus::BB_ERR, unsafe {
        bb_command_execute(rig.app, bs(&id), &mut ui)
    });
    assert_eq!("Nothing is selected.", last_error());
    let idx = [0u32];
    unsafe {
        bb_tab_select(
            rig.app,
            rig.tab,
            BbSelectOp::BB_SELECT_REPLACE,
            idx.as_ptr(),
            1,
        )
    };
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_command_execute(rig.app, bs(&id), &mut ui)
    });
    assert!(ui, "a dialog is the host's");
    let tab_new = w("tab.new");
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_command_execute(rig.app, bs(&tab_new), &mut ui)
    });
    assert!(!ui);

    // A dialog silences the folders; the title bar's commands are still the gate's to answer.
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_ui_set_dialog_open(rig.app, true)
    });
    assert!(rig.gate("tab.new").unwrap().contains("dialog"));
    unsafe { bb_ui_set_dialog_open(rig.app, false) };
    let flag = w("clipboardHasFiles");
    unsafe { bb_ui_set_flag(rig.app, bs(&flag), true) };
    assert_eq!(None, rig.gate("edit.paste"));
}

#[test]
fn a_key_press_rebinding_and_the_keyboard_page() {
    let rig = Rig::new();
    let mut hold = Vec::new();
    let mut out = none();
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_key_press(rig.app, chord(1, "T", &mut hold), focus(), false, &mut out)
    });
    let r: Value = serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap();
    assert_eq!("ran", r["result"]);

    let id = w("view.flat");
    let k = w("Ctrl+K");
    let chords = [bs(&k)];
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_keymap_set_chords(rig.app, bs(&id), chords.as_ptr(), 1)
    });
    let mut gesture = none();
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_keymap_gesture(rig.app, bs(&id), &mut gesture)
    });
    assert_eq!("Ctrl+K", unsafe { take(gesture) }.unwrap());
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_keymap_reset(
            rig.app,
            BbStr {
                ptr: ptr::null(),
                len: 0,
            },
        )
    });

    assert_eq!(BbStatus::BB_OK, rig.do2(bb_keys_do, "select", "tab.new"));
    assert_eq!(BbStatus::BB_OK, rig.do2(bb_keys_do, "change", ""));
    assert_eq!(BbStatus::BB_OK, rig.do2(bb_keys_do, "press", "B"));
    let page = rig.json(bb_keys_page);
    assert!(
        page["text"]
            .as_str()
            .unwrap()
            .contains("can\u{2019}t be used")
    );
    assert_eq!(
        BbStatus::BB_OK,
        rig.do2(bb_keys_do, "press", "Ctrl+Shift+Y")
    );
    assert_eq!(
        BbStatus::BB_ERR,
        rig.do2(bb_keys_do, "press", "nonsense key")
    );
}

#[test]
fn the_palette_through_the_c_surface() {
    let rig = Rig::new();
    let seed = w("");
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_palette_open(rig.app, bs(&seed))
    });
    let state = rig.json(bb_palette_state);
    assert_eq!(true, state["open"]);
    let call = |verb: &str, arg: &str| -> Value {
        let (v, a) = (w(verb), w(arg));
        let mut out = none();
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_palette_do(rig.app, bs(&v), bs(&a), &mut out)
        });
        serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap()
    };
    call("type", "new tab");
    let picked = call("pick", "");
    assert_eq!(true, picked["ran"]);
    assert_eq!(false, rig.json(bb_palette_state)["open"]);
    let v = w("nonsense");
    let a = w("");
    let mut out = none();
    assert_eq!(BbStatus::BB_ERR, unsafe {
        bb_palette_do(rig.app, bs(&v), bs(&a), &mut out)
    });
}

#[test]
fn the_bars_query_edit_and_page() {
    let rig = Rig::new();
    let q = |what: &str, arg: &str| -> Value {
        let (wh, a) = (w(what), w(arg));
        let mut out = none();
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_bars_query(rig.app, bs(&wh), bs(&a), &mut out)
        });
        serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap()
    };
    assert_eq!(true, q("state", "")["isDefault"]);
    assert_eq!(6, q("menubar", "").as_array().unwrap().len());
    assert!(q("menu", "File")["rows"].to_string().contains("tab.new"));
    let (t, tokens) = (w("title"), w("nav.back nav.up - view.preview"));
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_bars_set_toolbar(rig.app, bs(&t), bs(&tokens), -1)
    });
    assert_eq!(false, q("state", "")["isDefault"]);
    assert_eq!(4, q("toolbar", "title")["rows"].as_array().unwrap().len());
    assert_eq!(BbStatus::BB_OK, rig.do2(bb_bars_do, "save", "Work"));
    assert_eq!(BbStatus::BB_OK, rig.do2(bb_bars_do, "reset", ""));
    assert_eq!(true, q("state", "")["isDefault"]);
    assert_eq!(BbStatus::BB_OK, rig.do2(bb_bars_do, "apply", "Work"));
    assert_eq!(BbStatus::BB_OK, rig.do2(bb_barpage_do, "select", "title"));
    assert_eq!("title", q("page", "")["target"]["key"]);
    assert_eq!(BbStatus::BB_ERR, rig.do2(bb_bars_do, "apply", "Nope"));
    assert_eq!(BbStatus::BB_OK, rig.do2(bb_bars_do, "reset", ""));
}

#[test]
fn columns_and_the_property_seam_through_the_c_surface() {
    let rig = Rig::new();
    let cols = |rig: &Rig| -> Vec<String> {
        let mut out = none();
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_columns_state(rig.app, rig.tab, &mut out)
        });
        let v: Value = serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap();
        v["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect()
    };
    let call = |verb: &str, arg: &str| -> BbStatus {
        let (v, a) = (w(verb), w(arg));
        unsafe { bb_columns_do(rig.app, rig.tab, bs(&v), bs(&a)) }
    };
    assert_eq!(BbStatus::BB_OK, call("add", "Created"));
    assert_eq!(
        vec!["Name", "Size", "Type", "Modified", "Created"],
        cols(&rig)
    );
    assert_eq!(BbStatus::BB_OK, call("move", "Created 1"));
    assert_eq!(BbStatus::BB_ERR, call("width", "Created"));

    // The host reads shell properties.
    let flag = w("propertyProvider");
    unsafe { bb_ui_set_flag(rig.app, bs(&flag), true) };
    assert_eq!(BbStatus::BB_OK, call("add", "System.Image.Dimensions"));
    assert!(
        unsafe { bb_app_is_busy(rig.app) },
        "the host has not looked yet"
    );
    let mut out = none();
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_props_requests(rig.app, rig.tab, 0, 0, &mut out)
    });
    let asked: Value = serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap();
    assert_eq!(4, asked.as_array().unwrap().len());
    let answers: Vec<Value> = asked
        .as_array()
        .unwrap()
        .iter()
        .map(|e| serde_json::json!({ "path": e[0], "canonical": e[1], "value": { "display": "64 x 64" } }))
        .collect();
    let answers = w(&serde_json::to_string(&answers).unwrap());
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_props_supply(rig.app, rig.tab, bs(&answers))
    });
    rig.settle();
    let mut tile = none();
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_tab_tile_state(rig.app, rig.tab, &mut tile)
    });
    assert!(unsafe { take(tile) }.unwrap().contains("4:3"));
    assert!(
        rig.seen
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.0 == BbEventKind::BB_EVT_PROPERTIES_NEEDED)
    );
}

#[test]
fn saved_searches_workspaces_and_bookmarks_through_the_c_surface() {
    let rig = Rig::new();
    let q = w("report");
    unsafe { bb_tab_set_search(rig.app, rig.tab, bs(&q), false) };
    rig.settle();
    let (verb, arg) = (w("save-search"), w("Reports"));
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_saved_do(rig.app, rig.tab, bs(&verb), bs(&arg))
    });
    let saved_do = |verb: &str, arg: &str| -> BbStatus {
        let (v, a) = (w(verb), w(arg));
        unsafe { bb_saved_do(rig.app, 0, bs(&v), bs(&a)) }
    };
    assert_eq!(BbStatus::BB_OK, saved_do("save-workspace", "Projects"));
    let saved = rig.json(bb_saved_state);
    assert_eq!("Reports", saved["searches"][0]["name"]);
    assert_eq!("1 pane, 1 tab", saved["workspaces"][0]["shape"]);
    assert_eq!(BbStatus::BB_OK, saved_do("remove-search", "Reports"));
    let mut name = none();
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_saved_default_name(rig.app, rig.tab, true, &mut name)
    });
    assert!(unsafe { take(name) }.unwrap().starts_with("Workspace "));
}

#[test]
fn settings_page_flush_and_the_debounce() {
    let rig = Rig::with(60_000, false);
    let page = w("keyboard");
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_settings_page_open(rig.app, bs(&page))
    });
    assert_eq!("keyboard", rig.json(bb_settings_page_state)["page"]);
    let key = w("showHiddenItems");
    let on = w("true");
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_settings_set(rig.app, bs(&key), bs(&on))
    });
    let file = rig.data.join("settings.json");
    assert!(
        !std::fs::read_to_string(&file)
            .unwrap_or_default()
            .contains("showHiddenItems\": true")
    );
    assert_eq!(BbStatus::BB_OK, unsafe { bb_settings_flush(rig.app) });
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("showHiddenItems\": true")
    );

    let key = w("customCommands");
    let bad = w(r#"[{"id":"a","name":"x","command":""}]"#);
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_settings_set(rig.app, bs(&key), bs(&bad))
    });
    assert_eq!(BbStatus::BB_ERR, unsafe { bb_settings_page_close(rig.app) });
    assert!(last_error().contains("needs a program"), "{}", last_error());
    let ok = w("[]");
    unsafe { bb_settings_set(rig.app, bs(&key), bs(&ok)) };
    assert_eq!(BbStatus::BB_OK, unsafe { bb_settings_page_close(rig.app) });
    assert_eq!(BbStatus::BB_ERR, unsafe {
        bb_settings_page_open(rig.app, bs(&w("nowhere")))
    });
}

#[test]
fn themes_follow_the_system_and_announce_a_repush() {
    let rig = Rig::new();
    unsafe { bb_theme_set_system_appearance(rig.app, true, false) };
    assert_eq!("dark-plus", rig.json(bb_theme_state)["id"]);
    unsafe { bb_theme_set_system_appearance(rig.app, false, false) };
    assert_eq!("light-plus", rig.json(bb_theme_state)["id"]);
    let (verb, arg) = (w("slot"), w("light solarized-light"));
    let mut result = none();
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_theme_do(rig.app, bs(&verb), bs(&arg), &mut result)
    });
    assert_eq!("solarized-light", rig.json(bb_theme_state)["id"]);
    let (verb, arg) = (w("select"), w("nord"));
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_theme_do(rig.app, bs(&verb), bs(&arg), ptr::null_mut())
    });
    assert_eq!(false, rig.json(bb_theme_state)["following"]);
    assert!(
        rig.seen
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.0 == BbEventKind::BB_EVT_MODEL_CHANGED && e.2 == 2)
    );
    let mut count = 0u32;
    let mut buf = vec![0u32; bb_theme_token_count() as usize];
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_theme_palette(rig.app, buf.as_mut_ptr(), buf.len() as u32, &mut count)
    });
    assert_eq!(buf.len() as u32, count);
}

#[test]
fn the_declined_index_through_the_c_surface() {
    let rig = Rig::with(0, true);
    let state = rig.json(bb_index_state);
    assert_eq!(true, state["banner"]["show"]);
    let v = w("start");
    assert_eq!(BbStatus::BB_OK, unsafe { bb_index_do(rig.app, bs(&v)) });
    rig.settle();
    let state = rig.json(bb_index_state);
    assert!(
        state["status"]
            .as_str()
            .unwrap()
            .contains("permission declined")
    );
    let v = w("dismiss-banner");
    assert_eq!(BbStatus::BB_OK, unsafe { bb_index_do(rig.app, bs(&v)) });
    let v = w("bogus");
    assert_eq!(BbStatus::BB_ERR, unsafe { bb_index_do(rig.app, bs(&v)) });
}

#[test]
fn comparing_panes_gates_swapping_them() {
    let rig = Rig::new();
    let docs = w(&rig.path("Documents"));
    let mut pane = 0u64;
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_pane_split(rig.app, 0, BbOrientation::BB_VERTICAL, bs(&docs), &mut pane)
    });
    rig.settle();
    assert_eq!(None, rig.gate("pane.swap"));
    assert_eq!(BbStatus::BB_OK, unsafe { bb_compare_toggle(rig.app) });
    assert!(
        rig.gate("pane.swap")
            .unwrap()
            .contains("Stop comparing first")
    );
    assert_eq!(BbStatus::BB_OK, unsafe { bb_compare_toggle(rig.app) });
    assert_eq!(None, rig.gate("pane.swap"));
    assert_eq!(BbStatus::BB_OK, unsafe { bb_pane_swap(rig.app) });
}

#[test]
fn a_lowered_flat_cap_truncates_the_flat_listing_and_says_so() {
    let rig = Rig::new();
    unsafe {
        assert_eq!(BbStatus::BB_ERR, bb_flat_set_cap(rig.app, 0), "a cap of nothing is refused");
        assert_eq!(BbStatus::BB_OK, bb_flat_set_cap(rig.app, 2));
        assert_eq!(BbStatus::BB_OK, bb_tab_set_flat(rig.app, rig.tab, true));
    }
    rig.settle();
    let mut state: BbTabState = unsafe { std::mem::zeroed() };
    assert_eq!(BbStatus::BB_OK, unsafe { bb_tab_state(rig.app, rig.tab, &mut state) });
    assert_eq!(2, state.count, "the listing stops at the cap");
    let mut out = none();
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_tab_text(rig.app, rig.tab, BbTabText::BB_TEXT_STATUS, &mut out)
    });
    let status = unsafe { take(out) }.unwrap();
    assert!(status.contains("first 2"), "{status}");
    let mut notice = none();
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_tab_text(rig.app, rig.tab, BbTabText::BB_TEXT_NOTICE, &mut notice)
    });
    assert!(unsafe { take(notice) }.is_some(), "the banner says it is not everything");
}

#[test]
fn the_theme_editor_can_list_every_token_with_its_colour() {
    let rig = Rig::new();
    let mut out = none();
    assert_eq!(BbStatus::BB_OK, unsafe { bb_theme_tokens(rig.app, &mut out) });
    let tokens: Value = serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap();
    let tokens = tokens.as_array().unwrap();
    assert_eq!(bb_theme_token_count() as usize, tokens.len());
    assert_eq!("Theme.Window.Background", tokens[0]["key"]);
    assert!(tokens[0]["argb"].as_u64().unwrap() > 0, "a colour, not nothing");
    assert!(tokens.iter().all(|t| t["overridden"] == false));
}

#[test]
fn the_search_syntax_help_is_the_parsers_own_table() {
    let mut out = none();
    assert_eq!(BbStatus::BB_OK, unsafe { bb_search_syntax_help(&mut out) });
    let sections: Value = serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap();
    let sections = sections.as_array().unwrap();
    assert!(!sections.is_empty());
    let all = sections.iter().flat_map(|s| s["entries"].as_array().unwrap()).count();
    assert!(all > 5, "{all} help lines");
    assert!(sections[0]["title"].as_str().is_some_and(|t| !t.is_empty()));
}

#[test]
fn the_duplicate_scan_finds_copies_without_an_index_helper() {
    let rig = Rig::new();
    std::fs::create_dir_all(rig.tree.join("Copies")).unwrap();
    std::fs::copy(rig.tree.join("photo.jpg"), rig.tree.join("Copies/photo again.jpg")).unwrap();
    let path = w(&rig.tree.to_string_lossy());
    let mut out = none();
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_duplicates_scan(rig.app, bs(&path), 1, &mut out)
    }, "{}", last_error());
    let result: Value = serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap();
    let groups = result["groups"].as_array().unwrap();
    assert_eq!(1, groups.len(), "{result}");
    assert_eq!(2, groups[0]["files"].as_array().unwrap().len());
    assert_eq!(5000, groups[0]["sizeBytes"]);
    let shallowest = groups[0]["keep"]["shallowest"].as_u64().unwrap() as usize;
    assert_eq!("photo.jpg", groups[0]["files"][shallowest]["name"], "the copy nearest the top is kept");
}
