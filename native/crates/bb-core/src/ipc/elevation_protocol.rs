//! The line format for the elevated file-operation helper: `OPS\t<Verb>`, optionally followed by a
//! tab and one JSON document. Spec: Core/Ipc/ElevationProtocol.cs.
//!
//! **Unlike the index protocol, this one carries paths, and it has to.** The index helper's rule
//! (four verbs and never a path) is a rule about *that* helper: it lives for the whole session, it is
//! started at launch without anyone asking, and its job names no file. This helper inverts all
//! three: it lives for one operation and exits, it is started only by a click on a shield in a dialog
//! that names the items, and it exists *because* it takes paths. What replaces "never a path" here
//! is **one prompt per operation, one request per process, and the process exits when the request
//! is done**, made structural: the host accepts `Item` only before `Go`, and after `Go` the only verb
//! it will read is `Cancel`.
//!
//! **The bound is two-dimensional, and neither half should ever need raising.** No line is more
//! than one record, so the line cap stays `NavigationRequest::MAX_LINE_LENGTH`, the bound
//! `LineReader` exists to enforce. The number of records is capped separately by `MAX_ITEMS`,
//! refused as the host reads rather than after it has grown a buffer. A cap that has to be raised to
//! fit a big operation is not a cap; sending the whole plan on one line would have needed that.
//!
//! Tab is safe as the separator for the reason it is in the index protocol: every path is checked
//! against `NavigationRequest::is_acceptable_path`, which rejects control characters, and JSON
//! escapes any that survive inside a string. A single-line JSON document contains no raw tab or
//! newline. The payload is carried as opaque text here; what is inside it belongs to the elevation
//! models, not the framing.

use crate::cli::NavigationRequest;

/// What the two ends of the elevated-operation pipe can say to each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElevationVerb {
    /// Both ways, first: the protocol version. A mismatch ends the session.
    Hello,
    /// Helper → app: connected, checked, and waiting for a request.
    Ready,
    /// App → helper: the operation's header. Exactly one per session.
    Begin,
    /// App → helper: one item of the request. Never more than `MAX_ITEMS` of them.
    Item,
    /// App → helper: the request is complete; carry it out.
    Go,
    /// App → helper: stop. The executors' own cancel guarantees apply: nothing is left half-written
    /// and whatever got across stays across.
    Cancel,
    /// Helper → app: how far along, coalesced to the usual 100 ms.
    Progress,
    /// Helper → app: one item succeeded.
    Done,
    /// Helper → app: one item failed.
    Fault,
    /// Helper → app: the operation is over, with whatever the outcome needs that is not per-item:
    /// staging folders, the cancelled flag.
    End,
    /// Helper → app: the session cannot continue. Always the last thing it says.
    Fatal,
}

impl ElevationVerb {
    pub const ALL: [ElevationVerb; 11] = [
        ElevationVerb::Hello,
        ElevationVerb::Ready,
        ElevationVerb::Begin,
        ElevationVerb::Item,
        ElevationVerb::Go,
        ElevationVerb::Cancel,
        ElevationVerb::Progress,
        ElevationVerb::Done,
        ElevationVerb::Fault,
        ElevationVerb::End,
        ElevationVerb::Fatal,
    ];

    /// The verb as it is written on the wire.
    pub fn name(self) -> &'static str {
        match self {
            ElevationVerb::Hello => "Hello",
            ElevationVerb::Ready => "Ready",
            ElevationVerb::Begin => "Begin",
            ElevationVerb::Item => "Item",
            ElevationVerb::Go => "Go",
            ElevationVerb::Cancel => "Cancel",
            ElevationVerb::Progress => "Progress",
            ElevationVerb::Done => "Done",
            ElevationVerb::Fault => "Fault",
            ElevationVerb::End => "End",
            ElevationVerb::Fatal => "Fatal",
        }
    }

    /// Exact and case-sensitive: a number or a comma-separated composite is not a verb.
    pub fn parse(text: &str) -> Option<ElevationVerb> {
        ElevationVerb::ALL
            .into_iter()
            .find(|verb| verb.name() == text)
    }
}

/// One line of the elevated-operation protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElevationMessage {
    pub verb: ElevationVerb,
    pub payload: String,
}

impl ElevationMessage {
    pub fn new(verb: ElevationVerb, payload: &str) -> ElevationMessage {
        ElevationMessage {
            verb,
            payload: payload.to_string(),
        }
    }

    /// A verb that carries nothing.
    pub fn bare(verb: ElevationVerb) -> ElevationMessage {
        ElevationMessage::new(verb, "")
    }
}

impl std::fmt::Display for ElevationMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&ElevationProtocol::format(self))
    }
}

pub struct ElevationProtocol;

impl ElevationProtocol {
    /// Bumped when the shape of anything on the wire changes. Both ends greet with it and a
    /// mismatch ends the session rather than risking a misread plan.
    pub const PROTOCOL_VERSION: i32 = 1;

    /// Comfortably past any selection a person makes, and small enough that a hostile peer cannot
    /// make the helper accumulate. A request over this is refused whole.
    pub const MAX_ITEMS: usize = 10_000;

    /// The status text on `Fatal`.
    pub const MAX_STATUS_LENGTH: usize = 200;

    const SEPARATOR: char = '\t';
    const PREFIX: &'static str = "OPS";

    pub fn format(message: &ElevationMessage) -> String {
        let mut line = String::from(Self::PREFIX);
        line.push(Self::SEPARATOR);
        line.push_str(message.verb.name());
        if !message.payload.is_empty() {
            line.push(Self::SEPARATOR);
            line.push_str(&message.payload);
        }
        line
    }

    /// Parses a line, or refuses it. Never panics: everything on this wire is untrusted.
    pub fn try_parse(line: &str) -> Option<ElevationMessage> {
        if line.is_empty() || line.len() > NavigationRequest::MAX_LINE_LENGTH {
            return None;
        }
        // Two fields, or three where the third is the payload, which may itself contain nothing
        // that looks like a separator, since JSON escapes control characters.
        let parts: Vec<&str> = line.splitn(3, Self::SEPARATOR).collect();
        if parts.len() < 2 {
            return None;
        }
        if parts[0] != Self::PREFIX {
            return None;
        }
        let verb = ElevationVerb::parse(parts[1])?;
        let payload = if parts.len() == 3 { parts[2] } else { "" };
        if !Self::is_acceptable_payload(verb, payload) {
            return None;
        }
        Some(ElevationMessage::new(verb, payload))
    }

    /// Whether a verb may carry this payload at all: the shape check, before anything tries to read
    /// the JSON inside it.
    pub fn is_acceptable_payload(verb: ElevationVerb, payload: &str) -> bool {
        match verb {
            ElevationVerb::Hello => is_acceptable_version(payload),
            ElevationVerb::Fatal => is_acceptable_status(payload),
            ElevationVerb::Ready | ElevationVerb::Go | ElevationVerb::Cancel => payload.is_empty(),
            // Begin, Item, Progress, Done, Fault, End: one JSON document, non-empty.
            ElevationVerb::Begin
            | ElevationVerb::Item
            | ElevationVerb::Progress
            | ElevationVerb::Done
            | ElevationVerb::Fault
            | ElevationVerb::End => !payload.is_empty(),
        }
    }

    /// The version on a greeting, or None when it is not one.
    pub fn version_of(message: &ElevationMessage) -> Option<i32> {
        if message.verb != ElevationVerb::Hello {
            return None;
        }
        super::parse_int_lenient(&message.payload)
    }

    /// Flattens an error's text into something safe to put on a `Fatal` line: one line, no control
    /// characters, bounded.
    pub fn summarize(text: &str) -> String {
        let flattened: String = text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let flattened = flattened.trim();
        if flattened.is_empty() {
            return "The elevated helper failed.".to_string();
        }
        flattened.chars().take(Self::MAX_STATUS_LENGTH).collect()
    }
}

fn is_acceptable_version(payload: &str) -> bool {
    super::parse_int_lenient(payload).is_some_and(|version| version > 0)
}

fn is_acceptable_status(payload: &str) -> bool {
    let mut length = 0usize;
    for c in payload.chars() {
        if c.is_control() {
            return false;
        }
        length += 1;
        if length > ElevationProtocol::MAX_STATUS_LENGTH {
            return false;
        }
    }
    length > 0
}
