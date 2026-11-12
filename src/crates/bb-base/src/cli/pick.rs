//! Another program asking the user to choose a file in this app: which file to open, or where to
//! save one. The answer is one path, written to a pipe the asker made; this app writes nothing to
//! disk on anyone's behalf.
//!
//! Everything here arrives from outside - a command line, or the single-instance pipe - so every
//! field is checked before a request exists at all. The reply pipe most of all: it is a name this
//! app will open and write the user's choice to, so it may only ever be a plain local pipe name,
//! never a path, a server, or anything that could be read as one.

/// Whether the file must exist already or is about to be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickMode {
    Open,
    Save,
}

impl PickMode {
    pub fn name(self) -> &'static str {
        match self {
            PickMode::Open => "open",
            PickMode::Save => "save",
        }
    }

    /// Exact, as the wire format writes it. The command line lowercases first.
    pub fn parse(s: &str) -> Option<PickMode> {
        match s {
            "open" => Some(PickMode::Open),
            "save" => Some(PickMode::Save),
            _ => None,
        }
    }
}

/// One question: choose a file. Only ever built by [`PickRequest::new`], so one that exists has
/// passed every check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickRequest {
    pub mode: PickMode,
    /// The bare name of the pipe to answer on; the asker is its server. Never a path.
    pub reply_pipe: String,
    /// What is being asked, in the asker's words. Shown as text and never interpreted.
    pub title: String,
    /// `;`-separated wildcards (`*.mp4;*.mkv`) a chosen file should match, or empty for anything.
    pub filter: String,
    /// The file name offered for a save.
    pub default_name: String,
    /// A folder to begin in, or a file to begin beside; empty for wherever the app would open.
    pub start: String,
}

impl PickRequest {
    pub const MAX_PIPE_NAME: usize = 64;
    pub const MAX_TEXT: usize = 256;

    /// `Err` is a sentence for the command line's error list.
    pub fn new(
        mode: PickMode,
        reply_pipe: &str,
        title: &str,
        filter: &str,
        default_name: &str,
        start: &str,
    ) -> Result<PickRequest, String> {
        if !Self::is_acceptable_pipe_name(reply_pipe) {
            return Err(
                "'--reply-pipe' expects a plain pipe name: letters, digits, '.', '_' and '-'."
                    .to_string(),
            );
        }
        if !is_plain_text(title) {
            return Err("'--pick-title' is too long or holds control characters.".to_string());
        }
        if !Self::is_acceptable_filter(filter) {
            return Err(
                "'--pick-filter' expects wildcards separated by ';', like *.mp4;*.mkv.".to_string(),
            );
        }
        if !default_name.is_empty() && !Self::is_acceptable_file_name(default_name) {
            return Err("'--pick-name' expects a file name, not a path.".to_string());
        }
        Ok(PickRequest {
            mode,
            reply_pipe: reply_pipe.to_string(),
            title: title.to_string(),
            filter: filter.to_string(),
            default_name: default_name.to_string(),
            start: start.to_string(),
        })
    }

    /// Letters, digits, `.`, `_` and `-`, and not many of them. No separator of any kind, so the
    /// name cannot leave `\\.\pipe\` or name another machine.
    pub fn is_acceptable_pipe_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= Self::MAX_PIPE_NAME
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    }

    /// One path segment a file could be called: no separators, nothing Windows reserves, and not a
    /// name that means "here" or "up".
    pub fn is_acceptable_file_name(name: &str) -> bool {
        let trimmed = name.trim();
        !trimmed.is_empty()
            && trimmed == name
            && name.chars().count() <= Self::MAX_TEXT
            && name != "."
            && name != ".."
            && !name.ends_with(['.', ' '])
            && !name.chars().any(|c| {
                c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            })
    }

    /// Empty, or wildcards over file names joined by `;`.
    pub fn is_acceptable_filter(filter: &str) -> bool {
        is_plain_text(filter)
            && !filter
                .chars()
                .any(|c| matches!(c, '\\' | '/' | ':' | '"' | '<' | '>' | '|'))
    }

    /// The filter's patterns, lowercased; empty means anything goes.
    pub fn patterns(&self) -> Vec<String> {
        self.filter
            .split(';')
            .map(|p| p.trim().to_lowercase())
            .filter(|p| !p.is_empty() && p != "*" && p != "*.*")
            .collect()
    }

    /// Whether a file called `name` is one the asker wants. Only the two shapes a file filter ever
    /// takes are understood - `*.ext` and an exact name - and anything else matches nothing rather
    /// than everything.
    pub fn accepts(&self, name: &str) -> bool {
        let patterns = self.patterns();
        if patterns.is_empty() {
            return true;
        }
        let lower = name.to_lowercase();
        patterns.iter().any(|p| match p.strip_prefix('*') {
            Some(suffix) => !suffix.contains(['*', '?']) && lower.ends_with(suffix),
            None => !p.contains(['*', '?']) && *p == lower,
        })
    }
}

/// Text that is shown or carried and never interpreted: bounded, and with no control characters
/// (which cover the wire format's separator and a newline that would forge a second request).
fn is_plain_text(text: &str) -> bool {
    text.chars().count() <= PickRequest::MAX_TEXT && !text.chars().any(char::is_control)
}
