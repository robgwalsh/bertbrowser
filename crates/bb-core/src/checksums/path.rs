//! Turning a name listed inside a checksum file into a path on disk, or refusing to. Spec:
//! Core/Services/Checksums/ChecksumPath.cs.
//!
//! **A checksum file is untrusted input.** It arrives beside a download, from whoever made the
//! download, and every line in it is a string that is about to be turned into a path and opened. A
//! line reading `..\..\..\Windows\System32\config\SAM` must produce a visible refusal, not a read —
//! the verify report has a row for it, so a hostile file is something the user can see rather than
//! something that quietly happened. The same instinct as the rule that a virtual archive path must
//! never reach a `PathKey`-keyed table: a string that is syntactically a path is not thereby a path
//! this code may use.

use bb_paths::PathKey;

pub struct ChecksumPath;

impl ChecksumPath {
    /// The full path `name` refers to under `folder`, or None if it must not be resolved at all.
    /// None for a refusal, never a panic — a bad line is one row of a report, and it must not cost
    /// the other three hundred lines their answer.
    pub fn resolve(folder: &str, name: &str) -> Option<String> {
        if folder.is_empty() || name.is_empty() {
            return None;
        }

        // sha256sum files are written on Linux as often as not.
        let replaced = name.replace('/', "\\");
        let relative = replaced.trim();
        if relative.is_empty() {
            return None;
        }

        // Rooted in any of its several spellings: "C:\x", "\x", "\\server\share\x", "C:x". The
        // device and long-path prefixes start with "\\" and sidestep normalisation entirely.
        if relative.starts_with('\\') {
            return None;
        }
        if relative.as_bytes().get(1) == Some(&b':') {
            return None;
        }

        if relative.contains(['<', '>', '"', '|', '?', '*', '\0']) {
            return None;
        }

        if relative
            .split('\\')
            .filter(|s| !s.is_empty())
            .any(|segment| segment == ".." || segment == ".")
        {
            return None;
        }

        // `Path.GetFullPath(Path.Combine(folder, relative))`.
        let joined = if folder.ends_with(['\\', '/']) {
            format!("{folder}{relative}")
        } else {
            format!("{folder}\\{relative}")
        };
        let combined = PathKey::normalize_display(&joined).ok()?;

        // Belt and braces. Everything above is a syntactic rule about the listed name; this is the
        // one check on the answer, and it is the one that would still hold if a spelling of "up
        // one level" were invented tomorrow.
        let key = PathKey::canonicalize(&combined).ok()?;
        let root = PathKey::canonicalize(folder).ok()?;
        if PathKey::is_under_key(&key, &root) {
            Some(combined)
        } else {
            None
        }
    }

    /// How a path under `folder` is written into a checksum file: relative, with forward
    /// slashes, which is what every other tool reading one expects. None for a path that is not
    /// under the folder, or is the folder itself.
    pub fn relativise(folder: &str, full_path: &str) -> Option<String> {
        let root = PathKey::normalize_display(folder).ok()?;
        let full = PathKey::normalize_display(full_path).ok()?;
        let root_key = PathKey::canonicalize(&root).ok()?;
        let full_key = PathKey::canonicalize(&full).ok()?;

        if !PathKey::is_under_key(&full_key, &root_key) {
            return None;
        }

        // The key and the display path have the same character count (the fold is one char to
        // one char), so the remainder is counted in characters rather than bytes.
        let body_chars = root.trim_end_matches('\\').chars().count();
        let relative: String = full.chars().skip(body_chars + 1).collect();
        if relative.is_empty() {
            return None;
        }

        Some(relative.replace('\\', "/"))
    }
}
