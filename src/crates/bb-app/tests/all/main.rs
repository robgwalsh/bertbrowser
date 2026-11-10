//! One test binary for the crate rather than one per file: each binary links the whole dependency
//! graph, so bb-app's files used to cost a link apiece. Each file is a module; `common` is shared.

mod app;
mod common;
mod features;
mod ops;
mod panes;
mod system_seams;
mod tools;
