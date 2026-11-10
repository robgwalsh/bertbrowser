//! One test binary for the crate rather than one per file: each binary links the whole dependency
//! graph, so bb-ffi's files used to cost a link apiece. Each file is a module; `common` is shared.

mod features;
mod ffi;
mod ops;
mod panes;
