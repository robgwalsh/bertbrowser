//! Commands and keyboard shortcuts: the catalogue (the one place a command's name, icon and shipped
//! shortcut are written down), key chords, the keymap and its dispatch rules, the palette's matcher
//! and ordering, the file-verb gate, go-to parsing and select-by-pattern.
//! Spec: Core/Services/Commands/*.

mod catalog;
mod chord;
mod goto;
mod keymap;
mod matcher;
mod palette;
mod selection;
mod verbs;

pub use catalog::{
    CommandCatalog, CommandCategories, CommandContext, CommandDescriptor, CommandProminence,
};
pub use chord::{ChordKeyKind, ChordModifiers, KeyChord};
pub use goto::{GoToRules, GoToTarget};
pub use keymap::{Bindable, FocusKind, FocusState, Keymap, KeymapRules, Overrides};
pub use matcher::{CommandMatch, CommandMatcher, MatchRange, MatchTier};
pub use palette::{
    PaletteEntry, PaletteRow, PaletteRowKind, PaletteRules, PaletteState, PaletteSuggestions,
};
pub use selection::{SelectionMatch, SelectionPattern, SelectionRow};
pub use verbs::{FileVerb, FileVerbRules, FileVerbSnapshot, FileVerbState};
