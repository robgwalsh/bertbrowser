//! Asking whether to try again with an administrator token.
//! Spec: Core/Services/Elevation/IElevationPrompt.cs.
//!
//! A seam rather than a call into a dialog, for the reason the process launcher and the folder
//! handler are seams: a scripted harness run must be able to answer without a window, and a test
//! must be able to answer without a person.
//!
//! **It is consulted from the view model rather than from a view, which is the opposite of where
//! every other failure dialog in this app lives, and that is forced rather than chosen.** Each of
//! the four operations records itself in the undo history inside itself. A retry raised after the
//! method returned would need a second entry for what is, to the user, one action, and undoing one
//! without the other would put half of it back. So the offer has to happen inside the same
//! "is transferring" window, before the entry is recorded.

use super::models::ElevationOperation;

/// What the user is being asked to consent to. `items` are the paths Windows refused, in the order
/// they were selected: the dialog shows a bounded number of them and says how many more there are,
/// since a five-thousand-row list is not a confirmation. `is_undo` says the operation being retried
/// is putting something back rather than doing it, which reads differently and deserves different
/// words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElevationOffer {
    pub operation: ElevationOperation,
    pub items: Vec<String>,
    pub is_undo: bool,
}

impl ElevationOffer {
    pub fn new(operation: ElevationOperation, items: Vec<String>, is_undo: bool) -> ElevationOffer {
        ElevationOffer {
            operation,
            items,
            is_undo,
        }
    }
}

pub trait ElevationPrompt {
    /// True to go ahead and raise the UAC prompt.
    fn offer(&self, offer: &ElevationOffer) -> bool;
}

/// Answers no, always. What a context with nobody to ask gets: a background sweep, or a scripted
/// run that must never put a prompt on the user's desktop.
pub struct RefusingElevationPrompt;

impl ElevationPrompt for RefusingElevationPrompt {
    fn offer(&self, _offer: &ElevationOffer) -> bool {
        false
    }
}
