// Port of tests/BertBrowser.Core.Tests/TransferRateTests.cs.
use std::time::Duration;

use bb_core::transfer::{TransferEstimate, TransferRate};

fn at(seconds: f64) -> Duration {
    Duration::from_secs_f64(seconds)
}

fn known(bytes: i64) -> TransferEstimate {
    TransferEstimate::new(bytes, 1, true)
}

fn assert_close(expected: f64, actual: f64, tolerance: f64) {
    assert!(
        (expected - actual).abs() <= tolerance,
        "expected {expected} within {tolerance}, got {actual}"
    );
}

#[test]
fn one_sample_is_not_enough_to_name_a_rate() {
    let mut rate = TransferRate::new();
    rate.observe(0, at(0.0));

    assert_eq!(None, rate.bytes_per_second());
}

#[test]
fn two_samples_give_the_throughput_between_them() {
    let mut rate = TransferRate::new();
    rate.observe(0, at(0.0));
    rate.observe(1_000_000, at(1.0));

    assert_close(1_000_000.0, rate.bytes_per_second().unwrap(), 1.0);
}

#[test]
fn a_sample_too_close_to_the_last_is_held_rather_than_thrown_away() {
    // A fast reporter must still contribute its bytes; dropping them would under-report.
    let mut rate = TransferRate::new();
    rate.observe(0, at(0.0));
    rate.observe(500_000, at(0.1)); // too soon to divide by
    assert_eq!(None, rate.bytes_per_second());

    rate.observe(1_000_000, at(1.0));
    assert_close(1_000_000.0, rate.bytes_per_second().unwrap(), 1.0);
}

#[test]
fn a_sudden_spike_is_smoothed_rather_than_followed() {
    let mut rate = TransferRate::new();
    rate.observe(0, at(0.0));
    rate.observe(1_000_000, at(1.0)); // 1 MB/s
    rate.observe(1_000_000 + 100_000_000, at(2.0)); // one 100 MB/s second

    // Somewhere between the two, and much nearer the established figure.
    let bps = rate.bytes_per_second().unwrap();
    assert!((1_000_000.0..=50_000_000.0).contains(&bps), "{bps}");
}

#[test]
fn a_stalled_transfer_decays_towards_nothing() {
    let mut rate = TransferRate::new();
    rate.observe(0, at(0.0));
    rate.observe(10_000_000, at(1.0));
    let moving = rate.bytes_per_second().unwrap();

    for second in 2..=12 {
        rate.observe(10_000_000, at(second as f64)); // nothing more arrives
    }

    assert!(
        rate.bytes_per_second().unwrap() < moving / 10.0,
        "a transfer that stopped must not keep reporting the rate it had."
    );
}

#[test]
fn remaining_needs_a_rate() {
    let mut rate = TransferRate::new();
    rate.observe(0, at(0.0));

    assert_eq!(None, rate.remaining(0, &known(1_000_000)));
}

#[test]
fn remaining_divides_what_is_left_by_the_rate() {
    let mut rate = TransferRate::new();
    rate.observe(0, at(0.0));
    rate.observe(1_000_000, at(1.0)); // 1 MB/s

    let left = rate.remaining(1_000_000, &known(7_000_000));

    assert_close(6.0, left.unwrap().as_secs_f64(), 0.1);
}

#[test]
fn remaining_is_nothing_when_the_total_is_only_a_floor() {
    // The size index had no row for something in the plan. The remainder is unknown, not small,
    // and turning it into a time would be an invention.
    let mut rate = TransferRate::new();
    rate.observe(0, at(0.0));
    rate.observe(1_000_000, at(1.0));

    assert_eq!(
        None,
        rate.remaining(1_000_000, &TransferEstimate::new(7_000_000, 1, false))
    );
}

#[test]
fn remaining_is_zero_once_everything_is_accounted_for() {
    let mut rate = TransferRate::new();
    rate.observe(0, at(0.0));
    rate.observe(1_000_000, at(1.0));

    assert_eq!(
        Some(Duration::ZERO),
        rate.remaining(1_000_000, &known(1_000_000))
    );
}

#[test]
fn an_absurd_estimate_is_withheld_rather_than_shown() {
    let mut rate = TransferRate::new();
    rate.observe(0, at(0.0));
    rate.observe(1, at(1.0)); // one byte per second

    assert_eq!(None, rate.remaining(0, &known(i64::MAX / 2)));
}

#[test]
fn a_count_that_goes_backwards_re_seeds_instead_of_going_negative() {
    let mut rate = TransferRate::new();
    rate.observe(0, at(0.0));
    rate.observe(10_000_000, at(1.0));

    rate.observe(0, at(2.0)); // a second transfer reusing the instance
    rate.observe(2_000_000, at(3.0));

    assert!(rate.bytes_per_second().unwrap() > 0.0);
}
