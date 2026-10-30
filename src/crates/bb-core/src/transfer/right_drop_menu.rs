//! Explorer's right-drag menu: the verbs offered when the right button is released over a drop
//! target, rather than a verb chosen for the user by the modifier keys they happened to be
//! holding. Spec: Core/Services/Transfer/RightDropMenuRules.cs.
//!
//! Entries are *greyed, never removed*. The menu opens under the cursor at the end of a gesture,
//! so an item that is present on one drop and absent on the next moves everything below it, and
//! the thing below Copy is a verb that leaves the originals behind. Nothing here decides whether a
//! transfer is *safe*: that stays with the planner, re-run from scratch once a verb is picked.

/// What the menu at the end of a right-drag can offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RightDropVerb {
    Move,
    Copy,
    /// Leave the items where they are and put a `.lnk` pointing at each one here.
    Shortcut,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RightDropEntry {
    pub verb: RightDropVerb,
    /// The wording, count-aware.
    pub label: String,
    /// False when the verb has nothing to do here, or cannot apply at all.
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RightDropContext {
    /// The drop lands inside a container, which is rewritten to add the files rather than
    /// transferred into.
    pub destination_is_archive: bool,
    /// The items being dragged live inside a container, so their paths name nothing on disk.
    pub sources_are_virtual: bool,
    pub move_has_work: bool,
    pub copy_has_work: bool,
    /// How many items are being dragged, for the wording.
    pub item_count: usize,
}

pub struct RightDropMenuRules;

impl RightDropMenuRules {
    pub fn build(context: RightDropContext) -> Vec<RightDropEntry> {
        // A container is rewritten to add files to it. There is no move (the entry would have to
        // delete out of the source folder on the strength of a rewrite) and no shortcut, because a
        // .lnk stores a path and an entry inside a container has not got one.
        let archive = context.destination_is_archive;
        vec![
            RightDropEntry {
                verb: RightDropVerb::Move,
                label: "Move here".to_string(),
                enabled: !archive && context.move_has_work,
            },
            RightDropEntry {
                verb: RightDropVerb::Copy,
                label: "Copy here".to_string(),
                enabled: context.copy_has_work,
            },
            RightDropEntry {
                verb: RightDropVerb::Shortcut,
                label: if context.item_count == 1 {
                    "Create shortcut here".to_string()
                } else {
                    "Create shortcuts here".to_string()
                },
                enabled: !archive && !context.sources_are_virtual && context.item_count > 0,
            },
        ]
    }
}
