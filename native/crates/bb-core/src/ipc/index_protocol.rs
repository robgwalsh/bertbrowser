//! The wire format between BertBrowser and the elevated process that reads the MFT for it: one line
//! per message, `IDX\t<Verb>[\t<argument>]`. Spec: Core/Ipc/IndexProtocol.cs.
//!
//! **The elevated end accepts five verbs and never a path.** `Hello`, `Start`, `Shutdown`, `Ping`
//! and `Record` are the whole of what a medium-integrity process can ask an administrator-token
//! process to do, and none of them names a file. A later "re-index this folder" verb would put an
//! attacker-chosen path on the elevated surface; nothing is worth that. The rule is about *this*
//! helper and rests on three properties of it: it outlives every app, it can be started at sign-in
//! with nobody opening the program, and its job (reading a volume) names no file. The elevator
//! inverts all three, which is why that protocol may carry paths and this one never does.
//!
//! `Record` is the one verb that carries a value, admissible for a narrow reason: one integer from a
//! fixed menu (`ChangeLogPolicy::RETENTION_OPTIONS`, or 0), switching a feature on or off and
//! bounding how long its rows live, pointing the process at nothing. The helper keeps the policy
//! between sessions, deliberately, so the log has no holes where "what changed while I was away?" is
//! asked; the default is still off and the first app to attach overwrites it.
//!
//! Everything in the other direction is a state push, not a reply: the app mirrors what arrives and
//! answers the UI locally, so nothing it asks ever waits on a round trip. Format and discipline are
//! `NavigationRequest`'s: tab is safe as a separator precisely because every argument rule rejects
//! control characters, and a line that fails to parse is *ignored and reading continues*; one
//! malformed message must not end a session.

use crate::changes::ChangeLogPolicy;
use crate::cli::NavigationRequest;

/// What one message on the index pipe means.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndexVerb {
    /// Both ways, first: the protocol version each end speaks.
    Hello,
    /// Helper → app: the database is open and volumes are enumerated.
    Ready,
    /// App → helper: begin indexing.
    Start,
    /// App → helper: stop and exit.
    Shutdown,
    /// Helper → app: this drive letter's initial enumeration has begun.
    Building,
    /// Helper → app: this root key's index is complete.
    Complete,
    /// Helper → app: this drive letter is no longer building, complete or not.
    Idle,
    /// Helper → app: indexing cannot continue, and why.
    Fatal,
    /// Either way: are you there?
    Ping,
    /// The answer to `Ping`.
    Pong,
    /// App → helper: record file changes for this many hours, 0 for not at all.
    Record,
}

impl IndexVerb {
    pub const ALL: [IndexVerb; 11] = [
        IndexVerb::Hello,
        IndexVerb::Ready,
        IndexVerb::Start,
        IndexVerb::Shutdown,
        IndexVerb::Building,
        IndexVerb::Complete,
        IndexVerb::Idle,
        IndexVerb::Fatal,
        IndexVerb::Ping,
        IndexVerb::Pong,
        IndexVerb::Record,
    ];

    /// The verb as it is written on the wire.
    pub fn name(self) -> &'static str {
        match self {
            IndexVerb::Hello => "Hello",
            IndexVerb::Ready => "Ready",
            IndexVerb::Start => "Start",
            IndexVerb::Shutdown => "Shutdown",
            IndexVerb::Building => "Building",
            IndexVerb::Complete => "Complete",
            IndexVerb::Idle => "Idle",
            IndexVerb::Fatal => "Fatal",
            IndexVerb::Ping => "Ping",
            IndexVerb::Pong => "Pong",
            IndexVerb::Record => "Record",
        }
    }

    /// Exact and case-sensitive. The C# `Enum.TryParse` also accepted the underlying number ("7")
    /// and comma-separated lists, neither of which is a verb anyone meant to send; only the name is.
    pub fn parse(text: &str) -> Option<IndexVerb> {
        IndexVerb::ALL.into_iter().find(|verb| verb.name() == text)
    }
}

/// One message: a verb and at most one argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexMessage {
    pub verb: IndexVerb,
    pub argument: String,
}

impl IndexMessage {
    pub fn new(verb: IndexVerb, argument: &str) -> IndexMessage {
        IndexMessage {
            verb,
            argument: argument.to_string(),
        }
    }

    /// A verb that says everything by arriving.
    pub fn bare(verb: IndexVerb) -> IndexMessage {
        IndexMessage::new(verb, "")
    }
}

impl std::fmt::Display for IndexMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&IndexProtocol::format(self))
    }
}

pub struct IndexProtocol;

impl IndexProtocol {
    /// Bumped whenever the meaning of a verb changes. Both ends ship in one package, so a mismatch
    /// used to be nearly impossible; now that the helper outlives the app, a helper from the previous
    /// build carries on across an update holding the well-known endpoint, and a version both ends
    /// can compare is the only thing that lets the new app recognise it and ask it to go.
    ///
    /// Version 2: end-of-stream ends a *session* rather than the helper, `Shutdown` became the only
    /// way an app ends one, a session opens by replaying what is already known, and `Record` is
    /// remembered between sessions.
    pub const PROTOCOL_VERSION: i32 = 2;

    /// A status line long enough to be useful and short enough not to be a payload.
    pub const MAX_STATUS_LENGTH: usize = 200;

    const SEPARATOR: char = '\t';
    const PREFIX: &'static str = "IDX";

    /// Renders a message as one line.
    pub fn format(message: &IndexMessage) -> String {
        let mut line = String::from(Self::PREFIX);
        line.push(Self::SEPARATOR);
        line.push_str(message.verb.name());
        if !message.argument.is_empty() {
            line.push(Self::SEPARATOR);
            line.push_str(&message.argument);
        }
        line
    }

    /// Reads a line back. None for a wrong prefix, an unknown verb, an oversized line, or an
    /// argument the verb's own rule refuses; callers ignore the line and keep reading.
    pub fn try_parse(line: &str) -> Option<IndexMessage> {
        if line.is_empty() || line.len() > NavigationRequest::MAX_LINE_LENGTH {
            return None;
        }
        let parts: Vec<&str> = line.split(Self::SEPARATOR).collect();
        if parts.len() < 2 || parts.len() > 3 {
            return None;
        }
        if parts[0] != Self::PREFIX {
            return None;
        }
        let verb = IndexVerb::parse(parts[1])?;
        let argument = if parts.len() == 3 { parts[2] } else { "" };
        if !Self::is_acceptable_argument(verb, argument) {
            return None;
        }
        Some(IndexMessage::new(verb, argument))
    }

    /// Each verb's own rule for what it may carry.
    pub fn is_acceptable_argument(verb: IndexVerb, argument: &str) -> bool {
        match verb {
            IndexVerb::Hello => is_acceptable_version(argument),
            IndexVerb::Building | IndexVerb::Idle => Self::is_acceptable_drive(argument),
            IndexVerb::Complete => Self::is_acceptable_root_key(argument),
            IndexVerb::Fatal => Self::is_acceptable_status(argument),
            IndexVerb::Record => Self::is_acceptable_retention(argument),
            // Ready, Start, Shutdown, Ping and Pong say everything by arriving.
            IndexVerb::Ready
            | IndexVerb::Start
            | IndexVerb::Shutdown
            | IndexVerb::Ping
            | IndexVerb::Pong => argument.is_empty(),
        }
    }

    /// Exactly the retention menu, written the one way an integer is written: "24", never " 24",
    /// "024" or "24h". Whatever a lenient parse would forgive, the elevated end does not.
    pub fn is_acceptable_retention(candidate: &str) -> bool {
        if candidate.is_empty() || !candidate.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        let Ok(hours) = candidate.parse::<i32>() else {
            return false;
        };
        hours.to_string() == candidate && ChangeLogPolicy::is_acceptable_hours(hours)
    }

    /// A bare drive letter: "C", not "C:" and not a path.
    pub fn is_acceptable_drive(candidate: &str) -> bool {
        matches!(candidate.as_bytes(), [b'A'..=b'Z'])
    }

    /// A canonical volume root, as `PathKey::canonicalize` produces: "C:\" and nothing else.
    /// Narrower than `NavigationRequest::is_acceptable_path` on purpose, and it still goes through
    /// it first. The only root the indexer ever completes is a volume, so accepting a deeper path
    /// here would only widen what a peer can put into the state the search router trusts.
    pub fn is_acceptable_root_key(candidate: &str) -> bool {
        if !NavigationRequest::is_acceptable_path(candidate) {
            return false;
        }
        matches!(candidate.as_bytes(), [b'A'..=b'Z', b':', b'\\'])
    }

    /// Text bound for the status bar: bounded, and no control characters. Only `Fatal` carries
    /// any; the wording of the ordinary indexing line is derived on the app's side from the drives
    /// it was told about, so one place decides it and the two processes cannot disagree.
    pub fn is_acceptable_status(candidate: &str) -> bool {
        let mut length = 0usize;
        for c in candidate.chars() {
            if c.is_control() {
                return false;
            }
            length += 1;
            if length > Self::MAX_STATUS_LENGTH {
                return false;
            }
        }
        length > 0
    }

    /// The version an end is announcing, or None if it is not a `Hello`.
    pub fn version_of(message: &IndexMessage) -> Option<i32> {
        if message.verb != IndexVerb::Hello {
            return None;
        }
        super::parse_int_lenient(&message.argument)
    }
}

fn is_acceptable_version(candidate: &str) -> bool {
    super::parse_int_lenient(candidate).is_some_and(|version| version > 0)
}
