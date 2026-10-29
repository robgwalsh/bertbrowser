// Port of tests/BertBrowser.Core.Tests/Theming/ThemeJsonTests.cs.
use bb_core::theming::{
    ColorMap, ThemeCatalog, ThemeDefinition, ThemeJson, ThemeResolver, ThemeToken, no_lookup,
};

#[test]
fn a_definition_round_trips() {
    let original = ThemeDefinition {
        id: "ocean".into(),
        name: "Ocean".into(),
        base_theme_id: Some("dark-plus".into()),
        is_dark: true,
        colors: ColorMap::from([
            (ThemeToken::WindowBackground.name(), "#0B1E2D"),
            (ThemeToken::AccentBackground.name(), "#1F6FEB"),
        ]),
        is_built_in: false,
    };

    let round = ThemeJson::try_deserialize(&ThemeJson::serialize(&original)).unwrap();
    assert_eq!(original.id, round.id);
    assert_eq!(original.name, round.name);
    assert_eq!(original.base_theme_id, round.base_theme_id);
    assert_eq!(original.is_dark, round.is_dark);
    assert_eq!(original.colors, round.colors);
}

#[test]
fn a_hand_written_file_loads() {
    let json = r##"
        {
          // a user could reasonably write this by hand
          "id": "ocean",
          "name": "Ocean",
          "base": "dark-plus",
          "isDark": true,
          "colors": {
            "Theme.Window.Background": "#0B1E2D",
          }
        }
        "##;

    let theme = ThemeJson::try_deserialize(json).unwrap();
    assert_eq!("ocean", theme.id);
    assert_eq!(Some("dark-plus"), theme.base_theme_id.as_deref());
    assert_eq!(
        Some("#0B1E2D"),
        theme.colors.get(ThemeToken::WindowBackground.name())
    );
}

#[test]
fn properties_a_future_version_adds_are_ignored() {
    let json = r#"{ "id": "ocean", "name": "Ocean", "somethingNew": 42, "colors": {} }"#;

    let theme = ThemeJson::try_deserialize(json).unwrap();
    assert_eq!("ocean", theme.id);
}

#[test]
fn a_theme_with_no_name_falls_back_to_its_id() {
    let theme = ThemeJson::try_deserialize(r#"{ "id": "ocean" }"#).unwrap();
    assert_eq!("ocean", theme.name);
}

#[test]
fn malformed_input_is_rejected_without_throwing() {
    for json in ["", "not json", "{ \"name\": \"No id\" }", "null"] {
        let error = ThemeJson::try_deserialize(json).expect_err(json);
        assert!(!error.trim().is_empty(), "{json:?}");
    }
}

/// An exported theme has to stand on its own: no base, every token spelled out, and re-importing
/// it must give back exactly the colours that were on screen.
#[test]
fn an_exported_theme_is_self_contained() {
    let monokai = ThemeResolver::resolve(ThemeCatalog::monokai(), ThemeCatalog::find);

    let exported = ThemeJson::try_deserialize(&ThemeJson::serialize_resolved(&monokai)).unwrap();
    assert_eq!(None, exported.base_theme_id);
    assert_eq!(ThemeToken::COUNT, exported.colors.len());

    // Resolved without any lookup at all: nothing to inherit from but the built-in floor.
    let reimported = ThemeResolver::resolve(&exported, no_lookup);
    for token in ThemeToken::ALL {
        assert_eq!(monokai[token], reimported[token], "{}", token.name());
    }
}
