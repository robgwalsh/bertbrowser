//! One entry on the "New" submenu, persisted in settings.json.
//! Spec: Core/Services/NewItem/NewFileTemplate.cs.
//!
//! Plain fields so the file stays hand-editable, and deliberately only two shapes: a template is
//! either empty or a file on disk. Windows' ShellNew has a third — bytes held in the registry — but
//! those are written out to a real file once, at import, so nothing downstream has to know.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NewFileTemplate {
    /// What the menu says, e.g. "Text Document".
    pub label: String,
    /// Including the dot, e.g. ".txt".
    pub extension: String,
    /// A file to copy the new file's contents from, or None for an empty one.
    pub template_path: Option<String>,
    /// Whether it appears on the menu. True is also what an entry written before this existed
    /// deserializes to.
    pub enabled: bool,
}

impl Default for NewFileTemplate {
    fn default() -> Self {
        NewFileTemplate {
            label: String::new(),
            extension: String::new(),
            template_path: None,
            enabled: true,
        }
    }
}

impl NewFileTemplate {
    pub fn new(label: &str, extension: &str) -> NewFileTemplate {
        NewFileTemplate {
            label: label.to_string(),
            extension: extension.to_string(),
            ..Default::default()
        }
    }

    /// The name the dialog opens with, before its extension: "New Text Document".
    pub fn default_base_name(&self) -> String {
        format!("New {}", self.label)
    }

    /// What ships when the user has never configured the list.
    pub fn defaults() -> Vec<NewFileTemplate> {
        vec![
            NewFileTemplate::new("Text Document", ".txt"),
            NewFileTemplate::new("Markdown Document", ".md"),
            NewFileTemplate::new("JSON File", ".json"),
        ]
    }
}
