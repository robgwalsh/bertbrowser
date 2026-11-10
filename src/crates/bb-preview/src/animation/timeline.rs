//! When each frame of an animation is on screen, and which frame a position lands on. Always
//! loops: a position past the end wraps, so the clock driving playback never has to reset.
//!
//! The GIF's own loop count is deliberately ignored: a preview that stops after three passes
//! reads as a frozen picture, and the seek bar is how someone gets a particular frame to hold.
//! Spec: Core/Services/Preview/Animation/AnimationTimeline.cs.

pub struct AnimationTimeline {
    starts: Vec<i64>,
    delays: Vec<i32>,
    duration: i64,
}

impl AnimationTimeline {
    /// Delays at or below this are what browsers replace, and so what GIF authors have long
    /// relied on: a declared 0 or 10 ms plays at 100 ms everywhere else.
    pub const MIN_HONOURED_DELAY_MS: i32 = 10;

    /// What a delay too short to honour is played at.
    pub const DEFAULT_DELAY_MS: i32 = 100;

    /// None for no frames at all: an animation has at least one.
    pub fn new(delays_ms: &[i32]) -> Option<AnimationTimeline> {
        if delays_ms.is_empty() {
            return None;
        }

        let mut starts = Vec::with_capacity(delays_ms.len());
        let mut delays = Vec::with_capacity(delays_ms.len());
        let mut at: i64 = 0;
        for &delay in delays_ms {
            starts.push(at);
            let honoured = if delay <= Self::MIN_HONOURED_DELAY_MS {
                Self::DEFAULT_DELAY_MS
            } else {
                delay
            };
            delays.push(honoured);
            at += i64::from(honoured);
        }
        Some(AnimationTimeline {
            starts,
            delays,
            duration: at,
        })
    }

    /// Each frame's delay after the too-short ones have been replaced.
    pub fn delays(&self) -> &[i32] {
        &self.delays
    }

    pub fn frame_count(&self) -> usize {
        self.starts.len()
    }

    /// One pass, in milliseconds. Never zero, since every delay is at least `DEFAULT_DELAY_MS` or
    /// above `MIN_HONOURED_DELAY_MS`.
    pub fn duration(&self) -> i64 {
        self.duration
    }

    /// A position folded into one pass. Negative positions wrap from the end.
    pub fn wrap(&self, position_ms: i64) -> i64 {
        position_ms.rem_euclid(self.duration)
    }

    /// Where `frame` starts, clamped to the frames that exist.
    pub fn start_of(&self, frame: isize) -> i64 {
        let last = self.starts.len() - 1;
        self.starts[frame.clamp(0, last as isize) as usize]
    }

    /// The frame on screen at `position_ms`, after wrapping.
    pub fn frame_at(&self, position_ms: i64) -> usize {
        let p = self.wrap(position_ms);
        // The last start at or before `p`; frame 0 starts at 0, so there is always one.
        self.starts.partition_point(|&start| start <= p) - 1
    }

    /// How long from `position_ms` until the frame changes. Always at least 1, so a timer set
    /// from it can never spin.
    pub fn until_next_frame(&self, position_ms: i64) -> i64 {
        let p = self.wrap(position_ms);
        let frame = self.frame_at(p);
        let end = self.starts.get(frame + 1).copied().unwrap_or(self.duration);
        (end - p).max(1)
    }
}
