//! Judges one results file against a baseline. A row's status says what was observed whichever
//! gates are on — a slower median is `Slower` in CI too, where it is reported and not gated; only
//! the report's flags honour the gates. Spec: Core/Benchmarking/BenchCompare.cs.

use std::collections::{HashMap, HashSet};

use super::{AllocStats, BenchError, BenchResultSet, ProcessStats, TimeStats};

/// How strict a comparison is.
#[derive(Clone, Debug, PartialEq)]
pub struct CompareOptions {
    /// A median this much slower than the baseline is a regression.
    pub time_threshold_pct: f64,
    /// For nanosecond results, a slowdown smaller than this in absolute terms is never a
    /// regression, whatever the percentage — a 2 ns benchmark that reads 3 ns is noise.
    pub time_noise_floor_ns: f64,
    /// The same floor for millisecond results.
    pub time_noise_floor_ms: f64,
    /// Replaces every entry's own `AllocStats::tolerance_pct` when set.
    pub alloc_tolerance_pct_override: Option<f64>,
    /// Whether a slower median counts towards `CompareReport::time_regressed`. Off in CI.
    pub gate_time: bool,
    /// Whether more allocation counts towards `CompareReport::alloc_regressed`.
    pub gate_alloc: bool,
    /// Whether a benchmark present in the baseline but missing now is a failure.
    pub strict: bool,
    /// A process snapshot whose Rust allocator or process heap grew by more than this is a regression.
    /// One each because they see different code: Rust in the first, C++ and the system DLLs in the second.
    pub memory_threshold_pct: f64,
    /// Growth in either smaller than this in absolute terms is never a regression.
    pub memory_noise_floor_bytes: i64,
    /// Whether a bigger process counts towards `CompareReport::memory_regressed`. Off in CI.
    pub gate_memory: bool,
    /// The same threshold for private bytes, and much the wider: over identical runs they moved by
    /// ten megabytes in a hundred (the rasteriser, memory not yet handed back).
    pub private_bytes_threshold_pct: f64,
    /// The floor that goes with it.
    pub private_bytes_noise_floor_bytes: i64,
}

impl Default for CompareOptions {
    fn default() -> Self {
        CompareOptions {
            time_threshold_pct: 15.0,
            time_noise_floor_ns: 200.0,
            time_noise_floor_ms: 5.0,
            alloc_tolerance_pct_override: None,
            gate_time: true,
            gate_alloc: true,
            strict: false,
            memory_threshold_pct: 10.0,
            memory_noise_floor_bytes: 1024 * 1024,
            gate_memory: true,
            private_bytes_threshold_pct: 25.0,
            private_bytes_noise_floor_bytes: 16 * 1024 * 1024,
        }
    }
}

/// What a comparison concluded about one benchmark. Ordered worst first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompareStatus {
    AllocUp,
    MemoryUp,
    Slower,
    Errored,
    Removed,
    New,
    AllocDown,
    MemoryDown,
    Faster,
    Same,
}

/// One benchmark, baseline against current.
#[derive(Clone, Debug, PartialEq)]
pub struct CompareRow {
    pub id: String,
    pub tier: String,
    pub status: CompareStatus,
    pub baseline_median: Option<f64>,
    pub current_median: Option<f64>,
    pub unit: Option<String>,
    pub time_delta_pct: Option<f64>,
    pub baseline_alloc: Option<i64>,
    pub current_alloc: Option<i64>,
    pub alloc_delta_pct: Option<f64>,
    pub note: Option<String>,
    pub baseline_process: Option<ProcessStats>,
    pub current_process: Option<ProcessStats>,
    pub memory_delta_pct: Option<f64>,
}

/// The whole verdict. The flags are what the gates read; the rows are what a person reads.
#[derive(Clone, Debug, PartialEq)]
pub struct CompareReport {
    pub rows: Vec<CompareRow>,
    pub time_regressed: bool,
    pub alloc_regressed: bool,
    pub has_errors: bool,
    pub has_removed: bool,
    pub memory_regressed: bool,
}

pub struct BenchCompare;

impl BenchCompare {
    pub fn compare(
        baseline: &BenchResultSet,
        current: &BenchResultSet,
        options: &CompareOptions,
    ) -> Result<CompareReport, BenchError> {
        if baseline.run.real != current.run.real {
            return Err(BenchError::MixedCorpora);
        }

        let before: HashMap<&str, usize> = baseline
            .benchmarks
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id.as_str(), i))
            .collect();
        let mut seen: HashSet<&str> = HashSet::new();
        let mut rows = Vec::with_capacity(current.benchmarks.len());
        let mut time_regressed = false;
        let mut alloc_regressed = false;
        let mut memory_regressed = false;
        let mut has_errors = false;

        for now in &current.benchmarks {
            let was = before
                .get(now.id.as_str())
                .map(|&i| &baseline.benchmarks[i]);
            seen.insert(now.id.as_str());

            if let Some(error) = &now.error {
                has_errors = true;
                rows.push(CompareRow {
                    id: now.id.clone(),
                    tier: now.tier.clone(),
                    status: CompareStatus::Errored,
                    baseline_median: was.and_then(|w| w.time.as_ref()).map(|t| t.median),
                    current_median: None,
                    unit: was.and_then(|w| w.time.as_ref()).map(|t| t.unit.clone()),
                    time_delta_pct: None,
                    baseline_alloc: was
                        .and_then(|w| w.memory.as_ref())
                        .map(|m| m.allocated_bytes),
                    current_alloc: None,
                    alloc_delta_pct: None,
                    note: Some(error.clone()),
                    baseline_process: was.and_then(|w| w.process.clone()),
                    current_process: None,
                    memory_delta_pct: None,
                });
                continue;
            }

            let Some(was) = was else {
                rows.push(CompareRow {
                    id: now.id.clone(),
                    tier: now.tier.clone(),
                    status: CompareStatus::New,
                    baseline_median: None,
                    current_median: now.time.as_ref().map(|t| t.median),
                    unit: now.time.as_ref().map(|t| t.unit.clone()),
                    time_delta_pct: None,
                    baseline_alloc: None,
                    current_alloc: now.memory.as_ref().map(|m| m.allocated_bytes),
                    alloc_delta_pct: None,
                    note: None,
                    baseline_process: None,
                    current_process: now.process.clone(),
                    memory_delta_pct: None,
                });
                continue;
            };

            let (slower, faster, time_delta) =
                judge_time(was.time.as_ref(), now.time.as_ref(), options);
            let (alloc_up, alloc_down, alloc_delta) =
                judge_alloc(was.memory.as_ref(), now.memory.as_ref(), options);
            let (memory_up, memory_down, memory_delta) =
                judge_process(was.process.as_ref(), now.process.as_ref(), options);

            time_regressed |= slower && options.gate_time;
            alloc_regressed |= alloc_up && options.gate_alloc;
            memory_regressed |= memory_up && options.gate_memory;

            let status = if alloc_up {
                CompareStatus::AllocUp
            } else if memory_up {
                CompareStatus::MemoryUp
            } else if slower {
                CompareStatus::Slower
            } else if alloc_down {
                CompareStatus::AllocDown
            } else if memory_down {
                CompareStatus::MemoryDown
            } else if faster {
                CompareStatus::Faster
            } else {
                CompareStatus::Same
            };

            rows.push(CompareRow {
                id: now.id.clone(),
                tier: now.tier.clone(),
                status,
                baseline_median: was.time.as_ref().map(|t| t.median),
                current_median: now.time.as_ref().map(|t| t.median),
                unit: now
                    .time
                    .as_ref()
                    .or(was.time.as_ref())
                    .map(|t| t.unit.clone()),
                time_delta_pct: time_delta,
                baseline_alloc: was.memory.as_ref().map(|m| m.allocated_bytes),
                current_alloc: now.memory.as_ref().map(|m| m.allocated_bytes),
                alloc_delta_pct: alloc_delta,
                note: None,
                baseline_process: was.process.clone(),
                current_process: now.process.clone(),
                memory_delta_pct: memory_delta,
            });
        }

        let mut has_removed = false;
        for gone in baseline
            .benchmarks
            .iter()
            .filter(|b| !seen.contains(b.id.as_str()))
        {
            has_removed = true;
            rows.push(CompareRow {
                id: gone.id.clone(),
                tier: gone.tier.clone(),
                status: CompareStatus::Removed,
                baseline_median: gone.time.as_ref().map(|t| t.median),
                current_median: None,
                unit: gone.time.as_ref().map(|t| t.unit.clone()),
                time_delta_pct: None,
                baseline_alloc: gone.memory.as_ref().map(|m| m.allocated_bytes),
                current_alloc: None,
                alloc_delta_pct: None,
                note: Some("in the baseline, not in this run".to_string()),
                baseline_process: gone.process.clone(),
                current_process: None,
                memory_delta_pct: None,
            });
        }

        Ok(CompareReport {
            rows,
            time_regressed,
            alloc_regressed,
            has_errors,
            has_removed,
            memory_regressed,
        })
    }
}

/// (slower, faster, delta %).
fn judge_time(
    was: Option<&TimeStats>,
    now: Option<&TimeStats>,
    o: &CompareOptions,
) -> (bool, bool, Option<f64>) {
    let (Some(was), Some(now)) = (was, now) else {
        return (false, false, None);
    };
    if was.median <= 0.0 {
        return (false, false, None);
    }

    let delta = now.median - was.median;
    let delta_pct = delta / was.median * 100.0;
    let floor = match now.unit.as_str() {
        "ns" => o.time_noise_floor_ns,
        "ms" => o.time_noise_floor_ms,
        _ => 0.0,
    };

    let slower = delta_pct > o.time_threshold_pct && delta > floor;
    let faster = -delta_pct > o.time_threshold_pct && -delta > floor;
    (slower, faster, Some(delta_pct))
}

/// (up, down, delta %).
fn judge_alloc(
    was: Option<&AllocStats>,
    now: Option<&AllocStats>,
    o: &CompareOptions,
) -> (bool, bool, Option<f64>) {
    let (Some(was), Some(now)) = (was, now) else {
        return (false, false, None);
    };

    let tolerance = o.alloc_tolerance_pct_override.unwrap_or(now.tolerance_pct);
    let delta = now.allocated_bytes - was.allocated_bytes;
    let delta_pct = if was.allocated_bytes == 0 {
        (delta == 0).then_some(0.0)
    } else {
        Some(delta as f64 / was.allocated_bytes as f64 * 100.0)
    };

    // A baseline of zero bytes is exact: any allocation at all is a regression, whatever the
    // tolerance says, because there is no number for a percentage to be of.
    let up = if was.allocated_bytes == 0 {
        delta > 0
    } else {
        now.allocated_bytes as f64 > was.allocated_bytes as f64 * (1.0 + tolerance / 100.0)
    };
    let down = was.allocated_bytes > 0
        && (now.allocated_bytes as f64) < was.allocated_bytes as f64 * (1.0 - tolerance / 100.0);

    (up, down, delta_pct)
}

/// A process snapshot against its baseline. The Rust allocator, the process heap and private bytes are
/// each judged against their threshold and floor, and any one over is "up"; the reported delta is private
/// bytes'. The working set is never judged — Windows trims it when it likes, so it says as much
/// about the machine's afternoon as about the app.
fn judge_process(
    was: Option<&ProcessStats>,
    now: Option<&ProcessStats>,
    o: &CompareOptions,
) -> (bool, bool, Option<f64>) {
    let (Some(was), Some(now)) = (was, now) else {
        return (false, false, None);
    };

    let private_bytes = moved(
        was.private_bytes,
        now.private_bytes,
        o.private_bytes_threshold_pct,
        o.private_bytes_noise_floor_bytes,
    );
    let allocator = moved(
        was.allocator_bytes,
        now.allocator_bytes,
        o.memory_threshold_pct,
        o.memory_noise_floor_bytes,
    );
    let heap = moved(
        was.process_heap_bytes,
        now.process_heap_bytes,
        o.memory_threshold_pct,
        o.memory_noise_floor_bytes,
    );
    let delta_pct = (was.private_bytes > 0)
        .then(|| (now.private_bytes - was.private_bytes) as f64 / was.private_bytes as f64 * 100.0);

    let up = private_bytes > 0 || allocator > 0 || heap > 0;
    let down = private_bytes < 0 || allocator < 0 || heap < 0;
    (up, !up && down, delta_pct)
}

/// +1 when `now` is over the threshold and the floor, -1 when it is under both, else 0.
fn moved(was: i64, now: i64, threshold_pct: f64, floor_bytes: i64) -> i32 {
    let delta = now - was;
    if delta.abs() <= floor_bytes {
        return 0;
    }
    if was <= 0 {
        return delta.signum() as i32;
    }
    if delta.abs() as f64 > was as f64 * threshold_pct / 100.0 {
        delta.signum() as i32
    } else {
        0
    }
}
