//! Shell, registry and COM services that were in the App project but are not painting: the one
//! process launcher, the folder-handler registration and the run-as lookup behind the registry seam,
//! the pipes and the two helpers' endpoints (index and file-operation), single instance and the
//! foreground rule, the update feed and installer hooks, and the startup trace.
//! Spec: src/BertBrowser.App/Interop/*, App/Services/*.
//!
//! Everything that decides is in `bb-core` and tested there; this crate is the Win32 that carries
//! the decision out, and each piece is behind a seam (a trait, or a fake registry) so a test never
//! touches the user's machine.

pub mod elevation;
pub mod folder_handler;
pub mod foreground;
pub mod indexing;
pub mod launcher;
pub mod pipe;
pub mod registry;
pub mod run_as;
pub mod shell_new;
pub mod single_instance;
pub mod startup_trace;
pub mod updates;

pub use elevation::SystemElevation;
pub use folder_handler::FolderHandlerRegistry;
pub use launcher::{LaunchRequest, SystemLauncher};
pub use registry::{Hive, MemoryRegistry, Registry, SystemRegistry};
pub use single_instance::SingleInstance;
