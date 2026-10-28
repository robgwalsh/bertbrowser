//! Creating a new folder or file: the "New" submenu's types (`NewFileTemplate`, imported from
//! Windows' ShellNew by `ShellNewImport`), the naming rule the dialog previews (`NewItemPattern`),
//! the planner that refuses before anything is written, and the executor that never adopts what
//! is already there. Spec: Core/Services/NewItem/*.cs.

mod executor;
mod models;
mod pattern;
mod planner;
mod shell_new_import;
mod template;

pub use executor::NewItemExecutor;
pub use models::{
    FailedNewItem, NewItemKind, NewItemOutcome, NewItemPlan, NewItemRejection, RejectedNewItem,
};
pub use pattern::NewItemPattern;
pub use planner::{FileSystemNewItemProbe, NewItemPlanner, NewItemProbe};
pub use shell_new_import::{ShellNewEntry, ShellNewImport, ShellNewKind};
pub use template::NewFileTemplate;
