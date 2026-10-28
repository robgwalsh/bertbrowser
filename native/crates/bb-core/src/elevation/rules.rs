//! What may never be done with an administrator token, however the user answers the prompt.
//! Spec: Core/Services/Elevation/ElevationRules.cs.
//!
//! **The refusal belongs where the privilege is.** Two planners rely, explicitly, on the app being
//! `asInvoker`: `TransferPlanner` refuses drive roots and nothing else, so dragging `C:\Windows`
//! onto another folder is refused today only by its ACL; and `NewItemPlanner` says in so many words
//! that it does not consult `ProtectedLocations`. A shield button would quietly undo the first of
//! those.
//!
//! The answer is not to tighten the planners. Creating a file in the profile root is the ordinary
//! thing this app is for, and refusing it would cost legitimate unelevated work to guard against a
//! case that only exists one layer up. So the check lives at the escalation boundary instead: the
//! unelevated path is exactly what it always was, and the extra rule applies only where the extra
//! privilege does.
//!
//! It is asked about the item being *acted on*, never the destination. Copying into
//! `C:\Program Files` is the headline case for this whole feature; moving `C:\Program Files` is not.

use bb_paths::PathKey;

use crate::delete::ProtectedLocations;

pub struct ElevationRules;

impl ElevationRules {
    /// True for something that must not be moved, renamed or deleted with a token: the folders
    /// `ProtectedLocations::default_keys` names, and anything inside a Recycle Bin.
    ///
    /// The Recycle Bin clause is not symmetry for its own sake: those `$R` files are what a
    /// pending Ctrl+Z restores from, and an elevated operation is exactly the one with enough
    /// rights to reach another account's.
    pub fn is_refused_for_elevation(path: &str) -> bool {
        if path.trim().is_empty() {
            return true;
        }
        if ProtectedLocations::is_inside_recycle_bin(path) {
            return true;
        }
        match Self::key_of(path) {
            None => true,
            Some(key) => ProtectedLocations::default_keys().contains(&key),
        }
    }

    /// The database key for a path, or None when it is not a path at all, which is itself a
    /// refusal, since nothing unparseable should reach a process holding a token.
    pub fn key_of(path: &str) -> Option<String> {
        PathKey::canonicalize(path).ok()
    }
}
