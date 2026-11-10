// How a saved custom command reads back: missing fields take the safe defaults, and the file is camelCase.
use bb_core::custom_commands::CustomCommandDefinition;

#[test]
fn a_command_saved_without_an_id_or_the_newer_switches_reads_as_the_safe_defaults() {
    let read: CustomCommandDefinition =
        serde_json::from_str(r#"{"name":"Open","command":"x.exe"}"#).unwrap();
    assert_eq!(32, read.id.len());
    assert!(read.id.bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(read.show_in_menu);
    assert!(read.applies_to_files);
    assert!(!read.applies_to_directories);
    assert!(!read.run_elevated);
    assert_ne!(read.id, CustomCommandDefinition::default().id);
}

#[test]
fn a_definition_round_trips_in_camel_case() {
    let original = CustomCommandDefinition {
        name: "Diff".into(),
        arguments: "{path}".into(),
        run_elevated: true,
        ..Default::default()
    };
    let json = serde_json::to_string(&original).unwrap();
    assert!(json.contains("\"showInMenu\":true"));
    assert!(json.contains("\"runElevated\":true"));
    let back: CustomCommandDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(original, back);
}
