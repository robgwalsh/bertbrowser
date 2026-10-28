//! One operation in the undo history, in whichever state it is in now.
//! Spec: Core/Services/UndoHistory/IUndoableRecord.cs.
//!
//! **Immutable, and a step returns a new one.** An undo produces the record a redo runs from, and a
//! redo produces the record the next undo runs from, each holding only what actually moved. That is
//! what keeps a partial undo from being replayed: the items that could not be put back are simply
//! not in the next record, so nothing ever tries them twice, and nothing a later step does can reach
//! back into a record that has already been spent.
//!
//! **Retire is where data goes.** Up to then everything the record holds (a displaced file, a
//! deleted folder, an archive's other version) can still be put back. The history calls it once,
//! when the entry is released: pushed out by the budget, dropped as an abandoned redo branch,
//! cleared, or left behind when the session ends.

use super::host::UndoHost;
use super::models::{HeldEntry, StepResult, UndoItemDetail, UndoKind};

pub trait UndoableRecord: Send {
    fn kind(&self) -> UndoKind;

    /// True when the operation is in effect, so the next step is an undo; false when it has been
    /// undone, so the next step is a redo.
    fn is_applied(&self) -> bool;

    /// What the operation was, in the imperative: "Move 3 items to Documents". Carried unchanged
    /// from record to record so an entry keeps its name through undo and redo.
    fn description(&self) -> &str;

    /// How many items the next step would act on.
    fn item_count(&self) -> usize;

    /// The items, as the user would describe them: where each came from and went to.
    fn details(&self) -> Vec<UndoItemDetail>;

    /// What this app is holding for the record in its current state.
    fn held(&self) -> Vec<HeldEntry>;

    /// Undoes the operation when it is applied, redoes it when it is not.
    fn step(&self, host: &dyn UndoHost) -> StepResult;

    /// Commits whatever the record is holding. Never panics.
    fn retire(&self);
}
