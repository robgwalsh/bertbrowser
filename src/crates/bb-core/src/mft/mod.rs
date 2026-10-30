//! The NTFS master file table and USN journal: raw readers, the per-volume indexer, the directory
//! size builder, the index state, and the helper host/client that split the index across the
//! elevation boundary. Spec: Core/Services/Mft/*, Core/Interop/NtfsNative.cs,
//! src/BertBrowser.Indexer/*.cs, src/BertBrowser.App/Services/Indexing/*.cs.
//!
//! Everything that needs an administrator token (a raw `\\.\C:` handle, the USN FSCTLs) sits behind
//! `VolumeHandle` / `VolumeAccess` (`raw_volume.rs` is the real one, `fixtures.rs` the in-memory
//! fake), so the indexer, the size roll-up and the parsers are all exercised without elevation.
//! The host and the client talk over `IndexStream`, which the tests satisfy with an in-memory pair.

mod autostart;
mod banner_rules;
mod client;
mod dir_size_builder;
pub mod fixtures;
mod host;
mod ntfs_layout;
mod null;
mod path_builder;
mod presence;
mod raw_volume;
mod reader;
mod service;
mod state;
mod transport;
mod usn_record;
mod volume_indexer;

use std::sync::{Arc, Mutex};

pub use autostart::{AutoStartState, IndexerAutoStartTask};
pub use banner_rules::{IndexerBanner, IndexerBannerRules};
pub use client::MftIndexClient;
pub use dir_size_builder::MftDirectorySizeBuilder;
pub use host::{IndexSessionEnd, MftIndexHost};
pub use ntfs_layout::{DataRun, NtfsLayout};
pub use null::NullMftIndex;
pub use path_builder::{DirCache, MftNode, MftPathBuilder, ResolvedDir};
pub use presence::{
    IndexerPresence, IndexerPresenceProbe, NoIndexerPresence, WindowsIndexerPresence,
};
pub use raw_volume::{RawVolume, RawVolumeAccess};
pub use reader::{BootGeometry, MftFileRecord, MftReader, VolumeRead};
pub use service::{ChangeRecorderOptions, MftIndexService};
pub use state::MftIndexState;
pub use transport::{
    IndexHostLaunch, IndexHostLaunchResult, IndexHostLauncher, IndexStream, IndexTransport,
    IndexTransportFactory, StreamRead, StreamWrite,
};
pub use usn_record::{UsnRecord, UsnRecordParser};
pub use volume_indexer::{JournalData, MftVolumeIndexer, VolumeAccess, VolumeHandle};

use crate::changes::ChangeLogPolicy;

/// Whether looking for a helper may also start one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexStartMode {
    /// Attach to a running helper, and if there is none, ask for one, which prompts.
    AttachOrLaunch,
    /// Attach to a running helper, and otherwise do nothing at all. What startup uses by default:
    /// a prompt nobody asked for, on every launch, is what the banner exists to remove.
    AttachOnly,
}

/// What an index service tells its subscribers, from a worker thread.
pub trait IndexListener: Send + Sync {
    /// A volume root key whose initial index just completed, so open searches can re-query.
    fn index_refreshed(&self, _root_key: &str) {}
    /// `status_text` (or anything behind it) changed.
    fn status_changed(&self) {}
}

/// The index service: the real one, the client mirroring a helper, the null one, and the test's
/// controllable one all answer these identically. Spec: `IMftIndexService`.
pub trait MftIndex: Send + Sync {
    /// Begins indexing (or looking for a helper). Safe to call once at startup; a no-op if already
    /// started. `Err` carries the message of an indexer that cannot run at all, which the host puts
    /// on the wire as `Fatal`.
    fn start(&self) -> Result<(), String> {
        self.start_with(IndexStartMode::AttachOrLaunch)
    }
    /// As `start`, saying whether an absent helper may be started (which raises an elevation
    /// prompt). Hosts with no separate helper ignore the mode.
    fn start_with(&self, mode: IndexStartMode) -> Result<(), String>;
    /// Ends the helper, if there is one to end. A no-op for an in-process indexer.
    fn stop(&self);
    /// True once at least one volume's initial enumeration has completed.
    fn any_indexed(&self) -> bool;
    /// True while any volume's initial enumeration is still running.
    fn is_building(&self) -> bool;
    /// The bare drive letters currently building, relayed exactly by the host so the client words
    /// the status line with the same function.
    fn building_drives(&self) -> Vec<String>;
    /// Every volume root whose index is complete, replayed to an app that attached late.
    fn completed_roots(&self) -> Vec<String>;
    /// Whether there is a separate elevated helper, and whether it is running.
    fn presence(&self) -> IndexerPresence;
    /// True if `path_key` sits on a volume whose live MFT index is complete.
    fn is_indexed(&self, path_key: &str) -> bool;
    /// Human-readable indexing state for the status bar; empty when idle.
    fn status_text(&self) -> String;
    /// True when the status describes a failure the user could act on. Nothing retries by itself:
    /// a retry is a UAC prompt.
    fn can_retry(&self) -> bool;
    /// True when asking for the index could achieve something, so a button offering to is worth
    /// showing. Wider than `can_retry`: a helper not running yet is not a failure.
    fn can_start(&self) -> bool;
    /// Tries again after a failure. A no-op unless `can_retry`.
    fn retry(&self);
    /// What the USN tail writes beyond the index: off by default.
    fn change_log(&self) -> ChangeLogPolicy;
    /// Fire-and-forget: the client relays it to the helper, whose own default is off.
    fn set_change_log(&self, policy: ChangeLogPolicy);
    /// Subscribes; the id unsubscribes.
    fn subscribe(&self, listener: Arc<dyn IndexListener>) -> u64;
    fn unsubscribe(&self, id: u64);
}

/// A subscription id and its listener.
type Subscriber = (u64, Arc<dyn IndexListener>);

/// The two events an index raises, with the bookkeeping every implementation shares. Listeners are
/// invoked outside the lock, so one may unsubscribe from inside its callback.
#[derive(Default)]
pub struct IndexListeners {
    inner: Mutex<(u64, Vec<Subscriber>)>,
}

impl IndexListeners {
    pub fn new() -> IndexListeners {
        IndexListeners::default()
    }

    pub fn subscribe(&self, listener: Arc<dyn IndexListener>) -> u64 {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.0 += 1;
        let id = inner.0;
        inner.1.push((id, listener));
        id
    }

    pub fn unsubscribe(&self, id: u64) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.1.retain(|(i, _)| *i != id);
    }

    fn snapshot(&self) -> Vec<Arc<dyn IndexListener>> {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.1.iter().map(|(_, l)| l.clone()).collect()
    }

    pub fn index_refreshed(&self, root_key: &str) {
        for listener in self.snapshot() {
            listener.index_refreshed(root_key);
        }
    }

    pub fn status_changed(&self) {
        for listener in self.snapshot() {
            listener.status_changed();
        }
    }
}

/// USN_RECORD reason bits the indexer and the change log act on.
pub mod usn_reason {
    pub const DATA_EXTEND: u32 = 0x0000_0002;
    pub const FILE_CREATE: u32 = 0x0000_0100;
    pub const FILE_DELETE: u32 = 0x0000_0200;
    pub const RENAME_OLD_NAME: u32 = 0x0000_1000;
    pub const RENAME_NEW_NAME: u32 = 0x0000_2000;
    pub const CLOSE: u32 = 0x8000_0000;
}
