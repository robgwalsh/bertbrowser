//! The argument template of a custom context-menu command, expanded for one target path.
//! Spec: Core/Services/CommandTemplate.cs.

use super::win_path::{directory_name, replace_ignore_ascii_case};
use bb_paths::file_name;

/// Tokens (case-insensitive): `{path}` the full path, `{name}` the file name, `{dir}` the parent
/// directory. Substituted verbatim — a template quotes them (`"{path}"`) when paths may contain
/// spaces. A blank template defaults to the quoted full path.
pub struct CommandTemplate;

impl CommandTemplate {
    pub fn expand(template: Option<&str>, full_path: &str) -> String {
        let template = match template {
            Some(t) if !t.trim().is_empty() => t,
            _ => return format!("\"{full_path}\""),
        };

        let directory = directory_name(full_path).unwrap_or_else(|| full_path.to_string());
        let expanded = replace_ignore_ascii_case(template, "{path}", full_path);
        let expanded = replace_ignore_ascii_case(&expanded, "{name}", file_name(full_path));
        replace_ignore_ascii_case(&expanded, "{dir}", &directory)
    }
}
