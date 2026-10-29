// Port of tests/BertBrowser.Core.Tests/GifFrameCompositorTests.cs
use bb_core::cancel::CancelToken;
use bb_core::preview::animation::{GifDisposal, GifFrameCompositor, GifSubFrame};

// A 2×1 canvas throughout: pixel 0 on the left, pixel 1 on the right.

const RED: [u8; 4] = [0, 0, 255, 255];
const BLUE: [u8; 4] = [255, 0, 0, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

fn pixels(pixels: &[[u8; 4]]) -> Vec<u8> {
    pixels.concat()
}

fn pixel_at(canvas: &[u8], index: usize) -> [u8; 4] {
    canvas[index * 4..index * 4 + 4].try_into().unwrap()
}

fn frame(
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    pixels: Vec<u8>,
    disposal: GifDisposal,
) -> GifSubFrame {
    GifSubFrame {
        left,
        top,
        width,
        height,
        pixels,
        disposal,
    }
}

fn compose(frames: Vec<GifSubFrame>) -> Vec<Vec<u8>> {
    GifFrameCompositor::compose(2, 1, frames, &CancelToken::none()).unwrap()
}

#[test]
fn a_kept_frame_stays_under_the_next() {
    let frames = compose(vec![
        frame(0, 0, 2, 1, pixels(&[RED, RED]), GifDisposal::Keep),
        frame(1, 0, 1, 1, BLUE.to_vec(), GifDisposal::Keep),
    ]);

    assert_eq!(RED, pixel_at(&frames[1], 0));
    assert_eq!(BLUE, pixel_at(&frames[1], 1));
}

#[test]
fn a_transparent_pixel_shows_what_is_beneath() {
    let frames = compose(vec![
        frame(0, 0, 2, 1, pixels(&[RED, RED]), GifDisposal::Keep),
        frame(0, 0, 2, 1, pixels(&[CLEAR, BLUE]), GifDisposal::Keep),
    ]);

    assert_eq!(RED, pixel_at(&frames[1], 0));
    assert_eq!(BLUE, pixel_at(&frames[1], 1));
}

#[test]
fn a_cleared_frame_leaves_its_rectangle_transparent() {
    let frames = compose(vec![
        frame(0, 0, 1, 1, RED.to_vec(), GifDisposal::Keep),
        frame(1, 0, 1, 1, BLUE.to_vec(), GifDisposal::Clear),
        frame(0, 0, 1, 1, CLEAR.to_vec(), GifDisposal::Keep),
    ]);

    assert_eq!(BLUE, pixel_at(&frames[1], 1)); // shown in full first
    assert_eq!(RED, pixel_at(&frames[2], 0)); // outside the rectangle, untouched
    assert_eq!(CLEAR, pixel_at(&frames[2], 1)); // then cleared
}

#[test]
fn a_restored_frame_puts_back_what_was_there_before() {
    let frames = compose(vec![
        frame(0, 0, 2, 1, pixels(&[RED, RED]), GifDisposal::Keep),
        frame(0, 0, 2, 1, pixels(&[BLUE, BLUE]), GifDisposal::Restore),
        frame(0, 0, 1, 1, CLEAR.to_vec(), GifDisposal::Keep),
    ]);

    assert_eq!(BLUE, pixel_at(&frames[1], 0));
    assert_eq!(RED, pixel_at(&frames[2], 0));
    assert_eq!(RED, pixel_at(&frames[2], 1));
}

#[test]
fn each_frame_is_its_own_copy() {
    let frames = compose(vec![
        frame(0, 0, 1, 1, RED.to_vec(), GifDisposal::Keep),
        frame(0, 0, 1, 1, BLUE.to_vec(), GifDisposal::Keep),
    ]);

    assert_eq!(RED, pixel_at(&frames[0], 0));
    assert_eq!(BLUE, pixel_at(&frames[1], 0));
}

#[test]
fn a_rectangle_off_the_canvas_is_clipped_rather_than_throwing() {
    let frames = compose(vec![
        frame(
            1,
            0,
            3,
            2,
            pixels(&[BLUE, BLUE, BLUE, BLUE, BLUE, BLUE]),
            GifDisposal::Clear,
        ),
        frame(-5, -5, 1, 1, RED.to_vec(), GifDisposal::Keep),
        frame(i32::MAX, 0, i32::MAX, 1, RED.to_vec(), GifDisposal::Keep),
    ]);

    assert_eq!(CLEAR, pixel_at(&frames[0], 0));
    assert_eq!(BLUE, pixel_at(&frames[0], 1));
    assert_eq!(CLEAR, pixel_at(&frames[1], 1));
    assert_eq!(3, frames.len());
}

#[test]
fn a_pixel_buffer_shorter_than_its_rectangle_is_drawn_as_far_as_it_goes() {
    let frames = compose(vec![frame(0, 0, 2, 1, RED.to_vec(), GifDisposal::Keep)]);

    assert_eq!(RED, pixel_at(&frames[0], 0));
    assert_eq!(CLEAR, pixel_at(&frames[0], 1));
}
