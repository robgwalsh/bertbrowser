//! Works out what creating a folder or file would produce, without touching anything.
//! Spec: Core/Services/NewItem/NewItemPlanner.cs.
//!
//! This exists for one reason: the dialog asks on every keystroke, so the rule it previews has to
//! be the rule the create obeys. A refusal the user can see while the name is still editable is
//! worth far more than an error afterwards.
//!
//! The delete area's protected locations are deliberately *not* consulted. That list guards a
//! handful of folders against being *deleted*, exact-match only; creating inside them is the
//! ordinary thing this app is for — a new file in the profile root is not a mistake. Nor does
//! anything here lean on the manifest: the process is `asInvoker`, and a refused create is retried
//! with an administrator token one layer up, which is where the privilege actually is.

use bb_paths::PathKey;

use super::{NewItemKind, NewItemPattern, NewItemPlan, NewItemRejection, RejectedNewItem};
use crate::rename::paths;

/// The filesystem questions the planner asks. A trait so the rules that stop a create landing on
/// something can be unit-tested without a real disk.
pub trait NewItemProbe {
    fn directory_exists(&self, path: &str) -> bool;

    fn file_exists(&self, path: &str) -> bool;
}

/// The real one, over the local disk.
pub struct FileSystemNewItemProbe;

impl NewItemProbe for FileSystemNewItemProbe {
    fn directory_exists(&self, path: &str) -> bool {
        paths::directory_exists(path)
    }

    fn file_exists(&self, path: &str) -> bool {
        paths::file_exists(path)
    }
}

static LOCAL: FileSystemNewItemProbe = FileSystemNewItemProbe;

pub struct NewItemPlanner<'a> {
    probe: &'a dyn NewItemProbe,
}

impl<'a> NewItemPlanner<'a> {
    pub fn new(probe: &'a dyn NewItemProbe) -> NewItemPlanner<'a> {
        NewItemPlanner { probe }
    }

    /// A planner over the real disk.
    pub fn local() -> NewItemPlanner<'static> {
        NewItemPlanner { probe: &LOCAL }
    }

    /// What creating `name` in `directory` would produce. Never fails: an unusable path is a
    /// refusal like any other.
    pub fn plan(
        &self,
        directory: &str,
        name: &str,
        kind: NewItemKind,
        template_path: Option<&str>,
    ) -> NewItemPlan {
        let cleaned = NewItemPattern::clean(name);
        let mut plan = NewItemPlan {
            directory: directory.to_string(),
            name: cleaned.clone(),
            kind,
            template_path: template_path.map(str::to_string),
            rejected: None,
        };

        if !is_usable(directory) || !self.probe.directory_exists(directory) {
            plan.rejected = Some(RejectedNewItem::new(
                NewItemRejection::ParentMissing,
                "The folder to create this in is no longer there.".to_string(),
            ));
            return plan;
        }

        // The pattern's message, not one of ours: a name Windows refuses should be explained the
        // same way here as it is in the rename dialog.
        if let Some(problem) = NewItemPattern::validate(&cleaned, "") {
            plan.rejected = Some(RejectedNewItem::new(NewItemRejection::InvalidName, problem));
            return plan;
        }

        if let Some(template) = template_path.filter(|t| !t.is_empty())
            && !self.probe.file_exists(template)
        {
            plan.rejected = Some(RejectedNewItem::new(
                NewItemRejection::TemplateMissing,
                format!("The template for this file type is missing: {template}"),
            ));
            return plan;
        }

        let target = plan.target_path();
        if !is_usable(&target) {
            plan.rejected = Some(RejectedNewItem::new(
                NewItemRejection::InvalidName,
                "That name can't be used here.".to_string(),
            ));
            return plan;
        }

        // Files and folders share one namespace, so either kind in the way is a collision.
        if self.probe.directory_exists(&target) || self.probe.file_exists(&target) {
            plan.rejected = Some(RejectedNewItem::new(
                NewItemRejection::NameTaken,
                format!("'{cleaned}' already exists in this folder."),
            ));
            return plan;
        }

        plan
    }

    /// The name the dialog opens with: `base_name`, stepped aside to "(2)" and so on if that is
    /// already taken.
    pub fn suggest_name(
        &self,
        directory: &str,
        base_name: &str,
        kind: NewItemKind,
        extension: &str,
    ) -> String {
        NewItemPattern::suggest_name(self.probe, directory, base_name, kind, extension)
    }
}

/// Whether a path can be canonicalized at all. The other planners swallow exactly this for the
/// same reason: an unusable path is the user's problem to see, not a failure out of a method the
/// dialog calls on every keystroke.
fn is_usable(path: &str) -> bool {
    PathKey::canonicalize(path).is_ok()
}
