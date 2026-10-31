// Port of tests/BertBrowser.Core.Tests/Benchmarking/BenchResultsJsonTests.cs.
use std::collections::BTreeMap;

use bb_core::benchmarking::{BenchEntry, BenchError, BenchResultsJson, ProcessStats, TimeStats};

use crate::benchmarking::bench_compare_tests::{entry, set};

#[test]
fn round_trips() {
    let entry_b = BenchEntry {
        id: "ui.listing-10k.list".to_string(),
        tier: "B".to_string(),
        parameters: BTreeMap::from([
            ("files".to_string(), "10000".to_string()),
            ("dirs".to_string(), "50".to_string()),
        ]),
        n: 5,
        time: Some(TimeStats {
            unit: "ms".to_string(),
            mean: 812.4,
            median: 790.0,
            p95: 905.0,
            std_dev: 48.2,
            min: 770.0,
            max: 905.0,
        }),
        memory: None,
        process: Some(ProcessStats {
            working_set_bytes: 412_000_000,
            private_bytes: 380_000_000,
            heap_bytes: 96_000_000,
            realized_rows: 38,
            items: 10_000,
            retained_thumbnails: 0,
            peak_working_set_bytes: None,
        }),
        error: None,
    };
    let set = set(
        vec![entry_b.clone(), entry("core.x.y", 123.4, Some(2048))],
        true,
    );

    let json = BenchResultsJson::serialize(&set);
    let back = BenchResultsJson::deserialize(&json).expect("deserialize");

    assert_eq!(set.schema_version, back.schema_version);
    assert_eq!(set.generated_utc, back.generated_utc);
    assert_eq!(set.tool, back.tool);
    assert_eq!(set.git, back.git);
    assert_eq!(set.machine, back.machine);
    assert_eq!(set.run, back.run);
    assert_eq!(set.run.tiers, back.run.tiers);
    assert_eq!(2, back.benchmarks.len());
    assert_eq!(
        entry_b.parameters["files"],
        back.benchmarks[0].parameters["files"]
    );
    assert_eq!(entry_b.time, back.benchmarks[0].time);
    assert_eq!(entry_b.process, back.benchmarks[0].process);
    assert!(back.benchmarks[0].memory.is_none());
    assert_eq!(set.benchmarks[1].memory, back.benchmarks[1].memory);
    assert!(back.run.real);
}

#[test]
fn process_stats_peak_is_optional_so_an_older_baseline_still_reads() {
    let with = BenchEntry {
        id: "ui.memory.idle".to_string(),
        tier: "B".to_string(),
        parameters: BTreeMap::new(),
        n: 5,
        time: None,
        memory: None,
        process: Some(ProcessStats {
            working_set_bytes: 150_000_000,
            private_bytes: 70_000_000,
            heap_bytes: 9_400_000,
            realized_rows: 2,
            items: 2,
            retained_thumbnails: 0,
            peak_working_set_bytes: Some(175_000_000),
        }),
        error: None,
    };
    let mut without = with.clone();
    without.id = "ui.old".to_string();
    without
        .process
        .as_mut()
        .expect("process")
        .peak_working_set_bytes = None;

    let json = BenchResultsJson::serialize(&set(vec![with, without], false));
    let back = BenchResultsJson::deserialize(&json).expect("deserialize");

    assert_eq!(1, json.matches("\"peakWorkingSetBytes\"").count());
    assert_eq!(
        Some(175_000_000),
        back.benchmarks[0]
            .process
            .as_ref()
            .expect("process")
            .peak_working_set_bytes
    );
    assert_eq!(
        None,
        back.benchmarks[1]
            .process
            .as_ref()
            .expect("process")
            .peak_working_set_bytes
    );
}

#[test]
fn serialize_is_camel_case_and_omits_nulls() {
    let json = BenchResultsJson::serialize(&set(vec![entry("a", 1.0, None)], false));
    assert!(json.contains("\"schemaVersion\": 2"), "{json}");
    assert!(json.contains("\"benchmarks\""), "{json}");
    assert!(!json.contains("\"memory\""), "{json}");
    assert!(!json.contains("\"error\""), "{json}");
    assert!(!json.contains("SchemaVersion"), "{json}");
}

#[test]
fn deserialize_wrong_schema_version_throws() {
    let json = BenchResultsJson::serialize(&set(vec![], false))
        .replace("\"schemaVersion\": 2", "\"schemaVersion\": 3");
    let e = BenchResultsJson::deserialize(&json).expect_err("refused");
    assert!(matches!(e, BenchError::SchemaVersion(3)));
    assert!(e.to_string().contains("schema version 3"), "{e}");
}

#[test]
fn deserialize_garbage_throws() {
    assert!(matches!(
        BenchResultsJson::deserialize("not json"),
        Err(BenchError::NotResultsFile(_))
    ));
    assert!(matches!(
        BenchResultsJson::deserialize("null"),
        Err(BenchError::NotResultsFile(_))
    ));
}

#[test]
fn write_creates_directory() {
    let dir = bb_test_support::unique_temp_path("bertbrowser-bench-test", "");
    let path = dir.join("nested").join("results.json");
    let result = (|| {
        BenchResultsJson::write(&set(vec![], false), &path)?;
        assert!(path.exists());
        BenchResultsJson::read(&path)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(result.expect("read back").benchmarks.is_empty());
}
