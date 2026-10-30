// Port of tests/BertBrowser.Core.Tests/ImageZoomTests.cs
use bb_core::preview::ImageZoom;

fn assert_close(expected: f64, actual: f64, precision: i32) {
    let scale = 10f64.powi(precision);
    assert_eq!(
        (expected * scale).round(),
        (actual * scale).round(),
        "expected {expected}, got {actual}"
    );
}

#[test]
fn a_notch_in_and_a_notch_out_come_back_to_where_they_started() {
    let zoomed = ImageZoom::step(2.0, 1.0, 1.0);
    assert_close(2.0 * ImageZoom::STEP_FACTOR, zoomed, 9);
    assert_close(2.0, ImageZoom::step(zoomed, -1.0, 1.0), 9);
}

#[test]
fn zooming_out_stops_at_fit() {
    assert_eq!(1.0, ImageZoom::step(1.1, -5.0, 1.0));
}

#[test]
fn zooming_out_may_go_below_fit_when_one_to_one_is_smaller() {
    // A small image shown at 100% is smaller than fitted; the wheel must not snap it back up.
    assert_eq!(0.5, ImageZoom::clamp(0.5, 0.5));
    assert_eq!(0.5, ImageZoom::step(0.5, -3.0, 0.5));
}

#[test]
fn zooming_in_stops_at_the_ceiling() {
    assert_eq!(
        ImageZoom::MAX_SCALE,
        ImageZoom::step(ImageZoom::MAX_SCALE, 3.0, 1.0)
    );
}

#[test]
fn the_point_under_the_cursor_stays_put() {
    // Image point 100 drawn at 100 * 2 + 10 = 210; after zooming to 3 it must still land at 210.
    let offset = ImageZoom::zoom_offset(10.0, 100.0, 2.0, 3.0);
    assert_eq!(210.0, 100.0 * 3.0 + offset);
}

#[test]
fn an_image_smaller_than_the_pane_is_centred() {
    // Fitted 200 wide at origin 50 in a 300 pane; at scale 1 it belongs where layout put it.
    assert_eq!(0.0, ImageZoom::clamp_offset(80.0, 50.0, 200.0, 1.0, 300.0));
}

#[test]
fn a_zoomed_image_cannot_be_dragged_past_its_edges() {
    // 200 wide at scale 2 = 400 in a 300 pane, origin 50: left edge at most 0, right edge at
    // least 300.
    assert_eq!(
        -50.0,
        ImageZoom::clamp_offset(500.0, 50.0, 200.0, 2.0, 300.0)
    );
    assert_eq!(
        -150.0,
        ImageZoom::clamp_offset(-500.0, 50.0, 200.0, 2.0, 300.0)
    );
    assert_eq!(
        -100.0,
        ImageZoom::clamp_offset(-100.0, 50.0, 200.0, 2.0, 300.0)
    );
}
