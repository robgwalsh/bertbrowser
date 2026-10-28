//! BertBrowser's file-operation helper: the process that carries out one move, copy, delete, rename
//! or creation the user's own token was refused, and then exits.
//! Spec: src/BertBrowser.Elevator/Program.cs.
//!
//! It is started by the app, only ever from a click on a shield in a dialog naming the items,
//! connects back to a pipe the app is already listening on, takes exactly one request, does it
//! through the same Core executors the app uses, reports what happened, and returns. It never opens
//! a window, never launches a program, and never opens the database. One prompt per operation, one
//! request per process, and a process that exits when the request is done.
//!
//! It exits when the app does: the pipe going away ends the read loop, and a watchdog on the
//! parent's process handle cancels the work and closes the stream, so nothing outlives the app.

mod pipe;
mod staging_acl;

use std::process::ExitCode;
use std::time::Duration;

use bb_core::cancel::CancelToken;
use bb_core::delete::ShellRecycleBin;
use bb_core::elevation::{ElevationHost, ElevationStream};
use bb_core::ipc::{ElevatorArguments, IndexEndpoint, PipeIdentity, PipeOwner};
use bb_paths::PathKey;

use pipe::PipeStream;

/// Generous: the app has just been through a UAC prompt.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match ElevatorArguments::try_parse(&args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    ExitCode::from(run(&options))
}

fn run(options: &ElevatorArguments) -> u8 {
    let pipe = match PipeStream::connect(&options.pipe_name, CONNECT_TIMEOUT) {
        Ok(pipe) => pipe,
        Err(_) => {
            eprintln!("The app is not listening.");
            return 3;
        }
    };

    // The name could have been guessed or raced for. Only this establishes that the endpoint
    // belongs to the process that started us.
    if !PipeOwner::owns_pipe(&pipe, options.parent_process_id) {
        eprintln!("The pipe does not belong to the process that started this one.");
        return 4;
    }

    // A coherence check rather than a boundary. It rules out a stale build or a raced name; what
    // protects the user is the prompt they answered.
    if !is_our_app(options.parent_process_id) {
        eprintln!("The process that started this one is not the app beside it.");
        return 5;
    }

    // UAC gives the same user a different token, not a different user, so this identity is the
    // caller's. The argument exists only so a mismatch can be refused, which covers the
    // over-the-shoulder credential prompt where the elevating account is somebody else's.
    let own_sid = match IndexEndpoint::current_user_sid() {
        Ok(sid) if PipeIdentity::same_account(&sid, &options.user_sid) => sid,
        _ => {
            eprintln!("This helper is not running as the user that started it.");
            return 6;
        }
    };

    let cancel = CancelToken::new();
    let watcher_cancel = cancel.clone();
    let watcher_stream = pipe.clone();
    let watching = PipeOwner::watch_for_exit(options.parent_process_id, move || {
        // Belt to the pipe's braces. Cancelling ends the operation the way the Cancel verb does,
        // and the executors' own guarantees mean nothing is left half-written.
        watcher_cancel.cancel();
        watcher_stream.shutdown();
    });
    if watching.is_err() {
        eprintln!("Could not watch the app that started this helper.");
        return 1;
    }

    // One object serving both roles, as the app registers it: the planner and the executor must
    // agree about what has a Recycle Bin.
    let bin = ShellRecycleBin::default();
    let grant = staging_acl::grant_to(&own_sid);
    ElevationHost::new(Box::new(pipe))
        .with_recycle_bin(&bin)
        .with_recycle_probe(&bin)
        .with_grant_access(&grant)
        .run(&cancel);
    0
}

/// Whether the process on the other end is the app sitting beside this helper. Compared through
/// `PathKey` and never ordinally, or a casing difference would read as a mismatch.
fn is_our_app(process_id: u32) -> bool {
    let Some(image) = PipeOwner::image_path_of(process_id) else {
        return false;
    };
    let Ok(here) = std::env::current_exe() else {
        return false;
    };
    let Some(directory) = here.parent() else {
        return false;
    };
    let expected = directory.join("BertBrowser.exe");
    match (
        PathKey::canonicalize(&image),
        PathKey::canonicalize(&expected.to_string_lossy()),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
