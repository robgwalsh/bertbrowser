//! The elevated file-operation helper's command line: which pipe to call back on, which process
//! started it, and whose SID that process is expected to be running as. Spec:
//! Core/Ipc/ElevatorArguments.cs.
//!
//! **There is deliberately no `--data-dir`.** The index helper needs one because it writes to the
//! database; this one never opens it. Every argument that does not exist is one an attacker cannot
//! choose, and the whole surface here is a pipe name, a number and a SID, none of which names a
//! file.
//!
//! **`user_sid` is a check, never a source of truth.** UAC gives the same user a different token,
//! not a different user, so the helper's own token already *is* the calling user's SID, which is
//! what it uses when it grants that user access to a staging folder it creates. This argument exists
//! only so a mismatch can be refused, which covers the over-the-shoulder credential prompt where the
//! elevating identity belongs to somebody else. Trusting it instead would be taking an
//! attacker-chosen SID and writing it into an ACL.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElevatorArguments {
    pub pipe_name: String,
    pub parent_process_id: u32,
    pub user_sid: String,
}

impl ElevatorArguments {
    pub const MAX_PIPE_NAME_LENGTH: usize = 256;

    /// The endpoint prefix this helper will talk to, and nothing else. Separate from the index
    /// helper's so neither can ever be handed the other's pipe.
    pub const PIPE_PREFIX: &'static str = "BertBrowser.Elevate.";

    const USAGE: &'static str =
        "Usage: BertBrowser.Elevator --pipe <name> --parent-pid <id> --user-sid <sid>";

    /// Parses, or explains. An unrecognised token is an error and never a positional value, the
    /// same rule `IndexerArguments` and `CommandLine` follow, and for the same reason: a mistyped
    /// flag silently becoming data is worse than a message.
    pub fn try_parse<S: AsRef<str>>(args: &[S]) -> Result<ElevatorArguments, String> {
        let mut pipe: Option<&str> = None;
        let mut sid: Option<&str> = None;
        let mut parent_pid: u32 = 0;

        let mut i = 0;
        while i < args.len() {
            let arg = args[i].as_ref();
            let value = args.get(i + 1).map(|v| v.as_ref());
            match (arg, value) {
                ("--pipe", Some(v)) => {
                    pipe = Some(v);
                    i += 1;
                }
                ("--parent-pid", Some(v)) => {
                    match super::parse_int_lenient(v) {
                        Some(pid) if pid > 0 => parent_pid = pid as u32,
                        _ => return Err("--parent-pid must be a process id.".to_string()),
                    }
                    i += 1;
                }
                ("--user-sid", Some(v)) => {
                    sid = Some(v);
                    i += 1;
                }
                _ => {
                    return Err(format!(
                        "'{arg}' is not a recognised argument. {}",
                        Self::USAGE
                    ));
                }
            }
            i += 1;
        }

        let Some(pipe) = pipe.filter(|p| Self::is_acceptable_pipe_name(p)) else {
            return Err(format!(
                "--pipe is missing or not an acceptable endpoint. {}",
                Self::USAGE
            ));
        };
        if parent_pid == 0 {
            return Err(format!("--parent-pid is missing. {}", Self::USAGE));
        }
        let Some(sid) = sid.filter(|s| Self::is_acceptable_sid(s)) else {
            return Err(format!(
                "--user-sid is missing or malformed. {}",
                Self::USAGE
            ));
        };

        Ok(ElevatorArguments {
            pipe_name: pipe.to_string(),
            parent_process_id: parent_pid,
            user_sid: sid.to_string(),
        })
    }

    /// An endpoint this helper is willing to connect to: bounded, free of the characters a pipe
    /// name cannot hold, and carrying this helper's own prefix.
    pub fn is_acceptable_pipe_name(candidate: &str) -> bool {
        if super::is_blank(candidate) {
            return false;
        }
        if candidate.chars().count() > Self::MAX_PIPE_NAME_LENGTH {
            return false;
        }
        if candidate.chars().any(|c| {
            c.is_control() || matches!(c, '\\' | '/' | '*' | '?' | '|' | '<' | '>' | '"' | ':')
        }) {
            return false;
        }
        candidate.starts_with(Self::PIPE_PREFIX)
    }

    /// The textual form of a SID: `S-1-…`, digits and hyphens only. Not a full parse; the helper
    /// compares it against its own identity, which is the actual check.
    pub fn is_acceptable_sid(candidate: &str) -> bool {
        if super::is_blank(candidate) {
            return false;
        }
        let length = candidate.chars().count();
        if !(3..=256).contains(&length) {
            return false;
        }
        let Some(rest) = candidate.strip_prefix("S-") else {
            return false;
        };
        rest.chars().all(|c| c.is_ascii_digit() || c == '-')
    }
}
