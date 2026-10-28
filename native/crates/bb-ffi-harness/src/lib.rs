//! The harness's build of the C ABI: `bb-ffi` with its `fixtures` feature on, as a second staticlib
//! (`bbcore_harness.lib`). The product links `bbcore.lib`, built without the feature, so the fixture
//! writers and the test-support code behind them are compiled into the harness only.
//!
//! Re-exporting is what keeps every `extern "C"` function of the ABI reachable from this archive.

pub use bbcore::*;
