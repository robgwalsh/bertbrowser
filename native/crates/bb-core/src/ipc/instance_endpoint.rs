//! The name of the pipe a running copy of BertBrowser listens on so a second launch can hand over
//! its command line instead of starting a whole second app. Spec: Core/Ipc/InstanceEndpoint.cs.
//!
//! **The name is random, and that is the security-relevant part of it.** Pipe names live in one
//! machine-wide namespace with no per-user partitioning: any account on the machine may create any
//! name that is not already taken. A predictable `BertBrowser.<SID>` could therefore be claimed by
//! another signed-in user before the real copy started, and the consequences were not subtle: the
//! genuine first instance could never create its listener, and every launch after that would connect
//! to the squatter, write the folder path it was asked to open, and exit having opened nothing. A
//! DACL cannot answer this: it governs who may *open* an endpoint, not who may take the name.
//! Randomness can, so the name carries a 128-bit nonce, and the copy that owns it publishes the name
//! where only its own account can read it.

pub struct InstanceEndpoint;

impl InstanceEndpoint {
    /// 16 random bytes, hex-encoded.
    pub const NONCE_LENGTH: usize = 32;

    /// Long enough for the longest SID plus the nonce, short enough not to be a payload.
    pub const MAX_NAME_LENGTH: usize = 256;

    /// What every endpoint for `user_key` begins with.
    pub fn prefix(user_key: &str) -> String {
        format!("BertBrowser.{user_key}.")
    }

    /// The endpoint name for one user and one nonce.
    pub fn name(user_key: &str, nonce: &str) -> String {
        Self::prefix(user_key) + nonce
    }

    /// Whether `candidate` is a name this app would have produced for `user_key`.
    ///
    /// This is **not** the gate: the published file sits where only one account can write, and
    /// that is what keeps a name trustworthy. What this stops is a truncated, hand-edited or
    /// half-written file turning into a pipe path: the name is about to be interpolated into
    /// `\\.\pipe\`, so a separator in it would name a different object entirely, and a name with no
    /// nonce left on it would be exactly the guessable one this design exists to avoid.
    pub fn is_acceptable(candidate: &str, user_key: &str) -> bool {
        if candidate.is_empty() || user_key.is_empty() {
            return false;
        }
        if candidate.chars().count() > Self::MAX_NAME_LENGTH {
            return false;
        }
        let Some(nonce) = candidate.strip_prefix(&Self::prefix(user_key)) else {
            return false;
        };
        // Everything after the prefix is the nonce, and nothing else, which rules out separators,
        // wildcards and control characters without having to name them.
        nonce.len() == Self::NONCE_LENGTH
            && nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
    }
}
