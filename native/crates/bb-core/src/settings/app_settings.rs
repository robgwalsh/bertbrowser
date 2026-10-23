//! The settings file: every persisted preference with its default, and the few answers derived
//! from them. In the C# app this lived in the App project; here it is Core, since nothing in it is
//! about a window. The file is a fresh camelCase JSON document — no compatibility with the C#
//! app's settings.json is kept (see `legacy`). Spec: App/Services/AppSettings.cs.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::placeholders::{
    ChecksumAlgorithm, CommandBarSet, CustomCommandDefinition, NewFileTemplate, RenameRule,
    SavedCommandBarSet, ThemeId, UndoBudget,
};
use crate::FileTime;
use crate::changes::ChangeLogPolicy;
use crate::columns::ColumnSetting;
use crate::commands::Overrides;
use crate::layout::SessionLayout;

// TODO(port): the parsed AspectRatio type arrives with Models; the setting is its text form.
/// 4:3, matching the shape most photos and video thumbnails already are.
pub const DEFAULT_TILE_ASPECT_RATIO: &str = "4:3";

/// Where middle-clicking a drive/device row in the sidebar tree opens it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DrivesOpenTarget {
    #[default]
    NewTab,
    NewPanel,
}

/// Where a list of saved things — workspaces, saved searches — is offered. Settings lists them
/// whichever is chosen, so `Hidden` takes the shortcut away without taking away the way back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SectionPlacement {
    /// The list's own sidebar section, as it always was.
    #[default]
    Sidebar,
    /// A dropdown in the title bar, beside the app's name.
    TitleBar,
    Hidden,
}

#[derive(Debug)]
pub enum SettingsError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsError::Io(e) => write!(f, "settings file: {e}"),
            SettingsError::Json(e) => write!(f, "settings json: {e}"),
        }
    }
}

impl std::error::Error for SettingsError {}

impl From<std::io::Error> for SettingsError {
    fn from(e: std::io::Error) -> Self {
        SettingsError::Io(e)
    }
}

impl From<serde_json::Error> for SettingsError {
    fn from(e: serde_json::Error) -> Self {
        SettingsError::Json(e)
    }
}

/// Every persisted preference. A missing field takes its default and an unknown one is ignored, so
/// a file from a newer or older build, or a hand-edited one, loads rather than failing.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppSettings {
    pub window_left: Option<f64>,
    pub window_top: Option<f64>,
    pub window_width: Option<f64>,
    pub window_height: Option<f64>,
    pub window_maximized: bool,
    pub last_path: Option<String>,

    /// The pane and tab arrangement from the last session. None means never saved, and a first
    /// launch then behaves exactly as it always did; `last_path` stays as the fallback for that
    /// case, and for a session too damaged to restore.
    pub session: Option<SessionLayout>,

    /// When true (default), startup restores `session`. When false, startup ignores `session` and
    /// `last_path` and opens a single pane/single tab at `startup_default_path` instead.
    pub restore_last_session: bool,

    /// Where startup opens when `restore_last_session` is false. None, or a path that no longer
    /// exists, falls back to the user profile.
    pub startup_default_path: Option<String>,

    /// When false (default), hidden files/folders are excluded from listings and search; when true
    /// they appear with a dimmed icon, like Explorer.
    pub show_hidden_items: bool,

    /// Per-directory thumbnail-zoom slider position (0..1), keyed by canonical path. 0 (or absent)
    /// = details list. Only folders explicitly zoomed are stored.
    pub directory_thumbnail_scales: BTreeMap<String, f64>,

    /// Mouse-wheel vertical scroll speed multiplier (1 = OS default). Defaults to 2x.
    pub scroll_speed_multiplier: f64,

    /// Whether a newly opened tab shows its preview pane. Not nullable: "never configured" and
    /// "off" mean the same thing for a pane. Visibility itself is per tab — this is the value a
    /// new tab starts from, and toggling in any tab writes it.
    pub show_preview_pane: bool,

    /// Width of the preview pane, in device-independent pixels. Global rather than per tab on
    /// purpose: panes differ in width, so a remembered per-tab width reads as the splitter moving
    /// on its own.
    pub preview_pane_width: f64,

    /// Whether a newly opened tab shows the metadata pane instead. The two share one column, so
    /// this wins over `show_preview_pane` if a hand-edited file sets both.
    pub show_metadata_pane: bool,

    /// Whether a video that finishes in the preview pane goes on to the next one. Off by default:
    /// selecting a file should never start a chain of playback on its own.
    pub preview_auto_advance: bool,

    /// Whether a video that finishes in the preview pane starts over. The alternative to
    /// `preview_auto_advance` — at most one of the two is ever on.
    pub preview_loop: bool,

    /// Whether selecting a video starts playing it, rather than stopping at its poster frame.
    pub preview_auto_play: bool,

    /// The file list's columns, in order, with their widths. None means never configured, which
    /// ships the catalogue defaults; an empty list means stripped back on purpose. What a *new*
    /// tab starts from — the column set itself is per tab.
    pub file_list_columns: Option<Vec<ColumnSetting>>,

    /// How much of a text file the preview reads. The rest is not shown and the pane says so.
    pub preview_text_max_bytes: i32,

    /// How much of each file a `content:` search reads. The one content-search bound a person
    /// might genuinely want to move, because it is about their files rather than about the app.
    /// **The other ceilings are deliberately not settings** — they are safety valves measured
    /// against real hardware, not preferences.
    pub search_content_max_bytes: i32,

    /// The floor the duplicate finder starts from, in bytes. Not cosmetic: it bounds both the
    /// shortlist's memory and how much has to be read.
    pub duplicate_min_size_bytes: i64,

    /// Which algorithms the checksum window starts with, comma-separated (`"Sha256"`,
    /// `"Md5,Sha256"`). A string so the file stays hand-editable; unparseable names are dropped
    /// rather than resetting the lot. A tool's own state, not a preference about the app.
    pub checksum_algorithms: String,

    /// Whether the index helper keeps a log of file changes for the "What changed" window.
    /// **Off by default, and deliberately**: a record of every file created, changed, deleted or
    /// renamed on every drive is sensitive. Nothing is written until the user turns this on, and
    /// turning it off deletes what was kept. **With this on, recording continues while the app is
    /// closed** — the helper keeps the last policy pushed, so the timeline has no holes.
    pub record_file_changes: bool,

    /// Whether launching the app should start the index helper — and therefore ask for
    /// administrator rights — when one is not already running. Off by default: the usual launch
    /// attaches to a helper that is already there, and a banner offers to start one otherwise.
    pub start_indexer_at_launch: bool,

    /// How long recorded changes are kept, in hours — one of `ChangeLogPolicy::RETENTION_OPTIONS`.
    pub file_change_retention_hours: i32,

    /// The "Mark now" timestamp the change timeline's "since the mark" range counts from; None
    /// until pressed. Kept across sessions so an installer that needs a reboot can still be
    /// looked at afterwards.
    pub change_timeline_mark_utc: Option<FileTime>,

    /// How many operations the undo history keeps; None for `UndoBudget::DEFAULT_ENTRIES`.
    pub undo_max_steps: Option<i32>,

    /// How much set-aside data the undo history may hold, in gigabytes; None for
    /// `UndoBudget::DEFAULT_GIGABYTES`. On by default, unlike the change log, because nothing it
    /// keeps outlives the session.
    pub undo_max_held_gigabytes: Option<i32>,

    /// Whether the duplicate finder leaves Windows and Program Files out. On by default: those
    /// trees are largely one file under several names, and what genuine duplicates they hold are
    /// not the user's to remove.
    pub duplicate_skip_system_folders: bool,

    /// Shape of a thumbnail tile as "width:height". Anything unparseable falls back to 4:3, so
    /// hand-editing this cannot break the view.
    pub tile_aspect_ratio: String,

    pub custom_commands: Vec<CustomCommandDefinition>,

    /// Keyboard shortcuts the user has changed, by command id, each with the chords it now has.
    /// None means nothing was ever changed. Only the changes, never the whole keymap: a command
    /// absent from here keeps the shortcut it ships with, so a later build's new default still
    /// arrives. An empty list means the shortcut was taken away on purpose. Written only by the
    /// keymap service, never by the settings page's debounce.
    pub key_bindings: Option<Overrides>,

    /// What the user pinned to the top of the command palette, in the order they pinned it. A
    /// preference, not a history — nothing gets here except by pressing Pin — which is why it is
    /// kept while the palette's "recent" list is not.
    pub palette_pins: Vec<String>,

    /// Whether other programs' right-click entries — 7-Zip, Git, TortoiseSVN — appear in the
    /// menus. On by default: nothing is recorded, and an extension the user installed is one they
    /// expect to see.
    pub show_shell_extensions: bool,

    /// Extensions unticked on the Context Menu page, by stable id (`clsid:{…}` or `verb:name`).
    /// Everything not listed is shown, so a newly installed extension appears without a visit to
    /// Settings.
    pub hidden_shell_extensions: Vec<String>,

    /// The app's own entries unticked on the Context Menu page (`copy-path`, `open-terminal`…).
    /// An id this build does not know is kept rather than dropped.
    pub hidden_built_in_menu_items: Vec<String>,

    /// The order of both right-click menus, as layout tokens (`app:cut`, `-`, `cmd:<id>`,
    /// `clsid:{…}`, `@more`). None means never arranged. Order only: whether an entry is shown is
    /// still the hidden lists above.
    pub context_menu_layout: Option<Vec<String>>,

    /// The menu bar and the toolbars. None means never arranged — or arranged back to what ships
    /// — so a later build's additions arrive by themselves. Written only by the command-bar
    /// service.
    pub command_bars: Option<CommandBarSet>,

    /// Arrangements of the menu bar and toolbars saved under a name, in name order. A preference
    /// like `palette_pins`, which is why it is here and not in the database.
    pub saved_command_bars: Vec<SavedCommandBarSet>,

    /// The file types on the "New" submenu, in menu order. None means never configured, which
    /// ships `NewFileTemplate::defaults()`; an empty list means they removed them all on purpose.
    pub new_file_types: Option<Vec<NewFileTemplate>>,

    /// Active theme when `follow_system_theme` is off. None means the user has never picked one,
    /// which is what lets a first launch pick for itself rather than overriding a Windows setting.
    /// Left alone while following, so unticking the checkbox restores exactly the theme that was
    /// in use before.
    pub theme_id: Option<ThemeId>,

    /// Whether the theme follows the Windows light/dark and high-contrast settings, live. None
    /// means never configured, and the answer for None is `loaded_from_disk` — a brand-new
    /// install follows, and anyone who already had a settings file does not (see
    /// [`follows_system_theme`](Self::follows_system_theme)). The theme service writes its answer
    /// straight back, so this is None exactly once per install.
    pub follow_system_theme: Option<bool>,

    /// The theme used while Windows is in light mode and following. None means Light+. Ignored
    /// entirely when not following — it is not a second copy of `theme_id` and never overwrites it.
    pub light_theme_id: Option<ThemeId>,

    /// As `light_theme_id`, for dark mode. None means Dark+. High contrast has no slot: it always
    /// resolves to the accessibility theme.
    pub dark_theme_id: Option<ThemeId>,

    /// Whether these settings came from a file rather than from the defaults. Not persisted — it
    /// is a fact about this load, and the only way to tell a brand-new install from a user who has
    /// never changed anything. A file too damaged to parse counts as new, which is the right
    /// answer: every other preference has just been lost too.
    #[serde(skip)]
    loaded_from_disk: bool,

    /// Per-token colour tweaks, keyed by theme id and then by token. Kept per theme so switching
    /// away and back does not discard the edits made to either one.
    pub theme_overrides: BTreeMap<ThemeId, BTreeMap<String, String>>,

    /// Whether the rename dialog opens with its options panel already showing.
    pub advanced_rename_expanded: bool,

    /// The rename options to bring back next time the panel is opened, or None when it never has
    /// been. **The knobs persist; the text does not**: the template, find and replace are blanked
    /// before this is written, because they are the content of one rename rather than a setting —
    /// a regular expression from last week sitting behind F2 is a trap. The plain box never reads
    /// this.
    pub advanced_rename: Option<RenameRule>,

    /// Whether double-clicking a zip, 7z, tar or rar walks into it instead of launching it. On by
    /// default, which is what Explorer, Directory Opus, Total Commander and Files all do.
    pub enter_archives_on_double_click: bool,

    /// Where middle-clicking a drive/device opens it. Defaults to a new tab, matching every other
    /// "open elsewhere" action in the app.
    pub drives_open_target: DrivesOpenTarget,

    /// Where the workspace switcher lives. Not nullable: "never configured" and the sidebar mean
    /// the same thing.
    pub workspaces_placement: SectionPlacement,

    /// Where saved searches are offered for running, with the same default as
    /// `workspaces_placement`.
    pub saved_searches_placement: SectionPlacement,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            window_left: None,
            window_top: None,
            window_width: None,
            window_height: None,
            window_maximized: false,
            last_path: None,
            session: None,
            restore_last_session: true,
            startup_default_path: None,
            show_hidden_items: false,
            directory_thumbnail_scales: BTreeMap::new(),
            scroll_speed_multiplier: 2.0,
            show_preview_pane: false,
            preview_pane_width: 360.0,
            show_metadata_pane: false,
            preview_auto_advance: false,
            preview_loop: false,
            preview_auto_play: false,
            file_list_columns: None,
            preview_text_max_bytes: 1024 * 1024,
            search_content_max_bytes: 1024 * 1024,
            duplicate_min_size_bytes: 1024 * 1024,
            checksum_algorithms: ChecksumAlgorithm::Sha256.name().to_string(),
            record_file_changes: false,
            start_indexer_at_launch: false,
            file_change_retention_hours: ChangeLogPolicy::DEFAULT_RETENTION_HOURS,
            change_timeline_mark_utc: None,
            undo_max_steps: None,
            undo_max_held_gigabytes: None,
            duplicate_skip_system_folders: true,
            tile_aspect_ratio: DEFAULT_TILE_ASPECT_RATIO.to_string(),
            custom_commands: Vec::new(),
            key_bindings: None,
            palette_pins: Vec::new(),
            show_shell_extensions: true,
            hidden_shell_extensions: Vec::new(),
            hidden_built_in_menu_items: Vec::new(),
            context_menu_layout: None,
            command_bars: None,
            saved_command_bars: Vec::new(),
            new_file_types: None,
            theme_id: None,
            follow_system_theme: None,
            light_theme_id: None,
            dark_theme_id: None,
            loaded_from_disk: false,
            theme_overrides: BTreeMap::new(),
            advanced_rename_expanded: false,
            advanced_rename: None,
            enter_archives_on_double_click: true,
            drives_open_target: DrivesOpenTarget::NewTab,
            workspaces_placement: SectionPlacement::Sidebar,
            saved_searches_placement: SectionPlacement::Sidebar,
        }
    }
}

impl AppSettings {
    /// The settings in `path`, or the defaults when there is no such file, it cannot be read, or it
    /// does not parse. Only a file that parsed sets `loaded_from_disk`.
    pub fn load(path: &Path) -> AppSettings {
        let Ok(text) = std::fs::read_to_string(path) else {
            return AppSettings::default();
        };
        match Self::from_json(&text) {
            Ok(mut loaded) => {
                loaded.loaded_from_disk = true;
                loaded
            }
            Err(_) => AppSettings::default(),
        }
    }

    /// Parses the file's text. A JSON `null` is not a settings document. Says nothing about a disk:
    /// `loaded_from_disk` stays false.
    pub fn from_json(text: &str) -> Result<AppSettings, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The file's text, indented, in the order the fields are declared.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("settings serialize")
    }

    /// Writes the file, creating its directory.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_json())?;
        Ok(())
    }

    /// Whether these settings came from a file rather than from the defaults.
    pub fn loaded_from_disk(&self) -> bool {
        self.loaded_from_disk
    }

    /// Whether the theme follows Windows: the stored answer, or for a never-configured install
    /// `!loaded_from_disk` — matching Windows is the default only because there is no settings
    /// file. Not `theme_id.is_none()`, which every existing user who never opened Settings also
    /// has, and flipping all of them on upgrade is exactly the surprise.
    pub fn follows_system_theme(&self) -> bool {
        self.follow_system_theme.unwrap_or(!self.loaded_from_disk)
    }

    /// The recording settings as the one value the index service takes. A stored retention that is
    /// no longer on the menu falls back to the default rather than failing at startup.
    pub fn effective_change_log_policy(&self) -> ChangeLogPolicy {
        if !self.record_file_changes {
            return ChangeLogPolicy::OFF;
        }
        let hours = self.file_change_retention_hours;
        let hours = if ChangeLogPolicy::is_acceptable_hours(hours) && hours > 0 {
            hours
        } else {
            ChangeLogPolicy::DEFAULT_RETENTION_HOURS
        };
        ChangeLogPolicy::from_hours(hours).unwrap_or(ChangeLogPolicy::OFF)
    }

    /// The two undo settings as the budget the history enforces.
    pub fn effective_undo_budget(&self) -> UndoBudget {
        UndoBudget::from_settings(self.undo_max_steps, self.undo_max_held_gigabytes)
    }

    /// The file-list columns as the list will show them: the user's layout, or the defaults when
    /// nothing has been customised, with every row run through the layout rules so a hand-edited
    /// or stale setting cannot produce a list with no Name column or a width of zero.
    pub fn resolved_file_list_columns(&self) -> Vec<ColumnSetting> {
        crate::columns::ColumnLayoutRules::normalize(self.file_list_columns.as_deref())
    }

    /// `checksum_algorithms` as the enum, never empty — an unreadable or empty setting falls back
    /// to SHA-256 rather than opening a window that computes nothing.
    pub fn resolved_checksum_algorithms(&self) -> Vec<ChecksumAlgorithm> {
        let parsed: Vec<ChecksumAlgorithm> = self
            .checksum_algorithms
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .filter_map(ChecksumAlgorithm::parse_ignore_case)
            .collect();

        if parsed.is_empty() {
            vec![ChecksumAlgorithm::Sha256]
        } else {
            ChecksumAlgorithm::normalise(parsed)
        }
    }

    /// The menu entries the "New" submenu should show: what is configured, or the shipped defaults
    /// when nothing ever has been. One place, so the menu, the settings page and the harness cannot
    /// disagree about what is on offer.
    pub fn resolved_new_file_types(&self) -> Vec<NewFileTemplate> {
        self.new_file_types
            .clone()
            .unwrap_or_else(NewFileTemplate::defaults)
            .into_iter()
            .filter(|t| t.enabled && !t.extension.is_empty())
            .collect()
    }
}
