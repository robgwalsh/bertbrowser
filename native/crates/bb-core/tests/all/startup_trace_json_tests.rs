// Port of tests/BertBrowser.Core.Tests/Benchmarking/StartupTraceJsonTests.cs.
use std::collections::BTreeMap;

use bb_core::FileTime;
use bb_core::benchmarking::{
    BenchError, StartupMarks, StartupMemory, StartupMemoryPoints, StartupTraceData,
    StartupTraceJson,
};

fn trace(instance: &str, error: Option<&str>) -> StartupTraceData {
    StartupTraceData {
        pid: 1,
        process_start_utc: FileTime::from_ymd_hms(1970, 1, 1, 0, 0, 0),
        marks_ms: BTreeMap::new(),
        instance: instance.to_string(),
        indexer: "skipped".to_string(),
        side_effects_skipped: true,
        error: error.map(str::to_string),
        memory: None,
    }
}

#[test]
fn round_trips() {
    let trace = StartupTraceData {
        pid: 1234,
        process_start_utc: FileTime::from_ymd_hms(2026, 10, 2, 12, 0, 0),
        marks_ms: BTreeMap::from([
            (StartupMarks::MAIN.to_string(), 41.5),
            (StartupMarks::FIRST_LISTING.to_string(), 1210.25),
        ]),
        instance: "first".to_string(),
        indexer: "skipped".to_string(),
        side_effects_skipped: true,
        error: None,
        memory: None,
    };

    let back = StartupTraceJson::deserialize(&StartupTraceJson::serialize(&trace)).expect("back");

    assert_eq!(trace.pid, back.pid);
    assert_eq!(trace.process_start_utc, back.process_start_utc);
    assert_eq!(41.5, back.marks_ms[StartupMarks::MAIN]);
    assert_eq!(1210.25, back.marks_ms[StartupMarks::FIRST_LISTING]);
    assert_eq!("first", back.instance);
    assert!(back.side_effects_skipped);
    assert!(back.error.is_none());
}

#[test]
fn memory_round_trips_and_is_absent_from_an_older_trace() {
    let reading = StartupMemory {
        working_set_bytes: 96_000_000,
        private_bytes: 61_000_000,
        heap_bytes: 9_500_000,
        peak_working_set_bytes: 104_000_000,
    };
    let mut trace = trace("first", None);
    trace.memory = Some(BTreeMap::from([(
        StartupMemoryPoints::COLLECTED.to_string(),
        reading.clone(),
    )]));

    let json = StartupTraceJson::serialize(&trace);
    assert!(json.contains("\"heapBytes\": 9500000"), "{json}");
    assert_eq!(
        reading,
        StartupTraceJson::deserialize(&json)
            .expect("back")
            .memory
            .expect("memory")[StartupMemoryPoints::COLLECTED]
    );

    // A trace from a build that did not read its memory has no such property at all.
    trace.memory = None;
    let older = StartupTraceJson::serialize(&trace);
    assert!(!older.contains("\"memory\""), "{older}");
    assert!(
        StartupTraceJson::deserialize(&older)
            .expect("back")
            .memory
            .is_none()
    );
}

#[test]
fn serialize_is_camel_case() {
    let json = StartupTraceJson::serialize(&trace("second", Some("late")));
    assert!(json.contains("\"marksMs\""), "{json}");
    assert!(json.contains("\"sideEffectsSkipped\": true"), "{json}");
    assert!(json.contains("\"error\": \"late\""), "{json}");
}

#[test]
fn deserialize_empty_throws() {
    assert!(matches!(
        StartupTraceJson::deserialize("null"),
        Err(BenchError::NotStartupTrace(_))
    ));
}
