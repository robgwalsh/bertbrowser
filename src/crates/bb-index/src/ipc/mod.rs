//! Inter-process communication between the app, the elevated index helper, the elevated
//! file-operation helper and a second app instance: the two line protocols and their argument
//! rules, the endpoint names, the framing both pipes share, and the Win32 checks of who is on the
//! other end. Everything that decides is pure and tested; the Win32 calls sit in their own files and
//! answer "no" rather than fail when a question cannot be asked. Spec: src/BertBrowser.Core/Ipc/*.cs.
//!
//! The pipes themselves (creating a named pipe with a DACL, connecting to one) are not here: in the
//! C# they live in the App's transports and the helpers' `Program.cs`, and a `std::fs::File` over the
//! raw handle is what `LineReader` and `write_line` read and write.

mod elevation_protocol;
mod elevator_arguments;
mod index_endpoint;
mod index_protocol;
mod indexer_arguments;
mod indexer_presence_lock;
mod instance_endpoint;
mod line_channel;
mod peer_integrity;
mod pipe_identity;
mod pipe_owner;

pub use elevation_protocol::{ElevationMessage, ElevationProtocol, ElevationVerb};
pub use elevator_arguments::ElevatorArguments;
pub use index_endpoint::IndexEndpoint;
pub use index_protocol::{IndexMessage, IndexProtocol, IndexVerb};
pub use indexer_arguments::{IndexerArguments, IndexerCommand};
pub use indexer_presence_lock::{IndexerPresenceLock, PresenceClaim};
pub use instance_endpoint::InstanceEndpoint;
pub use line_channel::{LineReader, read_line, write_line};
pub use peer_integrity::PeerIntegrity;
pub use pipe_identity::PipeIdentity;
pub use pipe_owner::PipeOwner;

/// What the identity-derived names can fail with. The argument rules on the protocols never fail,
/// they refuse; this is for the two places the C# threw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a SID this app would have produced.
    UnacceptableSid(String),
    /// The current process's token reports no user SID. Deliberately not substituted with a
    /// constant: two users whose tokens both failed would derive one pipe name.
    NoUserSid,
    /// A Win32 call failed, with `GetLastError`.
    Win32 { function: &'static str, code: u32 },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UnacceptableSid(sid) => {
                write!(f, "Not a SID this app would have produced: {sid:?}.")
            }
            Error::NoUserSid => write!(f, "No user SID for the current process."),
            Error::Win32 { function, code } => write!(f, "{function} failed with error {code}."),
        }
    }
}

impl std::error::Error for Error {}

/// `int.TryParse` with its default style: surrounding whitespace and a leading sign are forgiven,
/// nothing else is. Used only for the greeting's version, where the sender is our own code.
fn parse_int_lenient(text: &str) -> Option<i32> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<i32>().ok()
}

/// `string.IsNullOrWhiteSpace`, for a `&str` that cannot be null.
fn is_blank(text: &str) -> bool {
    text.trim().is_empty()
}

/// A NUL-terminated UTF-16 copy for a `W` entry point.
fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `GetLastError` wrapped as this module's error.
fn last_error(function: &'static str) -> Error {
    let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    Error::Win32 { function, code }
}

/// Closes a kernel handle on drop, whatever path a function leaves by.
struct OwnedHandle(windows_sys::Win32::Foundation::HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
        }
    }
}
