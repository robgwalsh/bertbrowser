//! A latch a long write waits on, so the user can hold it still and let it go again.
//! Spec: Core/Services/Transfer/PauseGate.cs.
//!
//! **It is waited on from inside a native callback.** `CopyFileExW` invokes its progress routine
//! on the thread doing the copy, and the copier forwards that straight to the progress closure the
//! executor passed, so a progress callback that blocks blocks the OS copy itself, mid-file. That is
//! the whole mechanism: there is no pause verb to send anywhere.
//!
//! **Which is also why `wait` never panics or unwinds.** A cancelled wait returns `false` instead;
//! the copier's own token check stops the copy at the next chunk. Cancelling therefore has to
//! `resume` as well as trip the token: the token alone releases *this* wait, but leaves the gate
//! shut for whatever runs next.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::cancel::CancelToken;

/// How often a held waiter looks at the token. A `CancelToken` is a flag with no waker, so a
/// cancel is noticed on the next look rather than at once; the latency is invisible next to a
/// user pressing a button.
const CANCEL_POLL: Duration = Duration::from_millis(10);

pub struct PauseGate {
    /// True means open. A gate starts open: nothing is paused until someone asks.
    open: Mutex<bool>,
    changed: Condvar,
}

impl Default for PauseGate {
    fn default() -> Self {
        PauseGate {
            open: Mutex::new(true),
            changed: Condvar::new(),
        }
    }
}

impl PauseGate {
    pub fn new() -> PauseGate {
        PauseGate::default()
    }

    pub fn is_paused(&self) -> bool {
        !*self.lock()
    }

    pub fn pause(&self) {
        *self.lock() = false;
        self.changed.notify_all();
    }

    pub fn resume(&self) {
        *self.lock() = true;
        self.changed.notify_all();
    }

    /// Blocks while paused. Returns false when the wait ended because the run was cancelled
    /// rather than resumed.
    pub fn wait(&self, cancel: &CancelToken) -> bool {
        let mut open = self.lock();
        // The overwhelmingly common case, taken without touching the token's slow path.
        if *open {
            return !cancel.is_cancelled();
        }
        loop {
            if *open {
                return true;
            }
            if cancel.is_cancelled() {
                return false;
            }
            open = self
                .changed
                .wait_timeout(open, CANCEL_POLL)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, bool> {
        // A poisoned flag is still a flag: the bool cannot be half-written.
        self.open.lock().unwrap_or_else(|e| e.into_inner())
    }
}
