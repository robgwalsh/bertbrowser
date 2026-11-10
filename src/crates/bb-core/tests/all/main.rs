//! One file per C# test file, one test per C# test method. Theories iterate their rows and name the
//! failing row in the assertion message, so `tools/parity/tests.ps1` can match names one to one.

mod cli;
mod crate_layout_tests;
mod fs;
mod search;
mod updates;
mod util;
