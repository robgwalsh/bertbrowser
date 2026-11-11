//! The argument template of a custom context-menu command, expanded for one target path.
//! Spec: Core/Services/CommandTemplate.cs.

use super::win_path::directory_name;
use bb_paths::file_name;

/// Tokens (case-insensitive): `{path}` the full path, `{name}` the file name, `{dir}` the parent
/// directory. Substituted verbatim — a template quotes them (`"{path}"`) when paths may contain
/// spaces. A blank template defaults to the quoted full path. A token that sits directly before a
/// closing `"` has its trailing backslashes doubled ([`escape_trailing_backslashes`]), so a drive
/// root does not read as an escaped quote.
pub struct CommandTemplate;

/// A value about to be written inside double quotes in a command line, with its trailing
/// backslashes doubled. `CommandLineToArgvW` reads `\"` as an escaped quote, so `"C:\"` would
/// swallow the quote and whatever follows; `"C:\\"` is the argument `C:\`.
pub fn escape_trailing_backslashes(value: &str) -> String {
    let kept = value.trim_end_matches('\\').len();
    let mut out = String::with_capacity(value.len() + (value.len() - kept));
    out.push_str(value);
    out.push_str(&value[kept..]);
    out
}

/// `text` with each `needle` (ASCII, any case) replaced by `value`, doubling the value's trailing
/// backslashes where the token is immediately followed by a closing quote.
fn substitute(text: &str, needle: &str, value: &str) -> String {
    let folded = text.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let quoted = escape_trailing_backslashes(value);
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    while let Some(i) = folded[from..].find(&needle) {
        let end = from + i + needle.len();
        out.push_str(&text[from..from + i]);
        out.push_str(if text[end..].starts_with('"') {
            &quoted
        } else {
            value
        });
        from = end;
    }
    out.push_str(&text[from..]);
    out
}

impl CommandTemplate {
    pub fn expand(template: Option<&str>, full_path: &str) -> String {
        let template = match template {
            Some(t) if !t.trim().is_empty() => t,
            _ => return format!("\"{}\"", escape_trailing_backslashes(full_path)),
        };

        let directory = directory_name(full_path).unwrap_or_else(|| full_path.to_string());
        let expanded = substitute(template, "{path}", full_path);
        let expanded = substitute(&expanded, "{name}", file_name(full_path));
        substitute(&expanded, "{dir}", &directory)
    }
}
