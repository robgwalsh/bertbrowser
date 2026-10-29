//! What a queue of long writes allows: what may be reordered, what the whole queue still costs,
//! and how it describes itself. Spec: Core/Services/Transfer/TransferQueueRules.cs.
//!
//! Pure, so the ordering is settled by tests rather than by clicking. The one rule everything
//! else follows from is that **the running job is first and stays first**: it has already written
//! bytes, so moving it would mean either stopping it or lying about the order.

use super::estimator::TransferEstimate;
use crate::format::thousands;

/// Where one queued write has got to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueuedJobState {
    /// In the queue, not started. The only state that can be reordered.
    Waiting,
    /// Running now. There is at most one of these, and it is always first.
    Running,
    /// Finished, however it turned out; the outcome carries the detail.
    Done,
    /// Taken out of the queue before it started, or stopped part-way.
    Cancelled,
}

/// One queued write, as the queue rules see it: enough to order it and to total it, and nothing
/// about how it is carried out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedJobSummary {
    /// Stable for the job's whole life; the queue is reordered, so an index is not.
    pub id: i32,
    /// "Copy", "Move", "Extract", "Compress", "Archive edit": the noun in the row.
    pub kind: String,
    /// "12 items to Archive", already phrased for display.
    pub description: String,
    /// How many things this job will write.
    pub items: usize,
    /// Its byte total, and whether that total can be trusted.
    pub estimate: TransferEstimate,
    pub state: QueuedJobState,
}

pub struct TransferQueueRules;

impl TransferQueueRules {
    /// Jobs still to come, in the order they will run. Excludes the running one.
    pub fn waiting(queue: &[QueuedJobSummary]) -> Vec<&QueuedJobSummary> {
        queue
            .iter()
            .filter(|j| j.state == QueuedJobState::Waiting)
            .collect()
    }

    /// True when this job can move one place earlier: it must be waiting, and there must be
    /// another waiting job ahead of it. A waiting job can never overtake the running one.
    pub fn can_move_up(queue: &[QueuedJobSummary], id: i32) -> bool {
        let waiting = Self::waiting(queue);
        Self::index_of(&waiting, id).is_some_and(|at| at > 0)
    }

    /// True when this job can move one place later.
    pub fn can_move_down(queue: &[QueuedJobSummary], id: i32) -> bool {
        let waiting = Self::waiting(queue);
        Self::index_of(&waiting, id).is_some_and(|at| at + 1 < waiting.len())
    }

    /// The queue with one waiting job moved by `delta` places among the other waiting ones. A move
    /// that is not allowed returns the queue unchanged rather than failing: this answers a button,
    /// and a button at the end of a list is pressed.
    pub fn move_job(queue: &[QueuedJobSummary], id: i32, delta: i32) -> Vec<QueuedJobSummary> {
        if delta == 0
            || (delta < 0 && !Self::can_move_up(queue, id))
            || (delta > 0 && !Self::can_move_down(queue, id))
        {
            return queue.to_vec();
        }

        // Reorder the waiting jobs among themselves, then lay them back into the slots the waiting
        // jobs occupied. Anything running or finished keeps its place exactly.
        let mut waiting: Vec<QueuedJobSummary> =
            Self::waiting(queue).into_iter().cloned().collect();
        let at = Self::index_of(&waiting.iter().collect::<Vec<_>>(), id)
            .expect("checked by can_move_up/down");
        let to = (at as i64 + delta as i64).clamp(0, waiting.len() as i64 - 1) as usize;
        let moved = waiting.remove(at);
        waiting.insert(to, moved);

        let mut next = waiting.into_iter();
        queue
            .iter()
            .map(|j| {
                if j.state == QueuedJobState::Waiting {
                    next.next().expect("one waiting job per waiting slot")
                } else {
                    j.clone()
                }
            })
            .collect()
    }

    /// What the whole queue still has to write, the running job included, since its own surface
    /// reports how far into it we are. **One floor makes the total a floor**: if any job's estimate
    /// came back incomplete the sum is a lower bound, and `is_usable` going false is what stops a
    /// percentage and a time remaining being drawn over a number nobody knows.
    pub fn remaining(queue: &[QueuedJobSummary]) -> TransferEstimate {
        let mut bytes = 0i64;
        let mut files = 0usize;
        let mut complete = true;
        for job in queue {
            if !matches!(job.state, QueuedJobState::Waiting | QueuedJobState::Running) {
                continue;
            }
            bytes += job.estimate.bytes;
            files += job.estimate.files;
            complete &= job.estimate.complete;
        }
        TransferEstimate {
            bytes,
            files,
            complete,
        }
    }

    /// What the status strip says. The running job's own headline is the subject; the queue only
    /// ever adds to it, so a queue of one reads exactly as it did before there was a queue.
    pub fn headline(_queue: &[QueuedJobSummary], running_headline: &str, paused: bool) -> String {
        if !paused {
            running_headline.to_string()
        } else if running_headline.is_empty() {
            "Paused".to_string()
        } else {
            format!("Paused \u{2014} {running_headline}")
        }
    }

    /// "3 waiting", or nothing at all when the queue holds only what is running. Empty rather than
    /// "0 waiting", so the strip carries no separator with nothing after it.
    pub fn waiting_text(queue: &[QueuedJobSummary]) -> String {
        match Self::waiting(queue).len() {
            0 => String::new(),
            n => format!("{} waiting", thousands(n as i64)),
        }
    }

    fn index_of(jobs: &[&QueuedJobSummary], id: i32) -> Option<usize> {
        jobs.iter().position(|j| j.id == id)
    }
}
