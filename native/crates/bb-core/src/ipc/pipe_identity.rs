//! Comparing the two forms Windows hands back for "who is on the other end of this pipe". Spec:
//! Core/Ipc/PipeIdentity.cs.
//!
//! **The two names are not in the same form.** The pipe's impersonation user name is the bare
//! account ("Rob"), while the process's own identity is qualified ("DESKTOP-K0BI3BS\Rob"). Comparing
//! them whole never matches, and it fails *closed*, so the symptom is not an error but a pipe that
//! accepts every connection and immediately drops it. That cost a real afternoon once: hand-offs
//! looked like they worked while a second full copy of the app started anyway. One home for the
//! rule, and one comment, so it cannot be rediscovered.
//!
//! This is defence in depth and not a boundary. The DACL is the real gate; nothing between two
//! processes of the same user ever is one.

use bb_paths::fold_upper;

pub struct PipeIdentity;

impl PipeIdentity {
    /// True when two Windows account names name the same account, in either form.
    pub fn same_account(left: &str, right: &str) -> bool {
        fold_upper(Self::account_part(left)) == fold_upper(Self::account_part(right))
    }

    /// The account portion of a name that may or may not carry a domain prefix.
    pub fn account_part(name: &str) -> &str {
        match name.rfind('\\') {
            Some(i) => &name[i + 1..],
            None => name,
        }
    }
}
