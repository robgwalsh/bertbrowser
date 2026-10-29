// Port of tests/BertBrowser.Core.Tests/Benchmarking/BenchCompareTests.cs.
use std::collections::BTreeMap;

use bb_core::FileTime;
use bb_core::benchmarking::{
    AllocStats, BenchCompare, BenchEntry, BenchError, BenchResultSet, CURRENT_SCHEMA_VERSION,
    CompareOptions, CompareReport, CompareStatus, GitInfo, MachineInfo, ProcessStats, RunInfo,
    TimeStats,
};

fn compare(baseline: Vec<BenchEntry>, current: Vec<BenchEntry>) -> CompareReport {
    compare_with(baseline, current, &CompareOptions::default())
}

fn compare_with(
    baseline: Vec<BenchEntry>,
    current: Vec<BenchEntry>,
    options: &CompareOptions,
) -> CompareReport {
    BenchCompare::compare(&set(baseline, false), &set(current, false), options).expect("compare")
}

// ---- helpers shared with the other benchmarking test files, as BenchCompareTests' statics were ----

pub fn set(entries: Vec<BenchEntry>, real: bool) -> BenchResultSet {
    set_on(entries, real, "test-machine")
}

pub fn set_on(entries: Vec<BenchEntry>, real: bool, machine_key: &str) -> BenchResultSet {
    BenchResultSet {
        schema_version: CURRENT_SCHEMA_VERSION,
        generated_utc: FileTime::from_ymd_hms(2026, 10, 2, 12, 0, 0),
        tool: "test".to_string(),
        git: GitInfo {
            sha: "0123456789abcdef".to_string(),
            branch: "main".to_string(),
            dirty: false,
        },
        machine: MachineInfo {
            key: machine_key.to_string(),
            cpu: "Test CPU".to_string(),
            logical_cores: 8,
            ram_gb: 16,
            os_build: 26200,
            runtime: "10.0.2".to_string(),
            power_plan: Some("Balanced".to_string()),
            app_running: false,
            indexer_running: false,
        },
        run: RunInfo {
            job: "default".to_string(),
            scale: 1,
            real,
            tiers: vec!["A".to_string()],
            render_mode: None,
            repeat: None,
            toolchain: None,
        },
        benchmarks: entries,
    }
}

/// A Tier A nanosecond entry with no allocation tolerance.
pub fn entry(id: &str, median: f64, alloc: Option<i64>) -> BenchEntry {
    entry_full(id, median, alloc, "ns", "A", 0.0)
}

pub fn entry_in(id: &str, median: f64, alloc: Option<i64>, unit: &str, tier: &str) -> BenchEntry {
    entry_full(id, median, alloc, unit, tier, 0.0)
}

pub fn entry_tol(id: &str, median: f64, alloc: Option<i64>, tolerance: f64) -> BenchEntry {
    entry_full(id, median, alloc, "ns", "A", tolerance)
}

pub fn entry_full(
    id: &str,
    median: f64,
    alloc: Option<i64>,
    unit: &str,
    tier: &str,
    tolerance: f64,
) -> BenchEntry {
    BenchEntry {
        id: id.to_string(),
        tier: tier.to_string(),
        parameters: BTreeMap::new(),
        n: 15,
        time: Some(TimeStats {
            unit: unit.to_string(),
            mean: median,
            median,
            p95: median * 1.05,
            std_dev: median * 0.02,
            min: median * 0.98,
            max: median * 1.1,
        }),
        memory: alloc.map(|a| AllocStats {
            allocated_bytes: a,
            gen0_per_k_ops: 1.0,
            gen1_per_k_ops: 0.0,
            gen2_per_k_ops: 0.0,
            tolerance_pct: tolerance,
        }),
        process: None,
        error: None,
    }
}

/// A process snapshot: the working set is twice the private bytes and the peak 10 MB over that.
pub fn snapshot(id: &str, private_mb: f64, managed_mb: f64) -> BenchEntry {
    snapshot_full(id, private_mb, managed_mb, "A", None)
}

pub fn snapshot_in(id: &str, private_mb: f64, managed_mb: f64, tier: &str) -> BenchEntry {
    snapshot_full(id, private_mb, managed_mb, tier, None)
}

pub fn snapshot_full(
    id: &str,
    private_mb: f64,
    managed_mb: f64,
    tier: &str,
    working_set_mb: Option<f64>,
) -> BenchEntry {
    const MB: f64 = 1024.0 * 1024.0;
    let working_set = working_set_mb.unwrap_or(private_mb * 2.0);
    BenchEntry {
        id: id.to_string(),
        tier: tier.to_string(),
        parameters: BTreeMap::new(),
        n: 5,
        time: None,
        memory: None,
        process: Some(ProcessStats {
            working_set_bytes: (working_set * MB) as i64,
            private_bytes: (private_mb * MB) as i64,
            heap_bytes: (managed_mb * MB) as i64,
            realized_rows: 5,
            items: 10,
            retained_thumbnails: 0,
            peak_working_set_bytes: Some(((working_set + 10.0) * MB) as i64),
        }),
        error: None,
    }
}

pub fn failed(id: &str) -> BenchEntry {
    BenchEntry {
        id: id.to_string(),
        tier: "A".to_string(),
        parameters: BTreeMap::new(),
        n: 0,
        time: None,
        memory: None,
        process: None,
        error: Some("threw NullReferenceException".to_string()),
    }
}

// ---- tests ----

#[test]
fn same_numbers_same() {
    let report = compare(
        vec![entry("a", 1000.0, Some(512))],
        vec![entry("a", 1000.0, Some(512))],
    );
    assert_eq!(1, report.rows.len());
    assert_eq!(CompareStatus::Same, report.rows[0].status);
    assert!(!report.time_regressed);
    assert!(!report.alloc_regressed);
}

#[test]
fn slower_when_median_exceeds_threshold() {
    for (before, after, slower) in [
        (1_000_000.0, 1_150_000.0, false), // exactly at the threshold is not over it
        (1_000_000.0, 1_150_001.0, true),
        (1_000_000.0, 1_300_000.0, true),
    ] {
        let report = compare(
            vec![entry("a", before, Some(0))],
            vec![entry("a", after, Some(0))],
        );
        let expected = if slower {
            CompareStatus::Slower
        } else {
            CompareStatus::Same
        };
        assert_eq!(expected, report.rows[0].status, "{before} -> {after}");
        assert_eq!(slower, report.time_regressed, "{before} -> {after}");
    }
}

#[test]
fn slower_suppressed_below_noise_floor() {
    // 2 ns to 3 ns is +50% and 1 ns — well under the 200 ns floor.
    let report = compare(
        vec![entry("a", 2.0, Some(0))],
        vec![entry("a", 3.0, Some(0))],
    );
    assert_eq!(CompareStatus::Same, report.rows[0].status);
    assert!(!report.time_regressed);
}

#[test]
fn noise_floor_is_per_unit() {
    // 10 ms to 14 ms is +40% and 4 ms — under the 5 ms floor for milliseconds.
    let report = compare(
        vec![entry_in("ui", 10.0, None, "ms", "B")],
        vec![entry_in("ui", 14.0, None, "ms", "B")],
    );
    assert_eq!(CompareStatus::Same, report.rows[0].status);

    // 100 ms to 140 ms clears both.
    let report = compare(
        vec![entry_in("ui", 100.0, None, "ms", "B")],
        vec![entry_in("ui", 140.0, None, "ms", "B")],
    );
    assert_eq!(CompareStatus::Slower, report.rows[0].status);
}

#[test]
fn faster_is_symmetric() {
    let report = compare(
        vec![entry("a", 1_000_000.0, Some(0))],
        vec![entry("a", 700_000.0, Some(0))],
    );
    assert_eq!(CompareStatus::Faster, report.rows[0].status);
    assert!(!report.time_regressed);
}

#[test]
fn time_gate_off_still_reports_slower() {
    let report = compare_with(
        vec![entry("a", 1_000_000.0, Some(0))],
        vec![entry("a", 2_000_000.0, Some(0))],
        &CompareOptions {
            gate_time: false,
            ..CompareOptions::default()
        },
    );
    assert_eq!(CompareStatus::Slower, report.rows[0].status);
    assert!(!report.time_regressed);
}

#[test]
fn alloc_up_honours_entry_tolerance() {
    for (before, after, tolerance, up) in [
        (1000, 1000, 0.0, false),
        (1000, 1001, 0.0, true),  // exact: one byte more is a regression
        (1000, 1020, 2.0, false), // within the entry's 2% tolerance
        (1000, 1021, 2.0, true),
    ] {
        let report = compare(
            vec![entry("a", 1000.0, Some(before))],
            vec![entry_tol("a", 1000.0, Some(after), tolerance)],
        );
        let expected = if up {
            CompareStatus::AllocUp
        } else {
            CompareStatus::Same
        };
        assert_eq!(
            expected, report.rows[0].status,
            "{before} -> {after} @ {tolerance}%"
        );
        assert_eq!(
            up, report.alloc_regressed,
            "{before} -> {after} @ {tolerance}%"
        );
    }
}

#[test]
fn alloc_tolerance_override_replaces_entry() {
    let baseline = entry("a", 1000.0, Some(1000));
    let current = entry_tol("a", 1000.0, Some(1050), 0.0);

    assert_eq!(
        CompareStatus::AllocUp,
        compare(vec![baseline.clone()], vec![current.clone()]).rows[0].status
    );
    assert_eq!(
        CompareStatus::Same,
        compare_with(
            vec![baseline],
            vec![current],
            &CompareOptions {
                alloc_tolerance_pct_override: Some(10.0),
                ..CompareOptions::default()
            }
        )
        .rows[0]
            .status
    );
}

#[test]
fn alloc_from_zero_any_allocation_is_up() {
    let report = compare(
        vec![entry("a", 1000.0, Some(0))],
        vec![entry_tol("a", 1000.0, Some(24), 50.0)],
    );
    assert_eq!(CompareStatus::AllocUp, report.rows[0].status);
}

#[test]
fn alloc_up_outranks_slower() {
    let report = compare(
        vec![entry("a", 1_000_000.0, Some(100))],
        vec![entry("a", 2_000_000.0, Some(200))],
    );
    assert_eq!(CompareStatus::AllocUp, report.rows[0].status);
    assert!(report.time_regressed);
    assert!(report.alloc_regressed);
}

#[test]
fn alloc_down_reported_but_not_a_regression() {
    let report = compare(
        vec![entry("a", 1000.0, Some(1000))],
        vec![entry("a", 1000.0, Some(500))],
    );
    assert_eq!(CompareStatus::AllocDown, report.rows[0].status);
    assert!(!report.alloc_regressed);
}

#[test]
fn managed_heap_judged_at_ten_percent_over_a_megabyte() {
    for (before, after, expected) in [
        (13.0, 13.9, CompareStatus::Same), // +0.9 MB: under the 1 MB floor, whatever the percentage
        (13.0, 14.2, CompareStatus::Same), // +1.2 MB clears the floor but is 9.2%
        (13.0, 14.5, CompareStatus::MemoryUp), // +1.5 MB and 11.5%
        (13.0, 120.0, CompareStatus::MemoryUp), // the fifteen-tab leak this gate was written after
        (20.0, 17.0, CompareStatus::MemoryDown),
    ] {
        let report = compare(
            vec![snapshot("m", 100.0, before)],
            vec![snapshot("m", 100.0, after)],
        );
        assert_eq!(expected, report.rows[0].status, "{before} -> {after}");
        assert_eq!(
            expected == CompareStatus::MemoryUp,
            report.memory_regressed,
            "{before} -> {after}"
        );
        assert!(!report.time_regressed, "{before} -> {after}");
        assert!(!report.alloc_regressed, "{before} -> {after}");
    }
}

#[test]
fn private_bytes_judged_looser_than_the_managed_heap() {
    for (before, after, expected) in [
        (100.0, 112.0, CompareStatus::Same), // the spread measured between identical runs
        (100.0, 124.0, CompareStatus::Same), // over the 16 MB floor, under 25%
        (100.0, 130.0, CompareStatus::MemoryUp),
        (40.0, 55.0, CompareStatus::Same), // +37%, but 15 MB is under the floor
        (100.0, 70.0, CompareStatus::MemoryDown),
    ] {
        let report = compare(
            vec![snapshot("m", before, 13.0)],
            vec![snapshot("m", after, 13.0)],
        );
        assert_eq!(expected, report.rows[0].status, "{before} -> {after}");
        assert_eq!(
            expected == CompareStatus::MemoryUp,
            report.memory_regressed,
            "{before} -> {after}"
        );
    }
}

#[test]
fn working_set_is_reported_and_never_judged() {
    let report = compare(
        vec![snapshot_full("m", 100.0, 13.0, "A", Some(150.0))],
        vec![snapshot_full("m", 100.0, 13.0, "A", Some(600.0))],
    );
    assert_eq!(CompareStatus::Same, report.rows[0].status);
    assert!(!report.memory_regressed);
    assert_eq!(
        600 * 1024 * 1024,
        report.rows[0]
            .current_process
            .as_ref()
            .expect("current")
            .working_set_bytes
    );
}

#[test]
fn memory_gate_off_still_reports_memory_up() {
    let report = compare_with(
        vec![snapshot("m", 100.0, 13.0)],
        vec![snapshot("m", 100.0, 30.0)],
        &CompareOptions {
            gate_memory: false,
            ..CompareOptions::default()
        },
    );
    assert_eq!(CompareStatus::MemoryUp, report.rows[0].status);
    assert!(!report.memory_regressed);
}

#[test]
fn memory_up_wins_over_a_shrinking_other_number() {
    // Private bytes fell and the managed heap rose: the rise is the news.
    let report = compare(
        vec![snapshot("m", 200.0, 13.0)],
        vec![snapshot("m", 100.0, 30.0)],
    );
    assert_eq!(CompareStatus::MemoryUp, report.rows[0].status);
}

#[test]
fn new_removed_errored() {
    let report = compare(
        vec![
            entry("kept", 1000.0, Some(0)),
            entry("gone", 1000.0, Some(0)),
        ],
        vec![
            entry("kept", 1000.0, Some(0)),
            entry("fresh", 1000.0, Some(0)),
            failed("broken"),
        ],
    );

    let status = |id: &str| {
        let matching: Vec<_> = report.rows.iter().filter(|r| r.id == id).collect();
        assert_eq!(1, matching.len(), "{id}");
        matching[0].status
    };
    assert_eq!(CompareStatus::New, status("fresh"));
    assert_eq!(CompareStatus::Removed, status("gone"));
    assert_eq!(CompareStatus::Errored, status("broken"));
    assert!(report.has_errors);
    assert!(report.has_removed);
    assert!(!report.time_regressed);
    assert!(!report.alloc_regressed);
}

#[test]
fn real_against_synthetic_refused() {
    let synthetic = set(vec![entry("a", 1.0, Some(0))], false);
    let real = set(vec![entry("a", 1.0, Some(0))], true);
    let options = CompareOptions::default();
    assert!(matches!(
        BenchCompare::compare(&synthetic, &real, &options),
        Err(BenchError::MixedCorpora)
    ));
    assert!(matches!(
        BenchCompare::compare(&real, &synthetic, &options),
        Err(BenchError::MixedCorpora)
    ));
}

#[test]
fn time_stats_from_median_and_nearest_rank_p95() {
    let stats = TimeStats::from_samples(&[5.0, 1.0, 4.0, 2.0, 3.0], "ms").expect("stats");
    assert_eq!(3.0, stats.median);
    assert_eq!(5.0, stats.p95);
    assert_eq!(1.0, stats.min);
    assert_eq!(5.0, stats.max);
    assert_eq!(3.0, stats.mean);

    let even = TimeStats::from_samples(&[1.0, 2.0, 3.0, 4.0], "ms").expect("stats");
    assert_eq!(2.5, even.median);

    let samples: Vec<f64> = (1..=100).map(f64::from).collect();
    let many = TimeStats::from_samples(&samples, "ns").expect("stats");
    assert_eq!(95.0, many.p95);
}

#[test]
fn time_stats_from_empty_throws() {
    assert!(matches!(
        TimeStats::from_samples(&[], "ms"),
        Err(BenchError::NoSamples)
    ));
}
