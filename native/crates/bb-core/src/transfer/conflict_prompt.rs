//! Asking how to settle destination names that are already taken.
//! Spec: Core/Services/Transfer/IConflictPrompt.cs.
//!
//! A seam for the reason `IUserConfirm` is one: a modal raised from inside the shell would block
//! the scripted run that hosts the same window offscreen, and a test has nobody to ask. It is also
//! what lets *every* way of transferring reach the same dialog: the question used to live in the
//! drop pipeline, which is why pasting onto a name that already existed quietly numbered the
//! newcomer and never asked at all. The answer is per path; only the clashing items need an entry,
//! and anything unlisted falls back to `KeepBoth`, which can destroy nothing.

use bb_paths::PathKey;

use super::models::{ConflictDefaults, Resolutions, TransferPlan};

pub trait ConflictPrompt {
    /// What to do with each clash in `plan`, keyed by `PathKey::canonicalize` of the source path,
    /// or None when the user cancelled, which abandons the whole transfer rather than doing part
    /// of it.
    fn ask(&self, plan: &TransferPlan) -> Option<Resolutions>;
}

/// Answers without asking: what a context with nobody to ask gets. Keep both rather than cancel,
/// because this is not a question about whether to proceed; the user already asked for the
/// transfer. The one exception is a clash with a byte-identical file, which `ConflictDefaults`
/// settles as Skip, through the same call the dialog seeds its rows from.
#[derive(Default)]
pub struct KeepBothConflictPrompt;

impl ConflictPrompt for KeepBothConflictPrompt {
    fn ask(&self, plan: &TransferPlan) -> Option<Resolutions> {
        Some(
            plan.conflicts()
                .into_iter()
                .filter_map(|t| {
                    PathKey::canonicalize(&t.source_path)
                        .ok()
                        .map(|key| (key, ConflictDefaults::for_transfer(t)))
                })
                .collect(),
        )
    }
}
