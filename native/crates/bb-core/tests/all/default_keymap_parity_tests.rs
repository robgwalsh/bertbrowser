// Port of tests/BertBrowser.Core.Tests/DefaultKeymapParityTests.cs: the shortcuts as they behaved
// when they were thirty KeyBindings and two hand-written key handlers, pinned chord by chord against
// what has focus.
use std::sync::LazyLock;

use bb_core::commands::{FocusKind, FocusState, KeyChord, Keymap, KeymapRules};

static KEYMAP: LazyLock<Keymap> =
    LazyLock::new(|| KeymapRules::resolve(&KeymapRules::catalog_bindables(), None));

fn press(chord: &str, focus: FocusKind, settings: bool) -> Option<String> {
    let mut state = FocusState::new(focus);
    state.settings_open = settings;
    KeymapRules::dispatch(&KEYMAP, &KeyChord::parse(chord), &state)
}

/// Bound on the window, with a chord no text box uses: live wherever focus is.
#[test]
fn window_shortcuts_fire_wherever_focus_is() {
    for (chord, command) in [
        ("Alt+Left", "nav.back"),
        ("Alt+Right", "nav.forward"),
        ("Alt+Up", "nav.up"),
        ("F5", "nav.refresh"),
        ("Ctrl+T", "tab.new"),
        ("Ctrl+W", "tab.close"),
        ("Ctrl+Tab", "tab.next"),
        ("Ctrl+Shift+Tab", "tab.previous"),
        ("Ctrl+1", "tab.goto-1"),
        ("Ctrl+8", "tab.goto-8"),
        ("Ctrl+9", "tab.goto-last"),
        ("Ctrl+Alt+Right", "pane.split-right"),
        ("Ctrl+Alt+Down", "pane.split-below"),
        ("Ctrl+Shift+W", "pane.close"),
        ("F6", "pane.next"),
        ("Shift+F6", "pane.previous"),
        ("Ctrl+Shift+D", "tools.disk-usage"),
        ("Ctrl+Shift+U", "tools.duplicates"),
        ("Ctrl+Shift+H", "tools.changes"),
        ("F7", "tools.compare"),
        ("Ctrl+Shift+T", "tab.reopen"),
        ("Ctrl+P", "view.preview"),
        ("Alt+P", "view.preview"),
        ("Ctrl+M", "view.metadata"),
        ("Alt+M", "view.metadata"),
        ("Ctrl+B", "view.flat"),
        ("Ctrl+F", "search.folder"),
        ("Ctrl+E", "search.folder"),
        ("Ctrl+Shift+F", "search.pc"),
    ] {
        for focus in FocusKind::ALL {
            assert_eq!(
                Some(command.to_string()),
                press(chord, focus, false),
                "{chord} with {focus:?}"
            );
        }
    }
}

/// Backspace was a window binding too, but a text box handled the key first.
#[test]
fn backspace_navigates_except_while_typing() {
    assert_eq!(
        Some("nav.back".to_string()),
        press("Backspace", FocusKind::FileList, false)
    );
    assert_eq!(
        Some("nav.back".to_string()),
        press("Backspace", FocusKind::Other, false)
    );
    assert_eq!(
        Some("nav.back".to_string()),
        press("Backspace", FocusKind::TextReadOnly, false)
    );
    assert_eq!(None, press("Backspace", FocusKind::TextEditable, false));
}

/// Guarded by hand with "focus is not a text box", which the preview's read-only text is too.
#[test]
fn undo_and_redo_stay_out_of_every_text_control() {
    for (chord, command) in [
        ("Ctrl+Z", "edit.undo"),
        ("Ctrl+Y", "edit.redo"),
        ("Ctrl+Shift+Z", "edit.redo"),
        ("Ctrl+Alt+Z", "edit.undo-history"),
    ] {
        assert_eq!(
            Some(command.to_string()),
            press(chord, FocusKind::FileList, false),
            "{chord}"
        );
        assert_eq!(
            Some(command.to_string()),
            press(chord, FocusKind::Other, false),
            "{chord}"
        );
        assert_eq!(
            None,
            press(chord, FocusKind::TextEditable, false),
            "{chord}"
        );
        assert_eq!(
            None,
            press(chord, FocusKind::TextReadOnly, false),
            "{chord}"
        );
    }
}

/// Handled in the tab's own view, and only while its list had the keyboard.
#[test]
fn list_shortcuts_belong_to_the_list() {
    for (chord, command) in [
        ("Enter", "file.open"),
        ("Ctrl+Shift+Enter", "file.run-as-admin"),
        ("F2", "file.rename"),
        ("Del", "file.delete"),
        ("Shift+Del", "file.delete-permanently"),
        ("Ctrl+Shift+N", "file.new-folder"),
        ("Alt+Enter", "file.properties"),
        ("Ctrl+C", "edit.copy"),
        ("Ctrl+X", "edit.cut"),
        ("Ctrl+V", "edit.paste"),
        ("Ctrl+Shift+C", "edit.copy-path"),
    ] {
        assert_eq!(
            Some(command.to_string()),
            press(chord, FocusKind::FileList, false),
            "{chord}"
        );
        assert_eq!(None, press(chord, FocusKind::Other, false), "{chord}");
        assert_eq!(
            None,
            press(chord, FocusKind::TextEditable, false),
            "{chord}"
        );
        assert_eq!(
            None,
            press(chord, FocusKind::TextReadOnly, false),
            "{chord}"
        );
    }
}

/// The settings page cleared the window's bindings and both handlers returned early.
#[test]
fn nothing_about_the_folders_fires_while_settings_is_open() {
    for chord in [
        "Backspace",
        "F5",
        "Ctrl+T",
        "Ctrl+W",
        "Ctrl+Z",
        "Ctrl+F",
        "Ctrl+Shift+F",
        "Alt+P",
        "F7",
        "Del",
    ] {
        for focus in FocusKind::ALL {
            assert_eq!(None, press(chord, focus, true), "{chord} with {focus:?}");
        }
    }
}
