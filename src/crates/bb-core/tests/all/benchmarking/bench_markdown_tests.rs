// Port of tests/BertBrowser.Core.Tests/Benchmarking/BenchMarkdownTests.cs.
use std::collections::BTreeMap;

use bb_core::benchmarking::{BenchCompare, BenchEntry, BenchMarkdown, CompareOptions};

use crate::benchmarking::bench_compare_tests::{entry, entry_in, set, snapshot, snapshot_in};

#[test]
fn render_compare_regressions_first_with_counts() {
    let baseline = set(
        vec![
            entry("z.same", 1000.0, Some(0)),
            entry("a.slower", 1_000_000.0, Some(0)),
            entry("m.alloc", 1000.0, Some(100)),
        ],
        false,
    );
    let current = set(
        vec![
            entry("z.same", 1000.0, Some(0)),
            entry("a.slower", 2_000_000.0, Some(0)),
            entry("m.alloc", 1000.0, Some(200)),
        ],
        false,
    );

    let options = CompareOptions::default();
    let report = BenchCompare::compare(&baseline, &current, &options).expect("compare");
    let md = BenchMarkdown::render_compare(&report, &options, "Test");

    let lines: Vec<&str> = md.split('\n').map(str::trim_end).collect();
    assert_eq!("## Test", lines[0]);
    assert!(
        md.contains("3 benchmarks: 1 allocates more, 1 slower, 1 same"),
        "{md}"
    );

    let rows: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| l.starts_with("| `"))
        .collect();
    assert!(rows[0].starts_with("| `m.alloc`"), "{}", rows[0]);
    assert!(rows[1].starts_with("| `a.slower`"), "{}", rows[1]);
    assert!(rows[2].starts_with("| `z.same`"), "{}", rows[2]);
    assert!(rows[0].contains("+100.0%"), "{}", rows[0]);
    assert!(
        rows[1].contains("| 1.00 ms | 2.00 ms | +100.0% |"),
        "{}",
        rows[1]
    );
}

#[test]
fn render_compare_says_which_gates_are_on() {
    let empty = set(vec![], false);
    let options = CompareOptions::default();
    let report = BenchCompare::compare(&empty, &empty, &options).expect("compare");

    assert!(BenchMarkdown::render_compare(&report, &options, "t").contains("Time gate on at 15%"));
    assert!(
        BenchMarkdown::render_compare(
            &report,
            &CompareOptions {
                gate_time: false,
                ..CompareOptions::default()
            },
            "t"
        )
        .contains("Time gate off (reported only)")
    );
    assert!(
        BenchMarkdown::render_compare(
            &report,
            &CompareOptions {
                alloc_tolerance_pct_override: Some(3.0),
                ..CompareOptions::default()
            },
            "t"
        )
        .contains("allocation gate on at 3% for every benchmark")
    );
}

#[test]
fn render_results_one_table_per_tier() {
    let set = set(
        vec![
            entry("core.a", 1234.0, Some(2048)),
            entry_in("ui.b", 812.0, None, "ms", "B"),
            entry_in("startup.c", 1240.0, None, "ms", "C"),
            BenchEntry {
                id: "core.broken".to_string(),
                tier: "A".to_string(),
                parameters: BTreeMap::new(),
                n: 0,
                time: None,
                memory: None,
                process: None,
                error: Some("boom".to_string()),
            },
        ],
        false,
    );

    let md = BenchMarkdown::render_results(&set);

    assert!(md.contains("### Tier A — Core micro-benchmarks"), "{md}");
    assert!(md.contains("### Tier B — UI scenarios"), "{md}");
    assert!(md.contains("### Tier C — Startup"), "{md}");
    assert!(
        md.contains("| `core.a` | 1.23 µs | 1.30 µs | 2.00 KB | 1 | 15 |"),
        "{md}"
    );
    assert!(md.contains("| `ui.b` | 812 ms |"), "{md}");
    assert!(md.contains("| `startup.c` | 1.24 s |"), "{md}");
    assert!(md.contains("| `core.broken` | error: boom |"), "{md}");
}

#[test]
fn render_results_snapshots_get_their_own_table_under_the_tiers_timings() {
    let set = set(
        vec![
            entry_in("ui.listing.list", 268.0, None, "ms", "B"),
            snapshot_in("ui.memory.idle", 70.0, 9.0, "B"),
            entry_in("startup.firstListing", 714.0, None, "ms", "C"),
            snapshot_in("startup.memory.collected", 60.0, 8.0, "C"),
        ],
        false,
    );

    let md = BenchMarkdown::render_results(&set);

    // Not a row of dashes among the timings…
    assert!(!md.contains("| `ui.memory.idle` | —"), "{md}");
    // …but a row of its own, with the row counts only the harness knows.
    assert!(
        md.contains("| Memory snapshot | Rust allocator | Process heap | Private bytes | Working set | Peak working set | Items | Realized | n |"),
        "{md}"
    );
    assert!(
        md.contains(
            "| `ui.memory.idle` | 9.00 MB | 2.00 MB | 70.0 MB | 140 MB | 150 MB | 10 | 5 | 5 |"
        ),
        "{md}"
    );
    assert!(
        md.contains("| Memory snapshot | Rust allocator | Process heap | Private bytes | Working set | Peak working set | n |"),
        "{md}"
    );
    assert!(
        md.contains(
            "| `startup.memory.collected` | 8.00 MB | 2.00 MB | 60.0 MB | 120 MB | 130 MB | 5 |"
        ),
        "{md}"
    );
}

#[test]
fn render_compare_snapshots_are_judged_in_their_own_table() {
    let baseline = set(
        vec![
            entry("core.a", 1000.0, Some(0)),
            snapshot("ui.memory.soak", 100.0, 13.0),
        ],
        false,
    );
    let current = set(
        vec![
            entry("core.a", 1000.0, Some(0)),
            snapshot("ui.memory.soak", 200.0, 120.0),
        ],
        false,
    );

    let options = CompareOptions::default();
    let report = BenchCompare::compare(&baseline, &current, &options).expect("compare");
    let md = BenchMarkdown::render_compare(&report, &options, "t");

    assert!(md.contains("1 holds more memory, 1 same"), "{md}");
    assert!(md.contains("Process-memory gate on"), "{md}");
    assert!(md.contains("### Process memory"), "{md}");
    assert!(
        md.contains(
            "| `ui.memory.soak` | A | 100 MB | 200 MB | +100.0% | 13.0 MB | 120 MB | +823.1% |"
        ),
        "{md}"
    );
    assert!(!md.contains("| `ui.memory.soak` | A | — |"), "{md}");

    let no_gate = CompareOptions {
        gate_memory: false,
        ..CompareOptions::default()
    };
    let report = BenchCompare::compare(&baseline, &current, &no_gate).expect("compare");
    assert!(
        BenchMarkdown::render_compare(&report, &no_gate, "t")
            .contains("Process-memory gate off (reported only)")
    );
}

#[test]
fn time_scales_to_read_well() {
    for (value, unit, expected) in [
        (999.0, "ns", "999 ns"),
        (1500.0, "ns", "1.50 µs"),
        (278_000_000.0, "ns", "278 ms"),
        (2.46e9, "ns", "2.46 s"),
        (812.4, "ms", "812 ms"),
        (0.5, "ms", "500 µs"),
    ] {
        assert_eq!(
            expected,
            BenchMarkdown::time(Some(value), Some(unit)),
            "{value} {unit}"
        );
    }
}

#[test]
fn bytes_scales_to_read_well() {
    for (value, expected) in [
        (512, "512 B"),
        (2048, "2.00 KB"),
        (1_843_200, "1.76 MB"),
        (3_221_225_472, "3.00 GB"),
    ] {
        assert_eq!(expected, BenchMarkdown::bytes(Some(value)), "{value}");
    }
}
