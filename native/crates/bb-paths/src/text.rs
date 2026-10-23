//! Turning paths into text a person is about to paste somewhere else. The opposite job from
//! `PathKey`: this leaves a path exactly as it is and only decides how to wrap it.
//! Spec: src/BertBrowser.Core/Paths/PathText.cs.

/// The line break the clipboard text uses (`Environment.NewLine` on Windows).
pub const NEWLINE: &str = "\r\n";

pub struct PathText;

impl PathText {
    /// One path, wrapped for pasting into a command line. Quoted *unconditionally*, as Explorer's
    /// own "Copy as path" does: a result that is only sometimes paste-safe is worse than one that
    /// always is.
    pub fn quote(path: &str) -> String {
        format!("\"{path}\"")
    }

    /// Several paths, one per line, each quoted: what "Copy as path" puts on the clipboard for a
    /// multiple selection.
    pub fn for_clipboard<'a>(paths: impl IntoIterator<Item = &'a str>) -> String {
        paths
            .into_iter()
            .map(Self::quote)
            .collect::<Vec<_>>()
            .join(NEWLINE)
    }

    /// Several names, one per line, unquoted: a name is not a path and is far more often wanted as
    /// bare text.
    pub fn names_for_clipboard<'a>(paths: impl IntoIterator<Item = &'a str>) -> String {
        paths
            .into_iter()
            .map(crate::file_name)
            .collect::<Vec<_>>()
            .join(NEWLINE)
    }
}
