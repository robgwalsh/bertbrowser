//! File operations and the undo history.

use super::*;

// ---- operations ----

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbDeleteMode {
    /// Into the Windows Recycle Bin.
    BB_DELETE_RECYCLE = 0,
    /// Into this app's holding folder; undoable for the session.
    BB_DELETE_STAGED = 1,
    /// Erased in place; no undo.
    BB_DELETE_PERMANENT = 2,
}

/// Moves the sources into a folder. Queued behind a running operation. The outcome arrives as
/// `BB_EVT_OP_FINISHED`; a move is on the undo history.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_move(
    app: *mut BbApp,
    src: *const BbStr,
    n: u32,
    dest: BbStr,
    out_op: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let op = app.app.op_move(strings(src, n), &dest.to_string_lossy())?;
        if let Some(out_op) = opt_mut(out_op) {
            *out_op = op.0;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_copy(
    app: *mut BbApp,
    src: *const BbStr,
    n: u32,
    dest: BbStr,
    out_op: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let op = app.app.op_copy(strings(src, n), &dest.to_string_lossy())?;
        if let Some(out_op) = opt_mut(out_op) {
            *out_op = op.0;
        }
        Ok(())
    })
}

/// Deletes the sources. `BB_BUSY` while another operation runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_delete(
    app: *mut BbApp,
    src: *const BbStr,
    n: u32,
    mode: BbDeleteMode,
    out_op: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let mode = match mode {
            BbDeleteMode::BB_DELETE_RECYCLE => DeleteMode::Recycle,
            BbDeleteMode::BB_DELETE_STAGED => DeleteMode::Staged,
            BbDeleteMode::BB_DELETE_PERMANENT => DeleteMode::Permanent,
        };
        let op = app.app.op_delete(strings(src, n), mode)?;
        if let Some(out_op) = opt_mut(out_op) {
            *out_op = op.0;
        }
        Ok(())
    })
}

/// Renames the sources by a pattern ("Holiday" numbers across the selection, each keeping its
/// extension). `BB_BUSY` while another operation runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_rename(
    app: *mut BbApp,
    src: *const BbStr,
    n: u32,
    pattern: BbStr,
    out_op: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let op = app
            .app
            .op_rename(strings(src, n), &pattern.to_string_lossy())?;
        if let Some(out_op) = opt_mut(out_op) {
            *out_op = op.0;
        }
        Ok(())
    })
}

/// Pauses or resumes the running transfer, mid-file. The queue stays held while paused.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_pause(app: *mut BbApp, paused: bool) -> BbStatus {
    guard(|| {
        app_of(app)?.app.op_pause(paused);
        Ok(())
    })
}

/// Cancels an operation: one still waiting never starts, the running one is told to stop (and a
/// pause is lifted so it can notice).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_cancel(app: *mut BbApp, op: u64) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.op_cancel(OpId(op))?))
}

/// Where an operation is.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbOpPhase {
    BB_OP_QUEUED = 0,
    BB_OP_RUNNING = 1,
    BB_OP_DONE = 2,
}

#[repr(C)]
pub struct BbOpInfo {
    pub phase: BbOpPhase,
    /// Meaningful once `phase` is `BB_OP_DONE`.
    pub ok: bool,
}

/// An operation's phase, and (in `out_message`, when non-NULL) the sentence it ended with.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_state(
    app: *mut BbApp,
    op: u64,
    out_info: *mut BbOpInfo,
    out_message: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let state = app.app.op_state(OpId(op)).ok_or(Fail::Stale)?;
        let (phase, ok, message) = match state {
            OpState::Queued => (BbOpPhase::BB_OP_QUEUED, false, String::new()),
            OpState::Running => (BbOpPhase::BB_OP_RUNNING, false, String::new()),
            OpState::Done { ok, message } => (BbOpPhase::BB_OP_DONE, ok, message),
        };
        if let Some(info) = opt_mut(out_info) {
            *info = BbOpInfo { phase, ok };
        }
        if let Some(m) = opt_mut(out_message) {
            *m = BbOwnedStr::from_str(&message);
        }
        Ok(())
    })
}

/// Undoes `steps` entries of the history, or redoes `-steps` when negative. `BB_BUSY` while another
/// operation runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_undo_step(app: *mut BbApp, steps: i32, out_op: *mut u64) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let op = app.app.undo_step(steps)?;
        if let Some(out_op) = opt_mut(out_op) {
            *out_op = op.0;
        }
        Ok(())
    })
}

#[repr(C)]
pub struct BbUndoState {
    pub can_undo: bool,
    pub can_redo: bool,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_undo_state(app: *mut BbApp, out_state: *mut BbUndoState) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let out_state = out(out_state, "out_state")?;
        *out_state = BbUndoState {
            can_undo: app.app.can_undo(),
            can_redo: app.app.can_redo(),
        };
        Ok(())
    })
}
