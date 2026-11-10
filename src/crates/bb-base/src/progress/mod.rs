//! Progress for the long writes (transfer, extract, create-archive, archive-edit, metadata): the
//! report, the coalescer that rations it, and the gate that pauses all of them.
//! `bb_core::transfer` re-exports all three.

mod coalescer;
mod pause_gate;

pub use coalescer::ProgressCoalescer;
pub use pause_gate::PauseGate;

/// Progress while a transfer runs. Byte-level, because item counts say nothing useful about a
/// single large file. The plan's byte total is deliberately absent: it comes from the directory
/// size index, which is the caller's business.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferProgress {
    /// Items finished.
    pub done: usize,
    /// Items in the plan.
    pub total: usize,
    /// The item in flight; empty on the terminal report.
    pub current_name: String,
    /// Bytes written so far across the whole plan. A move within one volume is a rename, so it
    /// moves no bytes and this stays at zero, which is correct, not a stall.
    pub bytes_done: i64,
    /// Bytes written into the item in flight.
    pub current_item_bytes: i64,
    /// That item's size as the OS reports it; 0 when not yet known.
    pub current_item_total: i64,
}
