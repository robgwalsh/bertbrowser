//! The side panes' exports, driven the way the C++ views drive them: raw pointers, views, owned
//! strings, JSON models, a worker-thread preview, a GIF handle, and a property host of C callbacks.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr;
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

struct Rig {
    app: *mut BbApp,
    tab: u64,
    dir: PathBuf,
    data: PathBuf,
}

impl Rig {
    fn new() -> Rig {
        let dir = unique_temp_path("bb-ffi-panes", "");
        let data = unique_temp_path("bb-ffi-panes-data", "");
        std::fs::create_dir_all(&dir).unwrap();
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
            settings_debounce_ms: 0,
            index_declined: true,
        };
        let mut app: *mut BbApp = ptr::null_mut();
        assert_eq!(BbStatus::BB_OK, unsafe { bb_app_create_ex(&cfg, &mut app) });
        let rig = Rig {
            app,
            tab: 0,
            dir,
            data,
        };
        rig.fixtures("metadata");
        let start = w(&rig.dir.to_string_lossy());
        let mut tab = 0u64;
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_app_initialize(app, bs(&start), &mut tab)
        });
        let mut rig = rig;
        rig.tab = tab;
        rig.settle();
        rig
    }

    fn fixtures(&self, kind: &str) {
        let (k, d) = (w(kind), w(&self.dir.to_string_lossy()));
        assert_eq!(BbStatus::BB_OK, unsafe { bb_fixture_write(bs(&k), bs(&d)) });
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

    fn path(&self, name: &str) -> String {
        self.dir.join(name).to_string_lossy().into_owned()
    }

    fn select(&self, names: &[&str]) {
        let listing = unsafe { bb_tab_listing(self.app, self.tab) };
        assert!(!listing.is_null());
        let count = unsafe { bb_listing_len(listing) };
        let mut wanted = Vec::new();
        for i in 0..count {
            let mut row = unsafe { std::mem::zeroed::<BbRow>() };
            assert_eq!(BbStatus::BB_OK, unsafe {
                bb_listing_row(listing, i, &mut row)
            });
            let name = String::from_utf16_lossy(unsafe {
                std::slice::from_raw_parts(row.name.ptr, row.name.len as usize)
            });
            if names.contains(&name.as_str()) {
                wanted.push(i);
            }
        }
        unsafe { bb_listing_release(listing) };
        assert_eq!(wanted.len(), names.len(), "every name is listed");
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_tab_select(
                self.app,
                self.tab,
                BbSelectOp::BB_SELECT_REPLACE,
                wanted.as_ptr(),
                wanted.len() as u32,
            )
        });
    }

    fn wait_op(&self, op: u64) {
        let start = Instant::now();
        loop {
            let mut info = unsafe { std::mem::zeroed::<BbOpInfo>() };
            let mut message = none();
            assert_eq!(BbStatus::BB_OK, unsafe {
                bb_op_state(self.app, op, &mut info, &mut message)
            });
            let text = unsafe { take(message) };
            if info.phase == BbOpPhase::BB_OP_DONE {
                assert!(info.ok, "the operation failed: {text:?}");
                self.settle();
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "operation never finished"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn model(&self) -> Value {
        let mut out = none();
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_metadata_model(self.app, self.tab, &mut out)
        });
        serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        unsafe { bb_app_destroy(self.app) };
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

#[test]
fn the_fixtures_are_real_files_and_peek_reads_them_back_through_the_codec() {
    let rig = Rig::new();
    for name in ["camera.jpg", "plain.jpg", "first.mp3", "second.mp3"] {
        assert!(
            std::fs::metadata(rig.path(name)).unwrap().len() > 0,
            "{name}"
        );
    }
    let (path, what) = (w(&rig.path("camera.jpg")), w("title"));
    let mut out = none();
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_metadata_peek(rig.app, bs(&path), bs(&what), &mut out)
    });
    assert_eq!("Old title", unsafe { take(out) }.unwrap());

    let nope = w("nonsense");
    assert_eq!(BbStatus::BB_ERR, unsafe {
        bb_metadata_peek(rig.app, bs(&path), bs(&nope), &mut out)
    });
}

#[test]
fn a_preview_is_built_from_a_worker_thread_as_json() {
    let rig = Rig::new();
    std::fs::write(rig.path("code.cs"), "using System;\n// hi\n").unwrap();
    let app = rig.app as usize;
    let path = rig.path("code.cs");
    let json = std::thread::spawn(move || {
        let p = w(&path);
        let mut out = none();
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_preview_build_path(app as *mut BbApp, bs(&p), 0, &mut out)
        });
        serde_json::from_str::<Value>(&unsafe { take(out) }.unwrap()).unwrap()
    })
    .join()
    .unwrap();
    assert_eq!(json["state"], "text");
    assert_eq!(json["lines"][0], "using System;");
    assert!(json["footer"].as_str().unwrap().contains("2 lines"));
}

/// A GIF89a of `frames` full-canvas frames, 8 bits to a code, a clear code before every pixel.
fn gif(width: u16, height: u16, frames: u16) -> Vec<u8> {
    let mut out = b"GIF89a".to_vec();
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&[0xF6, 0, 0]);
    let mut palette = vec![0u8; 128 * 3];
    palette[3..6].copy_from_slice(&[0xE0, 0x30, 0x30]);
    palette[6..9].copy_from_slice(&[0x30, 0xC0, 0x50]);
    out.extend_from_slice(&palette);
    for f in 0..frames {
        out.extend_from_slice(&[0x21, 0xF9, 0x04, 0x04]);
        out.extend_from_slice(&5u16.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0x2C, 0, 0, 0, 0]);
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&[0, 7]);
        let mut codes = Vec::new();
        for i in 0..(width as usize * height as usize) {
            if i % 100 == 0 {
                codes.push(128);
            }
            codes.push(1 + (f % 2) as u8);
        }
        codes.push(129);
        for block in codes.chunks(255) {
            out.push(block.len() as u8);
            out.extend_from_slice(block);
        }
        out.push(0);
    }
    out.push(0x3B);
    out
}

#[test]
fn a_gif_opens_with_its_first_frame_and_the_rest_arrive() {
    let rig = Rig::new();
    std::fs::write(rig.path("spin.gif"), gif(8, 8, 6)).unwrap();
    let p = w(&rig.path("spin.gif"));
    let (mut handle, mut out) = (0u64, none());
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_gif_open(rig.app, bs(&p), &mut handle, &mut out)
    });
    let info: Value = serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap();
    assert_eq!(info["animated"], true);
    assert_eq!(info["frames"], 6);
    assert_eq!(info["durationMs"], 6 * 50);
    assert_ne!(handle, 0);

    let start = Instant::now();
    let (mut ready, mut flags) = (0u32, 0u32);
    while flags & 1 == 0 && start.elapsed() < Duration::from_secs(5) {
        unsafe { bb_gif_ready(rig.app, handle, &mut ready, &mut flags) };
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!((ready, flags & 1), (6, 1));

    let mut pixels = vec![0u8; 8 * 8 * 4];
    let mut at = 0u32;
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_gif_frame(
            rig.app,
            handle,
            1,
            pixels.as_mut_ptr(),
            pixels.len(),
            &mut at,
        )
    });
    assert_eq!(at, 1);
    assert_eq!(
        &pixels[..4],
        &[0x50, 0xC0, 0x30, 255],
        "frame 1 is the green one, as BGRA"
    );

    let (mut frame, mut until) = (0u32, 0i64);
    unsafe { bb_gif_position(rig.app, handle, 125, &mut frame, &mut until) };
    assert_eq!((frame, until), (2, 25));
    unsafe { bb_gif_position(rig.app, handle, 325, &mut frame, &mut until) };
    assert_eq!(frame, 0, "a position past the end wraps");

    assert_eq!(BbStatus::BB_OK, unsafe { bb_gif_close(rig.app, handle) });
    assert_eq!(BbStatus::BB_STALE, unsafe {
        bb_gif_ready(rig.app, handle, &mut ready, &mut flags)
    });
}

#[test]
fn one_frame_is_a_picture_and_garbage_is_unreadable() {
    let rig = Rig::new();
    for (name, bytes, reason) in [
        ("still.gif", gif(4, 4, 1), "still"),
        ("junk.gif", b"not a gif".to_vec(), "unreadable"),
    ] {
        std::fs::write(rig.path(name), bytes).unwrap();
        let p = w(&rig.path(name));
        let (mut handle, mut out) = (7u64, none());
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_gif_open(rig.app, bs(&p), &mut handle, &mut out)
        });
        let info: Value = serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap();
        assert_eq!(
            (handle, info["animated"].as_bool(), info["reason"].as_str()),
            (0, Some(false), Some(reason))
        );
    }
}

#[test]
fn the_metadata_pane_edits_a_file_through_the_queue_and_one_undo_takes_it_back() {
    let rig = Rig::new();
    rig.select(&["camera.jpg"]);
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_metadata_load(rig.app, rig.tab, false)
    });
    assert_eq!(rig.model()["title"], "camera.jpg");

    let (id, text) = (w("title"), w("Harbour at dawn"));
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_metadata_set_text(rig.app, rig.tab, bs(&id), bs(&text))
    });
    assert_eq!(rig.model()["canApply"], true);

    let mut op = 0u64;
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_metadata_act(rig.app, rig.tab, 0, ptr::null(), 0, &mut op)
    });
    rig.wait_op(op);
    let (path, what) = (w(&rig.path("camera.jpg")), w("title"));
    let mut out = none();
    unsafe { bb_metadata_peek(rig.app, bs(&path), bs(&what), &mut out) };
    assert_eq!("Harbour at dawn", unsafe { take(out) }.unwrap());

    let mut undo = 0u64;
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_undo_step(rig.app, 1, &mut undo)
    });
    rig.wait_op(undo);
    unsafe { bb_metadata_peek(rig.app, bs(&path), bs(&what), &mut out) };
    assert_eq!("Old title", unsafe { take(out) }.unwrap());

    // Clearing the history lets go of what it was holding.
    assert_eq!(BbStatus::BB_OK, unsafe { bb_undo_clear(rig.app) });
}

#[test]
fn a_button_that_would_not_work_is_refused_with_its_reason() {
    let rig = Rig::new();
    rig.select(&["camera.jpg"]);
    unsafe { bb_metadata_load(rig.app, rig.tab, false) };
    let mut op = 0u64;
    assert_eq!(BbStatus::BB_ERR, unsafe {
        bb_metadata_act(rig.app, rig.tab, 0, ptr::null(), 0, &mut op)
    });
    let mut message = BbStr {
        ptr: ptr::null(),
        len: 0,
    };
    unsafe { bb_last_error(&mut message) };
    let text = String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(message.ptr, message.len as usize)
    });
    assert!(text.contains("would not apply"), "{text}");
}

// ---- the property host ---------------------------------------------------------------------------

unsafe extern "C" fn host_details(_: *mut c_void, _: BbStr, out: *mut BbStr) -> BbStatus {
    static TEXT: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
    let json = TEXT.get_or_init(|| {
        w(r#"[{"canonical":"System.Image.Dimensions","name":"Dimensions","value":"8 x 8"}]"#)
    });
    unsafe { *out = bs(json) };
    BbStatus::BB_OK
}

#[test]
fn the_hosts_details_appear_under_a_picture() {
    let rig = Rig::new();
    let host = BbPropertyHost {
        user: ptr::null_mut(),
        details: Some(host_details),
        read_fields: None,
        write_fields: None,
    };
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_set_property_host(rig.app, host)
    });
    rig.select(&["camera.jpg"]);
    unsafe { bb_metadata_load(rig.app, rig.tab, false) };
    let model = rig.model();
    assert_eq!(model["details"][0]["label"], "Dimensions");
    assert_eq!(model["details"][0]["value"], "8 x 8");
}

// ---- the arithmetic ------------------------------------------------------------------------------

#[test]
fn the_zoom_rules_are_the_cores() {
    assert_eq!(bb_zoom_step(1.0, 0.0, 1.0), 1.0);
    assert!((bb_zoom_step(1.0, 1.0, 1.0) - 1.2).abs() < 1e-9);
    assert_eq!(bb_zoom_step(1.0, -5.0, 1.0), 1.0, "never below fit");
    assert_eq!(bb_zoom_clamp(1000.0, 1.0), 64.0);
    // A picture smaller than the pane is centred; one larger than it cannot be dragged off.
    assert_eq!(bb_zoom_clamp_offset(30.0, 10.0, 100.0, 1.0, 200.0), 40.0);
    assert_eq!(bb_zoom_clamp_offset(500.0, 0.0, 100.0, 4.0, 200.0), 0.0);
    assert_eq!(bb_zoom_offset(0.0, 10.0, 1.0, 2.0), -10.0);
}

#[test]
fn settling_with_nothing_compared_says_so() {
    let rig = Rig::new();
    let p = w(&rig.path("camera.jpg"));
    let views = [bs(&p)];
    let mut identical = 0u32;
    assert_eq!(BbStatus::BB_ERR, unsafe {
        bb_compare_settle(rig.app, views.as_ptr(), 1, &mut identical)
    });
}
