//! What a shortcut plan holds and what writing it reports. Spec: Core/Services/Shortcuts/ShortcutModels.cs.

/// One `.lnk` to write: what it points at, and its path, name already stepped aside from anything
/// in the way — including the other links in the same plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutCreation {
    pub target_path: String,
    pub link_path: String,
}

impl ShortcutCreation {
    pub fn new(target_path: &str, link_path: &str) -> ShortcutCreation {
        ShortcutCreation {
            target_path: target_path.to_string(),
            link_path: link_path.to_string(),
        }
    }
}

/// An item no shortcut can be made for. `source_path` is empty when the destination itself is the
/// problem, since then no one source is at fault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedShortcut {
    pub source_path: String,
    /// The refusal, phrased for the status bar.
    pub message: String,
}

impl RejectedShortcut {
    pub fn new(source_path: &str, message: String) -> RejectedShortcut {
        RejectedShortcut {
            source_path: source_path.to_string(),
            message,
        }
    }
}

/// What creating shortcuts for a selection would produce, decided without writing anything.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShortcutPlan {
    /// The folder the links go in.
    pub directory: String,
    pub creations: Vec<ShortcutCreation>,
    pub problems: Vec<RejectedShortcut>,
}

impl ShortcutPlan {
    pub fn new(
        directory: &str,
        creations: Vec<ShortcutCreation>,
        problems: Vec<RejectedShortcut>,
    ) -> ShortcutPlan {
        ShortcutPlan {
            directory: directory.to_string(),
            creations,
            problems,
        }
    }

    pub fn empty() -> ShortcutPlan {
        ShortcutPlan::default()
    }

    pub fn has_work(&self) -> bool {
        !self.creations.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedShortcut {
    pub link_path: String,
    /// The failure, phrased for the status bar.
    pub message: String,
    /// Windows refused permission, rather than the path being unusable. The one failure an
    /// administrator token could fix — not offered here yet, but worth carrying so the message
    /// can say which kind of failure it was.
    pub access_denied: bool,
}

/// What actually got written.
///
/// There is deliberately no undo record, for the reason `NewItemOutcome` gives: creating is
/// additive, exactly as copying is, and nothing existing is touched. Ctrl+Z is left pointing at
/// whatever move, rename or delete came before — which is the more valuable thing to have next in
/// the undo history than a handful of links the user can select and delete.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShortcutOutcome {
    pub created: Vec<String>,
    pub failed: Vec<FailedShortcut>,
}

impl ShortcutOutcome {
    pub fn empty() -> ShortcutOutcome {
        ShortcutOutcome::default()
    }
}
