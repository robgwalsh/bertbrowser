//! Everything the rename dialog can ask for: a name template, a find/replace, a case transform and
//! a counter. Pure data — `RenamePattern::apply` is what turns it into names, and the dialog
//! previews every keystroke through that same function so a preview cannot drift from the result.
//! Spec: Core/Services/Rename/RenameRule.cs.

use serde::{Deserialize, Serialize};

/// How the scoped part of a name is re-cased.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RenameCase {
    #[default]
    AsIs,
    Lower,
    Upper,
    Title,
    Sentence,
}

/// Which part of the *existing* name find/replace and the case transform act on.
///
/// Never the finished name. A template's own literals — and `{parent}` — are not in scope, so
/// "{parent} - {name}" with a space-to-underscore replace still produces a name with spaces in it.
/// Cleaning up the finished name is a different operation and deliberately not this one; the value
/// is named for the source so the UI cannot promise otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RenameScope {
    /// The name without its extension. A folder is all stem.
    #[default]
    Stem,
    /// The extension, leading dot included. Empty for a folder.
    Extension,
    /// Stem and extension together.
    WholeName,
}

/// The scope and case are written as names, not numbers: reordering the enum later must not
/// reinterpret what was already saved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RenameRule {
    pub template: String,
    pub find: String,
    pub replace: String,
    pub use_regex: bool,
    pub match_case: bool,
    pub scope: RenameScope,
    pub case: RenameCase,
    pub counter_start: i32,
    pub counter_step: i32,
    /// True for the rule the plain F2 box has always produced: the template is taken literally,
    /// braces and all, and no token is expanded.
    ///
    /// This is what keeps the simple path byte-identical to what it was. '{' and '}' are legal
    /// Windows filename characters — `{6B99A0C1-…}.tmp` and `{id}.tsx` are names a file browser
    /// is routinely asked to produce — so a box that expanded tokens everywhere would refuse, or
    /// silently rewrite, a name it accepts today. Tokens are offered only by the expanded panel,
    /// where the list of them is on screen beside the box.
    pub is_literal: bool,
}

impl Default for RenameRule {
    /// What the expanded panel starts from before anything is typed.
    fn default() -> Self {
        RenameRule::new("{name}")
    }
}

impl RenameRule {
    /// A token-expanding rule over `template` with every other knob at its default.
    pub fn new(template: &str) -> RenameRule {
        RenameRule {
            template: template.to_string(),
            find: String::new(),
            replace: String::new(),
            use_regex: false,
            match_case: false,
            scope: RenameScope::Stem,
            case: RenameCase::AsIs,
            counter_start: 1,
            counter_step: 1,
            is_literal: false,
        }
    }

    /// The literal, token-free rule the plain rename box produces.
    pub fn simple(template: &str) -> RenameRule {
        RenameRule {
            is_literal: true,
            ..RenameRule::new(template)
        }
    }
}
