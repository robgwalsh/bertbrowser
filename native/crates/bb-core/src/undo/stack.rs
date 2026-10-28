//! The undo history: every operation this session can still take back or do again, oldest first,
//! with a cursor between what is in effect and what has been undone.
//! Spec: Core/Services/UndoHistory/UndoStack.cs.
//!
//! **It never touches disk.** Everything that commits data comes back as a [`Released`] for the
//! caller to retire, which is what lets every rule here be tested with fake records.
//!
//! Entries `[0, cursor)` are done and `[cursor, len)` are undone, strictly, since only the entry
//! either side of the cursor may take a step. A new operation discards everything after the cursor:
//! there is one line of history, as in every editor, and the branch a new action abandons is not
//! something anybody can get back to.
//!
//! **The budget releases the oldest done entry first, then the furthest undone one, and never the
//! next undo.** A single delete larger than the whole budget stays undoable, as it did when there
//! was one slot; the history is simply one entry deep until something newer arrives. Held bytes
//! that could only partly be measured count as what was measured, which is a floor, never as
//! nothing.

use std::fmt;

use super::budget::UndoBudget;
use super::models::{HeldSize, StepReport, UndoDirection};
use super::record::UndoableRecord;
use crate::FileTime;

/// Unique for the session, and never reused, so a row or a script can name an entry across steps
/// that replace its record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntryId(pub i32);

/// One operation in the history, and what has happened to it since.
pub struct UndoEntry {
    id: EntryId,
    record: Box<dyn UndoableRecord>,
    held: HeldSize,
    created_utc: FileTime,
    changed_utc: FileTime,
    last_direction: Option<UndoDirection>,
    last_report: Option<StepReport>,
}

impl UndoEntry {
    pub fn id(&self) -> EntryId {
        self.id
    }

    pub fn record(&self) -> &dyn UndoableRecord {
        self.record.as_ref()
    }

    /// In effect: the next step on it would be an undo.
    pub fn is_done(&self) -> bool {
        self.record.is_applied()
    }

    pub fn held(&self) -> HeldSize {
        self.held
    }

    pub fn created_utc(&self) -> FileTime {
        self.created_utc
    }

    /// When it was last done, undone or redone.
    pub fn changed_utc(&self) -> FileTime {
        self.changed_utc
    }

    /// The most recent step taken on it, and how it went; None for an entry that has only ever
    /// been done.
    pub fn last_direction(&self) -> Option<UndoDirection> {
        self.last_direction
    }

    pub fn last_report(&self) -> Option<&StepReport> {
        self.last_report.as_ref()
    }
}

/// What leaving the history took with it. The caller owes each entry's record one
/// [`UndoableRecord::retire`], off the UI thread: that is the moment the data goes.
#[derive(Default)]
pub struct Released {
    pub entries: Vec<UndoEntry>,
}

impl Released {
    pub fn nothing() -> Released {
        Released::default()
    }

    pub fn held(&self) -> HeldSize {
        self.entries
            .iter()
            .fold(HeldSize::NONE, |sum, e| sum + e.held)
    }

    pub fn retire_all(&self) {
        for entry in &self.entries {
            entry.record.retire();
        }
    }
}

/// The ways the history refuses a call, each an `ArgumentException` in the C#.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoError {
    /// A new operation is always in effect.
    NotApplied,
    /// That entry is not done.
    NotDone,
    /// That entry is not undone.
    NotUndone,
    /// That entry is no longer in the history.
    NotInHistory,
    /// Only the entry either side of the cursor can take a step.
    NotBesideCursor,
    /// The step did not change the entry's state.
    StateUnchanged,
}

impl fmt::Display for UndoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UndoError::NotApplied => "A new operation is always in effect.",
            UndoError::NotDone => "That entry is not done.",
            UndoError::NotUndone => "That entry is not undone.",
            UndoError::NotInHistory => "That entry is no longer in the history.",
            UndoError::NotBesideCursor => {
                "Only the entry either side of the cursor can take a step."
            }
            UndoError::StateUnchanged => "The step did not change the entry's state.",
        })
    }
}

impl std::error::Error for UndoError {}

pub struct UndoStack {
    entries: Vec<UndoEntry>,
    next_id: i32,
    budget: UndoBudget,
    cursor: usize,
    released_count: usize,
    released_bytes: i64,
}

impl UndoStack {
    pub fn new(budget: UndoBudget) -> UndoStack {
        UndoStack {
            entries: Vec::new(),
            next_id: 1,
            budget,
            cursor: 0,
            released_count: 0,
            released_bytes: 0,
        }
    }

    pub fn budget(&self) -> UndoBudget {
        self.budget
    }

    /// Oldest first.
    pub fn entries(&self) -> &[UndoEntry] {
        &self.entries
    }

    pub fn entry(&self, id: EntryId) -> Option<&UndoEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// How many entries are done; the index of the first undone one.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn next_undo(&self) -> Option<&UndoEntry> {
        if self.cursor > 0 {
            Some(&self.entries[self.cursor - 1])
        } else {
            None
        }
    }

    pub fn next_redo(&self) -> Option<&UndoEntry> {
        self.entries.get(self.cursor)
    }

    pub fn held(&self) -> HeldSize {
        self.entries
            .iter()
            .fold(HeldSize::NONE, |sum, e| sum + e.held)
    }

    /// How many entries have left the history to stay within the budget this session.
    pub fn released_count(&self) -> usize {
        self.released_count
    }

    /// What those entries were holding when they went.
    pub fn released_bytes(&self) -> i64 {
        self.released_bytes
    }

    /// Adds a new operation after the cursor, discarding the redo branch first.
    pub fn push(
        &mut self,
        record: Box<dyn UndoableRecord>,
        held: HeldSize,
        now_utc: FileTime,
    ) -> Result<(EntryId, Released), UndoError> {
        if !record.is_applied() {
            return Err(UndoError::NotApplied);
        }

        let mut released: Vec<UndoEntry> = self.entries.drain(self.cursor..).collect();

        let id = EntryId(self.next_id);
        self.next_id += 1;
        self.entries.push(UndoEntry {
            id,
            record,
            held,
            created_utc: now_utc,
            changed_utc: now_utc,
            last_direction: None,
            last_report: None,
        });
        self.cursor = self.entries.len();

        self.evict(&mut released);
        Ok((id, Released { entries: released }))
    }

    /// What undoing back to `target` would run, next undo first.
    pub fn undo_span(&self, target: EntryId) -> Result<Vec<EntryId>, UndoError> {
        let index = self.index_of(target)?;
        if index >= self.cursor {
            return Err(UndoError::NotDone);
        }
        Ok((index..self.cursor)
            .rev()
            .map(|i| self.entries[i].id)
            .collect())
    }

    /// What redoing forward to `target` would run, next redo first.
    pub fn redo_span(&self, target: EntryId) -> Result<Vec<EntryId>, UndoError> {
        let index = self.index_of(target)?;
        if index < self.cursor {
            return Err(UndoError::NotUndone);
        }
        Ok((self.cursor..=index).map(|i| self.entries[i].id).collect())
    }

    /// Records a step that moved something: the entry swaps to `next` and the cursor passes it.
    /// Returns anything the new held size pushed over the budget.
    pub fn applied(
        &mut self,
        entry: EntryId,
        next: Box<dyn UndoableRecord>,
        held: HeldSize,
        report: StepReport,
        now_utc: FileTime,
    ) -> Result<Released, UndoError> {
        let direction = self.direction(entry)?;
        if next.is_applied() != (direction == UndoDirection::Redo) {
            return Err(UndoError::StateUnchanged);
        }

        let index = self.index_of(entry)?;
        let slot = &mut self.entries[index];
        slot.record = next;
        slot.held = held;
        slot.last_direction = Some(direction);
        slot.last_report = Some(report);
        slot.changed_utc = now_utc;
        match direction {
            UndoDirection::Undo => self.cursor -= 1,
            UndoDirection::Redo => self.cursor += 1,
        }

        let mut released = Vec::new();
        self.evict(&mut released);
        Ok(Released { entries: released })
    }

    /// Records a step that moved nothing. The entry stays where it was, and says why.
    pub fn refused(
        &mut self,
        entry: EntryId,
        report: StepReport,
        now_utc: FileTime,
    ) -> Result<(), UndoError> {
        let direction = self.direction(entry)?;
        let index = self.index_of(entry)?;
        let slot = &mut self.entries[index];
        slot.last_direction = Some(direction);
        slot.last_report = Some(report);
        slot.changed_utc = now_utc;
        Ok(())
    }

    pub fn set_budget(&mut self, budget: UndoBudget) -> Released {
        self.budget = budget;
        let mut released = Vec::new();
        self.evict(&mut released);
        Released { entries: released }
    }

    /// Empties the history. Not counted as released for the budget's sake: the footer's "released"
    /// figure is about what the limits took, not what the user asked to let go.
    pub fn clear(&mut self) -> Released {
        let entries = std::mem::take(&mut self.entries);
        self.cursor = 0;
        Released { entries }
    }

    /// Which way the next step on `entry` goes. Only the two entries either side of the cursor
    /// can take one.
    pub fn direction(&self, entry: EntryId) -> Result<UndoDirection, UndoError> {
        if self.next_undo().is_some_and(|e| e.id == entry) {
            return Ok(UndoDirection::Undo);
        }
        if self.next_redo().is_some_and(|e| e.id == entry) {
            return Ok(UndoDirection::Redo);
        }
        Err(UndoError::NotBesideCursor)
    }

    fn index_of(&self, entry: EntryId) -> Result<usize, UndoError> {
        self.entries
            .iter()
            .position(|e| e.id == entry)
            .ok_or(UndoError::NotInHistory)
    }

    fn evict(&mut self, released: &mut Vec<UndoEntry>) {
        while self.over_budget() {
            let victim = if self.cursor > 1 {
                0 // oldest done, not the next undo
            } else if self.entries.len() > self.cursor {
                self.entries.len() - 1 // furthest undone
            } else {
                break; // only the next undo is left
            };

            let entry = self.entries.remove(victim);
            if victim < self.cursor {
                self.cursor -= 1;
            }
            self.released_count += 1;
            self.released_bytes += entry.held.bytes;
            released.push(entry);
        }
    }

    fn over_budget(&self) -> bool {
        self.entries.len() > self.budget.max_entries.max(0) as usize
            || self.held().bytes > self.budget.max_held_bytes
    }
}
