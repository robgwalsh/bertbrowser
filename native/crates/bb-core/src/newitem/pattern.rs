//! The naming rule for a new folder or file: what the dialog suggests, what part of it is
//! pre-selected, and whether what the user typed can be a name at all.
//! Spec: Core/Services/NewItem/NewItemPattern.cs.
//!
//! Pure, and separate from `NewItemPlanner`, for the reason `RenamePattern` is separate from the
//! rename planner: the dialog validates every keystroke, so the rule it previews has to be the
//! rule the create obeys rather than a second implementation that can drift. The character and
//! reserved-name rules are `RenamePattern::validate`'s — a name Windows refuses refuses it whether
//! the file is arriving by rename or by creation.

use bb_paths::{UniquePath, file_name};

use super::{NewItemKind, NewItemProbe};
use crate::rename::RenamePattern;
use crate::rename::paths::{combine, split_extension};

pub struct NewItemPattern;

impl NewItemPattern {
    /// Why `name` can't be created here, or None when it can. `extension` is one the chosen file
    /// type will append, or "" when it will not — checked *with* the name rather than separately,
    /// because the length limit applies to what lands on disk and the box only holds half of it.
    pub fn validate(name: &str, extension: &str) -> Option<String> {
        // Cleaned first, so the rule matches what a rename does and what Windows will actually
        // store: "Reports. " is a perfectly good request for a folder called "Reports".
        let cleaned = Self::clean(name);
        if cleaned.is_empty() {
            return Some("Enter a name.".to_string());
        }
        RenamePattern::validate(&format!("{cleaned}{extension}"))
    }

    /// The name the dialog opens with: `base_name`, or the first "(2)"-style variant of it that is
    /// free, so a second New Folder in the same place does not open on a name that is already
    /// refused.
    pub fn suggest_name(
        probe: &dyn NewItemProbe,
        directory: &str,
        base_name: &str,
        kind: NewItemKind,
        extension: &str,
    ) -> String {
        // An unusable directory is the planner's refusal to report, not a reason to fail here.
        let candidate = combine(directory, &format!("{base_name}{extension}"));
        let free = UniquePath::for_path(
            &candidate,
            kind == NewItemKind::Folder,
            |p| probe.directory_exists(p),
            |p| probe.file_exists(p),
        );
        file_name(&free).to_string()
    }

    /// The length, in UTF-16 units as a text box counts them, of the part of `name` the dialog
    /// pre-selects, so typing over the suggestion replaces the stem and leaves the extension
    /// alone.
    pub fn stem_length(name: &str, kind: NewItemKind) -> usize {
        let whole = name.encode_utf16().count();
        if kind == NewItemKind::Folder {
            return whole;
        }
        let stem = split_extension(name).0;
        // A dotfile such as .gitignore is all extension as far as Path is concerned; select the
        // lot.
        if stem.is_empty() {
            whole
        } else {
            stem.encode_utf16().count()
        }
    }

    /// Trailing spaces and periods are silently dropped by Windows, so take them off before
    /// anything sees the name — the same cleaning a rename does.
    pub fn clean(name: &str) -> String {
        name.trim().trim_end_matches(['.', ' ']).to_string()
    }
}
