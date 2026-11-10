//! A throwaway data directory, a throwaway tree, and an app built over the harness seams.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bb_app::{App, AppConfig, AppEvent, HarnessProbes, OpId, OpState, TabId};
use bb_test_support::unique_temp_path;

pub struct Fixture {
    pub app: App,
    pub probes: Arc<HarnessProbes>,
    pub events: Arc<Mutex<Vec<AppEvent>>>,
    pub root: PathBuf,
    pub data: PathBuf,
    pub staging: PathBuf,
    pub tab: TabId,
}

impl Fixture {
    /// The tree the smoke script browses: eight entries, three `report.md` at three depths.
    pub fn new() -> Fixture {
        Fixture::with(|_| {})
    }

    pub fn with(configure: impl FnOnce(&mut AppConfig)) -> Fixture {
        let root = unique_temp_path("bb-app-tree", "");
        let data = unique_temp_path("bb-app-data", "");
        let staging = unique_temp_path("bb-app-staging", "");
        make_tree(&root);
        std::fs::create_dir_all(&staging).unwrap();

        let mut config = AppConfig::harness(&data);
        config.watch_debounce_ms = 40;
        config.delete_staging_root = Some(staging.to_string_lossy().into_owned());
        configure(&mut config);
        let app = App::new(config).expect("app");
        let probes = app.probes().expect("harness probes");
        let events = Arc::new(Mutex::new(Vec::new()));
        {
            let events = events.clone();
            app.set_event_sink(Some(Arc::new(move |e| events.lock().unwrap().push(e))));
        }
        let tab = app.initialize(&root.to_string_lossy()).expect("tab");
        let fixture = Fixture {
            app,
            probes,
            events,
            root,
            data,
            staging,
            tab,
        };
        fixture.settle();
        fixture
    }

    pub fn path(&self, relative: &str) -> String {
        self.root.join(relative).to_string_lossy().into_owned()
    }

    pub fn settle(&self) {
        assert!(self.app.wait_idle(40, 15_000), "the app never went quiet");
    }

    pub fn names(&self) -> Vec<String> {
        self.app
            .listing(self.tab)
            .unwrap()
            .rows()
            .iter()
            .map(|r| r.name.clone())
            .collect()
    }

    pub fn index_of(&self, name: &str) -> u32 {
        self.names()
            .iter()
            .position(|n| n == name)
            .unwrap_or_else(|| panic!("{name} is not listed: {:?}", self.names())) as u32
    }

    pub fn select_names(&self, names: &[&str]) {
        let indices: Vec<u32> = names.iter().map(|n| self.index_of(n)).collect();
        self.app
            .select(self.tab, bb_app::SelectOp::Replace, &indices)
            .unwrap();
    }

    pub fn wait_op(&self, op: OpId) -> (bool, String) {
        let start = Instant::now();
        loop {
            if let Some(OpState::Done { ok, message }) = self.app.op_state(op) {
                self.settle();
                return (ok, message);
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "operation never finished"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn status(&self) -> String {
        self.app.tab_info(self.tab).unwrap().status
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.app.shutdown();
        let _ = std::fs::remove_dir_all(&self.root);
        let _ = std::fs::remove_dir_all(&self.data);
        let _ = std::fs::remove_dir_all(&self.staging);
    }
}

pub fn make_tree(root: &Path) {
    let write = |relative: &str, bytes: usize| {
        let full = root.join(relative);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, vec![b'x'; bytes]).unwrap();
    };
    write("Documents/report.md", 30);
    write("Documents/nested/report.md", 20);
    write("Pictures/holiday.png", 40);
    write("notes.txt", 100);
    write("report.md", 10);
    write("photo.jpg", 5000);
    write("archive.zip", 20000);
    write("budget.xlsx", 700);
}

/// Polls `condition` until it holds or the timeout passes.
pub fn eventually(timeout_ms: u64, mut condition: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(timeout_ms) {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    condition()
}
