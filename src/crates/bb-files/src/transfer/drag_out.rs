//! Whether a finished drag means "remove the originals", which is the one genuinely dangerous
//! question in dragging files out of this app. Spec: Core/Services/Transfer/DragOutContract.cs.
//!
//! `DoDragDrop` returning `Move` does **not** mean the source should delete anything. The shell's
//! protocol: a target that performed an *optimized move* relocated the files itself and reports
//! `CFSTR_PERFORMEDDROPEFFECT` = None, so the source must do nothing; `LOGICALPERFORMEDDROPEFFECT`
//! exists because some targets report Move in the older format even then, so it wins wherever both
//! are present; only a *non-optimized* move puts the removal on the source. The deliberate
//! asymmetry: sources are removed **only** when told Move or when a target reports nothing at all
//! (the documented fallback). Any other answer, including a target that contradicts itself, falls
//! back to leaving the files alone, and the removal it asks for goes through the reversible delete.

use std::ops::{BitAnd, BitOr};

/// The shell's `DROPEFFECT` values, mirrored here so the drag-out rule can live in Core. `SCROLL`
/// is a modifier bit the shell sets during auto-scroll and is never a verb.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct DropEffect(pub u32);

impl DropEffect {
    pub const NONE: DropEffect = DropEffect(0);
    pub const COPY: DropEffect = DropEffect(1);
    pub const MOVE: DropEffect = DropEffect(2);
    pub const LINK: DropEffect = DropEffect(4);
    pub const SCROLL: DropEffect = DropEffect(0x8000_0000);

    /// Verb bits only: the shell sets `SCROLL` during auto-scroll, and a raw comparison against
    /// `MOVE` would miss a value carrying it.
    pub const VERBS: DropEffect = DropEffect(1 | 2 | 4);

    pub fn contains(self, other: DropEffect) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for DropEffect {
    type Output = DropEffect;
    fn bitor(self, rhs: DropEffect) -> DropEffect {
        DropEffect(self.0 | rhs.0)
    }
}

impl BitAnd for DropEffect {
    type Output = DropEffect;
    fn bitand(self, rhs: DropEffect) -> DropEffect {
        DropEffect(self.0 & rhs.0)
    }
}

/// What the drag source has to do once a drag has ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DragOutAction {
    /// Nothing happened, or we already did the work ourselves.
    Nothing,
    /// Nothing to remove, but the folder may have changed underneath us: reload it.
    RefreshOnly,
    /// A foreign target copied the items and expects the source to remove the originals.
    RemoveSources,
}

pub struct DragOutContract;

impl DragOutContract {
    /// `handled_in_app`: this app's own drop pipeline took the drop, in which case the transfer has
    /// already happened and the originals are already where they belong. `returned`: what
    /// `DoDragDrop` returned. `logical_performed` / `performed`: the two effects read back off the
    /// data object, None if absent.
    pub fn decide(
        handled_in_app: bool,
        returned: DropEffect,
        logical_performed: Option<DropEffect>,
        performed: Option<DropEffect>,
    ) -> DragOutAction {
        // Our own pipeline already moved or copied the items through the executor. Acting on the
        // returned effect here would delete what we just placed.
        if handled_in_app {
            return DragOutAction::Nothing;
        }

        let verb = returned & DropEffect::VERBS;
        if verb == DropEffect::NONE {
            return DragOutAction::Nothing; // cancelled, or refused
        }
        if !verb.contains(DropEffect::MOVE) {
            return DragOutAction::RefreshOnly; // copy or link
        }

        // Logical wins over the legacy format wherever both are present; if neither target format
        // came back, the return value is all we have and the documented reading of it is "move".
        let Some(signal) = logical_performed.or(performed) else {
            return DragOutAction::RemoveSources;
        };

        // None means an optimized move: the target already relocated them. Anything else that is
        // not Move (a target contradicting itself) is not a licence to delete.
        if signal.contains(DropEffect::MOVE) {
            DragOutAction::RemoveSources
        } else {
            DragOutAction::RefreshOnly
        }
    }
}
