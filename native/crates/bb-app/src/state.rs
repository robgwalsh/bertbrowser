//! The state of the services this layer owns beyond tabs, operations and the keymap: what the
//! Settings page is showing, the palette, the command bars, the theme, and what the host has told us.

use std::sync::{Arc, Mutex};

use bb_core::data::Db;
use bb_core::settings::{AppPaths, AppSettings};

use crate::bars::{BarPage, BarsState};
use crate::config::AppConfig;
use crate::keys::KeyPage;
use crate::misc::UiState;
use crate::palette::PaletteModel;
use crate::settings_svc::SaveState;
use crate::themes::ThemeState;

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
    pub metadata: crate::metadata_pane::MetadataState,
    /// The animated GIFs being played, by handle.
    pub gifs: Mutex<std::collections::HashMap<u64, Arc<crate::gif_player::GifPlayer>>>,
}

impl Extra {
    pub fn new(
        settings: &AppSettings,
        paths: &AppPaths,
        _config: &AppConfig,
        _db: &Arc<Db>,
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
            flat_cap: std::sync::atomic::AtomicUsize::new(crate::tabs::FLAT_ENTRY_CAP),
            metadata: Default::default(),
            gifs: Default::default(),
        }
    }
}
