//! One launch of the real executable, as the app itself timed it: the marks on the way up and the
//! memory once the first listing is shown. The app writes; the benchmark tool reads.
//! Spec: Core/Benchmarking/StartupTraceData.cs.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::BenchError;
use crate::FileTime;

/// The marks the app writes on its way up, as milliseconds since the process started.
pub struct StartupMarks;

impl StartupMarks {
    pub const MAIN: &'static str = "main";
    pub const VELOPACK_DONE: &'static str = "velopackDone";
    pub const INSTANCE_CLAIMED: &'static str = "instanceClaimed";
    /// The application resources loaded, on the UI thread.
    pub const APP_XAML_LOADED: &'static str = "appXamlLoaded";
    /// settings.json parsed — the composition root waits on it before anything else.
    pub const SETTINGS_LOADED: &'static str = "settingsLoaded";
    pub const SERVICES_BUILT: &'static str = "servicesBuilt";
    pub const MIGRATED: &'static str = "migrated";
    pub const THEME_READY: &'static str = "themeReady";
    /// The shell view model and the graph behind it resolved.
    pub const SHELL_BUILT: &'static str = "shellBuilt";
    /// The main window's own layout loaded, before any pane exists.
    pub const WINDOW_XAML_LOADED: &'static str = "windowXamlLoaded";
    /// The first pane and its tab view built.
    pub const PANES_BUILT: &'static str = "panesBuilt";
    pub const COMMANDS_BUILT: &'static str = "commandsBuilt";
    /// The window constructed; what is left before `WINDOW_SHOWN` is showing it.
    pub const WINDOW_BUILT: &'static str = "windowBuilt";
    pub const WINDOW_SHOWN: &'static str = "windowShown";
    pub const CONTENT_RENDERED: &'static str = "contentRendered";
    pub const FIRST_LISTING: &'static str = "firstListing";
}

/// The two moments a traced launch reads its own memory.
pub struct StartupMemoryPoints;

impl StartupMemoryPoints {
    /// As it stood when the first listing was shown — what Task Manager would say.
    pub const FIRST_LISTING: &'static str = "firstListing";
    /// The same moment after a full collection — what the app is actually holding, and the
    /// steadier of the two.
    pub const COLLECTED: &'static str = "collected";
}

/// One launch. `marks_ms`: mark name to milliseconds since process start (`StartupMarks`).
/// `instance`: `first` or `second`, whether the single-instance claim won, which says whether
/// another copy was running during the measurement. `indexer`: what the app did about the index
/// helper — `skipped` under a trace. `side_effects_skipped`: the post-window work (helper attach,
/// change-log policy, update check, staging sweep) was left out, so the numbers describe "to first
/// listing". `error`: a mark never came and the fallback flushed what there was. `memory`: by
/// `StartupMemoryPoints` name; `None` in a trace written before it was recorded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupTraceData {
    pub pid: u32,
    pub process_start_utc: FileTime,
    pub marks_ms: BTreeMap<String, f64>,
    pub instance: String,
    pub indexer: String,
    pub side_effects_skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<BTreeMap<String, StartupMemory>>,
}

/// One reading of the real executable's memory, taken by the app itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupMemory {
    pub working_set_bytes: i64,
    pub private_bytes: i64,
    /// Bytes the global allocator holds.
    pub heap_bytes: i64,
    pub peak_working_set_bytes: i64,
}

pub struct StartupTraceJson;

impl StartupTraceJson {
    pub fn serialize(trace: &StartupTraceData) -> String {
        serde_json::to_string_pretty(trace).expect("a trace always serializes")
    }

    pub fn deserialize(json: &str) -> Result<StartupTraceData, BenchError> {
        let trace: Option<StartupTraceData> =
            serde_json::from_str(json).map_err(|e| BenchError::NotStartupTrace(e.to_string()))?;
        trace.ok_or_else(|| BenchError::NotStartupTrace("empty document.".to_string()))
    }

    pub fn write(trace: &StartupTraceData, path: &Path) -> Result<(), BenchError> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, Self::serialize(trace))?;
        Ok(())
    }

    pub fn read(path: &Path) -> Result<StartupTraceData, BenchError> {
        Self::deserialize(&std::fs::read_to_string(path)?)
    }
}
