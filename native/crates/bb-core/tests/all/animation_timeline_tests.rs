// Port of tests/BertBrowser.Core.Tests/AnimationTimelineTests.cs
use bb_core::preview::animation::{AnimationLimits, AnimationTimeline};

fn timeline(delays: &[i32]) -> AnimationTimeline {
    AnimationTimeline::new(delays).expect("at least one frame")
}

#[test]
fn a_delay_too_short_to_honour_plays_at_the_browser_default() {
    let timeline = timeline(&[0, 10, 20]);
    assert_eq!(&[100, 100, 20], timeline.delays());
    assert_eq!(220, timeline.duration());
}

#[test]
fn a_position_lands_on_the_frame_showing_then() {
    let timeline = timeline(&[100, 250, 50]);
    for (position, frame) in [(0, 0), (99, 0), (100, 1), (349, 1), (350, 2), (399, 2)] {
        assert_eq!(frame, timeline.frame_at(position), "position {position}");
    }
}

#[test]
fn past_the_end_wraps_to_the_start() {
    let timeline = timeline(&[100, 250, 50]);
    assert_eq!(0, timeline.frame_at(400));
    assert_eq!(1, timeline.frame_at(400 * 7 + 150));
    assert_eq!(2, timeline.frame_at(-1));
}

#[test]
fn the_next_change_is_the_end_of_the_current_frame() {
    let timeline = timeline(&[100, 250, 50]);
    assert_eq!(100, timeline.until_next_frame(0));
    assert_eq!(40, timeline.until_next_frame(310));
    assert_eq!(10, timeline.until_next_frame(390));
    assert_eq!(100, timeline.until_next_frame(400));
}

#[test]
fn start_of_is_clamped_to_the_frames_that_exist() {
    let timeline = timeline(&[100, 250, 50]);
    assert_eq!(350, timeline.start_of(2));
    assert_eq!(350, timeline.start_of(99));
    assert_eq!(0, timeline.start_of(-1));
}

#[test]
fn a_single_frame_is_its_own_whole_loop() {
    let timeline = timeline(&[0]);
    assert_eq!(1, timeline.frame_count());
    assert_eq!(0, timeline.frame_at(12345));
    assert!(timeline.until_next_frame(12345) >= 1);
}

#[test]
fn the_limits_are_asked_of_the_header() {
    for (frames, width, height, allowed) in [
        (1, 10, 10, false), // a still picture is not an animation
        (2, 10, 10, true),
        (AnimationLimits::MAX_FRAMES + 1, 1, 1, false),
        (100, 1000, 1000, false), // 400 MB decoded
        (2, 0, 10, false),
    ] {
        assert_eq!(
            allowed,
            AnimationLimits::allows(frames, width, height),
            "{frames} frames of {width} x {height}"
        );
    }
}
