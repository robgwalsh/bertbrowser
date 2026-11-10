//! `System.IO.Path` on strings, as the planners used it. `bb_paths` holds the key rules; these are
//! the handful of string questions a planner asks about a path that is not yet a key, and the two
//! existence tests every real probe answers (`Directory.Exists` / `File.Exists`: the entry's own
//! attributes, so a junction is a directory and a broken one still exists).

use std::os::windows::fs::MetadataExt;

use bb_paths::root_len;

use crate::attributes::DIRECTORY;

/// `Path.GetDirectoryName`: the containing folder, keeping a root's separator (`C:\a.txt` has
/// `C:\`), or None at a root or for a bare name (where .NET said "" and every caller here read
/// that as nothing).
pub fn directory_name(path: &str) -> Option<String> {
    let root = root_len(path);
    let body = {
        let trimmed = path.trim_end_matches(['\\', '/']);
        if trimmed.len() < root {
            &path[..root]
        } else {
            trimmed
        }
    };
    if body.len() <= root {
        return None;
    }
    let cut = body.rfind(['\\', '/'])?;
    if cut < root {
        Some(path[..root].to_string())
    } else {
        let parent = &body[..cut];
        // The separator was the root's own: `C:\a.txt` → `C:\`, not `C:`.
        if parent.len() < root {
            Some(path[..root].to_string())
        } else {
            Some(parent.to_string())
        }
    }
}

/// `Path.Combine(a, b)`: `b` when it is rooted, otherwise `a\b` with exactly one separator.
pub fn combine(directory: &str, name: &str) -> String {
    if is_rooted(name) || directory.is_empty() {
        return name.to_string();
    }
    if directory.ends_with(['\\', '/']) {
        format!("{directory}{name}")
    } else {
        format!("{directory}\\{name}")
    }
}

/// `Path.GetFileNameWithoutExtension` and `Path.GetExtension` in one: split at the last dot, the
/// extension keeping its dot, and a name with no dot having an empty extension.
pub fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    }
}

/// `Path.IsPathRooted`: starts with a separator, or with a drive letter and a colon.
pub fn is_rooted(path: &str) -> bool {
    let b = path.as_bytes();
    (!b.is_empty() && (b[0] == b'\\' || b[0] == b'/'))
        || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

/// `Path.TrimEndingDirectorySeparator`: one trailing separator off, unless the path is a root.
pub fn trim_ending_separator(path: &str) -> &str {
    if path.ends_with(['\\', '/']) && path.len() > root_len(path) {
        &path[..path.len() - 1]
    } else {
        path
    }
}

/// `Directory.Exists`.
pub fn directory_exists(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_attributes() & DIRECTORY != 0)
}

/// `File.Exists`.
pub fn file_exists(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_attributes() & DIRECTORY == 0)
}

/// Either kind: files and folders share one namespace.
pub fn exists(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Encodes for a Win32 call: UTF-16 with a terminating NUL.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
