//! How far back the undo history reaches. Spec: Core/Services/UndoHistory/UndoModels.cs
//! (`UndoBudget`).

/// At most this many entries, and at most this much held data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UndoBudget {
    pub max_entries: i32,
    pub max_held_bytes: i64,
}

impl UndoBudget {
    pub const DEFAULT_ENTRIES: i32 = 50;
    pub const DEFAULT_GIGABYTES: i32 = 10;

    /// The choices the Settings page offers.
    pub const ENTRY_OPTIONS: [i32; 5] = [10, 25, 50, 100, 200];
    pub const GIGABYTE_OPTIONS: [i32; 6] = [1, 5, 10, 25, 50, 100];

    pub fn new(max_entries: i32, max_held_bytes: i64) -> UndoBudget {
        UndoBudget {
            max_entries,
            max_held_bytes,
        }
    }

    /// A budget from the two settings, where None means "never set" and takes the default.
    /// Anything below one is lifted to one: a history that cannot hold a single entry is not a
    /// setting anybody asked for.
    pub fn from_settings(entries: Option<i32>, gigabytes: Option<i32>) -> UndoBudget {
        UndoBudget {
            max_entries: entries.unwrap_or(Self::DEFAULT_ENTRIES).max(1),
            max_held_bytes: i64::from(gigabytes.unwrap_or(Self::DEFAULT_GIGABYTES).max(1)) << 30,
        }
    }
}

impl Default for UndoBudget {
    fn default() -> Self {
        Self::from_settings(None, None)
    }
}
