//! One file per C# test file, one test per C# test method. Theories iterate their rows and name the
//! failing row in the assertion message, so `tools/parity/tests.ps1` can match names one to one.

mod changes;
mod cli;
mod columns;
mod compare;
mod crate_layout_tests;
mod data;
mod diff;
mod fs;
mod ipc;
mod layout;
mod mft;
mod rules;
mod search;
mod updates;
mod util;
