//! The few phrases every record's description is built from.
//! Spec: Core/Services/UndoHistory/UndoText.cs.

use bb_paths::file_name;

use crate::format::thousands;
use crate::rename::paths::trim_ending_separator;

pub struct UndoText;

impl UndoText {
    pub fn items(count: usize) -> String {
        if count == 1 {
            "1 item".to_string()
        } else {
            format!("{} items", thousands(count as i64))
        }
    }

    /// A folder by its own name, or by its whole path when it is a drive root.
    pub fn folder_name(path: &str) -> String {
        let name = file_name(trim_ending_separator(path));
        if name.is_empty() {
            path.to_string()
        } else {
            name.to_string()
        }
    }

    /// One item by name in quotes, several by count.
    pub fn named(paths: &[String]) -> String {
        if paths.len() == 1 {
            format!("'{}'", file_name(trim_ending_separator(&paths[0])))
        } else {
            Self::items(paths.len())
        }
    }

    /// "Move 3 items to Documents" as the object of "undo": its first letter lowered, so it reads
    /// as one sentence in the status bar.
    pub fn as_object(description: &str) -> String {
        let mut chars = description.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => {
                let mut out: String = first.to_lowercase().collect();
                out.push_str(chars.as_str());
                out
            }
        }
    }
}
