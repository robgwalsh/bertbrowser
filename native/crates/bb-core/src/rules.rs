//! Small pure rule sets that a dialog and the shell obey alike: saved searches, saved workspaces, the
//! flat view's are-you-sure, and the single-instance foreground raise.
//! Spec: Core/Services/{SavedSearches,SavedWorkspaces,FlatView,Foreground}/*.

use crate::FileTime;
use crate::archives::ArchivePath;
use crate::data::{DirSizeResult, SavedSearch, SavedSearchScope};
use crate::format::thousands;
use crate::layout::{SessionLayout, SessionLayoutRules};
use crate::search::SearchGrammar;

/// What running a saved search means: go to `navigate_to` first, then put `text` in the whole-PC box
/// (`global`) or the folder box. Always "navigate, then type", even when the folder is the one
/// already showing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedSearchRun {
    pub navigate_to: String,
    pub text: String,
    pub global: bool,
}

impl SavedSearchRun {
    pub fn new(navigate_to: &str, text: &str, global: bool) -> SavedSearchRun {
        SavedSearchRun {
            navigate_to: navigate_to.to_string(),
            text: text.to_string(),
            global,
        }
    }
}

pub struct SavedSearchRules;

impl SavedSearchRules {
    pub const MAX_DEFAULT_NAME_LENGTH: usize = 40;

    /// The first reason the search cannot be saved, in words for the user, or None when it can.
    /// `name_taken` excludes the search being edited; `is_archive_file` is the probe
    /// `ArchivePath::parse` asks, so a real folder called `x.zip` can still be pinned while a folder
    /// *inside* an archive cannot.
    pub fn validate(
        name: &str,
        query: &str,
        scope: SavedSearchScope,
        scope_path: Option<&str>,
        name_taken: impl Fn(&str) -> bool,
        is_archive_file: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Some("Give the search a name.".into());
        }
        if name_taken(trimmed) {
            return Some(format!(
                "There is already a saved search called \"{trimmed}\"."
            ));
        }
        // Blank is told apart first; the remaining "neither" is the too-broad case, which as a saved
        // search would show the folder listing and look like a bug.
        if query.trim().is_empty() {
            return Some("Type a search to save.".into());
        }
        let parsed = SearchGrammar::parse(query, true);
        if let Some(problem) = parsed.problem {
            return Some(problem);
        }
        if parsed.query.is_none() {
            return Some(
                "Too broad to save: type at least two characters, or a filter such as ext:jpg."
                    .into(),
            );
        }
        if scope == SavedSearchScope::Folder {
            let Some(path) = scope_path.filter(|p| !p.trim().is_empty()) else {
                return Some("Choose the folder to search in.".into());
            };
            if ArchivePath::parse(path, is_archive_file).is_some() {
                return Some("A folder inside an archive can't be pinned.".into());
            }
        } else if scope_path.is_some() {
            return Some("Only a pinned search has a folder.".into());
        }
        None
    }

    /// The name a new saved search starts with: the query itself, tidied and capped, so the common
    /// case is pressing Enter.
    pub fn default_name(query: &str) -> String {
        let collapsed = query.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            return "Saved search".to_string();
        }
        if collapsed.chars().count() <= Self::MAX_DEFAULT_NAME_LENGTH {
            return collapsed;
        }
        let head: String = collapsed
            .chars()
            .take(Self::MAX_DEFAULT_NAME_LENGTH - 1)
            .collect();
        format!("{}\u{2026}", head.trim_end())
    }

    /// Where and how to run `search` from a tab showing `current_path`, or None when there is nowhere
    /// to run it: a tab with no folder cannot run any search, even a whole-PC one.
    pub fn plan(search: &SavedSearch, current_path: &str) -> Option<SavedSearchRun> {
        match search.scope {
            SavedSearchScope::Folder => {
                let path = search.scope_path.as_deref().filter(|p| !p.is_empty())?;
                Some(SavedSearchRun::new(path, &search.query, false))
            }
            SavedSearchScope::CurrentFolder if !current_path.is_empty() => {
                Some(SavedSearchRun::new(current_path, &search.query, false))
            }
            SavedSearchScope::ThisPc if !current_path.is_empty() => {
                Some(SavedSearchRun::new(current_path, &search.query, true))
            }
            _ => None,
        }
    }
}

pub struct SavedWorkspaceRules;

impl SavedWorkspaceRules {
    pub const MAX_NAME_LENGTH: usize = 60;

    /// The first reason the workspace cannot be saved, in words for the user, or None when it can.
    pub fn validate(name: &str, name_taken: impl Fn(&str) -> bool) -> Option<String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Some("Give the workspace a name.".into());
        }
        if trimmed.chars().count() > Self::MAX_NAME_LENGTH {
            return Some(format!(
                "Keep the name under {} characters.",
                Self::MAX_NAME_LENGTH
            ));
        }
        if name_taken(trimmed) {
            return Some(format!(
                "There is already a workspace called \"{trimmed}\"."
            ));
        }
        None
    }

    /// The name a new workspace starts with: simply when it was saved (local wall-clock ticks).
    pub fn default_name(now_local: FileTime) -> String {
        let c = now_local.civil();
        format!(
            "Workspace {:04}-{:02}-{:02} {:02}:{:02}",
            c.year, c.month, c.day, c.hour, c.minute
        )
    }

    /// "2 panes, 5 tabs": the one-line shape of a layout.
    pub fn shape_text(layout: &SessionLayout) -> String {
        let panes = SessionLayoutRules::count_panes(layout);
        let tabs: usize = SessionLayoutRules::panes(layout)
            .iter()
            .map(|p| p.tabs.as_ref().map_or(0, Vec::len))
            .sum();
        format!(
            "{panes} pane{}, {tabs} tab{}",
            if panes == 1 { "" } else { "s" },
            if tabs == 1 { "" } else { "s" }
        )
    }

    /// Every folder the layout opens, in pane then tab order, each once (ignoring case).
    pub fn folders(layout: &SessionLayout) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut folders = Vec::new();
        for pane in SessionLayoutRules::panes(layout) {
            for tab in pane.tabs.iter().flatten() {
                if !tab.path.is_empty() && seen.insert(bb_paths::fold_upper(&tab.path)) {
                    folders.push(tab.path.clone());
                }
            }
        }
        folders
    }
}

/// Whether to list a subtree flat straight away, or ask first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatViewPreflight {
    /// True when the user should be asked before anything is listed.
    pub confirm: bool,
    /// What to ask, in words. Empty when there is nothing to ask.
    pub message: String,
    /// What the button that goes ahead should say.
    pub confirm_label: String,
}

impl FlatViewPreflight {
    pub fn run() -> FlatViewPreflight {
        FlatViewPreflight {
            confirm: false,
            message: String::new(),
            confirm_label: String::new(),
        }
    }
}

/// The decisions a flat branch view makes before it walks anything.
pub struct FlatViewRules;

impl FlatViewRules {
    /// Whether to go ahead, given what the index already knows about the folder's size. **None means
    /// go ahead**: a missing row means unknown, the standing rule for that table, so there is nothing
    /// to warn about and the cap plus its banner carry it instead.
    pub fn decide(
        display_path: &str,
        estimate: Option<&DirSizeResult>,
        cap: i64,
    ) -> FlatViewPreflight {
        let Some(estimate) = estimate else {
            return FlatViewPreflight::run();
        };
        // Files only, because that is all a flat view lists.
        let count = i64::from(estimate.file_count);
        if count <= cap {
            return FlatViewPreflight::run();
        }
        // "at least", because an incomplete row is a floor.
        let floor = if estimate.incomplete { "at least " } else { "" };
        FlatViewPreflight {
            confirm: true,
            message: format!(
                "\"{display_path}\" holds {floor}{} files. A flat view lists the first {} of them, which will take a moment.",
                thousands(count),
                thousands(cap)
            ),
            confirm_label: format!("Show {}", thousands(cap)),
        }
    }
}

/// The shell's answer to "is now a good moment to interrupt the user?": the values
/// `SHQueryUserNotificationState` returns, plus `Unknown` for a call that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserNotificationState {
    /// The query failed. Treated as permissive: a raise the user asked for must not be lost.
    Unknown = 0,
    NotPresent = 1,
    /// A full-screen application is running, or presentation settings are applied.
    Busy = 2,
    /// A full-screen Direct3D application has the display.
    RunningD3DFullScreen = 3,
    PresentationMode = 4,
    AcceptsNotifications = 5,
    QuietTime = 6,
    /// A Windows Store app is running. Says nothing about full screen on its own.
    App = 7,
}

impl UserNotificationState {
    pub fn from_i32(v: i32) -> UserNotificationState {
        match v {
            1 => UserNotificationState::NotPresent,
            2 => UserNotificationState::Busy,
            3 => UserNotificationState::RunningD3DFullScreen,
            4 => UserNotificationState::PresentationMode,
            5 => UserNotificationState::AcceptsNotifications,
            6 => UserNotificationState::QuietTime,
            7 => UserNotificationState::App,
            _ => UserNotificationState::Unknown,
        }
    }
}

/// What to do with a window a hand-off has just filled with the folder someone asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaiseAction {
    /// Restore it and take the foreground.
    Raise,
    /// Leave it exactly where it is and flash its taskbar button instead.
    Flash,
}

/// Whether the single-instance hand-off may pull the window to the front. **Never steal the
/// foreground from a full-screen app**: the app is the registered handler for `Directory` and
/// `Drive`, so anything on the system that shell-opens a folder starts a second copy, and the running
/// copy would land on top of a full-screen video. The request is still honoured; only the raise is
/// withheld. Spec: Core/Services/Foreground/ForegroundRaiseRules.cs.
pub struct ForegroundRaiseRules;

impl ForegroundRaiseRules {
    /// `foreground_is_ours`: the foreground window already belongs to this process, so there is
    /// nothing to interrupt. `foreground_covers_monitor`: only consulted for a Store app, which
    /// `App` alone cannot distinguish from one in a window.
    pub fn decide(
        state: UserNotificationState,
        foreground_is_ours: bool,
        foreground_covers_monitor: bool,
    ) -> RaiseAction {
        if foreground_is_ours {
            return RaiseAction::Raise;
        }
        match state {
            UserNotificationState::Busy
            | UserNotificationState::RunningD3DFullScreen
            | UserNotificationState::PresentationMode => RaiseAction::Flash,
            UserNotificationState::App if foreground_covers_monitor => RaiseAction::Flash,
            _ => RaiseAction::Raise,
        }
    }
}
