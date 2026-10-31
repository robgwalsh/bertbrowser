// Port of tests/BertBrowser.Core.Tests/PauseGateTests.cs. The latch a long write waits on. Its
// two unusual guarantees, that it never unwinds and that a cancelled wait says so rather than
// pretending it was resumed, exist because it is waited on from inside a native `CopyFileExW`
// callback.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bb_core::cancel::CancelToken;
use bb_core::transfer::PauseGate;

/// How long something that should happen is given before it is called a hang.
const PATIENCE: Duration = Duration::from_secs(5);

/// Long enough that a run which was going to proceed would have.
const LONG_ENOUGH_TO_NOTICE: Duration = Duration::from_millis(200);

fn finished<T>(task: &JoinHandle<T>, within: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < within {
        if task.is_finished() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    task.is_finished()
}

fn wait_for(flag: &AtomicBool, within: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < within {
        if flag.load(Ordering::SeqCst) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    flag.load(Ordering::SeqCst)
}

#[test]
fn a_gate_starts_open_and_waiting_on_it_returns_at_once() {
    let gate = PauseGate::new();

    assert!(!gate.is_paused());
    assert!(gate.wait(&CancelToken::none()));
}

#[test]
fn pausing_holds_a_waiter_until_it_is_resumed() {
    let gate = Arc::new(PauseGate::new());
    let entered = Arc::new(AtomicBool::new(false));
    gate.pause();

    let waiter = {
        let gate = gate.clone();
        let entered = entered.clone();
        std::thread::spawn(move || {
            entered.store(true, Ordering::SeqCst);
            gate.wait(&CancelToken::none())
        })
    };

    assert!(wait_for(&entered, PATIENCE));
    assert!(!finished(&waiter, LONG_ENOUGH_TO_NOTICE));
    assert!(gate.is_paused());

    gate.resume();

    assert!(finished(&waiter, PATIENCE));
    assert!(waiter.join().unwrap());
    assert!(!gate.is_paused());
}

/// The guarantee the copier depends on: a cancel while paused unblocks, and reports itself as a
/// cancel rather than as a resume, and does it without unwinding across the P/Invoke boundary out
/// of a callback Windows is still inside.
#[test]
fn cancelling_a_paused_wait_returns_false_without_throwing() {
    let gate = Arc::new(PauseGate::new());
    let cancellation = CancelToken::new();
    let entered = Arc::new(AtomicBool::new(false));
    gate.pause();

    let waiter = {
        let gate = gate.clone();
        let token = cancellation.clone();
        let entered = entered.clone();
        std::thread::spawn(move || {
            entered.store(true, Ordering::SeqCst);
            gate.wait(&token)
        })
    };

    assert!(wait_for(&entered, PATIENCE));
    cancellation.cancel();

    assert!(finished(&waiter, PATIENCE));
    assert!(!waiter.join().unwrap());
    assert!(gate.is_paused()); // cancelling a wait does not open the gate; the caller does
}

/// An already-cancelled run gets the same answer without ever having to block.
#[test]
fn an_open_gate_still_reports_an_already_cancelled_run() {
    let gate = PauseGate::new();
    let cancellation = CancelToken::new();
    cancellation.cancel();

    assert!(!gate.wait(&cancellation));
}

#[test]
fn pausing_twice_is_still_one_resume_away() {
    let gate = PauseGate::new();
    gate.pause();
    gate.pause();
    gate.resume();

    assert!(!gate.is_paused());
    assert!(gate.wait(&CancelToken::none()));
}
