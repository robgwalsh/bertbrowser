//! One row of the Settings checklist and one contributor to a menu, and the raw registrations it
//! is merged from. Raw: `ShellMenuRules` decides what a registration is called and whether it is
//! shown; every flag is reported rather than acted on here.
//! Spec: Core/Services/ShellMenu/ShellExtension.cs, ShellExtensionRegistration.cs.

use super::Guid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShellExtensionKind {
    /// A COM handler, asked to build its own items.
    Handler,
    /// A static registry verb: a name and a command line.
    Verb,
}

/// A handler or a verb, merged across every key family it is registered under, so 7-Zip is one
/// row and not four.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellExtension {
    /// What the hidden list stores. Stable across sessions and machines: `clsid:{guid}` or
    /// `verb:name`, lowercase.
    pub id: String,
    pub name: String,
    pub kind: ShellExtensionKind,
    pub clsid: Option<Guid>,
    /// Every registration of a verb, one per family — invoking on the background uses the
    /// `Directory\Background` one, whose command line differs.
    pub verbs: Vec<ShellVerbRegistration>,
    pub families: Vec<String>,
}

impl ShellExtension {
    pub fn handler_id(clsid: Guid) -> String {
        format!("clsid:{}", clsid.to_braced().to_lowercase())
    }

    pub fn verb_id(verb: &str) -> String {
        format!("verb:{}", verb.to_lowercase())
    }
}

/// A COM context-menu handler as registered under one key family
/// (`HKCR\*\shellex\ContextMenuHandlers\7-Zip`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellHandlerRegistration {
    /// The `HKCR`-relative key it was found under.
    pub family: String,
    /// The registration's own key name — sometimes a friendly name, sometimes the CLSID again.
    pub name: String,
    pub clsid: Guid,
    /// The default value of `HKCR\CLSID\{clsid}`, when there is one.
    pub class_name: Option<String>,
}

/// A static verb as registered under one key family (`HKCR\Directory\Background\shell\git_shell`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShellVerbRegistration {
    pub family: String,
    pub verb: String,
    /// The `MUIVerb` value, already resolved when it was an `@dll,-id` reference, else the key's
    /// default value; None when neither is set.
    pub display_name: Option<String>,
    /// The `Icon` value as written: a path, or `path,index`.
    pub icon: Option<String>,
    /// The `command` subkey's default value, or None.
    pub command: Option<String>,
    pub extended: bool,
    pub legacy_disable: bool,
    pub programmatic_only: bool,
    /// The command names a COM object rather than a program; the command line beside it is not
    /// what runs.
    pub has_delegate_execute: bool,
    /// A cascading menu built from the command store.
    pub has_sub_commands: bool,
}
