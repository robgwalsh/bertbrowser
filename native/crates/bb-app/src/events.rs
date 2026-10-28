//! What the app tells its host. Delivered through a sink the host installs; the sink is called on
//! whichever thread raised the event and **must only enqueue**.

use std::sync::{Arc, Mutex};

use crate::ids::{OpId, PaneId, TabId};

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
}

pub type EventSink = Arc<dyn Fn(AppEvent) + Send + Sync>;

/// The installed sink, swappable at any time.
#[derive(Default)]
pub struct EventHub {
    sink: Mutex<Option<EventSink>>,
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
}
