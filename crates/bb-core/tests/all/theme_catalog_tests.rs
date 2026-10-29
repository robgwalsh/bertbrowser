// Port of tests/BertBrowser.Core.Tests/Theming/ThemeCatalogTests.cs.
//
// Guards the themes we ship. These are the invariants to mutate when changing this code: delete a
// token from Dark+ and `root_themes_define_every_token` goes red; set `Theme.Text.Secondary` to
// something near the background and the contrast suite goes red.
use std::collections::HashSet;

use bb_core::theming::{
    ResolvedTheme, ThemeCatalog, ThemeColor, ThemeContrast, ThemeDefinition, ThemeResolver,
    ThemeToken,
};

fn resolve(id: &str) -> ResolvedTheme {
    ThemeResolver::resolve(ThemeCatalog::find(id).unwrap(), ThemeCatalog::find)
}

fn built_ins() -> impl Iterator<Item = &'static ThemeDefinition> {
    ThemeCatalog::built_ins().iter()
}

fn assert_contrast(
    theme: &ResolvedTheme,
    foreground: ThemeToken,
    background: ThemeToken,
    minimum: f64,
) {
    let ratio = ThemeContrast::ratio_of(theme, foreground, background);
    assert!(
        ratio >= minimum,
        "{}: {} on {} is {ratio:.2}:1, below the {minimum:.1}:1 minimum",
        theme.id,
        foreground.name(),
        background.name()
    );
}

#[test]
fn root_themes_define_every_token() {
    for theme in ThemeCatalog::roots() {
        let missing: Vec<&str> = ThemeToken::ALL
            .iter()
            .map(|t| t.name())
            .filter(|key| !theme.colors.contains_key(key))
            .collect();
        assert!(missing.is_empty(), "{}: {missing:?}", theme.id);
    }
}

#[test]
fn root_themes_have_no_base() {
    for theme in ThemeCatalog::roots() {
        assert_eq!(None, theme.base_theme_id, "{}", theme.id);
    }
}

#[test]
fn built_in_colours_all_parse() {
    for theme in built_ins() {
        let unparseable: Vec<String> = theme
            .colors
            .iter()
            .filter(|(_, literal)| ThemeColor::try_parse(literal).is_none())
            .map(|(key, literal)| format!("{key} = {literal}"))
            .collect();
        assert!(unparseable.is_empty(), "{}: {unparseable:?}", theme.id);
    }
}

#[test]
fn built_in_themes_only_use_known_tokens() {
    for theme in built_ins() {
        let unknown: Vec<&str> = theme
            .colors
            .keys()
            .filter(|key| !ThemeToken::is_known(key))
            .collect();
        assert!(unknown.is_empty(), "{}: {unknown:?}", theme.id);
    }
}

#[test]
fn built_in_themes_resolve_cleanly_and_completely() {
    for theme in built_ins() {
        let resolved = resolve(&theme.id);

        assert!(
            resolved.issues.is_empty(),
            "{}: {:?}",
            theme.id,
            resolved.issues
        );
        for token in ThemeToken::ALL {
            assert!(
                resolved.get(token.name()).is_some(),
                "{} is missing {}",
                theme.id,
                token.name()
            );
        }
    }
}

#[test]
fn built_in_ids_and_names_are_unique() {
    let count = ThemeCatalog::built_ins().len();
    let ids: HashSet<String> = built_ins().map(|t| t.id.to_lowercase()).collect();
    let names: HashSet<String> = built_ins().map(|t| t.name.to_lowercase()).collect();
    assert_eq!(count, ids.len());
    assert_eq!(count, names.len());
}

#[test]
fn is_dark_matches_the_window_background() {
    for theme in built_ins() {
        let resolved = resolve(&theme.id);
        let luminance = resolved[ThemeToken::WindowBackground].relative_luminance();

        assert_eq!(resolved.is_dark, luminance < 0.5, "{}", theme.id);
    }
}

#[test]
fn body_text_is_readable_on_every_surface() {
    for theme in built_ins() {
        let theme = resolve(&theme.id);
        let aa = ThemeContrast::AA_NORMAL_TEXT;

        assert_contrast(
            &theme,
            ThemeToken::TextPrimary,
            ThemeToken::WindowBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::TextPrimary,
            ThemeToken::SurfaceBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::MenuForeground,
            ThemeToken::MenuBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::InputForeground,
            ThemeToken::InputBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::TitleBarForeground,
            ThemeToken::TitleBarBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::ToolbarForeground,
            ThemeToken::ToolbarBackground,
            aa,
        );
    }
}

#[test]
fn text_on_a_coloured_background_is_readable() {
    for theme in built_ins() {
        let theme = resolve(&theme.id);
        let aa = ThemeContrast::AA_NORMAL_TEXT;

        assert_contrast(
            &theme,
            ThemeToken::ListSelectedForeground,
            ThemeToken::ListSelectedBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::ListSelectedInactiveForeground,
            ThemeToken::ListSelectedInactiveBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::StatusBarForeground,
            ThemeToken::StatusBarBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::MenuHoverForeground,
            ThemeToken::MenuHoverBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::AccentForeground,
            ThemeToken::AccentBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::TabActiveForeground,
            ThemeToken::TabActiveBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::InputSelectionForeground,
            ThemeToken::InputSelectionBackground,
            aa,
        );
    }
}

#[test]
fn incidental_text_clears_the_large_text_bar() {
    for theme in built_ins() {
        let theme = resolve(&theme.id);
        let large = ThemeContrast::AA_LARGE_TEXT;

        assert_contrast(
            &theme,
            ThemeToken::TextSecondary,
            ThemeToken::WindowBackground,
            large,
        );
        assert_contrast(
            &theme,
            ThemeToken::TextMuted,
            ThemeToken::SurfaceBackground,
            large,
        );
        assert_contrast(
            &theme,
            ThemeToken::TextPlaceholder,
            ThemeToken::InputBackground,
            large,
        );
        assert_contrast(
            &theme,
            ThemeToken::TabInactiveForeground,
            ThemeToken::TabInactiveBackground,
            large,
        );
        assert_contrast(
            &theme,
            ThemeToken::ListHeaderForeground,
            ThemeToken::ListHeaderBackground,
            large,
        );
        assert_contrast(
            &theme,
            ThemeToken::MenuGestureForeground,
            ThemeToken::MenuBackground,
            large,
        );
        assert_contrast(
            &theme,
            ThemeToken::TreeChevronForeground,
            ThemeToken::SurfaceBackground,
            large,
        );
    }
}

/// Code in the preview pane is read, not glanced at. Only the two roots define these colours and
/// every other built-in inherits them, so this is where a palette that reads on Dark+ and
/// disappears on a theme sitting two shades lighter gets caught.
#[test]
fn code_stays_readable_in_the_preview_pane() {
    for theme in built_ins() {
        let theme = resolve(&theme.id);
        let aa = ThemeContrast::AA_NORMAL_TEXT;

        assert_contrast(
            &theme,
            ThemeToken::SyntaxKeyword,
            ThemeToken::WindowBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::SyntaxString,
            ThemeToken::WindowBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::SyntaxNumber,
            ThemeToken::WindowBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::SyntaxPunctuation,
            ThemeToken::WindowBackground,
            aa,
        );
        // Comments are meant to recede, so they answer to the large-text bar rather than vanish
        // under a rule written for body text.
        assert_contrast(
            &theme,
            ThemeToken::SyntaxComment,
            ThemeToken::WindowBackground,
            ThemeContrast::AA_LARGE_TEXT,
        );
    }
}

/// A row must not lose its text the moment it is hovered or selected.
#[test]
fn row_text_survives_hover() {
    for theme in built_ins() {
        let theme = resolve(&theme.id);
        let aa = ThemeContrast::AA_NORMAL_TEXT;
        assert_contrast(
            &theme,
            ThemeToken::TextPrimary,
            ThemeToken::ListHoverBackground,
            aa,
        );
        assert_contrast(
            &theme,
            ThemeToken::ListSelectedForeground,
            ThemeToken::ListSelectedHoverBackground,
            aa,
        );
    }
}

/// The scrollbar thumb is translucent by design, so what matters is how it reads once composited
/// over the surface behind it.
#[test]
fn the_scrollbar_thumb_is_visible_against_the_list() {
    for theme in built_ins() {
        let resolved = resolve(&theme.id);
        let ratio = ThemeContrast::ratio(
            resolved[ThemeToken::ScrollBarThumb],
            resolved[ThemeToken::ListBackground],
        );
        assert!(
            ratio >= 1.4,
            "{}: scrollbar thumb is only {ratio:.2}:1 against the list",
            theme.id
        );
    }
}

const COMPARE_TINTS: [ThemeToken; 5] = [
    ThemeToken::CompareOnlyHere,
    ThemeToken::CompareNewer,
    ThemeToken::CompareOlder,
    ThemeToken::CompareDiffers,
    ThemeToken::CompareUnknown,
];

/// A row tinted by a comparison is still a row you have to read. The tints are translucent by
/// design, so unlike every other pair here the background is not a token but the composite of one
/// over another, which is why this does its own compositing rather than going through
/// `assert_contrast`. Make one of them opaque, or give it an alpha heavy enough to swallow the
/// text, and this is what says so.
#[test]
fn text_on_a_compare_tinted_row_is_readable() {
    for theme in built_ins() {
        let theme = resolve(&theme.id);
        let background = theme[ThemeToken::ListBackground];

        for token in COMPARE_TINTS {
            let tint = theme[token].composite_over(background);
            let ratio = ThemeContrast::ratio(theme[ThemeToken::TextPrimary], tint);
            assert!(
                ratio >= ThemeContrast::AA_NORMAL_TEXT,
                "{}: text on {} over the list background is {ratio:.2}:1, below the {:.1}:1 minimum",
                theme.id,
                token.name(),
                ThemeContrast::AA_NORMAL_TEXT
            );
        }
    }
}
