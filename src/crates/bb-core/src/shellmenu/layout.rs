//! The order of the app's right-click menus, as the Context menu page arranges it: one list of
//! tokens shared by the file list's and the folder tree's menus, each of which shows the entries it
//! has and skips the rest.
//!
//! The layout decides *order only*. Whether an entry is shown stays where it always lived — the
//! hidden lists and a command's `show_in_menu` — so an arrangement from an older or newer build can
//! never un-hide anything, and a built-in added by a later build appears at its default spot
//! instead of nowhere (`normalize`).
//!
//! `MORE` is where everything not placed yet goes: a command made before the menu could be
//! arranged, and, above all, a shell extension installed since the user last visited the page.
//! "Shown unless unticked" is the shell section's rule, and a token list with no catch-all would
//! quietly turn it into "hidden unless placed".
//! Spec: Core/Services/ShellMenu/MenuLayoutRules.cs.

use std::collections::HashSet;

use bb_paths::fold_upper;

use super::eq_ignore_case;

/// What one position of an arranged menu holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuPlacementKind {
    BuiltIn,
    Separator,
    Command,
    Shell,
}

/// One position of a menu as `MenuLayoutRules::arrange` lays it out. The id is the entry's own
/// (`cut`, a command's id, `clsid:{…}`), without its token prefix.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MenuPlacement {
    pub kind: MenuPlacementKind,
    pub id: String,
}

impl MenuPlacement {
    pub fn new(kind: MenuPlacementKind, id: &str) -> MenuPlacement {
        MenuPlacement {
            kind,
            id: id.to_string(),
        }
    }

    pub fn separator() -> MenuPlacement {
        MenuPlacement::new(MenuPlacementKind::Separator, "")
    }
}

/// The menu as it was before it could be arranged: the file list's order and groups, with the
/// user's commands and other programs' entries where their anchors were.
const DEFAULT: &[&str] = &[
    "app:new",
    MenuLayoutRules::SEPARATOR_TOKEN,
    "app:open",
    "app:run-as-admin",
    "app:open-new-tab",
    "app:open-new-pane",
    "app:open-terminal",
    "app:open-vscode",
    "app:disk-usage",
    "app:duplicates",
    "app:changes",
    "app:compare-panes",
    "app:compress",
    "app:extract",
    MenuLayoutRules::SEPARATOR_TOKEN,
    "app:copy-path",
    "app:copy-name",
    "app:compare-files",
    "app:settle-by-content",
    "app:verify-checksums",
    "app:checksum",
    MenuLayoutRules::SEPARATOR_TOKEN,
    "app:cut",
    "app:copy",
    "app:paste",
    MenuLayoutRules::SEPARATOR_TOKEN,
    "app:rename",
    "app:delete",
    "app:delete-permanently",
    "app:bookmark",
    MenuLayoutRules::MORE,
    MenuLayoutRules::SEPARATOR_TOKEN,
    "app:properties",
];

pub struct MenuLayoutRules;

impl MenuLayoutRules {
    pub const BUILT_IN_PREFIX: &'static str = "app:";
    pub const COMMAND_PREFIX: &'static str = "cmd:";
    pub const SEPARATOR_TOKEN: &'static str = "-";
    pub const MORE: &'static str = "@more";

    pub fn built_in_token(id: &str) -> String {
        format!("{}{id}", Self::BUILT_IN_PREFIX)
    }

    pub fn command_token(id: &str) -> String {
        format!("{}{id}", Self::COMMAND_PREFIX)
    }

    pub fn default_layout() -> &'static [&'static str] {
        DEFAULT
    }

    pub fn is_separator(token: &str) -> bool {
        token == Self::SEPARATOR_TOKEN
    }

    /// The built-in's id, when the token is one (`app:cut`).
    pub fn is_built_in(token: &str) -> Option<&str> {
        strip(token, Self::BUILT_IN_PREFIX)
    }

    /// The command's id, when the token is one (`cmd:…`).
    pub fn is_command(token: &str) -> Option<&str> {
        strip(token, Self::COMMAND_PREFIX)
    }

    /// Anything that is not one of the app's own kinds is a shell extension's id — `clsid:{…}` or
    /// `verb:name`, as `ShellExtension` assigns them.
    pub fn is_shell(token: &str) -> bool {
        !token.is_empty()
            && !Self::is_separator(token)
            && !eq_ignore_case(token, Self::MORE)
            && Self::is_built_in(token).is_none()
            && Self::is_command(token).is_none()
    }

    /// A stored layout made whole: the default when there is none; every built-in that is neither
    /// placed nor hidden put back after the nearest entry that precedes it by default, and a
    /// missing `MORE` the same way; any repeat of an entry dropped. A token this build does not
    /// recognise is kept, since it may be a later build's.
    pub fn normalize<S: AsRef<str>, H: AsRef<str>>(
        saved: Option<&[S]>,
        hidden_built_ins: impl IntoIterator<Item = H>,
    ) -> Vec<String> {
        let Some(saved) = saved else {
            return DEFAULT.iter().map(|t| t.to_string()).collect();
        };

        let hidden: HashSet<String> = hidden_built_ins
            .into_iter()
            .map(|h| fold_upper(h.as_ref()))
            .collect();
        let mut seen: HashSet<String> = HashSet::new();
        let mut result: Vec<String> = Vec::new();
        for raw in saved {
            let token = raw.as_ref().trim();
            if token.is_empty() {
                continue;
            }
            if Self::is_separator(token) || seen.insert(fold_upper(token)) {
                result.push(token.to_string());
            }
        }

        for (d, token) in DEFAULT.iter().enumerate() {
            if Self::is_separator(token) || seen.contains(&fold_upper(token)) {
                continue;
            }
            if let Some(id) = Self::is_built_in(token)
                && hidden.contains(&fold_upper(id))
            {
                continue;
            }

            let at = insertion_point(&result, d);
            result.insert(at, token.to_string());
            seen.insert(fold_upper(token));
        }

        result
    }

    /// One opening of one menu, in order. Each placed entry appears where the layout puts it when
    /// this menu has it; everything unplaced that this menu has appears at `MORE` — commands
    /// first, then other programs' entries, each run behind a separator, the way the two sections
    /// sat before. Separators are passed through as placed; the view tidies doubled and dangling
    /// ones afterwards, once it knows what is visible.
    ///
    /// `layout` is one from `normalize`; `built_ins` the built-in ids this menu declares;
    /// `commands` the ids of the commands shown and applicable here, in settings order;
    /// `shell_groups` the extensions with something to offer here, in catalog order.
    pub fn arrange<L: AsRef<str>, B: AsRef<str>, C: AsRef<str>, G: AsRef<str>>(
        layout: &[L],
        built_ins: impl IntoIterator<Item = B>,
        commands: &[C],
        shell_groups: &[G],
    ) -> Vec<MenuPlacement> {
        let built_in_set: HashSet<String> = built_ins
            .into_iter()
            .map(|b| fold_upper(b.as_ref()))
            .collect();
        let command_set: HashSet<String> =
            commands.iter().map(|c| fold_upper(c.as_ref())).collect();
        let shell_set: HashSet<String> = shell_groups
            .iter()
            .map(|s| fold_upper(s.as_ref()))
            .collect();

        let mut placed_commands: HashSet<String> = HashSet::new();
        let mut placed_shell: HashSet<String> = HashSet::new();
        for token in layout {
            let token = token.as_ref();
            if let Some(id) = Self::is_command(token) {
                placed_commands.insert(fold_upper(id));
            } else if Self::is_shell(token) {
                placed_shell.insert(fold_upper(token));
            }
        }

        let mut emitted: HashSet<String> = HashSet::new();
        let mut result: Vec<MenuPlacement> = Vec::new();

        for token in layout {
            let token = token.as_ref();
            if Self::is_separator(token) {
                result.push(MenuPlacement::separator());
            } else if let Some(built_in) = Self::is_built_in(token) {
                if built_in_set.contains(&fold_upper(built_in)) && emitted.insert(fold_upper(token))
                {
                    result.push(MenuPlacement::new(MenuPlacementKind::BuiltIn, built_in));
                }
            } else if let Some(command) = Self::is_command(token) {
                if command_set.contains(&fold_upper(command)) && emitted.insert(fold_upper(token)) {
                    result.push(MenuPlacement::new(MenuPlacementKind::Command, command));
                }
            } else if eq_ignore_case(token, Self::MORE) {
                if !emitted.insert(fold_upper(Self::MORE)) {
                    continue;
                }
                emit_run(
                    &mut result,
                    commands
                        .iter()
                        .map(AsRef::as_ref)
                        .filter(|c| !placed_commands.contains(&fold_upper(c))),
                    MenuPlacementKind::Command,
                );
                emit_run(
                    &mut result,
                    shell_groups
                        .iter()
                        .map(AsRef::as_ref)
                        .filter(|s| !placed_shell.contains(&fold_upper(s))),
                    MenuPlacementKind::Shell,
                );
            } else if shell_set.contains(&fold_upper(token)) && emitted.insert(fold_upper(token)) {
                result.push(MenuPlacement::new(MenuPlacementKind::Shell, token));
            }
        }

        result
    }
}

fn emit_run<'a>(
    result: &mut Vec<MenuPlacement>,
    ids: impl Iterator<Item = &'a str>,
    kind: MenuPlacementKind,
) {
    let mut first = true;
    for id in ids {
        if first {
            result.push(MenuPlacement::separator());
        }
        first = false;
        result.push(MenuPlacement::new(kind, id));
    }
}

/// Just after the nearest entry that comes before `DEFAULT[index]` by default and is present; the
/// top when none is.
fn insertion_point(result: &[String], index: usize) -> usize {
    for before in DEFAULT[..index].iter().rev() {
        if MenuLayoutRules::is_separator(before) {
            continue;
        }
        if let Some(at) = result.iter().position(|t| eq_ignore_case(t, before)) {
            return at + 1;
        }
    }

    // Nothing before it is present: put it ahead of the first default entry that follows and is.
    for after in &DEFAULT[index + 1..] {
        if MenuLayoutRules::is_separator(after) {
            continue;
        }
        if let Some(at) = result.iter().position(|t| eq_ignore_case(t, after)) {
            return at;
        }
    }

    result.len()
}

/// The token past an ASCII prefix, compared ignoring case.
fn strip<'a>(token: &'a str, prefix: &str) -> Option<&'a str> {
    let head = token.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &token[prefix.len()..])
}
