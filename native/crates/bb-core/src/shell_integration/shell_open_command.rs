//! Reading the command Windows would run to open a file, so it can be run with an administrator
//! token instead. This is what makes "Run as administrator" mean something for a file that is not
//! a program: `.sln` has no `runas` verb, but it does have an open command, and starting *that*
//! elevated is exactly what the user meant.
//!
//! **It refuses far more than it accepts, and deliberately.** Everything here ends in starting a
//! program with a token, so anything it cannot read confidently is answered with None and the item
//! is greyed out. Spec: Core/Services/ShellIntegration/ShellOpenCommand.cs.

/// A registered open command, split into the program to start and the arguments to give it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellOpenCommand {
    pub executable: String,
    pub arguments: String,
}

pub struct ShellOpenCommandParser;

impl ShellOpenCommandParser {
    /// The placeholders the shell substitutes the file's path for. Matched exactly, so
    /// `%SystemRoot%` is not mistaken for one — environment variables are expanded by the caller
    /// before the string arrives here.
    const PLACEHOLDERS: [&'static str; 8] = ["%1", "%L", "%l", "%V", "%v", "%D", "%d", "%*"];

    /// [`parse_with`](Self::parse_with) for the run-as-administrator caller: a command that never
    /// names the file is refused.
    pub fn parse(
        command: Option<&str>,
        file_path: &str,
        exists: impl Fn(&str) -> bool,
    ) -> Option<ShellOpenCommand> {
        Self::parse_with(command, file_path, exists, true)
    }

    /// `command` is the registered command, environment variables already expanded; `exists`
    /// answers whether a path names a file, injected so the decision is testable without a disk
    /// and because the unquoted case genuinely needs to ask. `require_placeholder` says whether a
    /// command that never names the file is refused: true for the run-as-administrator caller; the
    /// shell-menu caller passes false, because a static verb that takes its target from the
    /// working directory is legitimate and nothing is being started with a token. None when there
    /// is nothing safe to start.
    pub fn parse_with(
        command: Option<&str>,
        file_path: &str,
        exists: impl Fn(&str) -> bool,
        require_placeholder: bool,
    ) -> Option<ShellOpenCommand> {
        let command = command?.trim();
        if command.is_empty() || file_path.trim().is_empty() {
            return None;
        }

        let (executable, arguments) = split(command, &exists);
        let executable = executable?;
        if !exists(&executable) {
            return None;
        }

        // No placeholder means the command never asked for the file. Appending it anyway would hand
        // a program an argument it does not expect, elevated — so this is refused rather than
        // guessed at. It is mostly the DDE-era entries, which could not be started this way anyway.
        let placeholder = Self::PLACEHOLDERS.iter().find(|p| arguments.contains(*p));
        match placeholder {
            None if require_placeholder => None,
            None => Some(ShellOpenCommand {
                executable,
                arguments,
            }),
            Some(p) => Some(ShellOpenCommand {
                executable,
                arguments: arguments.replace(p, file_path),
            }),
        }
    }
}

/// The program and the rest, which is only obvious when the program is quoted. An unquoted path
/// containing spaces is genuinely ambiguous — `C:\Program Files\A B\x.exe %1` could be a program
/// called `C:\Program` — and Windows resolves it by probing each candidate. The same is done here,
/// longest first, because taking the first space would launch something other than the handler;
/// and if nothing matches, None, because the one thing not to do with a token in hand is start a
/// guess.
fn split(command: &str, exists: &impl Fn(&str) -> bool) -> (Option<String>, String) {
    if let Some(rest) = command.strip_prefix('"') {
        return match rest.find('"') {
            None => (None, String::new()),
            Some(close) => (
                Some(rest[..close].to_string()),
                rest[close + 1..].trim_start().to_string(),
            ),
        };
    }

    // Unquoted: try the longest prefix that names a file, then shorter ones.
    let mut space = command.rfind(' ');
    while let Some(at) = space.filter(|&at| at > 0) {
        let candidate = &command[..at];
        if exists(candidate) {
            return (
                Some(candidate.to_string()),
                command[at + 1..].trim_start().to_string(),
            );
        }
        space = command[..at].rfind(' ');
    }

    if exists(command) {
        (Some(command.to_string()), String::new())
    } else {
        (None, String::new())
    }
}
