//! The wire format a second instance uses to hand its command line to the one already running, and
//! the single rule deciding whether a path may be acted on at all. A path arriving over IPC is
//! untrusted input; what `is_acceptable_path` permits is narrow on purpose: an absolute local or UNC
//! path, and nothing else. One line per request, tab-separated: tab is safe because control
//! characters are refused. Spec: Core/Cli/NavigationRequest.cs.

use super::{CommandLineRequest, OpenIn, OpenTarget};

pub struct NavigationRequest;

impl NavigationRequest {
    /// Comfortably past any real path, and short enough that a hostile peer cannot make the listener
    /// allocate.
    pub const MAX_PATH_LENGTH: usize = 4096;
    /// Caps a whole line, so a peer cannot stream forever into a reader.
    pub const MAX_LINE_LENGTH: usize = 64 * 1024;

    const SEPARATOR: char = '\t';
    const PREFIX: &'static str = "OPEN";

    /// Whether a path may be navigated to. Rejects anything relative (it would resolve against
    /// whatever directory this process happens to be in), Win32 device paths, wildcards, control
    /// characters, and anything implausibly long.
    pub fn is_acceptable_path(candidate: &str) -> bool {
        if candidate.trim().is_empty() || candidate.chars().count() > Self::MAX_PATH_LENGTH {
            return false;
        }
        for c in candidate.chars() {
            // Control characters cover the separator, embedded newlines that would forge a second
            // request, and the NUL that truncates a string on the way into Win32.
            if c.is_control() || matches!(c, '*' | '?' | '|' | '<' | '>' | '"') {
                return false;
            }
        }
        // "\\.\PhysicalDrive0" and friends are not files. "\\?\" bypasses normalization.
        if candidate.starts_with(r"\\.\") || candidate.starts_with(r"\\?\") {
            return false;
        }
        is_fully_qualified(candidate)
    }

    /// Renders a request as one line. Targets failing `is_acceptable_path` are dropped rather than
    /// sent.
    pub fn format(request: &CommandLineRequest) -> String {
        let mut line = String::from(Self::PREFIX);
        line.push(Self::SEPARATOR);
        line.push_str(request.mode.name());
        for target in &request.targets {
            if !Self::is_acceptable_path(&target.path) {
                continue;
            }
            line.push(Self::SEPARATOR);
            line.push(if target.select { 'S' } else { '-' });
            line.push_str(&target.path);
        }
        line
    }

    /// Reads a line back. None for anything malformed; paths that fail `is_acceptable_path` are
    /// dropped, so a request that arrives half-corrupt opens the parts that were sound.
    pub fn try_parse(line: &str) -> Option<CommandLineRequest> {
        if line.is_empty() || line.len() > Self::MAX_LINE_LENGTH {
            return None;
        }
        let mut parts = line.split(Self::SEPARATOR);
        if parts.next()? != Self::PREFIX {
            return None;
        }
        let mode = OpenIn::parse(parts.next()?)?;
        let mut targets = Vec::new();
        for field in parts {
            if field.chars().count() < 2 {
                continue;
            }
            let select = match field.as_bytes()[0] {
                b'S' => true,
                b'-' => false,
                _ => continue,
            };
            let path = &field[1..];
            if !Self::is_acceptable_path(path) {
                continue;
            }
            targets.push(OpenTarget::new(path, select));
        }
        Some(CommandLineRequest {
            targets,
            mode,
            errors: Vec::new(),
        })
    }
}

/// `Path.IsPathFullyQualified`: a drive with a separator (`X:\`), or a UNC prefix. `\foo` is rooted
/// on "the current drive" and `C:foo` is drive-relative; both are refused.
fn is_fully_qualified(path: &str) -> bool {
    let b = path.as_bytes();
    let sep = |c: u8| c == b'\\' || c == b'/';
    if b.len() >= 2 && sep(b[0]) && sep(b[1]) {
        return true;
    }
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && sep(b[2])
}
