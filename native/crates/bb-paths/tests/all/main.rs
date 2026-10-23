//! One file per C# test file, one test per C# test method. Theories iterate their rows and name the
//! failing row in the assertion message, so `tools/parity/tests.ps1` can match names one to one.

mod natural_sort_tests;
mod path_key_tests;
mod path_text_tests;
mod unique_path_tests;
