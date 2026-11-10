//! What an incoming drop means, which differs sharply depending on who started it.
//! Spec: Core/Services/Transfer/DropInContract.cs.
//!
//! `DragOutContract`'s counterpart, and the danger runs the other way: the effect we report back
//! can make an *external source* delete its files, so reporting Move for a drag the user meant as
//! a copy destroys someone else's data on our say-so. Hence the asymmetry in the defaults: an
//! in-app drag defaults to **Move**, an external one to **Copy**, and Shift asks for a move
//! explicitly. An in-app drop must report `NONE`: the transfer has already happened, and telling
//! our own drag source "Move" would have `DragOutContract` read it as a foreign move and delete the
//! items we just placed. An external drop must report the verb it actually performed, or a move
//! leaves the source's copy behind.

use super::drag_out::DropEffect;
use super::models::TransferVerb;

/// Where the payload of a drop came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DropOrigin {
    /// A drag that started in this app, carrying the private item format.
    InApp,
    /// An ordinary `CF_HDROP` from another application.
    External,
}

/// What to do with the items, and what to put in the drag's effects once the drop has been
/// handled, which is how the *source* application learns what happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropInDecision {
    pub verb: TransferVerb,
    pub report: DropEffect,
}

pub struct DropInContract;

impl DropInContract {
    /// `allowed` is what the source is willing to permit. A source that only offers Copy must
    /// never be told Move: it may act on that.
    pub fn decide(
        origin: DropOrigin,
        control: bool,
        shift: bool,
        allowed: DropEffect,
    ) -> DropInDecision {
        let mut verb = match origin {
            // In-app: Ctrl copies, and everything else moves. Shift means nothing here; it is the
            // list's range-extend modifier, and has never selected the verb on this side.
            DropOrigin::InApp => {
                if control {
                    TransferVerb::Copy
                } else {
                    TransferVerb::Move
                }
            }
            // External: copying is the default, and a move has to be asked for. Ctrl still forces
            // a copy, so holding it can never turn into a move by accident.
            DropOrigin::External => {
                if shift && !control {
                    TransferVerb::Move
                } else {
                    TransferVerb::Copy
                }
            }
        };

        let offered = allowed & DropEffect::VERBS;

        // A source that will not permit the verb we chose gets the other one rather than a
        // refusal, so a drag from an app offering copy-only still works instead of silently doing
        // nothing.
        if verb == TransferVerb::Move && !offered.contains(DropEffect::MOVE) {
            verb = TransferVerb::Copy;
        } else if verb == TransferVerb::Copy
            && !offered.contains(DropEffect::COPY)
            && offered.contains(DropEffect::MOVE)
        {
            verb = TransferVerb::Move;
        }

        // Our own drop reports nothing, whatever it did.
        let report = match (origin, verb) {
            (DropOrigin::InApp, _) => DropEffect::NONE,
            (DropOrigin::External, TransferVerb::Copy) => DropEffect::COPY,
            (DropOrigin::External, TransferVerb::Move) => DropEffect::MOVE,
        };

        DropInDecision { verb, report }
    }

    /// Whether a drop offering these effects can be accepted at all. A source offering only
    /// `LINK` is asking for a shortcut, which this app does not make.
    pub fn can_accept(allowed: DropEffect) -> bool {
        allowed.contains(DropEffect::COPY | DropEffect::MOVE)
    }
}
