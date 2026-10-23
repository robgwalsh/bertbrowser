// Port of tests/BertBrowser.Core.Tests/KeymapRulesTests.cs.
use bb_core::commands::{
    Bindable, CommandContext, FocusKind, FocusState, KeyChord, Keymap, KeymapRules, Overrides,
};

fn three() -> Vec<Bindable> {
    vec![
        Bindable::new("a.one", CommandContext::Browser, &["Ctrl+1"]),
        Bindable::new("b.two", CommandContext::Browser, &["Ctrl+2", "F2"]),
        Bindable::new("c.three", CommandContext::FileList, &[]),
    ]
}

fn overrides(entries: &[(&str, &[&str])]) -> Overrides {
    entries
        .iter()
        .map(|(id, chords)| {
            (
                id.to_string(),
                chords.iter().map(|c| c.to_string()).collect(),
            )
        })
        .collect()
}

fn chords(keymap: &Keymap, id: &str) -> String {
    keymap
        .chords_for(id)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

fn chord(text: &str) -> KeyChord {
    KeyChord::parse(text)
}

// --- Resolve ---

#[test]
fn nothing_overridden_is_the_defaults() {
    let keymap = KeymapRules::resolve(&three(), None);
    assert_eq!("Ctrl+1", chords(&keymap, "a.one"));
    assert_eq!("Ctrl+2 F2", chords(&keymap, "b.two"));
    assert_eq!("", chords(&keymap, "c.three"));
    assert_eq!(Some("b.two"), keymap.owner_of(&chord("F2")));
    assert_eq!("Ctrl+2", keymap.gesture_text("b.two"));
    assert_eq!("", keymap.gesture_text("c.three"));
}

#[test]
fn an_override_replaces_a_commands_chords_outright() {
    let keymap = KeymapRules::resolve(&three(), Some(&overrides(&[("b.two", &["Ctrl+K"])])));
    assert_eq!("Ctrl+K", chords(&keymap, "b.two"));
    assert_eq!(None, keymap.owner_of(&chord("F2")));
}

#[test]
fn an_empty_override_takes_the_default_away() {
    let keymap = KeymapRules::resolve(&three(), Some(&overrides(&[("a.one", &[])])));
    assert_eq!("", chords(&keymap, "a.one"));
    assert_eq!(None, keymap.owner_of(&chord("Ctrl+1")));
}

#[test]
fn the_users_choice_beats_another_commands_default() {
    // c.three comes after a.one in the catalogue, and still wins: it was chosen.
    let keymap = KeymapRules::resolve(&three(), Some(&overrides(&[("c.three", &["Ctrl+1"])])));
    assert_eq!(Some("c.three"), keymap.owner_of(&chord("Ctrl+1")));
    assert_eq!("", chords(&keymap, "a.one"));
}

#[test]
fn two_overrides_claiming_one_chord_go_to_the_earlier_command() {
    let keymap = KeymapRules::resolve(
        &three(),
        Some(&overrides(&[
            ("b.two", &["Ctrl+K"]),
            ("a.one", &["Ctrl+K"]),
        ])),
    );
    assert_eq!(Some("a.one"), keymap.owner_of(&chord("Ctrl+K")));
    assert_eq!("", chords(&keymap, "b.two"));
}

#[test]
fn a_saved_chord_that_is_unreadable_or_refused_is_dropped_not_fatal() {
    let keymap = KeymapRules::resolve(
        &three(),
        Some(&overrides(&[("a.one", &["Ctrl+Banana", "Q", "Ctrl+Q"])])),
    );
    assert_eq!("Ctrl+Q", chords(&keymap, "a.one"));
}

#[test]
fn an_override_for_a_command_nobody_knows_is_ignored() {
    let keymap = KeymapRules::resolve(
        &three(),
        Some(&overrides(&[("from.elsewhere", &["Ctrl+1"])])),
    );
    assert_eq!(Some("a.one"), keymap.owner_of(&chord("Ctrl+1")));
    assert_eq!(None, keymap.context_of("from.elsewhere"));
}

// --- with_chords / with_defaults ---

#[test]
fn giving_a_chord_takes_it_from_whoever_had_it() {
    let overrides = KeymapRules::with_chords(&three(), None, "c.three", &[chord("F2")]);
    let keymap = KeymapRules::resolve(&three(), Some(&overrides));
    assert_eq!("F2", chords(&keymap, "c.three"));
    assert_eq!("Ctrl+2", chords(&keymap, "b.two"));
    assert_eq!(vec!["Ctrl+2".to_string()], overrides["b.two"]);
}

#[test]
fn setting_a_command_back_to_what_it_ships_with_stores_nothing() {
    let changed = KeymapRules::with_chords(&three(), None, "a.one", &[chord("Ctrl+K")]);
    let back = KeymapRules::with_chords(&three(), Some(&changed), "a.one", &[chord("Ctrl+1")]);
    assert!(back.is_empty());
}

#[test]
fn an_unknown_override_survives_an_edit_to_something_else() {
    let before = overrides(&[("custom:gone", &["Ctrl+G"])]);
    let after = KeymapRules::with_chords(&three(), Some(&before), "a.one", &[]);
    assert_eq!(vec!["Ctrl+G".to_string()], after["custom:gone"]);
    assert!(after["a.one"].is_empty());
}

#[test]
fn reset_removes_the_override() {
    let changed = KeymapRules::with_chords(&three(), None, "a.one", &[]);
    assert!(KeymapRules::with_defaults(&three(), Some(&changed), "a.one").is_empty());
    assert!(KeymapRules::with_defaults(&three(), None, "a.one").is_empty());
}

#[test]
fn reset_takes_a_default_back_from_the_command_it_was_given_to() {
    // c.three was given b.two's F2. Resetting b.two has to bring F2 home.
    let given = KeymapRules::with_chords(&three(), None, "c.three", &[chord("F2")]);
    let reset = KeymapRules::resolve(
        &three(),
        Some(&KeymapRules::with_defaults(&three(), Some(&given), "b.two")),
    );
    assert_eq!("Ctrl+2 F2", chords(&reset, "b.two"));
    assert_eq!("", chords(&reset, "c.three"));
}

#[test]
fn two_commands_sharing_an_id_cost_one_its_shortcut_not_the_app_its_startup() {
    let doubled = vec![
        Bindable::new("custom:x", CommandContext::Browser, &["Ctrl+1"]),
        Bindable::new("custom:x", CommandContext::Browser, &["Ctrl+2"]),
    ];
    assert_eq!(
        "Ctrl+1",
        chords(&KeymapRules::resolve(&doubled, None), "custom:x")
    );
}

#[test]
fn only_stepping_commands_repeat_while_the_key_is_held() {
    for (id, repeats) in [
        ("tab.next", true),
        ("view.thumbs-larger", true),
        ("tab.close", false),
        ("pane.copy-to-other", false),
        ("app.palette", false),
        ("app.exit", false),
        ("edit.undo", false),
    ] {
        assert_eq!(repeats, KeymapRules::repeats_when_held(id), "{id}");
    }
}

// --- Refuse ---

#[test]
fn keys_that_already_mean_something_cannot_be_bound() {
    for (text, context) in [
        ("P", CommandContext::Browser),
        ("Shift+P", CommandContext::FileList),
        ("Space", CommandContext::FileList),
        ("Down", CommandContext::FileList),
        ("Shift+Home", CommandContext::Browser),
        ("Tab", CommandContext::FileList),
        ("Esc", CommandContext::App),
        ("Enter", CommandContext::Browser),
        ("Del", CommandContext::App),
        ("Num+", CommandContext::Browser),
        ("Alt+F4", CommandContext::App),
        ("Alt+Space", CommandContext::Browser),
    ] {
        assert!(
            KeymapRules::refuse(&chord(text), context).is_some(),
            "{text}"
        );
    }
}

#[test]
fn everything_else_can() {
    for (text, context) in [
        ("F5", CommandContext::Browser),
        ("Shift+F6", CommandContext::Browser),
        ("Backspace", CommandContext::Browser),
        ("Enter", CommandContext::FileList),
        ("Shift+Del", CommandContext::FileList),
        ("Num+", CommandContext::FileList),
        ("Ctrl+P", CommandContext::Browser),
        ("Alt+Left", CommandContext::Browser),
        ("Ctrl+Alt+Down", CommandContext::Browser),
        ("Ctrl+,", CommandContext::App),
    ] {
        assert!(
            KeymapRules::refuse(&chord(text), context).is_none(),
            "{text}"
        );
    }
}

// --- Typing ---

#[test]
fn a_text_box_keeps_the_keys_it_uses() {
    for text in [
        "Backspace",
        "Enter",
        "Shift+Del",
        "Ctrl+A",
        "Ctrl+C",
        "Ctrl+V",
        "Ctrl+Z",
        "Ctrl+Y",
        "Ctrl+Shift+Z",
        "Ctrl+Left",
        "Ctrl+Shift+End",
        "Ctrl+Backspace",
        "Ctrl+Alt+Z",
        "Ctrl+Alt+Shift+2",
    ] {
        assert!(!KeymapRules::fires_in_editable_text(&chord(text)), "{text}");
    }
}

#[test]
fn and_gives_up_the_ones_it_does_not() {
    for text in [
        "F5",
        "Shift+F6",
        "Ctrl+T",
        "Ctrl+Shift+P",
        "Ctrl+Tab",
        "Alt+Left",
        "Alt+P",
        "Ctrl+Alt+Right",
        "Ctrl+F",
    ] {
        assert!(KeymapRules::fires_in_editable_text(&chord(text)), "{text}");
    }
}

#[test]
fn read_only_text_keeps_selection_copying_and_the_caret() {
    for (text, fires) in [
        ("Backspace", true),
        ("F5", true),
        ("Ctrl+T", true),
        ("Alt+Left", true),
        ("Ctrl+C", false),
        ("Ctrl+A", false),
        ("Ctrl+Z", false),
        ("Ctrl+Alt+Z", false),
        ("Down", false),
        ("Ctrl+Home", false),
    ] {
        assert_eq!(
            fires,
            KeymapRules::fires_in_read_only_text(&chord(text)),
            "{text}"
        );
    }
}

// --- Dispatch ---

fn contexts() -> Vec<Bindable> {
    vec![
        Bindable::new("app.cmd", CommandContext::App, &["Ctrl+Shift+P"]),
        Bindable::new("browser.cmd", CommandContext::Browser, &["Ctrl+T"]),
        Bindable::new("list.cmd", CommandContext::FileList, &["Del"]),
    ]
}

fn press(text: &str, state: FocusState) -> Option<String> {
    KeymapRules::dispatch(
        &KeymapRules::resolve(&contexts(), None),
        &chord(text),
        &state,
    )
}

fn state(focus: FocusKind) -> FocusState {
    FocusState::new(focus)
}

#[test]
fn alt_opens_the_menu_bar_except_where_that_would_take_the_keyboard_from_something() {
    assert!(KeymapRules::allows_menu_mode(&state(FocusKind::FileList)));
    assert!(KeymapRules::allows_menu_mode(&state(
        FocusKind::TextEditable
    )));
    // The menu bar is the settings page's own preview, so it stays reachable under it.
    let mut settings = state(FocusKind::Other);
    settings.settings_open = true;
    assert!(KeymapRules::allows_menu_mode(&settings));

    let mut blocked = state(FocusKind::Other);
    blocked.content_blocked = true;
    assert!(!KeymapRules::allows_menu_mode(&blocked));
    let mut recording = state(FocusKind::Other);
    recording.recording = true;
    assert!(!KeymapRules::allows_menu_mode(&recording));
    let mut palette = state(FocusKind::TextEditable);
    palette.palette_open = true;
    assert!(!KeymapRules::allows_menu_mode(&palette));
}

#[test]
fn an_unbound_chord_is_nobodys() {
    assert_eq!(None, press("Ctrl+K", state(FocusKind::Other)));
}

#[test]
fn a_list_command_needs_the_list_to_have_the_keyboard() {
    assert_eq!(
        Some("list.cmd".to_string()),
        press("Del", state(FocusKind::FileList))
    );
    assert_eq!(None, press("Del", state(FocusKind::Other)));
    assert_eq!(None, press("Del", state(FocusKind::TextEditable)));
}

#[test]
fn the_settings_page_silences_everything_about_the_folders() {
    let mut settings = state(FocusKind::Other);
    settings.settings_open = true;
    let mut list = state(FocusKind::FileList);
    list.settings_open = true;
    assert_eq!(None, press("Ctrl+T", settings));
    assert_eq!(None, press("Del", list));
    assert_eq!(Some("app.cmd".to_string()), press("Ctrl+Shift+P", settings));
}

#[test]
fn the_open_palette_leaves_only_what_works_from_anywhere() {
    let mut palette = state(FocusKind::TextEditable);
    palette.palette_open = true;
    assert_eq!(None, press("Ctrl+T", palette));
    assert_eq!(Some("app.cmd".to_string()), press("Ctrl+Shift+P", palette));
}

#[test]
fn nothing_fires_behind_a_dialog_or_into_the_recorder() {
    let mut blocked = state(FocusKind::Other);
    blocked.content_blocked = true;
    assert_eq!(None, press("Ctrl+T", blocked));
    let mut recording = state(FocusKind::Other);
    recording.recording = true;
    assert_eq!(None, press("Ctrl+Shift+P", recording));
}

#[test]
fn a_menu_keeps_its_own_keys() {
    // A context menu opened on the file list: Del or Enter must still be the menu's.
    let mut list = state(FocusKind::FileList);
    list.in_popup = true;
    let mut other = state(FocusKind::Other);
    other.in_popup = true;
    assert_eq!(None, press("Del", list));
    assert_eq!(None, press("Ctrl+T", other));
    assert_eq!(None, press("Ctrl+Shift+P", other));
}
