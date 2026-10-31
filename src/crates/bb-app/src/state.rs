//! The state of the services this layer owns beyond tabs, operations and the keymap: what the
//! Settings page is showing, the palette, the command bars, the theme, and what the host has told us.

use std::sync::{Arc, Mutex};

use bb_core::data::Db;
use bb_core::settings::{AppPaths, AppSettings};

use crate::commands::bars::{BarPage, BarsState};
use crate::commands::keys::KeyPage;
use crate::commands::palette::PaletteModel;
use crate::config::AppConfig;
use crate::services::settings::SaveState;
use crate::services::shell::UiState;
use crate::services::themes::ThemeState;

pub(crate) struct Extra {
    pub ui: Mutex<UiState>,
    pub save: SaveState,
    pub keys: Mutex<KeyPage>,
    pub bars: Mutex<BarsState>,
    pub barpage: Mutex<BarPage>,
    pub theme: Mutex<ThemeState>,
    pub palette: Mutex<PaletteModel>,
    /// What was run since the window opened, most recent first. Never written to disk: a list of
    /// what was run is a history, and a persisted one would owe the opt-in switch every history
    /// feature here has.
    pub recents: Mutex<Vec<String>>,
    /// What the thumbnail view goes back to, so toggling away and back keeps the size.
    pub last_thumbnail_scale: Mutex<f64>,
    /// The properties this machine reported for the Add column popup, and whether it has.
    pub machine_props: Mutex<(Vec<(String, String)>, bool)>,
    /// How many rows a flat listing shows before it stops and says so. The harness lowers it so a
    /// dozen files can reach the truncated case; a person never changes it.
    pub flat_cap: std::sync::atomic::AtomicUsize,
    /// The metadata pane of each tab, and the host's window onto the Windows property system.
    pub metadata: crate::side_pane::metadata_pane::MetadataState,
    /// The animated GIFs being played, by handle.
    pub gifs: Mutex<std::collections::HashMap<u64, Arc<crate::side_pane::gif_player::GifPlayer>>>,
    /// The tool windows' state: checksums, the file compare, the change timeline.
    pub tools: crate::tools::ToolsState,
    /// The operations UI: the transfer in flight, the queue, the questions a window answers.
    pub ops: crate::ops::ui::OpsUi,
    /// The delete the confirmation is asking about, and its survey.
    pub delete: crate::ops::delete_confirm::DeleteConfirm,
    /// Why the sign-in task did not change the last time it was asked to.
    pub autostart_warning: Mutex<String>,
    /// The New items page's "Import from Windows": whether one is reading, and what the last one said.
    pub new_types_import: Mutex<(bool, String)>,
    /// The workspace the window last came from (saved as, or switched to): what the title bar's switcher
    /// is called. Spec: ShellViewModel.CurrentWorkspaceName.
    pub current_workspace: Mutex<Option<String>>,
    /// The History window's last model, answered while an undo walk holds the history.
    pub history_cache: Mutex<serde_json::Value>,
}

impl Extra {
    pub fn new(
        settings: &AppSettings,
        paths: &AppPaths,
        _config: &AppConfig,
        _db: &Arc<Db>,
        ops: crate::ops::ui::OpsUi,
    ) -> Extra {
        Extra {
            ui: Mutex::new(UiState::default()),
            save: SaveState::default(),
            keys: Mutex::new(KeyPage::default()),
            bars: Mutex::new(BarsState::new(settings.command_bars.as_ref())),
            barpage: Mutex::new(BarPage::default()),
            theme: Mutex::new(ThemeState::new(paths)),
            palette: Mutex::new(PaletteModel::default()),
            recents: Mutex::new(Vec::new()),
            last_thumbnail_scale: Mutex::new(0.3),
            machine_props: Mutex::new((Vec::new(), false)),
            flat_cap: std::sync::atomic::AtomicUsize::new(crate::browse::tabs::FLAT_ENTRY_CAP),
            metadata: Default::default(),
            gifs: Default::default(),
            tools: Default::default(),
            ops,
            delete: Default::default(),
            autostart_warning: Mutex::default(),
            new_types_import: Mutex::default(),
            current_workspace: Mutex::default(),
            history_cache: Mutex::new(serde_json::json!({ "rows": [], "count": 0 })),
        }
    }
}
