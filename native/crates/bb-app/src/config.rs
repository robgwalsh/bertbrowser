//! How the app is built: where its data lives and which seams it gets.

use std::path::PathBuf;

/// Which set of seams to build the app over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fakes {
    /// The real ones (for now: unattended defaults where the UI has not landed yet).
    None,
    /// The harness set: nothing is launched, elevation is declined, prompts are recorded.
    Harness,
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    /// The data directory (`bertbrowser.db`, `settings.json`). Created if missing.
    pub data_dir: PathBuf,
    pub fakes: Fakes,
    /// How long typing in a search box waits before it runs. Zero runs at once (the harness).
    pub search_debounce_ms: u64,
    /// How long a folder change waits for quiet before the listing is merged.
    pub watch_debounce_ms: u64,
    /// Where deletes that cannot use the Recycle Bin are held. `None` is the root of each volume.
    pub delete_staging_root: Option<String>,
    /// Worker threads for listings and searches.
    pub workers: usize,
    /// How long the Settings page waits for quiet before writing the file. Zero writes at once.
    pub settings_debounce_ms: u64,
    /// Put the real index client behind a launcher that declines the elevation prompt (the
    /// harness's `--index-declined`).
    pub index_declined: bool,
}

impl AppConfig {
    pub fn new(data_dir: impl Into<PathBuf>) -> AppConfig {
        AppConfig {
            data_dir: data_dir.into(),
            fakes: Fakes::None,
            search_debounce_ms: 200,
            watch_debounce_ms: 250,
            delete_staging_root: None,
            workers: 4,
            settings_debounce_ms: 400,
            index_declined: false,
        }
    }

    /// The configuration the harness and the tests build over.
    pub fn harness(data_dir: impl Into<PathBuf>) -> AppConfig {
        AppConfig {
            fakes: Fakes::Harness,
            search_debounce_ms: 0,
            ..AppConfig::new(data_dir)
        }
    }
}
