//! A user-defined context menu entry, persisted in settings.json. Plain data: what it runs and for
//! what; the argument template is interpreted by `CommandTemplate`, which only holds the string
//! here.
//! Spec: src/BertBrowser.App/Services/CustomCommandDefinition.cs.

use serde::{Deserialize, Serialize};

use crate::shellmenu::Guid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomCommandDefinition {
    /// What the context-menu layout places this command by (`cmd:<id>`). A command saved before
    /// ids existed gets a fresh one on every load until the Settings page next saves it, and
    /// meanwhile sits where unplaced commands go — which is where every command sat before the
    /// menu could be arranged.
    pub id: String,
    /// Whether the command is on the menu. Off means the user took it off the menu without
    /// deleting it.
    pub show_in_menu: bool,
    pub name: String,
    /// Program to run (full path or anything resolvable by the shell).
    pub command: String,
    /// Argument template; see `CommandTemplate`.
    pub arguments: String,
    pub applies_to_files: bool,
    pub applies_to_directories: bool,
    /// Run this command as administrator, with a UAC prompt each time. Off by default, which is
    /// also what an older settings file without this property reads as — so commands configured
    /// before this existed keep running as the ordinary user, which is the safe way round.
    pub run_elevated: bool,
}

impl CustomCommandDefinition {
    /// A fresh id: 32 lowercase hex digits.
    pub fn new_id() -> String {
        Guid::random().to_simple()
    }
}

impl Default for CustomCommandDefinition {
    fn default() -> Self {
        CustomCommandDefinition {
            id: Self::new_id(),
            show_in_menu: true,
            name: String::new(),
            command: String::new(),
            arguments: String::new(),
            applies_to_files: true,
            applies_to_directories: false,
            run_elevated: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CustomCommandDefinition;

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
}
