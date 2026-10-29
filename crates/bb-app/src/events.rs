//! What the app tells its host. Delivered through a sink the host installs; the sink is called on
//! whichever thread raised the event and **must only enqueue**.

use std::sync::{Arc, Mutex};

use crate::ids::{OpId, PaneId, TabId};

/// The services whose state a host re-reads when it hears `ModelChanged`. The numbers cross the C
/// ABI (`BbEvent.a`), so they only ever grow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Model {
    /// The Settings page opened, closed or changed page.
    SettingsPage = 1,
    /// The theme in force, its slots or its colour tweaks changed: re-push the palette.
    Theme = 2,
    /// The menu bar or a toolbar arrangement (or the saved ones) changed.
    Bars = 3,
    /// What a key does changed.
    Keymap = 4,
    /// The command palette's rows or selection changed.
    Palette = 5,
    /// The Keyboard page's rows or recorder changed.
    KeyPage = 6,
    /// The Menus and toolbars page's editor changed.
    BarPage = 7,
    /// Saved searches, saved workspaces or bookmarks changed.
    Saved = 8,
    /// The index banner or status line changed.
    Index = 9,
    /// A tab's columns changed.
    Columns = 10,
    /// Two panes started or stopped being compared.
    Compare = 11,
    /// Something that decides whether a command can run changed (a dialog, the clipboard).
    Commands = 12,
    /// The metadata pane's rows, boxes or files moved (an apply finished).
    Metadata = 13,
    /// The transfer in flight, its queue or its pause moved.
    Transfer = 14,
    /// A transfer is waiting on the user's answer to a clash.
    Conflicts = 15,
    /// An operation was refused for permissions and is waiting on the user's answer.
    Elevation = 16,
    /// A worker is waiting on the user's yes or no (`ops_query("confirm")`).
    Confirm = 17,
    /// There is something to tell the user (`ops_query("notice")`).
    Notice = 18,
    /// The delete confirmation's survey measured another item (`ops_query("delete-survey")`).
    DeleteSurvey = 19,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppEvent {
    /// The tab's listing was replaced or merged; drop the snapshot you hold and ask for a new one.
    ListingChanged { tab: TabId, version: u64 },
    /// Path, history, flags, status text or selection of a tab moved.
    TabChanged { tab: TabId },
    /// Panes or tabs were added, removed, moved or activated.
    LayoutChanged,
    /// The settle predicate flipped.
    BusyChanged { busy: bool },
    /// The undo history gained, lost or moved an entry.
    UndoChanged,
    /// An operation finished (`ok` false when anything failed or it was refused).
    OpFinished { op: OpId, ok: bool },
    /// A setting changed.
    SettingsChanged { key: String },
    /// A pane's active tab changed or a pane got focus.
    PaneChanged { pane: PaneId },
    /// A service's state moved; see [`Model`].
    ModelChanged { model: Model },
    /// The tab shows shell-property columns with cells the host has not read: ask
    /// (`prop_requests`) and answer (`prop_supply`).
    PropertiesNeeded { tab: TabId },
}

pub type EventSink = Arc<dyn Fn(AppEvent) + Send + Sync>;

/// The installed sink, swappable at any time.
#[derive(Clone, Default)]
pub struct EventHub {
    sink: Arc<Mutex<Option<EventSink>>>,
}

impl EventHub {
    pub fn set(&self, sink: Option<EventSink>) {
        *self.sink.lock().unwrap_or_else(|p| p.into_inner()) = sink;
    }

    pub fn emit(&self, event: AppEvent) {
        let sink = self.sink.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if let Some(sink) = sink {
            sink(event);
        }
    }

    pub fn model(&self, model: Model) {
        self.emit(AppEvent::ModelChanged { model });
    }
}
