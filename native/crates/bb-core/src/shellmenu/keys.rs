//! Which `HKEY_CLASSES_ROOT` keys contribute context-menu entries for a selection — the same list
//! Explorer's default menu builds, so an extension that shows up there shows up here.
//!
//! The first item decides the type-specific keys, as it does in Explorer: a mixed selection is
//! asked about as whatever came first, and the handlers themselves look at the whole selection
//! through the data object. `AllFilesystemObjects` is always last, because it is where the
//! extensions that apply to anything on disk register.
//! Spec: Core/Services/ShellMenu/ShellMenuKeys.cs, ShellMenuTarget.cs.

use bb_paths::{fold_upper, root_len};

use super::eq_ignore_case;

/// One thing a shell context menu is asked about: a selected row, or the folder itself when the
/// right-click landed on empty space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellMenuTarget {
    pub full_path: String,
    pub is_directory: bool,
}

impl ShellMenuTarget {
    pub fn new(full_path: &str, is_directory: bool) -> ShellMenuTarget {
        ShellMenuTarget {
            full_path: full_path.to_string(),
            is_directory,
        }
    }
}

/// What the menu is over. The shell registers extensions separately for "these items" and for
/// "this folder's background", and a Git Bash Here belongs to the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShellMenuContext {
    Items,
    Background,
}

/// What the registry says about a file extension — the ProgID it maps to and its perceived type
/// — read by the app and handed in raw, so `ShellMenuKeys` decides without opening a key.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShellFileType {
    pub prog_id: Option<String>,
    pub perceived_type: Option<String>,
}

impl ShellFileType {
    pub fn new(prog_id: Option<&str>, perceived_type: Option<&str>) -> ShellFileType {
        ShellFileType {
            prog_id: prog_id.map(str::to_string),
            perceived_type: perceived_type.map(str::to_string),
        }
    }
}

pub struct ShellMenuKeys;

impl ShellMenuKeys {
    pub const STAR: &'static str = "*";
    pub const ALL_FILE_SYSTEM_OBJECTS: &'static str = "AllFilesystemObjects";
    pub const DIRECTORY: &'static str = "Directory";
    pub const FOLDER: &'static str = "Folder";
    pub const DRIVE: &'static str = "Drive";
    pub const BACKGROUND: &'static str = r"Directory\Background";
    pub const SYSTEM_FILE_ASSOCIATIONS: &'static str = "SystemFileAssociations";

    /// The keys that are not tied to one file type — what the Settings page scans so its list does
    /// not depend on what happens to be selected.
    pub const GENERIC: [&'static str; 6] = [
        Self::STAR,
        Self::ALL_FILE_SYSTEM_OBJECTS,
        Self::DIRECTORY,
        Self::FOLDER,
        Self::DRIVE,
        Self::BACKGROUND,
    ];

    /// `type_of` is what the registry says about an extension (".txt"), or None when it says
    /// nothing — injected so this decides without a registry.
    pub fn for_targets(
        targets: &[ShellMenuTarget],
        context: ShellMenuContext,
        type_of: impl Fn(&str) -> Option<ShellFileType>,
    ) -> Vec<String> {
        if context == ShellMenuContext::Background {
            return vec![Self::BACKGROUND.to_string()];
        }
        let Some(first) = targets.first() else {
            return vec![];
        };

        let mut keys: Vec<String> = Vec::new();
        if first.is_directory {
            keys.push(
                if is_drive_root(&first.full_path) {
                    Self::DRIVE
                } else {
                    Self::DIRECTORY
                }
                .to_string(),
            );
            keys.push(Self::FOLDER.to_string());
        } else {
            let extension = extension_of(&first.full_path);
            if extension.len() > 1 {
                let file_type = type_of(extension);
                if let Some(prog_id) = file_type
                    .as_ref()
                    .and_then(|t| t.prog_id.as_deref())
                    .filter(|p| !p.is_empty())
                {
                    keys.push(prog_id.to_string());
                }
                keys.push(format!("{}\\{extension}", Self::SYSTEM_FILE_ASSOCIATIONS));
                if let Some(perceived) = file_type
                    .as_ref()
                    .and_then(|t| t.perceived_type.as_deref())
                    .filter(|p| !p.is_empty())
                {
                    keys.push(format!("{}\\{perceived}", Self::SYSTEM_FILE_ASSOCIATIONS));
                }
            }
            keys.push(Self::STAR.to_string());
        }

        keys.push(Self::ALL_FILE_SYSTEM_OBJECTS.to_string());

        let mut seen = std::collections::HashSet::new();
        keys.retain(|k| seen.insert(fold_upper(k)));
        keys
    }
}

/// `Path.GetExtension`: from the last dot of the file name, including it; empty when there is none
/// or it is the last character.
fn extension_of(path: &str) -> &str {
    let name = bb_paths::file_name(path);
    match name.rfind('.') {
        Some(i) if i + 1 < name.len() => &name[i..],
        _ => "",
    }
}

fn is_drive_root(path: &str) -> bool {
    let root = &path[..root_len(path)];
    !root.is_empty()
        && eq_ignore_case(
            root.trim_end_matches(['\\', '/']),
            path.trim_end_matches(['\\', '/']),
        )
}
