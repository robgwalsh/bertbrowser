// Port of tests/BertBrowser.Core.Tests/FileComparePlanTests.cs
use bb_core::compare::{FileCompareMode, FileComparePlan, FileCompareSide};

fn side(name: &str) -> FileCompareSide {
    FileCompareSide::new(name, false)
}

fn text_side(name: &str, looks_like_text: bool) -> FileCompareSide {
    FileCompareSide::new(name, looks_like_text)
}

fn mode(left: &FileCompareSide, right: &FileCompareSide) -> FileCompareMode {
    FileComparePlan::mode_for(left, right, false)
}

#[test]
fn two_text_extensions_are_text() {
    assert_eq!(FileCompareMode::Text, mode(&side("a.cs"), &side("b.cs")));
}

#[test]
fn two_binaries_are_binary() {
    assert_eq!(
        FileCompareMode::Binary,
        mode(&side("a.exe"), &side("b.exe"))
    );
}

/// Both sides, not either. Diffing a .txt against a .exe as text would render one side as
/// mojibake and invite a conclusion about it.
#[test]
fn a_mixed_pair_is_binary() {
    assert_eq!(
        FileCompareMode::Binary,
        mode(&side("notes.txt"), &side("setup.exe"))
    );
    assert_eq!(
        FileCompareMode::Binary,
        mode(&side("setup.exe"), &side("notes.txt"))
    );
}

/// The second rung: an extensionless README or a Dockerfile has no extension to trust, so what
/// was actually decoded decides.
#[test]
fn two_convincingly_textual_unknowns_are_text() {
    assert_eq!(
        FileCompareMode::Text,
        mode(&text_side("README", true), &text_side("LICENCE", true))
    );
}

#[test]
fn one_unconvincing_unknown_makes_the_pair_binary() {
    assert_eq!(
        FileCompareMode::Binary,
        mode(&text_side("README", true), &text_side("blob", false))
    );
}

/// The extension is trusted over the content, so an empty .cs or one full of unusual characters
/// is still code, the same call the preview pane makes.
#[test]
fn a_known_text_extension_is_text_even_when_it_did_not_convince() {
    assert_eq!(
        FileCompareMode::Text,
        mode(&text_side("a.cs", false), &text_side("b.cs", false))
    );
}

/// "Show as text anyway": nobody is stuck with the automatic answer.
#[test]
fn forcing_text_wins() {
    assert_eq!(
        FileCompareMode::Text,
        FileComparePlan::mode_for(&side("a.exe"), &side("b.exe"), true)
    );
}

/// The preview classifier calls an extensionless file text, because for a pane a wrong guess is
/// one panel of mojibake to dismiss. Here a wrong guess is two panels with differences marked
/// between them, which reads as a finding, so a name with no extension has to earn it from what
/// was decoded.
#[test]
fn an_extensionless_name_does_not_count_as_text_on_its_own() {
    assert_eq!(FileCompareMode::Binary, mode(&side("blob"), &side("blob2")));
}
