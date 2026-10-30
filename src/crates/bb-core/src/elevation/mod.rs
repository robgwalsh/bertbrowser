//! The elevated file-operation retry: what a token may and may not be used for, the second pass
//! derived from the first one's permission failures and folded back into one outcome, the app end
//! of the helper's pipe, the helper end that hosts the real executors, and the seams (launcher,
//! transport, prompt) that keep a UAC prompt out of every test.
//! Spec: Core/Services/Elevation/*.cs.
//!
//! The app is `asInvoker`; only the two elevated helper exes touch an administrator token, and
//! starting one (`ShellExecuteExW` with `runas`) is the one process start outside the app's
//! launcher, behind [`ElevationLauncher`].

mod client;
mod host;
mod launcher;
mod models;
mod prompt;
mod retry;
mod rules;
mod wire;

pub use client::{ElevatedOperationRunner, ElevationClient};
pub use host::ElevationHost;
pub use launcher::win32 as elevated_process;
pub use launcher::{
    ElevationLaunch, ElevationLaunchResult, ElevationLauncher, ElevationStream, ElevationTransport,
    ElevationTransportFactory, ShellExecuteLauncher,
};
pub use models::{
    Elevated, ElevatedRun, ElevationHeader, ElevationNewItemResult, ElevationOperation,
    ElevationProgressReport, ElevationStatus, ElevationSummary, ElevationTransferItem,
    ElevationTransferResult,
};
pub use prompt::{ElevationOffer, ElevationPrompt, RefusingElevationPrompt};
pub use retry::{
    DeleteRetry, DeleteUndoRetry, ElevatedRetry, MergeError, NewItemRetry, RenameRetry,
    TransferRetry, TransferUndoRetry,
};
pub use rules::ElevationRules;
pub use wire::WireRecord;
