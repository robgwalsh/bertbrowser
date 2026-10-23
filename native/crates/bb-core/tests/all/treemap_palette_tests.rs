// Port of tests/BertBrowser.Core.Tests/Theming/TreemapPaletteTests.cs.
//
// The treemap's fills are derived from each theme's accent rather than authored, so the contrast
// guarantee the shipped tokens get from `theme_catalog_tests` has to be earned here by rule
// instead. Every built-in is run through it, including the light-accent ones.
use bb_core::diskusage::TreemapPalette;
use bb_core::theming::{
    ColorMap, ResolvedTheme, ThemeCatalog, ThemeColor, ThemeContrast, ThemeDefinition,
    ThemeResolver, ThemeToken,
};

fn resolve(id: &str) -> ResolvedTheme {
    ThemeResolver::resolve(ThemeCatalog::find(id).unwrap(), ThemeCatalog::find)
}

fn built_in_ids() -> impl Iterator<Item = &'static str> {
    ThemeCatalog::built_ins().iter().map(|t| t.id.as_str())
}

fn distance(a: ThemeColor, b: ThemeColor) -> f64 {
    let dr = f64::from(a.r) - f64::from(b.r);
    let dg = f64::from(a.g) - f64::from(b.g);
    let db = f64::from(a.b) - f64::from(b.b);
    (dr * dr + dg * dg + db * db).sqrt()
}

/// The one that matters, and the reason the fills are adjusted rather than just rotated: at a
/// fixed saturation and value, yellows and greens come out far lighter than blues, so a plain hue
/// rotation produces tiles the label disappears on. Delete the contrast pull in `legible` and
/// this goes red.
#[test]
fn every_fill_is_legible_under_the_label_colour() {
    for id in built_in_ids() {
        let theme = resolve(id);
        let label = theme[ThemeToken::TextOnAccent];

        for fill in TreemapPalette::ramp(&theme, 12) {
            let ratio = ThemeContrast::ratio(label, fill);
            assert!(
                ratio >= ThemeContrast::AA_NORMAL_TEXT,
                "{id}: label on {} is only {ratio:.2}:1",
                fill.to_hex()
            );
        }
    }
}

/// Neighbouring tiles have to be told apart, which is the entire job of the hue rotation.
#[test]
fn adjacent_fills_are_visibly_different() {
    for id in built_in_ids() {
        let ramp = TreemapPalette::ramp(&resolve(id), 8);

        for pair in ramp.windows(2) {
            assert!(
                distance(pair[0], pair[1]) > 20.0,
                "{id}: {} and {} are nearly the same colour",
                pair[0].to_hex(),
                pair[1].to_hex()
            );
        }
    }
}

#[test]
fn the_ramp_is_deterministic() {
    for id in built_in_ids() {
        let theme = resolve(id);

        assert_eq!(
            TreemapPalette::ramp(&theme, 6),
            TreemapPalette::ramp(&theme, 6),
            "{id}"
        );
    }
}

#[test]
fn the_requested_count_is_what_comes_back() {
    let theme = resolve(&ThemeCatalog::built_ins()[0].id);

    assert_eq!(5, TreemapPalette::ramp(&theme, 5).len());
    assert!(TreemapPalette::ramp(&theme, 0).is_empty());
    // The C# also asks for -1 and gets nothing; a usize count makes that unrepresentable.
}

/// A theme whose accent is nearly grey has no hue to rotate, so without a floor on saturation the
/// whole ramp would come back as one colour repeated.
#[test]
fn a_greyscale_accent_still_produces_distinguishable_fills() {
    let definition = ThemeDefinition {
        id: "grey-test".into(),
        name: "Grey".into(),
        base_theme_id: Some(ThemeCatalog::dark_plus().id.clone()),
        colors: ColorMap::from([(ThemeToken::AccentBackground.name(), "#FF808080")]),
        ..Default::default()
    };
    let grey = ThemeResolver::resolve(&definition, ThemeCatalog::find);

    let ramp = TreemapPalette::ramp(&grey, 6);

    for pair in ramp.windows(2) {
        assert!(
            distance(pair[0], pair[1]) > 20.0,
            "the ramp collapsed to one colour"
        );
    }
}
