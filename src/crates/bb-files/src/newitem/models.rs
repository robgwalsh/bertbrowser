//! What a create is asked about, what the planner answers, and what the executor reports.
//! Spec: Core/Services/NewItem/NewItemModels.cs.

use crate::rename::paths::combine;

/// What is being created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewItemKind {
    Folder,
    File,
}

/// Why the planner refused to create the item. All four are worth showing in the dialog while the
/// user can still do something about them, which is the point of planning at all: nothing is
/// written until a plan comes back clean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewItemRejection {
    /// Not a legal Windows file name — see `NewItemPattern::validate`.
    InvalidName,
    /// Something is already there. Creating is never allowed to overwrite, and for a folder it is
    /// not even allowed to quietly adopt what is there already.
    NameTaken,
    /// The folder being created in has gone since the menu was opened.
    ParentMissing,
    /// The chosen file type points at a template file that is no longer there. Refused up front
    /// rather than producing an empty file the user did not ask for.
    TemplateMissing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedNewItem {
    pub reason: NewItemRejection,
    /// User-facing explanation, shown under the name box as it is typed.
    pub message: String,
}

impl RejectedNewItem {
    pub fn new(reason: NewItemRejection, message: String) -> RejectedNewItem {
        RejectedNewItem { reason, message }
    }
}

/// The validated answer to "what would creating this produce?". Unlike a rename or a delete there
/// is only ever one item, so the plan carries one rejection rather than a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewItemPlan {
    /// The folder the item goes in.
    pub directory: String,
    /// The item's name, extension included.
    pub name: String,
    pub kind: NewItemKind,
    /// A file to copy the new file's contents from, or None for an empty one. Always None for a
    /// folder.
    pub template_path: Option<String>,
    /// Why this cannot be created, or None when it can.
    pub rejected: Option<RejectedNewItem>,
}

impl NewItemPlan {
    pub fn empty() -> NewItemPlan {
        NewItemPlan {
            directory: String::new(),
            name: String::new(),
            kind: NewItemKind::Folder,
            template_path: None,
            rejected: None,
        }
    }

    /// True when carrying the plan out would create something.
    pub fn has_work(&self) -> bool {
        self.rejected.is_none() && !self.name.is_empty()
    }

    pub fn target_path(&self) -> String {
        combine(&self.directory, &self.name)
    }
}

/// The failure, phrased for a message dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedNewItem {
    pub message: String,
    /// Windows refused permission. The only failure an administrator token could fix.
    pub access_denied: bool,
}

impl FailedNewItem {
    pub fn new(message: String) -> FailedNewItem {
        FailedNewItem {
            message,
            access_denied: false,
        }
    }
}

/// What actually happened on disk. There is deliberately no undo record: creating is additive,
/// exactly as copying is, and the item it makes is empty. Ctrl+Z is left pointing at whatever
/// move, rename or delete came before.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct NewItemOutcome {
    pub created_path: Option<String>,
    pub failed: Option<FailedNewItem>,
}

impl NewItemOutcome {
    pub fn empty() -> NewItemOutcome {
        NewItemOutcome::default()
    }

    pub fn created(&self) -> bool {
        self.created_path.is_some()
    }
}
