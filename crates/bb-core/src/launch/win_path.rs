//! The handful of `System.IO.Path` answers the launch helpers lean on, with .NET's Windows rules:
//! text only, never the disk.

use bb_paths::{PathKey, root_len};

pub(crate) fn is_separator(c: char) -> bool {
    c == '\\' || c == '/'
}

/// `Path.IsPathRooted`: starts with a separator, or has a drive (`X:`).
pub(crate) fn is_rooted(path: &str) -> bool {
    let b = path.as_bytes();
    (!b.is_empty() && is_separator(b[0] as char)) || (b.len() >= 2 && b[1] == b':')
}

/// `Path.IsPathFullyQualified`: `X:\…`, or a UNC/device path (`\\…`, `\?…`). `\tool.exe` is
/// rooted but not fully qualified — it resolves against the current drive.
pub(crate) fn is_fully_qualified(path: &str) -> bool {
    let b = path.as_bytes();
    if b.len() < 2 {
        return false;
    }
    if is_separator(b[0] as char) {
        return b[1] == b'?' || is_separator(b[1] as char);
    }
    b.len() >= 3 && b[1] == b':' && is_separator(b[2] as char) && b[0].is_ascii_alphabetic()
}

/// `Path.GetExtension`: from the file name's last dot, dot included, or empty when there is no
/// dot, the dot is last, or a separator comes first. Note that `.gitignore` *has* an extension by
/// this rule, as it does for .NET.
pub(crate) fn extension(path: &str) -> &str {
    let b = path.as_bytes();
    let mut i = b.len();
    while i > 0 {
        i -= 1;
        let c = b[i] as char;
        if c == '.' {
            return if i == b.len() - 1 { "" } else { &path[i..] };
        }
        if is_separator(c) || c == ':' {
            break;
        }
    }
    ""
}

/// `Path.HasExtension`.
pub(crate) fn has_extension(path: &str) -> bool {
    !extension(path).is_empty()
}

/// `Path.GetDirectoryName`: None for a root (`C:\`, `\\server\share`) or nothing; `""` for a bare
/// file name; otherwise everything before the last separator, never trimmed into the root.
pub(crate) fn directory_name(path: &str) -> Option<String> {
    let root = root_len(path);
    if path.len() <= root {
        return None;
    }
    // As .NET's GetDirectoryNameOffset: back to the last separator, then over any run of
    // separators before it (`C:\foo\\bar`), never into the root.
    let b = path.as_bytes();
    let mut end = path.len();
    while end > root {
        end -= 1;
        if is_separator(b[end] as char) {
            break;
        }
    }
    while end > root && is_separator(b[end - 1] as char) {
        end -= 1;
    }
    Some(path[..end].to_string())
}

/// `Path.Combine(dir, name)` for a `name` that is not rooted.
pub(crate) fn combine(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        return name.to_string();
    }
    if dir.ends_with(['\\', '/']) {
        format!("{dir}{name}")
    } else {
        format!("{dir}\\{name}")
    }
}

/// `Path.GetFullPath`, falling back to the text itself when Windows will not resolve it.
pub(crate) fn full_path(path: &str) -> String {
    PathKey::normalize_display(path).unwrap_or_else(|_| path.to_string())
}

/// `string.Replace(needle, replacement, OrdinalIgnoreCase)` for an ASCII needle.
pub(crate) fn replace_ignore_ascii_case(text: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    let folded = text.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    while let Some(i) = folded[from..].find(&needle) {
        out.push_str(&text[from..from + i]);
        out.push_str(replacement);
        from += i + needle.len();
    }
    out.push_str(&text[from..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_name_follows_dotnet() {
        assert_eq!(
            Some(r"C:\data\docs".into()),
            directory_name(r"C:\data\docs\report.txt")
        );
        assert_eq!(Some(r"C:\".into()), directory_name(r"C:\report.txt"));
        assert_eq!(None, directory_name(r"C:\"));
        assert_eq!(Some(r"C:\a\b".into()), directory_name(r"C:\a\b\"));
        assert_eq!(Some("".into()), directory_name("report.txt"));
        assert_eq!(None, directory_name(r"\\server\share"));
        assert_eq!(
            Some(r"\\server\share".into()),
            directory_name(r"\\server\share\x")
        );
        assert_eq!(
            Some(r"\\server\share\x".into()),
            directory_name(r"\\server\share\x\y")
        );
    }

    #[test]
    fn extension_follows_dotnet() {
        assert_eq!(".exe", extension(r"C:\tools\wt.exe"));
        assert_eq!("", extension(r"C:\tools\code"));
        assert_eq!("", extension(r"C:\a.b\code"));
        assert_eq!("", extension("file."));
        assert_eq!(".gitignore", extension(".gitignore"));
    }

    #[test]
    fn qualification_follows_dotnet() {
        assert!(is_fully_qualified(r"C:\x"));
        assert!(is_fully_qualified(r"\\server\share"));
        assert!(!is_fully_qualified(r"\tool.exe"));
        assert!(!is_fully_qualified("/tool.exe"));
        assert!(!is_fully_qualified(r"C:tool.exe"));
        assert!(is_rooted(r"\tool.exe"));
        assert!(is_rooted(r"C:tool.exe"));
        assert!(!is_rooted(r"sub\tool.exe"));
    }

    #[test]
    fn replace_is_case_insensitive_and_keeps_the_rest() {
        assert_eq!(
            "a X b X",
            replace_ignore_ascii_case("a {Path} b {PATH}", "{path}", "X")
        );
        assert_eq!("héllo", replace_ignore_ascii_case("héllo", "zzz", "X"));
    }
}
