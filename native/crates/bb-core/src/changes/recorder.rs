//! Buffers the changes one volume's USN tail resolves and writes them down a batch at a time. One per
//! volume, owned by that volume's tail thread, so nothing here locks. Spec: Core/Services/Changes/ChangeRecorder.cs.
//!
//! The policy is read once per `flush`, not cached: it is the user's switch, pushed to the helper
//! over the pipe. Off means nothing is buffered and, on the first flush after it was on, the table is
//! wiped. **Nothing here fails outward**: a database failure disables this recorder for the session
//! and the index carries on. A flush with nothing buffered touches nothing.

use std::sync::Arc;

use super::{ChangeEvent, ChangeLogPolicy, ChangeLogRules};
use crate::FileTime;
use crate::data::ChangeLogRepository;

pub struct ChangeRecorder {
    repository: Arc<ChangeLogRepository>,
    excluded_root_key: String,
    policy: Box<dyn Fn() -> ChangeLogPolicy + Send>,
    clock: Box<dyn Fn() -> FileTime + Send>,
    buffer: Vec<ChangeEvent>,
    last_prune: FileTime,
    was_enabled: bool,
    disabled: bool,
}

impl ChangeRecorder {
    pub fn new(
        repository: Arc<ChangeLogRepository>,
        excluded_root_key: String,
        policy: impl Fn() -> ChangeLogPolicy + Send + 'static,
        clock: impl Fn() -> FileTime + Send + 'static,
    ) -> ChangeRecorder {
        ChangeRecorder {
            repository,
            excluded_root_key,
            policy: Box::new(policy),
            clock: Box::new(clock),
            buffer: Vec::new(),
            last_prune: FileTime::UNKNOWN,
            was_enabled: false,
            disabled: false,
        }
    }

    /// True once a database failure has switched this recorder off for the session.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn add(&mut self, change: ChangeEvent) {
        if self.disabled || !(self.policy)().enabled {
            return;
        }
        if ChangeLogRules::is_excluded(&change.path_key, &self.excluded_root_key) {
            return;
        }
        self.buffer.push(change);
    }

    /// Writes whatever has been buffered since the last call, in one transaction.
    pub fn flush(&mut self) {
        if self.disabled {
            return;
        }
        let policy = (self.policy)();
        if let Err(_e) = self.flush_inner(policy) {
            // The contract is that this never reaches the tail.
            self.disabled = true;
            self.buffer.clear();
        }
    }

    fn flush_inner(&mut self, policy: ChangeLogPolicy) -> crate::data::Result<()> {
        if !policy.enabled {
            self.buffer.clear();
            if self.was_enabled {
                // The app wipes the table when the user turns recording off; this covers the batch
                // buffered between its wipe and this poll. Once, not on every poll.
                self.repository.clear()?;
                self.was_enabled = false;
            }
            return Ok(());
        }

        self.was_enabled = true;
        if self.buffer.is_empty() {
            return Ok(());
        }

        self.repository.record(&self.buffer)?;
        self.buffer.clear();

        // Only after a batch, never on an idle poll (the empty-flush rule above). Do not "fix" this
        // by pruning on idle polls: that is a write per second.
        let now = (self.clock)();
        if now.ticks_since(self.last_prune)
            >= crate::time::duration_ticks(ChangeLogRules::PRUNE_INTERVAL)
        {
            self.repository
                .prune(now, policy.retention, ChangeLogRules::MAX_ROWS)?;
            self.last_prune = now;
        }
        Ok(())
    }
}
