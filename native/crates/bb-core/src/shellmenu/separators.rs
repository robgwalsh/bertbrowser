//! Which separators of a menu should show once the user has unticked some of its entries. The
//! separators are declared between groups; when a whole group is hidden two of them meet, and when
//! the first or last group goes one is left at the edge.
//! Spec: Core/Services/ShellMenu/MenuSeparatorRules.cs.

/// One slot of a menu as the separator rule sees it: a separator, or anything else. `is_visible`
/// is ignored for a separator: the rule decides those from scratch each time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuSlot {
    pub is_separator: bool,
    pub is_visible: bool,
}

impl MenuSlot {
    pub fn item(visible: bool) -> MenuSlot {
        MenuSlot {
            is_separator: false,
            is_visible: visible,
        }
    }

    pub fn separator() -> MenuSlot {
        MenuSlot {
            is_separator: true,
            is_visible: true,
        }
    }
}

pub struct MenuSeparatorRules;

impl MenuSeparatorRules {
    /// Per slot: whether it should be visible. Items keep their own answer; a separator shows only
    /// between two visible items, and only the first of a run.
    pub fn apply(slots: &[MenuSlot]) -> Vec<bool> {
        let mut visible = vec![false; slots.len()];
        let mut pending_separator: Option<usize> = None;
        let mut any_item_before = false;

        for (i, slot) in slots.iter().enumerate() {
            if slot.is_separator {
                // Remembered, not shown: it shows only once an item follows it.
                if any_item_before && pending_separator.is_none() {
                    pending_separator = Some(i);
                }
                continue;
            }
            if !slot.is_visible {
                continue;
            }

            visible[i] = true;
            if let Some(pending) = pending_separator.take() {
                visible[pending] = true;
            }
            any_item_before = true;
        }

        visible
    }
}
