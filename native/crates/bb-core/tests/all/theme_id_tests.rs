// Port of tests/BertBrowser.Core.Tests/Theming/ThemeIdTests.cs.
//
// A theme id becomes a filename in an elevated process, so these are the rules standing between an
// imported *.json and a write outside the themes folder. Mutate `ThemeId::is_safe` to return true
// and the traversal theories below go red.
use bb_core::theming::ThemeId;

#[test]
fn ordinary_ids_are_accepted() {
    for id in ["dark-plus", "my-theme-2", "theme", "123", "Mixed-Case"] {
        assert!(ThemeId::is_safe(id), "{id}");
    }
}

#[test]
fn a_path_is_not_an_id() {
    for id in [
        // Rooted: a path join discards the themes folder entirely for these.
        r"C:\Windows\Temp\evil",
        r"\\server\share\evil",
        "/etc/passwd",
        // Traversal: resolves out of the folder even though it stays relative.
        r"..\..\..\Windows\Temp\evil",
        "../../evil",
        "..",
        ".",
        // A separator anywhere makes it more than one path segment.
        r"sub\theme",
        "sub/theme",
    ] {
        assert!(!ThemeId::is_safe(id), "{id}");
    }
}

#[test]
fn nothing_is_not_an_id() {
    // The C# rows are "", "   " and null; null is the empty string here.
    for id in ["", "   ", ""] {
        assert!(!ThemeId::is_safe(id), "{id:?}");
    }
}

#[test]
fn reserved_device_names_are_refused() {
    for id in ["con", "CON", "nul", "LPT1"] {
        assert!(!ThemeId::is_safe(id), "{id}");
    }
}

#[test]
fn names_windows_silently_rewrites_are_refused() {
    // Windows strips these, so "evil " and "evil" would be one file under two ids.
    for id in ["evil ", " evil", "evil."] {
        assert!(!ThemeId::is_safe(id), "{id:?}");
    }
}

#[test]
fn characters_a_filename_cannot_hold_are_refused() {
    for id in [
        "theme:stream",
        "theme*",
        "theme?",
        "theme|pipe",
        "theme\"quote",
    ] {
        assert!(!ThemeId::is_safe(id), "{id}");
    }
}

#[test]
fn an_id_long_enough_to_threaten_max_path_is_refused() {
    assert!(!ThemeId::is_safe(&"a".repeat(ThemeId::MAX_LENGTH + 1)));
}

#[test]
fn an_id_exactly_at_the_limit_is_accepted() {
    assert!(ThemeId::is_safe(&"a".repeat(ThemeId::MAX_LENGTH)));
}

#[test]
fn unique_slugs_a_display_name_down_to_letters_digits_and_hyphens() {
    assert_eq!("my-theme", ThemeId::unique("My Theme!", []));
}

#[test]
fn unique_suffixes_until_it_is_free() {
    assert_eq!(
        "my-theme-3",
        ThemeId::unique("My Theme", ["my-theme", "my-theme-2"])
    );
}

#[test]
fn unique_treats_taken_ids_case_insensitively() {
    assert_eq!("my-theme-2", ThemeId::unique("My Theme", ["MY-THEME"]));
}

#[test]
fn unique_always_produces_something_safe() {
    // The C# rows end with "" and null; null is the empty string here.
    for name in [r"C:\Windows\Temp\evil", r"..\..\evil", "!!!", "", "", "con"] {
        assert!(ThemeId::is_safe(&ThemeId::unique(name, [])), "{name:?}");
    }
}

#[test]
fn unique_falls_back_when_a_name_slugs_to_nothing() {
    assert_eq!("theme", ThemeId::unique("!!!", []));
}

#[test]
fn unique_keeps_a_very_long_name_within_the_limit() {
    assert!(ThemeId::unique(&"a".repeat(500), []).len() <= ThemeId::MAX_LENGTH);
}
