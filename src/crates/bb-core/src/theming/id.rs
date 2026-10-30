//! A theme's id is also its filename, so it is the one piece of a theme that can reach outside the
//! themes folder. An imported `*.json` is untrusted input (its `"id"` is whatever the author typed),
//! so an id of `C:\Windows\Temp\evil` or `..\..\..\something` would otherwise become a write
//! anywhere the user can write rather than a file in the themes folder: a path join discards the
//! first path entirely when the second is rooted. Deliberately not "clean the string up and carry
//! on": a path that has to be repaired is a path that was never a theme id. `is_safe` decides,
//! `unique` manufactures, and nothing else may build one.
//! Spec: src/BertBrowser.Core/Theming/ThemeId.cs.

use super::ids_equal;

pub struct ThemeId;

/// Names Windows resolves to a device no matter the extension, so `con.json` opens the console
/// rather than a file.
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

impl ThemeId {
    /// Long enough for any real name, short enough to never approach MAX_PATH.
    pub const MAX_LENGTH: usize = 64;

    /// Whether `id` is usable as a bare filename stem inside the themes folder: one path segment,
    /// no traversal, no device name, nothing Windows silently rewrites.
    pub fn is_safe(id: &str) -> bool {
        if id.trim().is_empty() {
            return false;
        }
        if id.chars().count() > ThemeId::MAX_LENGTH {
            return false;
        }

        // Named explicitly rather than left to the platform, so the rule cannot soften anywhere.
        if id
            .chars()
            .any(|c| r#"\/:*?"<>|"#.contains(c) || c.is_control())
        {
            return false;
        }

        // "." and ".." carry no invalid characters but are not names.
        if id.trim_matches('.').is_empty() {
            return false;
        }

        // Windows strips trailing dots and spaces, so "evil " and "evil" are the same file while
        // comparing as different ids: the gap where an overwrite hides.
        if id.starts_with(' ') || id.ends_with(' ') || id.ends_with('.') {
            return false;
        }

        !RESERVED_NAMES.iter().any(|r| r.eq_ignore_ascii_case(id))
    }

    /// Turns a display name into a safe id, then suffixes it until it is unique among `taken`, so
    /// "My Theme" and "my theme!" can coexist.
    pub fn unique<'a>(name: &str, taken: impl IntoIterator<Item = &'a str>) -> String {
        let mut slug = String::new();
        for c in name.trim().to_lowercase().chars() {
            if c.is_alphanumeric() {
                slug.push(c);
            } else if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
        }

        let mut basis = slug.trim_matches('-').to_string();
        if basis.chars().count() > ThemeId::MAX_LENGTH {
            basis = basis
                .chars()
                .take(ThemeId::MAX_LENGTH)
                .collect::<String>()
                .trim_matches('-')
                .to_string();
        }

        // A name that slugs to nothing, to a device name, or to a run of digits long enough to
        // trip the length rule all land here; the fallback is what keeps this total.
        if !ThemeId::is_safe(&basis) {
            basis = "theme".to_string();
        }

        let used: Vec<&str> = taken.into_iter().collect();
        let is_used = |candidate: &str| used.iter().any(|t| ids_equal(t, candidate));
        if !is_used(&basis) {
            return basis;
        }

        (2..)
            .map(|i| format!("{basis}-{i}"))
            .find(|candidate| !is_used(candidate))
            .expect("the suffix sequence is unbounded")
    }
}
