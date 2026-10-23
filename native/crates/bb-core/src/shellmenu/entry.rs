//! One line of the shell's contribution to a menu, in the form the view renders: text ready for a
//! header, its children, and an opaque handle the app's session turns back into an invocation.
//! `C` (the command handle) and `I` (the icon) are the caller's types, so this crate holds no COM
//! or UI type: the app wraps a handler and a command id in the first and a picture in the second,
//! and the harness puts whatever it likes in both.
//! Spec: Core/Services/ShellMenu/ShellMenuEntry.cs.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellMenuEntry<C, I = ()> {
    pub header: String,
    pub is_separator: bool,
    pub is_enabled: bool,
    pub children: Vec<ShellMenuEntry<C, I>>,
    pub command: Option<C>,
    pub icon: Option<I>,
}

impl<C, I> ShellMenuEntry<C, I> {
    pub fn separator() -> Self {
        ShellMenuEntry {
            header: String::new(),
            is_separator: true,
            is_enabled: false,
            children: vec![],
            command: None,
            icon: None,
        }
    }

    pub fn item(header: &str, command: C) -> Self {
        ShellMenuEntry {
            header: header.to_string(),
            is_separator: false,
            is_enabled: true,
            children: vec![],
            command: Some(command),
            icon: None,
        }
    }

    pub fn submenu(header: &str, children: Vec<ShellMenuEntry<C, I>>) -> Self {
        ShellMenuEntry {
            header: header.to_string(),
            is_separator: false,
            is_enabled: true,
            children,
            command: None,
            icon: None,
        }
    }

    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

/// What one extension offers for one opening of a menu, kept together under its
/// `ShellExtension::id` so the menu can put it wherever the user placed that extension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellMenuGroup<C, I = ()> {
    pub extension_id: String,
    pub entries: Vec<ShellMenuEntry<C, I>>,
}
