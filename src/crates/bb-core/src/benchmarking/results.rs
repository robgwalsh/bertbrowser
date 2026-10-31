//! One run's worth of benchmark results: the schema every tier writes and the compare tool reads.
//! A tier fills the fields it can answer and leaves the rest `None`; `TimeStats::unit` says which
//! clock a number is in. Spec: Core/Benchmarking/BenchResults.cs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::BenchError;
use crate::FileTime;

/// The schema this build writes and reads. Bumped when a field changes meaning: 3 splits 2's
/// `heapBytes`, which was only ever the process heap, into `allocatorBytes` and `processHeapBytes`.
pub const CURRENT_SCHEMA_VERSION: i32 = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchResultSet {
    pub schema_version: i32,
    pub generated_utc: FileTime,
    pub tool: String,
    pub git: GitInfo,
    pub machine: MachineInfo,
    pub run: RunInfo,
    pub benchmarks: Vec<BenchEntry>,
}

/// Which commit produced a result, and whether the tree had uncommitted changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    pub sha: String,
    pub branch: String,
    pub dirty: bool,
}

/// The machine a result came from. `key` is what baselines are filed under; the rest is the legend
/// a reader needs to interpret the numbers. `power_plan` is noise, not identity. `indexer_running`
/// matters because the helper tails the USN journal into the user's database the whole time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineInfo {
    pub key: String,
    pub cpu: String,
    pub logical_cores: u32,
    pub ram_gb: u32,
    pub os_build: u32,
    pub runtime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_plan: Option<String>,
    pub app_running: bool,
    pub indexer_running: bool,
}

/// How a run was configured. `scale`: the synthetic corpus multiplier, rows are `scale × 100,000`.
/// `real`: the run read the user's own index and folders, and is only ever compared against a real
/// baseline. `toolchain` is recorded because two toolchains do not report the same allocations for
/// code over SQLite or hashing — a comparison across them shows "regressions" that are only the
/// toolchain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub job: String,
    pub scale: u32,
    pub real: bool,
    pub tiers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toolchain: Option<String>,
}

/// One benchmark's result. `id` is stable across runs and is the key a comparison joins on
/// (`core.*`, `ui.*`, `startup.*`); `parameters` is whatever sized the work, so a number is never
/// read without its denominator. An entry carries `process` or `time`, not both. Any `error` fails
/// a CI comparison.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchEntry {
    pub id: String,
    pub tier: String,
    pub parameters: BTreeMap<String, String>,
    /// How many measurements the statistics summarise.
    pub n: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<TimeStats>,
    /// Tier A only: what one operation allocates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<AllocStats>,
    /// Tiers B and C: a memory snapshot of the whole process.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<ProcessStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Per-operation timing. `unit` is `ns` for Tier A and `ms` otherwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeStats {
    pub unit: String,
    pub mean: f64,
    pub median: f64,
    pub p95: f64,
    pub std_dev: f64,
    pub min: f64,
    pub max: f64,
}

impl TimeStats {
    /// Summarises raw samples. The median is the midpoint (mean of the two middle values for an even
    /// count) and p95 is nearest-rank, so with five samples it is simply the largest — honest about
    /// how little five samples can say.
    pub fn from_samples(samples: &[f64], unit: &str) -> Result<TimeStats, BenchError> {
        if samples.is_empty() {
            return Err(BenchError::NoSamples);
        }

        let mut sorted = samples.to_vec();
        sorted.sort_by(f64::total_cmp);
        let n = sorted.len();
        let mean = sorted.iter().sum::<f64>() / n as f64;
        let median = if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
        };
        let p95_rank = (0.95 * n as f64).ceil() as usize;
        let p95 = sorted[p95_rank.saturating_sub(1).min(n - 1)];
        let std_dev = if n < 2 {
            0.0
        } else {
            (sorted.iter().map(|s| (s - mean) * (s - mean)).sum::<f64>() / (n - 1) as f64).sqrt()
        };

        Ok(TimeStats {
            unit: unit.to_string(),
            mean,
            median,
            p95,
            std_dev,
            min: sorted[0],
            max: sorted[n - 1],
        })
    }
}

/// What one operation allocates. `tolerance_pct` travels with the result so the compare tool needs
/// only two files to decide — it is the benchmark author's statement of how much allocation jitter
/// the measured code has (SQLite, the thread pool), and is zero for pure code.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocStats {
    pub allocated_bytes: i64,
    pub gen0_per_k_ops: f64,
    pub gen1_per_k_ops: f64,
    pub gen2_per_k_ops: f64,
    pub tolerance_pct: f64,
}

/// A snapshot of a process's memory: the harness's at a point in a scenario (Tier B), or the real
/// executable's at its first listing (Tier C). Taken after mimalloc has handed back what it can unless
/// the entry's id says otherwise. `allocator_bytes`, `process_heap_bytes` and `private_bytes`
/// are what the compare gates; the working set is the number Task Manager shows and is reported, but Windows
/// trims and grows it for its own reasons. `realized_rows`: Tier B, rows the list has built, or -1
/// without a virtualizing panel; zero in Tier C. `peak_working_set_bytes`: the high-water mark a
/// snapshot taken after the work cannot otherwise see; `None` in a file written before it was
/// recorded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStats {
    pub working_set_bytes: i64,
    pub private_bytes: i64,
    /// Bytes Rust holds, counted at its allocator: requested and not yet freed, without mimalloc's free pages.
    pub allocator_bytes: i64,
    /// Bytes in use in the Windows process heap: the C++ side and the system DLLs, never Rust.
    pub process_heap_bytes: i64,
    pub realized_rows: i32,
    pub items: u32,
    pub retained_thumbnails: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_working_set_bytes: Option<i64>,
}
