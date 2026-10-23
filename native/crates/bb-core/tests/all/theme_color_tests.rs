// Port of tests/BertBrowser.Core.Tests/Theming/ThemeColorTests.cs.
use bb_core::theming::{ThemeColor, ThemeContrast};

#[test]
fn parses_every_supported_form() {
    for (text, a, r, g, b) in [
        ("#1E1E1E", 0xFF, 0x1E, 0x1E, 0x1E),
        ("1E1E1E", 0xFF, 0x1E, 0x1E, 0x1E),
        ("#1e1e1e", 0xFF, 0x1E, 0x1E, 0x1E),
        ("  #1E1E1E  ", 0xFF, 0x1E, 0x1E, 0x1E),
        ("#79797966", 0x79, 0x79, 0x79, 0x66),
        ("#ABC", 0xFF, 0xAA, 0xBB, 0xCC),
        ("#8ABC", 0x88, 0xAA, 0xBB, 0xCC),
    ] {
        let color = ThemeColor::try_parse(text).unwrap_or_else(|| panic!("{text:?}"));
        assert_eq!(ThemeColor::new(a, r, g, b), color, "{text:?}");
    }
}

#[test]
fn rejects_malformed_input_without_throwing() {
    for text in [
        Some(""),
        None,
        Some("#"),
        Some("#12"),
        Some("#12345"),
        Some("#123456789"),
        Some("#GGGGGG"),
        Some("rebeccapurple"),
    ] {
        assert!(text.and_then(ThemeColor::try_parse).is_none(), "{text:?}");
    }
}

#[test]
fn to_hex_round_trips_and_omits_opaque_alpha() {
    assert_eq!("#1E1E1E", ThemeColor::from_rgb(0x1E, 0x1E, 0x1E).to_hex());
    assert_eq!(
        "#66797979",
        ThemeColor::new(0x66, 0x79, 0x79, 0x79).to_hex()
    );

    for literal in ["#1E1E1E", "#66797979", "#000000", "#FFFFFF"] {
        let color = ThemeColor::try_parse(literal).unwrap_or_else(|| panic!("{literal}"));
        assert_eq!(literal, color.to_hex());
    }
}

#[test]
fn composite_over_blends_toward_the_background() {
    let half_white = ThemeColor::new(0x80, 0xFF, 0xFF, 0xFF);
    let over = half_white.composite_over(ThemeColor::from_rgb(0, 0, 0));

    assert_eq!(0xFF, over.a);
    assert!((0x7F..=0x81).contains(&over.r), "{}", over.r);

    // Opaque colours are unchanged, whatever is behind them.
    let opaque = ThemeColor::from_rgb(0x12, 0x34, 0x56);
    assert_eq!(
        opaque,
        opaque.composite_over(ThemeColor::from_rgb(0xFF, 0xFF, 0xFF))
    );
}

#[test]
fn luminance_orders_black_below_grey_below_white() {
    let black = ThemeColor::from_rgb(0, 0, 0).relative_luminance();
    let grey = ThemeColor::from_rgb(0x80, 0x80, 0x80).relative_luminance();
    let white = ThemeColor::from_rgb(0xFF, 0xFF, 0xFF).relative_luminance();

    assert!(black.abs() < 0.5e-6, "{black}");
    assert!((white - 1.0).abs() < 0.5e-6, "{white}");
    assert!(black < grey && grey < white);
}

#[test]
fn black_on_white_is_the_maximum_contrast_ratio() {
    let ratio = ThemeContrast::ratio(
        ThemeColor::from_rgb(0, 0, 0),
        ThemeColor::from_rgb(0xFF, 0xFF, 0xFF),
    );
    assert!((ratio - 21.0).abs() < 0.005, "{ratio}");
}

#[test]
fn hsv_round_trips() {
    for literal in [
        "#FF0000", "#00FF00", "#0000FF", "#1E1E1E", "#0F6CBD", "#FFFFFF", "#000000",
    ] {
        let original = ThemeColor::parse(literal);
        let (h, s, v) = original.to_hsv();
        let round = ThemeColor::from_hsv(h, s, v);

        assert_eq!(original.r, round.r, "{literal}");
        assert_eq!(original.g, round.g, "{literal}");
        assert_eq!(original.b, round.b, "{literal}");
    }
}
