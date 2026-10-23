//! The on-disk format for user themes. Kept in Core so the round trip is unit-tested: this is the
//! only part of theming the user can edit with a text editor. Comments and trailing commas are
//! accepted on the way in, because a hand-written file reasonably has both; colours a future
//! version adds are just extra properties to this one, so unknown members are ignored and a newer
//! theme file stays loadable by an older build. Spec: src/BertBrowser.Core/Theming/ThemeJson.cs.

use super::{ColorMap, ResolvedTheme, ThemeDefinition, ThemeToken};

pub struct ThemeJson;

impl ThemeJson {
    pub fn serialize(definition: &ThemeDefinition) -> String {
        serde_json::to_string_pretty(definition).expect("a theme definition always serializes")
    }

    /// Writes every token out explicitly with no `base`, so an exported theme is portable and
    /// readable rather than a diff against whatever Dark+ happened to be at the time.
    pub fn serialize_resolved(theme: &ResolvedTheme) -> String {
        let colors: ColorMap = ThemeToken::ALL
            .iter()
            .map(|&token| (token.name(), theme[token].to_hex()))
            .collect();
        ThemeJson::serialize(&ThemeDefinition {
            id: theme.id.clone(),
            name: theme.name.clone(),
            base_theme_id: None,
            is_dark: theme.is_dark,
            colors,
            is_built_in: false,
        })
    }

    /// Never panics: malformed JSON is a file the user typed into. The error is the message for
    /// the notice line.
    pub fn try_deserialize(json: &str) -> Result<ThemeDefinition, String> {
        let parsed: Option<ThemeDefinition> =
            serde_json::from_str(&strip_jsonc(json)).map_err(|e| e.to_string())?;
        let Some(parsed) = parsed else {
            return Err("The file is empty.".to_string());
        };
        if parsed.id.trim().is_empty() {
            return Err("The theme has no \"id\".".to_string());
        }
        if parsed.name.trim().is_empty() {
            let name = parsed.id.clone();
            return Ok(ThemeDefinition { name, ..parsed });
        }
        Ok(parsed)
    }
}

/// Removes `//` and `/* */` comments outside strings and the comma before a closing `}` or `]`, so
/// strict JSON parsing accepts what a person writes. Strings are walked escape-aware, so a `//`
/// inside a value is left alone.
fn strip_jsonc(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(bytes.len());
            out.push_str(&text[start..i]);
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
        } else if c == b',' {
            // A trailing comma is one followed only by whitespace and a closing bracket.
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b'}' || bytes[j] == b']') {
                i += 1;
                continue;
            }
            out.push(',');
            i += 1;
        } else {
            // Only ASCII is ever matched above, so a multi-byte character is copied whole.
            let ch = text[i..].chars().next().expect("inside the text");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}
