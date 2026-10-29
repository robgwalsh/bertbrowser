//! Turns a rising byte count into a throughput figure and a time remaining.
//! Spec: Core/Services/Transfer/TransferRate.cs.
//!
//! **Smoothed, because the raw figure is useless.** Copy throughput swings wildly between chunks,
//! and a number recomputed from the last interval alone flickers between "8 MB/s" and "400 MB/s"
//! several times a second. An exponential moving average is what makes it readable.
//!
//! **Time is passed in, never read from a clock.** That is what lets the tests assert on exact
//! figures rather than sleeping and hoping.

use std::time::Duration;

use super::estimator::TransferEstimate;

/// Weight given to the newest sample. Low enough to ride out a stutter, high enough that stopping
/// shows up within a couple of seconds.
const SMOOTHING: f64 = 0.25;

/// Below this, an interval is too short to divide by: the timer's own resolution would dominate.
const MINIMUM_SAMPLE: Duration = Duration::from_millis(250);

/// An estimate beyond this is not information. Better to say nothing.
const LONGEST_USEFUL_ESTIMATE: Duration = Duration::from_secs(99 * 3600);

#[derive(Default)]
pub struct TransferRate {
    seeded: bool,
    last_at: Duration,
    last_bytes: i64,
    bytes_per_second: Option<f64>,
}

impl TransferRate {
    pub fn new() -> TransferRate {
        TransferRate::default()
    }

    /// The smoothed throughput, or None until enough has been seen to say.
    pub fn bytes_per_second(&self) -> Option<f64> {
        self.bytes_per_second
    }

    /// Feeds in the cumulative byte count at a point in time. Samples closer together than
    /// `MINIMUM_SAMPLE` are held rather than dropped, so a fast reporter still contributes its
    /// bytes to the next interval instead of being thrown away.
    pub fn observe(&mut self, bytes_done: i64, elapsed: Duration) {
        if !self.seeded {
            self.seeded = true;
            self.last_at = elapsed;
            self.last_bytes = bytes_done;
            return;
        }

        let interval = elapsed.saturating_sub(self.last_at);
        if interval < MINIMUM_SAMPLE {
            return;
        }

        // A count that went backwards means a new transfer reusing this instance; re-seed rather
        // than reporting a negative rate.
        let moved = bytes_done - self.last_bytes;
        if moved < 0 {
            self.reset();
            self.observe(bytes_done, elapsed);
            return;
        }

        let sample = moved as f64 / interval.as_secs_f64();
        self.bytes_per_second = Some(match self.bytes_per_second {
            Some(previous) => (previous * (1.0 - SMOOTHING)) + (sample * SMOOTHING),
            None => sample,
        });
        self.last_at = elapsed;
        self.last_bytes = bytes_done;
    }

    /// How long the rest should take, or None when there is no honest answer: no throughput yet, a
    /// stalled transfer, or, the one worth being strict about, a byte total the size index could
    /// not complete, where the remainder is unknown rather than small.
    pub fn remaining(&self, bytes_done: i64, estimate: &TransferEstimate) -> Option<Duration> {
        if !estimate.is_usable() {
            return None;
        }
        let rate = self.bytes_per_second?;
        if rate <= 0.0 {
            return None;
        }
        let left = estimate.bytes - bytes_done;
        if left <= 0 {
            return Some(Duration::ZERO);
        }
        let seconds = left as f64 / rate;
        if seconds > LONGEST_USEFUL_ESTIMATE.as_secs_f64() {
            None
        } else {
            Some(Duration::from_secs_f64(seconds))
        }
    }

    pub fn reset(&mut self) {
        self.seeded = false;
        self.last_at = Duration::ZERO;
        self.last_bytes = 0;
        self.bytes_per_second = None;
    }
}
