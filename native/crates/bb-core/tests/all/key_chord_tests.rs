// Port of tests/BertBrowser.Core.Tests/KeyChordTests.cs.
use bb_core::commands::{ChordKeyKind, ChordModifiers, KeyChord};

#[test]
fn text_round_trips_to_one_spelling() {
    for (text, expected) in [
        ("Ctrl+T", "Ctrl+T"),
        ("ctrl+shift+p", "Ctrl+Shift+P"),
        ("Shift+Ctrl+P", "Ctrl+Shift+P"),
        ("Alt+Ctrl+Z", "Ctrl+Alt+Z"),
        (" F7 ", "F7"),
        ("Delete", "Del"),
        ("Shift+delete", "Shift+Del"),
        ("Alt+Return", "Alt+Enter"),
        ("Back", "Backspace"),
        ("Ctrl+PgDn", "Ctrl+PageDown"),
    ] {
        let chord = KeyChord::try_parse(text).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(expected, chord.to_string());
        assert_eq!(chord, KeyChord::parse(&chord.to_string()));
    }
}

#[test]
fn a_key_that_is_itself_punctuation_is_not_mistaken_for_a_separator() {
    for (text, modifiers, key) in [
        ("Ctrl+=", ChordModifiers::CTRL, "="),
        ("Ctrl+,", ChordModifiers::CTRL, ","),
        ("Num+", ChordModifiers::NONE, "Num+"),
        ("Ctrl+Num+", ChordModifiers::CTRL, "Num+"),
        ("Ctrl+Num-", ChordModifiers::CTRL, "Num-"),
        ("Ctrl+-", ChordModifiers::CTRL, "-"),
    ] {
        let chord = KeyChord::parse(text);
        assert_eq!(modifiers, chord.modifiers(), "{text}");
        assert_eq!(key, chord.key(), "{text}");
    }
}

#[test]
fn nonsense_is_not_a_chord() {
    for text in ["", "Ctrl+", "Ctrl", "Win+E", "Ctrl+Banana", "Ctrl++"] {
        assert!(KeyChord::try_parse(text).is_none(), "{text:?}");
    }
}

#[test]
fn each_key_knows_what_it_does_unbound() {
    for (key, kind) in [
        ("P", ChordKeyKind::Printable),
        ("Space", ChordKeyKind::Printable),
        ("F12", ChordKeyKind::Function),
        ("Enter", ChordKeyKind::Editing),
        ("Home", ChordKeyKind::Caret),
        ("Num5", ChordKeyKind::Numpad),
    ] {
        assert_eq!(kind, KeyChord::parse(key).kind(), "{key}");
    }
}

#[test]
fn equal_chords_are_equal_however_they_were_spelled() {
    assert_eq!(
        KeyChord::parse("Ctrl+Shift+Delete"),
        KeyChord::parse("shift+ctrl+del")
    );
    assert_ne!(KeyChord::parse("Ctrl+P"), KeyChord::parse("Ctrl+Shift+P"));
}
