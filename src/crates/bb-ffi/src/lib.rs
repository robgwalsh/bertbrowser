//! The C ABI between the Rust core and the C++ UI. cbindgen reads this crate alone and writes
//! `include/bbcore.h`.
//!
//! Rules that every export keeps:
//! - Every `bb_*` call is made from the UI thread, and none blocks on long work. Results arrive as
//!   events through the sink.
//! - The sink is invoked on Rust threads and **may only enqueue**. Rust never re-enters C++ any other
//!   way.
//! - Every export runs under `catch_unwind`: a panic becomes `BB_PANIC`, never an unwind across the
//!   boundary. The message is readable with `bb_last_error` on the same thread.
//! - A handle or id that names something that is gone (a closed tab, a destroyed pane) answers
//!   `BB_STALE`; ids are never reused within a run.
//! - Strings cross as UTF-16. `BbStr` is a borrowed view: valid for the call when it is an argument,
//!   and for the documented lifetime of its owner when it is a result. `BbOwnedStr` is freed with
//!   `bb_str_free`; a null `ptr` means "none".
//! - Events are Rust-owned; the receiver frees each with `bb_event_free` once it is done with it.

#![deny(clippy::undocumented_unsafe_blocks)]
#![allow(non_camel_case_types)]

use std::any::Any;
use std::cell::RefCell;
use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

use bb_app::{
    App, AppConfig, AppError, AppEvent, Fakes, Listing, OpId, OpState, PaneId, SelectOp, TabId,
};
use bb_commands::commands::{ChordModifiers, FocusKind, FocusState, KeyChord};
use bb_core::delete::DeleteMode;
use bb_core::layout::SplitOrientation;

mod alloc;
pub use alloc::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod abi;
pub use abi::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod events;
pub use events::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod lifecycle;
pub use lifecycle::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod tabs;
pub use tabs::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod listing;
pub use listing::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod ops_core;
pub use ops_core::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod keys;
pub use keys::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod settings;
pub use settings::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod features;
pub use features::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod ops_ffi;
pub use ops_ffi::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod panes;
pub use panes::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod startup;
pub use startup::*;
#[allow(
    clippy::missing_safety_doc,
    reason = "every export is `unsafe extern \"C\"` under the one contract in the crate docs: UI thread, pointers valid for the call"
)]
mod tools;
pub use tools::*;
