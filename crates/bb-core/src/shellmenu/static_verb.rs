//! The program to start for a static verb, for one target.
//!
//! Reuses `ShellOpenCommandParser::parse_with`, which already splits a registered command into its program
//! and arguments and substitutes the shell's placeholders — `%1`, `%L`, `%V` and friends all mean
//! "the thing clicked", which is the item for an item verb and the folder for a background one.
//! `%W` is the one placeholder that means something else (the working directory) and is
//! substituted here first, so the parser's own rules stay as its other caller, the
//! run-as-administrator path, relies on them.
//!
//! Unlike that caller, a verb with no placeholder at all is still run: a "Git Bash Here" that
//! takes its folder from the working directory is a legitimate registration, and there is no
//! token in hand to be careful with.
//! Spec: Core/Services/ShellMenu/StaticVerbCommand.cs.

use bb_paths::root_len;

use super::{ShellMenuContext, ShellMenuTarget, ShellOpenCommand};

pub struct StaticVerbCommand;

impl StaticVerbCommand {
    /// `command` is the verb's registered command line, environment variables already expanded;
    /// `target_path` the item, or the folder for a background verb; `working_directory` the
    /// folder the menu was over; `exists` whether a path names a file — injected so this is
    /// testable without a disk, as the parser's own is.
    pub fn resolve(
        command: Option<&str>,
        target_path: &str,
        working_directory: &str,
        exists: impl Fn(&str) -> bool,
    ) -> Option<ShellOpenCommand> {
        let command = command.filter(|c| !c.trim().is_empty())?;
        let expanded = command
            .replace("%W", working_directory)
            .replace("%w", working_directory);
        crate::shell_integration::ShellOpenCommandParser::parse_with(
            Some(&expanded),
            target_path,
            exists,
            false,
        )
    }

    /// Where a verb runs: the folder for a background verb or a folder item, and the item's own
    /// folder otherwise.
    pub fn working_directory_for(
        target: &ShellMenuTarget,
        context: ShellMenuContext,
        folder: &str,
    ) -> String {
        if context == ShellMenuContext::Background || target.is_directory {
            return target.full_path.clone();
        }
        directory_name(&target.full_path).unwrap_or_else(|| folder.to_string())
    }
}

/// `Path.GetDirectoryName`: the path without its last component, the root kept whole
/// (`C:\a.cs` → `C:\`), None at a root.
fn directory_name(path: &str) -> Option<String> {
    let root = root_len(path);
    if path.len() <= root {
        return None;
    }
    Some(match path.rfind(['\\', '/']) {
        Some(i) if i < root => path[..root].to_string(),
        Some(i) => path[..i].to_string(),
        None => String::new(),
    })
}
