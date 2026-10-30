//! The operations UI's exports, driven the way the C++ side does: UTF-16 views in, JSON text out.

use std::path::PathBuf;
use std::ptr;

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

struct Rig {
    app: *mut BbApp,
    tree: PathBuf,
    data: PathBuf,
}

impl Rig {
    fn new() -> Rig {
        let tree = unique_temp_path("bb-ffi-ops-tree", "");
        let data = unique_temp_path("bb-ffi-ops-data", "");
        std::fs::create_dir_all(&tree).unwrap();
        let data_w = w(&data.to_string_lossy());
        let cfg = BbAppConfig {
            data_dir: bs(&data_w),
            fakes: BbFakes::BB_FAKES_HARNESS,
            search_debounce_ms: 0,
            watch_debounce_ms: 40,
            delete_staging_root: BbStr {
                ptr: ptr::null(),
                len: 0,
            },
            workers: 0,
        };
        let mut app: *mut BbApp = ptr::null_mut();
        assert_eq!(BbStatus::BB_OK, unsafe { bb_app_create(&cfg, &mut app) });
        let start = w(&tree.to_string_lossy());
        let mut tab = 0u64;
        assert_eq!(BbStatus::BB_OK, unsafe {
            bb_app_initialize(app, bs(&start), &mut tab)
        });
        Rig { app, tree, data }
    }

    fn query(&self, what: &str, arg: &str) -> Result<Value, String> {
        let (what, arg) = (w(what), w(arg));
        let mut out = none();
        if unsafe { bb_ops_query(self.app, bs(&what), bs(&arg), &mut out) } != BbStatus::BB_OK {
            return Err(last_error());
        }
        Ok(serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap())
    }

    fn run(&self, verb: &str, arg: &str) -> Result<Value, String> {
        let (verb, arg) = (w(verb), w(arg));
        let mut out = none();
        if unsafe { bb_ops_do(self.app, bs(&verb), bs(&arg), &mut out) } != BbStatus::BB_OK {
            return Err(last_error());
        }
        Ok(serde_json::from_str(&unsafe { take(out) }.unwrap()).unwrap())
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        unsafe { bb_app_destroy(self.app) };
        let _ = std::fs::remove_dir_all(&self.tree);
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

#[test]
fn the_queue_model_crosses_as_json_and_unknown_names_are_refused_by_name() {
    let rig = Rig::new();
    assert_eq!(
        Value::Bool(false),
        rig.query("queue", "").unwrap()["active"]
    );
    rig.run("pose-transfer", "queued|C:\\Archive").unwrap();
    let q = rig.query("queue", "").unwrap();
    assert_eq!(4, q["jobs"].as_array().unwrap().len());
    assert_eq!(Value::Bool(true), q["canPause"]);
    rig.run("pose-transfer", "off").unwrap();

    let err = rig.run("no-such-verb", "").unwrap_err();
    assert!(err.contains("no-such-verb"), "{err}");
    let err = rig.query("no-such-model", "").unwrap_err();
    assert!(err.contains("no-such-model"), "{err}");
    let err = rig.query("rename-preview", "not json").unwrap_err();
    assert!(err.contains("JSON"), "{err}");
}

#[test]
fn the_archive_fixtures_are_written_for_the_harness() {
    let dir = unique_temp_path("bb-ffi-archives", "");
    let (kind, path) = (w("archives"), w(&dir.to_string_lossy()));
    assert_eq!(BbStatus::BB_OK, unsafe {
        bb_fixture_write(bs(&kind), bs(&path))
    });
    for name in ["locked.zip", "sealed.7z", "plain.7z"] {
        assert!(dir.join(name).metadata().unwrap().len() > 50, "{name}");
    }
    let _ = std::fs::remove_dir_all(dir);
}
