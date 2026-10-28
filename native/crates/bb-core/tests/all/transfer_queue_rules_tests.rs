// Port of tests/BertBrowser.Core.Tests/TransferQueueRulesTests.cs. What a queue of long writes
// allows. The rule everything follows from is that the running job is first and stays first: it
// has already written bytes, so moving it would mean either stopping it or lying about the order.
use bb_core::transfer::{QueuedJobState, QueuedJobSummary, TransferEstimate, TransferQueueRules};

fn job(id: i32, state: QueuedJobState) -> QueuedJobSummary {
    job_sized(id, state, 1024, true)
}

fn job_sized(id: i32, state: QueuedJobState, bytes: i64, complete: bool) -> QueuedJobSummary {
    QueuedJobSummary {
        id,
        kind: "Copy".to_string(),
        description: format!("job {id}"),
        items: 1,
        estimate: TransferEstimate::new(bytes, 1, complete),
        state,
    }
}

fn ids(queue: &[QueuedJobSummary]) -> Vec<i32> {
    queue.iter().map(|j| j.id).collect()
}

use QueuedJobState::*;

// --- What may move ---

#[test]
fn the_running_job_cannot_be_moved() {
    let queue = vec![job(1, Running), job(2, Waiting)];

    assert!(!TransferQueueRules::can_move_up(&queue, 1));
    assert!(!TransferQueueRules::can_move_down(&queue, 1));
}

/// The first waiting job is already as early as anything can be: behind the one that is running.
#[test]
fn a_waiting_job_cannot_overtake_the_running_one() {
    let queue = vec![job(1, Running), job(2, Waiting), job(3, Waiting)];

    assert!(!TransferQueueRules::can_move_up(&queue, 2));
    assert_eq!(
        vec![1, 2, 3],
        ids(&TransferQueueRules::move_job(&queue, 2, -1))
    );
}

#[test]
fn the_last_waiting_job_cannot_move_down() {
    let queue = vec![job(1, Running), job(2, Waiting), job(3, Waiting)];

    assert!(!TransferQueueRules::can_move_down(&queue, 3));
    assert_eq!(
        vec![1, 2, 3],
        ids(&TransferQueueRules::move_job(&queue, 3, 1))
    );
}

#[test]
fn a_waiting_job_swaps_with_the_waiting_job_before_it() {
    let queue = vec![
        job(1, Running),
        job(2, Waiting),
        job(3, Waiting),
        job(4, Waiting),
    ];

    assert!(TransferQueueRules::can_move_up(&queue, 4));
    assert_eq!(
        vec![1, 2, 4, 3],
        ids(&TransferQueueRules::move_job(&queue, 4, -1))
    );
    assert_eq!(
        vec![1, 3, 2, 4],
        ids(&TransferQueueRules::move_job(&queue, 2, 1))
    );
}

/// Finished jobs keep their slot. They are history, and history that reshuffles when the queue
/// below it is reordered would be a puzzle rather than a record.
#[test]
fn finished_jobs_stay_where_they_are() {
    let queue = vec![
        job(1, Done),
        job(2, Cancelled),
        job(3, Running),
        job(4, Waiting),
        job(5, Waiting),
    ];

    assert_eq!(
        vec![1, 2, 3, 5, 4],
        ids(&TransferQueueRules::move_job(&queue, 5, -1))
    );
}

#[test]
fn moving_nowhere_changes_nothing() {
    let queue = vec![job(1, Running), job(2, Waiting)];

    assert_eq!(queue, TransferQueueRules::move_job(&queue, 2, 0));
}

#[test]
fn moving_a_job_the_queue_does_not_hold_changes_nothing() {
    let queue = vec![job(1, Running), job(2, Waiting)];

    assert_eq!(
        vec![1, 2],
        ids(&TransferQueueRules::move_job(&queue, 99, -1))
    );
}

// --- What is left to write ---

#[test]
fn remaining_counts_the_running_job_and_every_waiting_one() {
    let queue = vec![
        job_sized(1, Done, 900, true),
        job_sized(2, Running, 100, true),
        job_sized(3, Waiting, 20, true),
        job_sized(4, Cancelled, 700, true),
    ];

    let remaining = TransferQueueRules::remaining(&queue);

    assert_eq!(120, remaining.bytes);
    assert_eq!(2, remaining.files);
    assert!(remaining.is_usable());
}

/// One unmeasured job makes the whole total a floor. A percentage drawn over a number nobody knows
/// is worse than no percentage; the same rule `TransferEstimate` already applies within one plan.
#[test]
fn one_incomplete_estimate_makes_the_whole_queue_incomplete() {
    let queue = vec![
        job_sized(1, Running, 100, true),
        job_sized(2, Waiting, 50, false),
    ];

    let remaining = TransferQueueRules::remaining(&queue);

    assert_eq!(150, remaining.bytes);
    assert!(!remaining.complete);
    assert!(!remaining.is_usable());
}

#[test]
fn an_empty_queue_owes_nothing_and_knows_it() {
    let remaining = TransferQueueRules::remaining(&[]);

    assert_eq!(0, remaining.bytes);
    assert!(remaining.complete);
}

// --- How it says so ---

#[test]
fn waiting_text_counts_only_the_jobs_still_to_start() {
    assert_eq!("", TransferQueueRules::waiting_text(&[]));
    assert_eq!("", TransferQueueRules::waiting_text(&[job(1, Running)]));
    assert_eq!(
        "1 waiting",
        TransferQueueRules::waiting_text(&[job(1, Running), job(2, Waiting)])
    );
    assert_eq!(
        "2 waiting",
        TransferQueueRules::waiting_text(&[job(1, Done), job(2, Waiting), job(3, Waiting)])
    );
}

/// A queue of one reads exactly as it did before there was a queue.
#[test]
fn headline_leaves_a_running_jobs_own_words_alone() {
    let queue = vec![job(1, Running)];

    assert_eq!(
        "Copying 2 of 9 \u{2014} photo.jpg",
        TransferQueueRules::headline(&queue, "Copying 2 of 9 \u{2014} photo.jpg", false)
    );
}

#[test]
fn headline_says_so_while_paused() {
    let queue = vec![job(1, Running)];

    assert_eq!(
        "Paused \u{2014} Copying 2 of 9 \u{2014} photo.jpg",
        TransferQueueRules::headline(&queue, "Copying 2 of 9 \u{2014} photo.jpg", true)
    );
    assert_eq!("Paused", TransferQueueRules::headline(&queue, "", true));
}
