//! Drives the `extern "C"` surface exactly as the C++ side does: raw pointers, views, owned strings
//! and a sink. Everything here goes through the exported functions, never through bb-app.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bb_test_support::unique_temp_path;
use bbcore::*;

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn bs(units: &[u16]) -> BbStr {
    BbStr {
        ptr: units.as_ptr(),
        len: units.len() as u32,
    }
}

unsafe fn text(s: BbStr) -> String {
    if s.ptr.is_null() {
        return String::new();
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(s.ptr, s.len as usize) })
}

unsafe fn take(s: BbOwnedStr) -> Option<String> {
    if s.ptr.is_null() {
        return None;
    }
    let t = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(s.ptr, s.len as usize) });
    unsafe { bb_str_free(s) };
    Some(t)
}

fn last_error() -> String {
    let mut s = BbStr {
        ptr: ptr::null(),
        len: 0,
    };
    unsafe {
        bb_last_error(&mut s);
        text(s)
    }
}

struct Seen(Mutex<Vec<(BbEventKind, u64, u64, String)>>);

unsafe extern "C" fn sink(ctx: *mut c_void, evt: *mut BbEvent) {
    let seen = unsafe { &*(ctx as *const Seen) };
    let e = unsafe { &*evt };
    seen.0
        .lock()
        .unwrap()
        .push((e.kind, e.id, e.a, unsafe { text(e.text) }));
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
        let tree = unique_temp_path("bb-ffi-tree", "");
        let data = unique_temp_path("bb-ffi-data", "");
        for (rel, len) in [
            ("Documents/report.md", 3),
            ("notes.txt", 100),
            ("photo.jpg", 5000),
            ("archive.zip", 20000),
        ] {
            let full = tree.join(rel);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, vec![b'x'; len]).unwrap();
        }
        let data_w = w(&data.to_string_lossy());
        let staging = unique_temp_path("bb-ffi-staging", "");
        std::fs::create_dir_all(&staging).unwrap();
        let staging_w = w(&staging.to_string_lossy());
        let cfg = BbAppConfig {
            data_dir: bs(&data_w),
            fakes: BbFakes::BB_FAKES_HARNESS,
            search_debounce_ms: 0,
            watch_debounce_ms: 40,
            delete_staging_root: bs(&staging_w),
            workers: 0,
        };
        let mut app: *mut BbApp = ptr::null_mut();
        assert_eq!(BbStatus::BB_OK, unsafe { bb_app_create(&cfg, &mut app) });
        assert!(!app.is_null());
        let seen = Box::new(Seen(Mutex::new(Vec::new())));
        unsafe {
            bb_app_set_event_sink(app, Some(sink), &*seen as *const Seen as *mut c_void);
        }
        let start = w(&tree.to_string_lossy());
        let mut tab = 0u64;
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_app_initialize(app, bs(&start), &mut tab)
        });
        assert_ne!(0, tab);
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
            } else {
                let since = *quiet.get_or_insert_with(Instant::now);
                if since.elapsed() > Duration::from_millis(40) {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the app never went quiet");
    }

    fn path(&self, rel: &str) -> String {
        self.tree.join(rel).to_string_lossy().into_owned()
    }

    fn names(&self) -> Vec<String> {
        unsafe {
            let l = bb_tab_listing(self.app, self.tab);
            assert!(!l.is_null());
            let n = bb_listing_len(l);
            let mut out = Vec::new();
            for i in 0..n {
                let mut row = std::mem::zeroed::<BbRow>();
                assert_eq!(BbStatus::BB_OK, bb_listing_row(l, i, &mut row));
                out.push(text(row.name));
            }
            bb_listing_release(l);
            out
        }
    }

    fn wait_op(&self, op: u64) -> (bool, String) {
        let start = Instant::now();
        loop {
            let mut info = BbOpInfo {
                phase: BbOpPhase::BB_OP_QUEUED,
                ok: false,
            };
            let mut message = BbOwnedStr {
                ptr: ptr::null_mut(),
                len: 0,
            };
            assert_eq!(BbStatus::BB_OK, unsafe {
                bb_op_state(self.app, op, &mut info, &mut message)
            });
            let message = unsafe { take(message) }.unwrap_or_default();
            if info.phase == BbOpPhase::BB_OP_DONE {
                self.settle();
                return (info.ok, message);
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "operation never finished"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn select(&self, op: BbSelectOp, idx: &[u32]) {
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_tab_select(self.app, self.tab, op, idx.as_ptr(), idx.len() as u32)
        });
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        unsafe { bb_app_destroy(self.app) };
        let _ = std::fs::remove_dir_all(&self.tree);
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

fn exists(p: &str) -> bool {
    Path::new(p).exists()
}

#[test]
fn the_version_is_a_static_view() {
    let v = unsafe { text(bb_version()) };
    assert!(v.starts_with("2."), "{v}");
}

#[test]
fn a_panic_is_a_status_with_a_message_and_not_an_unwind() {
    assert_eq!(BbStatus::BB_PANIC, bb_selftest_panic());
    assert_eq!("selftest: deliberate panic", last_error());
}

#[test]
fn null_and_stale_handles_answer_instead_of_crashing() {
    unsafe {
        assert_eq!(BbStatus::BB_ERR, bb_tab_refresh(ptr::null_mut(), 1));
        assert_eq!("null app", last_error());
        assert!(!bb_app_is_busy(ptr::null_mut()));
        assert!(bb_tab_listing(ptr::null_mut(), 1).is_null());
        assert_eq!(0, bb_listing_len(ptr::null()));
        bb_listing_release(ptr::null());
        bb_app_destroy(ptr::null_mut());
        bb_str_free(BbOwnedStr {
            ptr: ptr::null_mut(),
            len: 0,
        });
        assert_eq!(
            BbStatus::BB_ERR,
            bb_app_create(ptr::null(), ptr::null_mut())
        );
    }
    let rig = Rig::new();
    unsafe {
        assert_eq!(BbStatus::BB_STALE, bb_tab_refresh(rig.app, 999_999));
        assert!(bb_tab_listing(rig.app, 999_999).is_null());
        assert_eq!(BbStatus::BB_STALE, bb_pane_close(rig.app, 999_999));
        assert_eq!(BbStatus::BB_STALE, bb_op_cancel(rig.app, 999_999));
        let mut state = std::mem::zeroed::<BbTabState>();
        assert_eq!(
            BbStatus::BB_STALE,
            bb_tab_state(rig.app, 999_999, &mut state)
        );
        let mut row = std::mem::zeroed::<BbRow>();
        let l = bb_tab_listing(rig.app, rig.tab);
        assert_eq!(BbStatus::BB_ERR, bb_listing_row(l, 999, &mut row));
        assert!(last_error().contains("out of range"));
        bb_listing_release(l);
    }
}

#[test]
fn the_listing_is_a_refcounted_snapshot_of_views() {
    let rig = Rig::new();
    unsafe {
        let mut state = std::mem::zeroed::<BbTabState>();
        assert_eq!(BbStatus::BB_OK, bb_tab_state(rig.app, rig.tab, &mut state));
        assert_eq!(4, state.count);
        assert_eq!(rig.tab, state.id);
        assert!(!state.can_back && state.can_up && !state.is_flattened);

        let l = bb_tab_listing(rig.app, rig.tab);
        assert_eq!(state.version, bb_listing_version(l));
        bb_listing_retain(l);
        bb_listing_release(l);

        let mut row = std::mem::zeroed::<BbRow>();
        assert_eq!(BbStatus::BB_OK, bb_listing_row(l, 0, &mut row));
        assert_eq!("Documents", text(row.name));
        assert!(row.flags & BB_ROW_DIRECTORY != 0);
        assert_eq!(-1, row.size, "an unmeasured folder is unknown, never zero");
        assert_eq!(rig.path("Documents"), text(row.full_path));
        assert_ne!(0, row.row_id);

        assert_eq!(BbStatus::BB_OK, bb_listing_row(l, 3, &mut row));
        assert_eq!("photo.jpg", text(row.name));
        assert_eq!(5000, row.size);
        let first_id = row.row_id;

        let mut cell = BbStr {
            ptr: ptr::null(),
            len: 0,
        };
        assert_eq!(BbStatus::BB_OK, bb_listing_cell(l, 3, 4, &mut cell));
        assert_eq!("4.9 KB", text(cell));
        assert_eq!(BbStatus::BB_OK, bb_listing_cell(l, 3, 5, &mut cell));
        assert_eq!("JPG file", text(cell));

        // A refresh makes a new snapshot; the old one is untouched and still readable.
        std::fs::write(rig.tree.join("zzz.txt"), b"x").unwrap();
        assert_eq!(BbStatus::BB_OK, bb_tab_refresh(rig.app, rig.tab));
        rig.settle();
        assert_eq!(4, bb_listing_len(l));
        let l2 = bb_tab_listing(rig.app, rig.tab);
        assert_eq!(5, bb_listing_len(l2));
        assert!(bb_listing_version(l2) > bb_listing_version(l));
        assert_eq!(BbStatus::BB_OK, bb_listing_row(l2, 3, &mut row));
        assert_eq!(first_id, row.row_id, "row ids are stable across a reload");
        bb_listing_release(l);
        bb_listing_release(l2);
    }
    let seen = rig.seen.0.lock().unwrap();
    assert!(
        seen.iter()
            .any(|e| e.0 == BbEventKind::BB_EVT_LISTING_CHANGED && e.1 == rig.tab)
    );
    assert!(
        seen.iter()
            .any(|e| e.0 == BbEventKind::BB_EVT_BUSY_CHANGED && e.2 == 1)
    );
    assert_eq!(
        Some(0),
        seen.iter()
            .rev()
            .find(|e| e.0 == BbEventKind::BB_EVT_BUSY_CHANGED)
            .map(|e| e.2),
        "the last busy flip is to quiet"
    );
}

#[test]
fn navigation_search_and_the_texts_of_a_tab() {
    let rig = Rig::new();
    unsafe {
        let docs = w(&rig.path("Documents"));
        assert_eq!(
            BbStatus::BB_OK,
            bb_tab_navigate(rig.app, rig.tab, bs(&docs))
        );
        rig.settle();
        assert_eq!(vec!["report.md".to_string()], rig.names());
        let mut t = BbOwnedStr {
            ptr: ptr::null_mut(),
            len: 0,
        };
        assert_eq!(
            BbStatus::BB_OK,
            bb_tab_text(rig.app, rig.tab, BbTabText::BB_TEXT_TITLE, &mut t)
        );
        assert_eq!(Some("Documents".to_string()), take(t));
        assert_eq!(
            BbStatus::BB_OK,
            bb_tab_text(rig.app, rig.tab, BbTabText::BB_TEXT_ERROR, &mut t)
        );
        assert_eq!(None, take(t), "no error is NULL, not empty");
        assert_eq!(
            BbStatus::BB_OK,
            bb_tab_text(rig.app, rig.tab, BbTabText::BB_TEXT_STATUS, &mut t)
        );
        assert_eq!(Some("1 item(s)".to_string()), take(t));

        assert_eq!(BbStatus::BB_OK, bb_tab_breadcrumb(rig.app, rig.tab, &mut t));
        let crumbs = take(t).unwrap();
        assert!(
            crumbs.lines().last().unwrap().starts_with("Documents\t"),
            "{crumbs}"
        );

        assert_eq!(BbStatus::BB_OK, bb_tab_back(rig.app, rig.tab));
        rig.settle();
        assert_eq!(4, rig.names().len());
        assert_eq!(BbStatus::BB_ERR, bb_tab_back(rig.app, rig.tab));
        assert_eq!(BbStatus::BB_OK, bb_tab_forward(rig.app, rig.tab));
        assert_eq!(BbStatus::BB_OK, bb_tab_up(rig.app, rig.tab));
        rig.settle();

        let missing = w("Q:\\no\\such\\folder");
        assert_eq!(
            BbStatus::BB_ERR,
            bb_tab_navigate(rig.app, rig.tab, bs(&missing))
        );
        assert!(
            last_error().starts_with("Folder not found"),
            "{}",
            last_error()
        );

        let q = w("report");
        assert_eq!(
            BbStatus::BB_OK,
            bb_tab_set_search(rig.app, rig.tab, bs(&q), false)
        );
        rig.settle();
        let mut state = std::mem::zeroed::<BbTabState>();
        assert_eq!(BbStatus::BB_OK, bb_tab_state(rig.app, rig.tab, &mut state));
        assert!(state.is_flattened && !state.is_flat_browse);
        assert_eq!(1, state.count);

        let empty = w("");
        assert_eq!(
            BbStatus::BB_OK,
            bb_tab_set_search(rig.app, rig.tab, bs(&empty), false)
        );
        rig.settle();
        assert_eq!(BbStatus::BB_OK, bb_tab_set_flat(rig.app, rig.tab, true));
        rig.settle();
        assert_eq!(BbStatus::BB_OK, bb_tab_state(rig.app, rig.tab, &mut state));
        assert!(state.is_flattened && state.is_flat_browse && state.is_flat_view);
        assert_eq!(4, state.count, "files only");
        assert_eq!(BbStatus::BB_OK, bb_tab_set_flat(rig.app, rig.tab, false));
        rig.settle();

        assert_eq!(BbStatus::BB_OK, bb_tab_columns(rig.app, rig.tab, &mut t));
        let columns = take(t).unwrap();
        assert!(
            columns.lines().next().unwrap().starts_with("Name\tName\t"),
            "{columns}"
        );
    }
}

#[test]
fn selection_is_held_in_rust_and_read_back() {
    let rig = Rig::new();
    // Documents, archive.zip, notes.txt, photo.jpg.
    rig.select(BbSelectOp::BB_SELECT_REPLACE, &[2, 3]);
    unsafe {
        let mut count = 0u32;
        let mut idx = [0u32; 8];
        assert_eq!(
            BbStatus::BB_OK,
            bb_tab_selection(rig.app, rig.tab, idx.as_mut_ptr(), 8, &mut count)
        );
        assert_eq!(2, count);
        assert_eq!([2, 3], idx[..2]);
        // A short buffer still reports the total.
        let mut one = [0u32; 1];
        assert_eq!(
            BbStatus::BB_OK,
            bb_tab_selection(rig.app, rig.tab, one.as_mut_ptr(), 1, &mut count)
        );
        assert_eq!((2, [2]), (count, one));

        let mut paths = BbOwnedStr {
            ptr: ptr::null_mut(),
            len: 0,
        };
        assert_eq!(
            BbStatus::BB_OK,
            bb_tab_selection_paths(rig.app, rig.tab, &mut paths)
        );
        let paths = take(paths).unwrap();
        assert_eq!(2, paths.lines().count());
        assert!(
            paths.contains("notes.txt") && paths.contains("photo.jpg"),
            "{paths}"
        );
    }
    rig.select(BbSelectOp::BB_SELECT_INVERT, &[]);
    rig.select(BbSelectOp::BB_SELECT_TOGGLE, &[0]);
    unsafe {
        let mut state = std::mem::zeroed::<BbTabState>();
        assert_eq!(BbStatus::BB_OK, bb_tab_state(rig.app, rig.tab, &mut state));
        // {2,3} inverted is {0,1}; toggling 0 leaves {1}.
        assert_eq!(1, state.selected);
    }
    rig.select(BbSelectOp::BB_SELECT_CLEAR, &[]);
}

#[test]
fn a_move_and_its_undo_through_the_c_surface() {
    let rig = Rig::new();
    let sources = [w(&rig.path("photo.jpg")), w(&rig.path("archive.zip"))];
    let views: Vec<BbStr> = sources.iter().map(|s| bs(s)).collect();
    let dest = w(&rig.path("Documents"));
    let mut op = 0u64;
    unsafe {
        assert_eq!(
            BbStatus::BB_OK,
            bb_op_move(rig.app, views.as_ptr(), 2, bs(&dest), &mut op)
        );
    }
    let (ok, message) = rig.wait_op(op);
    assert!(ok, "{message}");
    assert!(message.starts_with("Moved 2 item(s)"), "{message}");
    assert!(exists(&rig.path("Documents\\photo.jpg")));
    assert!(!rig.names().contains(&"photo.jpg".to_string()));

    unsafe {
        let mut u = BbUndoState {
            can_undo: false,
            can_redo: false,
        };
        assert_eq!(BbStatus::BB_OK, bb_undo_state(rig.app, &mut u));
        assert!(u.can_undo && !u.can_redo);
        assert_eq!(BbStatus::BB_OK, bb_undo_step(rig.app, 1, &mut op));
    }
    let (ok, message) = rig.wait_op(op);
    assert!(ok, "{message}");
    assert!(exists(&rig.path("photo.jpg")));
    assert!(rig.names().contains(&"photo.jpg".to_string()));
    unsafe {
        let mut u = BbUndoState {
            can_undo: true,
            can_redo: false,
        };
        assert_eq!(BbStatus::BB_OK, bb_undo_state(rig.app, &mut u));
        assert!(!u.can_undo && u.can_redo);
        assert_eq!(BbStatus::BB_OK, bb_undo_step(rig.app, -1, &mut op));
    }
    let (ok, _) = rig.wait_op(op);
    assert!(ok);
    assert!(exists(&rig.path("Documents\\photo.jpg")));
    unsafe {
        assert_eq!(BbStatus::BB_ERR, bb_undo_step(rig.app, 0, &mut op));
    }
}

#[test]
fn delete_rename_copy_and_the_busy_status() {
    let rig = Rig::new();
    unsafe {
        // Paused and queued: the app is writing, so a rename is refused with BB_BUSY.
        assert_eq!(BbStatus::BB_OK, bb_op_pause(rig.app, true));
        let src = [w(&rig.path("notes.txt"))];
        let views: Vec<BbStr> = src.iter().map(|s| bs(s)).collect();
        let dest = w(&rig.path("Documents"));
        let mut copy = 0u64;
        assert_eq!(
            BbStatus::BB_OK,
            bb_op_copy(rig.app, views.as_ptr(), 1, bs(&dest), &mut copy)
        );
        let pattern = w("x");
        let mut op = 0u64;
        assert_eq!(
            BbStatus::BB_BUSY,
            bb_op_rename(rig.app, views.as_ptr(), 1, bs(&pattern), &mut op)
        );
        assert_eq!(BbStatus::BB_OK, bb_op_pause(rig.app, false));
        let (ok, _) = rig.wait_op(copy);
        assert!(ok);
        assert!(exists(&rig.path("Documents\\notes.txt")));

        let pattern = w("Holiday");
        let two = [w(&rig.path("notes.txt")), w(&rig.path("photo.jpg"))];
        let views: Vec<BbStr> = two.iter().map(|s| bs(s)).collect();
        assert_eq!(
            BbStatus::BB_OK,
            bb_op_rename(rig.app, views.as_ptr(), 2, bs(&pattern), &mut op)
        );
        let (ok, message) = rig.wait_op(op);
        assert!(ok, "{message}");
        let names = rig.names();
        assert!(
            names.contains(&"Holiday 1.txt".to_string())
                && names.contains(&"Holiday 2.jpg".to_string()),
            "{names:?}"
        );

        let gone = [w(&rig.path("archive.zip"))];
        let views: Vec<BbStr> = gone.iter().map(|s| bs(s)).collect();
        assert_eq!(
            BbStatus::BB_OK,
            bb_op_delete(
                rig.app,
                views.as_ptr(),
                1,
                BbDeleteMode::BB_DELETE_STAGED,
                &mut op
            )
        );
        let (ok, message) = rig.wait_op(op);
        assert!(ok, "{message}");
        assert!(!exists(&rig.path("archive.zip")));
        assert_eq!(BbStatus::BB_OK, bb_undo_step(rig.app, 1, &mut op));
        rig.wait_op(op);
        assert!(exists(&rig.path("archive.zip")));
    }
}

#[test]
fn panes_tabs_commands_keys_settings_and_theme() {
    let rig = Rig::new();
    unsafe {
        // A split makes a second pane with its own tab.
        let pics = w(&rig.path("Documents"));
        let mut pane = 0u64;
        assert_eq!(
            BbStatus::BB_OK,
            bb_pane_split(rig.app, 0, BbOrientation::BB_VERTICAL, bs(&pics), &mut pane)
        );
        assert_ne!(0, pane);
        rig.settle();
        let mut json = BbOwnedStr {
            ptr: ptr::null_mut(),
            len: 0,
        };
        assert_eq!(BbStatus::BB_OK, bb_app_state_json(rig.app, &mut json));
        let state: String = take(json).unwrap();
        assert!(
            state.contains("\"kind\":\"split\"") && state.contains("\"vertical\""),
            "{state}"
        );
        assert_eq!(BbStatus::BB_OK, bb_pane_close(rig.app, pane));

        let mut tab2 = 0u64;
        assert_eq!(
            BbStatus::BB_OK,
            bb_tab_open(rig.app, 0, bs(&pics), true, &mut tab2)
        );
        rig.settle();
        assert_eq!(BbStatus::BB_OK, bb_tab_close(rig.app, tab2));
        assert_eq!(BbStatus::BB_STALE, bb_tab_close(rig.app, tab2));

        // F5 in the file list is nav.refresh; Backspace while typing is nobody's.
        let mut cmd = BbOwnedStr {
            ptr: ptr::null_mut(),
            len: 0,
        };
        let f5 = w("F5");
        let list = BbFocusState {
            focus: BbFocusKind::BB_FOCUS_FILE_LIST,
            flags: 0,
        };
        assert_eq!(
            BbStatus::BB_OK,
            bb_key_dispatch(
                rig.app,
                BbKeyChord {
                    modifiers: 0,
                    key: bs(&f5)
                },
                list,
                &mut cmd
            )
        );
        assert_eq!(Some("nav.refresh".to_string()), take(cmd));
        let backspace = w("Backspace");
        let typing = BbFocusState {
            focus: BbFocusKind::BB_FOCUS_TEXT_EDITABLE,
            flags: 0,
        };
        assert_eq!(
            BbStatus::BB_OK,
            bb_key_dispatch(
                rig.app,
                BbKeyChord {
                    modifiers: 0,
                    key: bs(&backspace)
                },
                typing,
                &mut cmd
            )
        );
        assert_eq!(None, take(cmd));
        let nonsense = w("NotAKey");
        assert_eq!(
            BbStatus::BB_OK,
            bb_key_dispatch(
                rig.app,
                BbKeyChord {
                    modifiers: 7,
                    key: bs(&nonsense)
                },
                list,
                &mut cmd
            )
        );
        assert_eq!(None, take(cmd));

        let id = w("nav.back");
        let mut reason = BbOwnedStr {
            ptr: ptr::null_mut(),
            len: 0,
        };
        assert_eq!(
            BbStatus::BB_OK,
            bb_command_why_not(rig.app, bs(&id), &mut reason)
        );
        assert_eq!(Some("Nothing to go back to.".to_string()), take(reason));
        assert_eq!(BbStatus::BB_ERR, bb_command_run(rig.app, bs(&id)));
        let up = w("nav.up");
        assert_eq!(
            BbStatus::BB_OK,
            bb_command_why_not(rig.app, bs(&up), &mut reason)
        );
        assert_eq!(None, take(reason));
        let unknown = w("no.such");
        assert_eq!(
            BbStatus::BB_ERR,
            bb_command_why_not(rig.app, bs(&unknown), &mut reason)
        );
        assert_eq!(BbStatus::BB_OK, bb_command_run(rig.app, bs(&up)));
        rig.settle();

        // Settings round-trip as JSON, validated, and the change is announced.
        let key = w("showHiddenItems");
        let mut value = BbOwnedStr {
            ptr: ptr::null_mut(),
            len: 0,
        };
        assert_eq!(
            BbStatus::BB_OK,
            bb_settings_get(rig.app, bs(&key), &mut value)
        );
        assert_eq!(Some("false".to_string()), take(value));
        let on = w("true");
        assert_eq!(BbStatus::BB_OK, bb_settings_set(rig.app, bs(&key), bs(&on)));
        assert_eq!(
            BbStatus::BB_OK,
            bb_settings_get(rig.app, bs(&key), &mut value)
        );
        assert_eq!(Some("true".to_string()), take(value));
        let bad = w("\"yes\"");
        assert_eq!(
            BbStatus::BB_ERR,
            bb_settings_set(rig.app, bs(&key), bs(&bad))
        );
        let nokey = w("noSuchSetting");
        assert_eq!(
            BbStatus::BB_ERR,
            bb_settings_set(rig.app, bs(&nokey), bs(&on))
        );
        rig.settle();

        // The palette: one ARGB per token, in token order.
        let n = bb_theme_token_count();
        let mut argb = vec![0u32; n as usize];
        let mut count = 0u32;
        assert_eq!(
            BbStatus::BB_OK,
            bb_theme_palette(rig.app, argb.as_mut_ptr(), n, &mut count)
        );
        assert_eq!(n, count);
        assert!(argb.iter().any(|c| *c >> 24 == 0xFF));
    }
    let seen = rig.seen.0.lock().unwrap();
    assert!(
        seen.iter()
            .any(|e| e.0 == BbEventKind::BB_EVT_SETTINGS_CHANGED && e.3 == "showHiddenItems")
    );
    assert!(
        seen.iter()
            .any(|e| e.0 == BbEventKind::BB_EVT_LAYOUT_CHANGED)
    );
}

#[test]
fn enter_on_a_file_reaches_the_refusing_launcher() {
    let rig = Rig::new();
    unsafe {
        assert_eq!(BbStatus::BB_OK, bb_tab_open_item(rig.app, rig.tab, 1));
        let mut t = BbOwnedStr {
            ptr: ptr::null_mut(),
            len: 0,
        };
        assert_eq!(
            BbStatus::BB_OK,
            bb_tab_text(rig.app, rig.tab, BbTabText::BB_TEXT_STATUS, &mut t)
        );
        let status = take(t).unwrap();
        assert!(status.contains("was not launched"), "{status}");
        // The harness probes count it.
        let mut json = BbOwnedStr {
            ptr: ptr::null_mut(),
            len: 0,
        };
        assert_eq!(BbStatus::BB_OK, bb_app_state_json(rig.app, &mut json));
        assert!(take(json).unwrap().contains("\"launches\":1"));
    }
}

#[test]
fn destroy_with_work_in_flight_and_events_still_queued_is_clean() {
    let rig = Rig::new();
    let p = w(&rig.path("Documents"));
    unsafe {
        bb_tab_navigate(rig.app, rig.tab, bs(&p));
        let q = w("report");
        bb_tab_set_search(rig.app, rig.tab, bs(&q), false);
    }
    // Dropping the rig destroys the app immediately.
}
