// Port of tests/BertBrowser.Core.Tests/AspectRatioTests.cs
use bb_core::preview::AspectRatio;

#[test]
fn try_parse_accepts_width_colon_height() {
    for (text, width, height) in [
        ("4:3", 4, 3),
        ("16:9", 16, 9),
        (" 3 : 2 ", 3, 2),
        ("1:1", 1, 1),
    ] {
        let value = AspectRatio::try_parse(Some(text)).unwrap_or_else(|| panic!("{text:?}"));
        assert_eq!(width, value.width, "{text:?}");
        assert_eq!(height, value.height, "{text:?}");
    }
}

#[test]
fn try_parse_rejects_anything_else() {
    for text in [
        None,
        Some(""),
        Some("   "),
        Some("4"),
        Some("4:3:2"),
        Some("4/3"),
        Some("four:three"),
        Some("0:3"),
        Some("4:0"),
        Some("-4:3"),
        Some("4:-3"),
        Some("4.5:3"),
        Some("101:3"),
        Some("4:101"),
    ] {
        assert_eq!(None, AspectRatio::try_parse(text), "{text:?}");
    }
}

#[test]
fn parse_falls_back_to_default_rather_than_throwing() {
    for text in [None, Some("nonsense"), Some("0:0")] {
        assert_eq!(AspectRatio::DEFAULT, AspectRatio::parse(text), "{text:?}");
    }
}

#[test]
fn default_is_four_by_three() {
    assert_eq!("4:3", AspectRatio::DEFAULT.to_string());
}

#[test]
fn presets_lead_with_the_default_shapes_and_all_round_trip() {
    assert!(AspectRatio::PRESETS.contains(&AspectRatio::DEFAULT));
    for preset in AspectRatio::PRESETS {
        assert_eq!(*preset, AspectRatio::parse(Some(&preset.to_string())));
    }
}

#[test]
fn height_for_scales_the_width_by_the_ratio() {
    for (text, width, expected) in [
        ("4:3", 200.0, 150.0),
        ("1:1", 200.0, 200.0),
        ("16:9", 160.0, 90.0),
        ("3:4", 120.0, 160.0),
    ] {
        let actual = AspectRatio::parse(Some(text)).height_for(width);
        assert!((expected - actual).abs() < 1e-6, "{text}: {actual}");
    }
}

/// A zero ratio is reachable however careful the factories are; it must not hand the view a NaN
/// height.
#[test]
fn height_for_treats_a_default_constructed_ratio_as_square() {
    let zero = AspectRatio {
        width: 0,
        height: 0,
    };
    assert_eq!(180.0, zero.height_for(180.0));
}
