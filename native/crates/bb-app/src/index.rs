//! What the window says about the search index: the banner that offers to start the helper, the
//! status line, and the two buttons behind them. **The app never launches the helper at startup**:
//! it attaches (`AttachOnly`), and a banner offers the prompt (`IndexerBannerRules`). Dismissing the
//! banner is "not now" and is not persisted, so nobody can permanently lose the only way in.
//! Pressing Start is what raises the prompt, and only then. Spec: ShellViewModel (indexer banner,
//! StartIndexer, RetryIndexing), Core/Services/Mft/IndexerBannerRules.
//!
//! The declined configuration (`AppConfig::index_declined`) puts the **real** `MftIndexClient`
//! behind a launcher that starts nothing and reports a declined elevation prompt, so everything
//! below the launcher is genuine: the harness's `--index-declined` takes the path the app takes.

use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use bb_core::mft::{
    IndexHostLaunchResult, IndexHostLauncher, IndexListener, IndexStartMode, IndexStream,
    IndexTransport, IndexTransportFactory, IndexerBanner, IndexerBannerRules, IndexerPresence,
    MftIndex, MftIndexClient,
};
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::events::Model;
use crate::ids::{AppError, AppResult};

/// A launcher whose prompt is always declined.
struct DeclinedLauncher;

impl IndexHostLauncher for DeclinedLauncher {
    fn launch(&self) -> IndexHostLaunchResult {
        IndexHostLaunchResult::declined()
    }

    fn can_elevate(&self) -> bool {
        true
    }

    fn wait_for_exit(&self, _process_id: u32, _timeout: Duration) {}
}

/// An endpoint nobody ever connects to.
struct EmptyEndpoint;

impl IndexTransport for EmptyEndpoint {
    fn endpoint(&self) -> String {
        "bertbrowser-index-declined".to_string()
    }

    fn accept(
        &self,
        _launched_process_id: Option<u32>,
        _timeout: Duration,
    ) -> Option<Arc<dyn IndexStream>> {
        None
    }
}

struct EmptyTransports;

impl IndexTransportFactory for EmptyTransports {
    fn try_create(&self) -> Result<Arc<dyn IndexTransport>, String> {
        Ok(Arc::new(EmptyEndpoint))
    }
}

/// The index of a run where the elevation prompt will be declined.
pub(crate) fn declined_index() -> Arc<dyn MftIndex> {
    Arc::new(MftIndexClient::new(
        Arc::new(DeclinedLauncher),
        Arc::new(EmptyTransports),
        None,
    ))
}

struct Listener(Weak<Inner>);

impl IndexListener for Listener {
    fn status_changed(&self) {
        if let Some(inner) = self.0.upgrade() {
            inner.hub.model(Model::Index);
        }
    }

    fn index_refreshed(&self, _root_key: &str) {
        if let Some(inner) = self.0.upgrade() {
            inner.hub.model(Model::Index);
        }
    }
}

fn presence_name(p: IndexerPresence) -> &'static str {
    match p {
        IndexerPresence::NotApplicable => "not-applicable",
        IndexerPresence::NotRunning => "not-running",
        IndexerPresence::Running => "running",
    }
}

impl Inner {
    /// Subscribes to the index and, when there is a helper to look for, attaches to one. Counted as
    /// loading until the attempt has an answer, so a settle does not slip in front of the banner.
    pub(crate) fn index_attach(self: &Arc<Self>) {
        let _ = self
            .seams
            .index
            .subscribe(Arc::new(Listener(Arc::downgrade(self))));
        if self.seams.index.presence() == IndexerPresence::NotApplicable {
            return;
        }
        self.schedule(|inner| {
            let _ = inner.seams.index.start_with(IndexStartMode::AttachOnly);
            inner.index_wait_for_answer();
        });
    }

    /// Waits (briefly) for a session to reach a state worth showing: a helper running, a failure, or
    /// a "nobody has asked" that offers a start.
    fn index_wait_for_answer(&self) {
        let index = &self.seams.index;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if index.presence() == IndexerPresence::Running
                || index.can_start()
                || index.can_retry()
                || !index.status_text().is_empty()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.hub.model(Model::Index);
    }

    pub(crate) fn index_banner(&self) -> IndexerBanner {
        let index = &self.seams.index;
        let dismissed = lock(&self.extra.ui).banner_dismissed;
        // Something went wrong and the status bar is already saying so: two places describing one
        // problem reads as two problems.
        let failure = if index.can_retry() {
            index.status_text()
        } else {
            String::new()
        };
        IndexerBannerRules::decide(index.presence(), index.can_start(), dismissed, &failure)
    }

    pub(crate) fn index_dismiss_banner(&self) {
        lock(&self.extra.ui).banner_dismissed = true;
        self.hub.model(Model::Index);
    }

    /// Pressing Start: the one thing that raises the elevation prompt.
    pub(crate) fn index_start(self: &Arc<Self>) -> AppResult<()> {
        if !self.seams.index.can_start() && !self.seams.index.can_retry() {
            return Err(AppError::Invalid(
                "The search index is already running, or cannot be started here.".into(),
            ));
        }
        self.schedule(|inner| {
            let _ = inner.seams.index.start_with(IndexStartMode::AttachOrLaunch);
            // The session is asynchronous; hold the settle until it has said yes or no.
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(10) {
                let index = &inner.seams.index;
                if index.presence() == IndexerPresence::Running
                    || index.can_retry()
                    || !index.status_text().is_empty()
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            inner.hub.model(Model::Index);
        });
        Ok(())
    }

    pub(crate) fn index_retry(self: &Arc<Self>) -> AppResult<()> {
        if !self.seams.index.can_retry() {
            return Err(AppError::Invalid("Indexing has not failed.".into()));
        }
        self.schedule(|inner| {
            inner.seams.index.retry();
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(10) {
                let index = &inner.seams.index;
                if index.presence() == IndexerPresence::Running
                    || index.can_retry()
                    || !index.status_text().is_empty()
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            inner.hub.model(Model::Index);
        });
        Ok(())
    }

    pub(crate) fn index_json(&self) -> Value {
        let index = &self.seams.index;
        let banner = self.index_banner();
        json!({
            "status": index.status_text(),
            "canRetry": index.can_retry(),
            "canStart": index.can_start(),
            "presence": presence_name(index.presence()),
            "anyIndexed": index.any_indexed(),
            "building": index.is_building(),
            "banner": {
                "show": banner.show,
                "message": banner.message,
                "canStart": banner.can_start,
            },
        })
    }
}
