// The helper's resource use: the counters it sends, the rates the app makes of two of them, and the
// words both the title-bar tip and Settings show.
use bb_index::mft::{CurrentProcessUsage, IndexerUsage, ProcessUsageProbe, ProcessUsageSample};

fn sample(uptime_ms: u64, cpu_100ns: u64, read: u64, write: u64) -> ProcessUsageSample {
    ProcessUsageSample {
        pid: 4120,
        working_set: 84 * 1024 * 1024,
        private_bytes: 71 * 1024 * 1024,
        peak_working_set: 120 * 1024 * 1024,
        cpu_100ns,
        read_bytes: read,
        write_bytes: write,
        handles: 1312,
        uptime_ms,
        cpus: 8,
    }
}

fn row(usage: &IndexerUsage, id: &str) -> bb_index::mft::UsageRow {
    usage
        .rows()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("no row {id}"))
}

fn value(usage: &IndexerUsage, id: &str) -> String {
    row(usage, id).value
}

fn detail(usage: &IndexerUsage, id: &str) -> String {
    row(usage, id).detail
}

#[test]
fn a_sample_round_trips_through_its_argument() {
    let s = sample(3_600_000, 123_456_789, 10, 20);
    assert_eq!(Some(s), ProcessUsageSample::from_argument(&s.to_argument()));
}

#[test]
fn a_figure_a_newer_helper_adds_is_skipped_and_a_missing_one_is_zero() {
    let s = ProcessUsageSample::from_argument("pid=7,gpu=99").unwrap();
    assert_eq!(7, s.pid);
    assert_eq!(0, s.working_set);
}

/// Two seconds of the helper's clock, 0.16 s of CPU on eight processors: one percent of the
/// machine, as Task Manager would say it.
#[test]
fn rates_are_measured_against_the_helpers_own_clock() {
    let first = sample(10_000, 0, 0, 0);
    let second = sample(12_000, 1_600_000, 4 * 1024 * 1024, 0);

    let usage = IndexerUsage::advance(Some(&first), second);

    assert!((usage.cpu_percent.unwrap() - 1.0).abs() < 1e-9);
    assert_eq!(Some(2.0 * 1024.0 * 1024.0), usage.read_per_second);
    assert_eq!(Some(0.0), usage.write_per_second);
    assert_eq!("1.0%", value(&usage, "cpu"));
    // The rate and the total apart, so a host can hold the rate to a fixed column.
    assert_eq!("2 MB/s", value(&usage, "read"));
    assert_eq!("4 MB total", detail(&usage, "read"));
    assert_eq!("0 B/s", value(&usage, "write"));
    assert_eq!("0 B total", detail(&usage, "write"));
}

#[test]
fn nothing_to_divide_by_means_no_rate_rather_than_a_made_up_one() {
    let first = sample(10_000, 0, 0, 0);
    let restarted = ProcessUsageSample {
        pid: 9999,
        ..sample(12_000, 1_600_000, 100, 0)
    };
    let clock_stood_still = sample(10_000, 1_600_000, 100, 0);

    for usage in [
        IndexerUsage::advance(None, first),
        IndexerUsage::advance(Some(&first), restarted),
        IndexerUsage::advance(Some(&first), clock_stood_still),
    ] {
        assert_eq!(None, usage.cpu_percent);
        assert_eq!(None, usage.read_per_second);
        assert_eq!("\u{2014}", value(&usage, "cpu"));
        assert_eq!("\u{2014}", value(&usage, "read"));
        assert!(detail(&usage, "read").ends_with(" total"));
    }
}

#[test]
fn rows_are_worded_in_one_place_in_a_fixed_order() {
    let usage = IndexerUsage::advance(None, sample(3 * 3_600_000 + 12 * 60_000, 0, 0, 0));
    let rows = usage.rows();

    let ids: Vec<&str> = rows.iter().map(|r| r.id).collect();
    assert_eq!(
        vec![
            "memory", "private", "peak", "cpu", "read", "write", "handles", "uptime", "pid"
        ],
        ids
    );
    assert_eq!("84 MB", value(&usage, "memory"));
    assert_eq!("71 MB", value(&usage, "private"));
    assert_eq!("120 MB", value(&usage, "peak"));
    assert_eq!("1,312", value(&usage, "handles"));
    assert_eq!("3 h 12 min", value(&usage, "uptime"));
    assert_eq!("4120", value(&usage, "pid"));
}

#[test]
fn uptime_is_worded_at_the_scale_it_has_reached() {
    let at = |ms| value(&IndexerUsage::advance(None, sample(ms, 0, 0, 0)), "uptime");
    assert_eq!("45 s", at(45_000));
    assert_eq!("12 min", at(12 * 60_000));
    assert_eq!("2 d 4 h", at((2 * 24 + 4) * 3_600_000));
}

#[test]
fn small_but_busy_cpu_is_not_called_zero() {
    let first = sample(10_000, 0, 0, 0);
    // 0.0016 s of CPU over 2 s on 8 processors: 0.01%.
    let usage = IndexerUsage::advance(Some(&first), sample(12_000, 16_000, 0, 0));
    assert_eq!("<0.1%", value(&usage, "cpu"));
    let idle = IndexerUsage::advance(Some(&first), sample(12_000, 0, 0, 0));
    assert_eq!("0%", value(&idle, "cpu"));
}

/// The real probe, on this test process: every figure it reads must be one Windows really gave.
#[test]
fn the_real_probe_reads_this_process() {
    let s = CurrentProcessUsage
        .sample()
        .expect("the counters of our own process");
    assert_eq!(u64::from(std::process::id()), s.pid);
    assert!(s.working_set > 1024 * 1024, "{s:?}");
    assert!(s.private_bytes > 0, "{s:?}");
    assert!(s.peak_working_set >= s.working_set, "{s:?}");
    assert!(s.handles > 0, "{s:?}");
    assert!(s.cpus >= 1, "{s:?}");
    // The process started this run, not before the epoch or in the future.
    assert!(s.uptime_ms < 24 * 3_600_000, "{s:?}");
}

/// The column a host sizes from the template must hold every rate the formatter writes, at every
/// magnitude and at the top of each unit, or the total beside it would shift after all.
#[test]
fn no_rate_is_wider_than_the_template_its_column_is_sized_from() {
    let first = sample(10_000, 0, 0, 0);
    let mut per_second: u64 = 1;
    while per_second < 1u64 << 42 {
        for bytes_per_second in [
            per_second,
            per_second * 1023 / 1024,
            per_second * 1000 / 1024,
        ] {
            // One second between samples, so the read delta is the rate.
            let usage = IndexerUsage::advance(Some(&first), sample(11_000, 0, bytes_per_second, 0));
            let rate = value(&usage, "read");
            assert!(
                rate.chars().count() <= bb_index::mft::RATE_WIDTH_TEMPLATE.chars().count(),
                "{rate:?} is wider than the template"
            );
        }
        per_second *= 2;
    }
}
