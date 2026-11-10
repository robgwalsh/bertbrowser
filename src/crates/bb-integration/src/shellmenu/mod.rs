//! The right-click menus: the app's own entries and which are unticked, the one token list that
//! orders both menus, the separator rule, and the decisions behind hosting other programs'
//! context-menu entries — which key families to ask, which registrations become one extension,
//! which are never shown, and how a handler's menu is tidied. The menu is composed per extension,
//! never asked of the shell whole, which is what makes "hide this one" possible at all. Nothing
//! here opens a registry key or a COM object: the app reads, this decides.
//! Spec: Core/Services/ShellMenu/*.

mod built_in;
mod entry;
mod extension;
mod guid;
mod keys;
mod layout;
mod open_command;
mod rules;
mod separators;
mod static_verb;

pub use built_in::{BuiltInMenuItem, BuiltInMenuItems, BuiltInMenuPlaces};
pub use entry::{ShellMenuEntry, ShellMenuGroup};
pub use extension::{
    ShellExtension, ShellExtensionKind, ShellHandlerRegistration, ShellVerbRegistration,
};
pub use guid::Guid;
pub use keys::{ShellFileType, ShellMenuContext, ShellMenuKeys, ShellMenuTarget};
pub use layout::{MenuLayoutRules, MenuPlacement, MenuPlacementKind};
pub use open_command::ShellOpenCommand;
pub use rules::ShellMenuRules;
pub use separators::{MenuSeparatorRules, MenuSlot};
pub use static_verb::StaticVerbCommand;

/// `StringComparison.OrdinalIgnoreCase`, without allocating.
pub(crate) fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.chars().map(fold).eq(b.chars().map(fold))
}

/// One character to one character, as `ToUpperInvariant` did.
pub(crate) fn fold(c: char) -> char {
    let mut u = c.to_uppercase();
    match (u.next(), u.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}
