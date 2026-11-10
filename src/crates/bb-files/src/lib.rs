//! Files on disk, one operation at a time: listing and walking a folder, and the executors that
//! change what is there - rename, delete, transfer (move/copy), new item, shortcut, file dates - with
//! their planners, undo data and the definition of "the same file". No database and no index: those
//! are bb-core's, above this. bb-core re-exports each module at its old path (`bb_core::transfer`).

pub mod delete;
pub mod equality;
pub mod fs;
pub mod models;
pub mod newitem;
pub mod rename;
pub mod shortcuts;
pub mod timestamps;
pub mod transfer;
