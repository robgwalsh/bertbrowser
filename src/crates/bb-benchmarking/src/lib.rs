//! The benchmark results schema every tier writes, the comparison that judges a run against its
//! baseline, the Markdown both are read in, the machine key baselines are filed under, and the
//! startup trace the real executable writes. Spec: Core/Benchmarking/*.
//!
//! Three very different measurers feed one shape: micro-benchmarks over Core (nanoseconds, bytes
//! allocated), the UI harness over whole scenarios (milliseconds, process memory) and timed launches
//! of the real executable. One schema is what lets one baseline file hold a machine's whole picture
//! and one compare judge all of it. Pure data and pure rules — the app and the harness emit pieces
//! of it and neither may reference the benchmark tool.

mod compare;
mod machine_key;
mod markdown;
mod performance_doc;
mod results;
mod results_json;
mod startup_trace;

pub use compare::{BenchCompare, CompareOptions, CompareReport, CompareRow, CompareStatus};
pub use machine_key::{MachineFacts, MachineKey, MachineProbe, Win32Probe};
pub use markdown::BenchMarkdown;
pub use performance_doc::PerformanceDoc;
pub use results::{
    AllocStats, BenchEntry, BenchResultSet, CURRENT_SCHEMA_VERSION, GitInfo, MachineInfo,
    ProcessStats, RunInfo, TimeStats,
};
pub use results_json::BenchResultsJson;
pub use startup_trace::{
    StartupMarks, StartupMemory, StartupMemoryPoints, StartupTraceData, StartupTraceJson,
};

/// What the benchmarking code can refuse.
#[derive(Debug)]
pub enum BenchError {
    /// Statistics were asked of no samples at all.
    NoSamples,
    /// A real-data run against a synthetic baseline or the reverse: the ids coincide, the corpora
    /// do not.
    MixedCorpora,
    /// Garbage or an empty document where a results file was expected.
    NotResultsFile(String),
    /// A results file from another schema, refused rather than half-read — a silently missing
    /// field would compare as "no regression".
    SchemaVersion(i32),
    /// Garbage or an empty document where a startup trace was expected.
    NotStartupTrace(String),
    Io(std::io::Error),
}

impl std::fmt::Display for BenchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BenchError::NoSamples => write!(f, "At least one sample is needed."),
            BenchError::MixedCorpora => write!(
                f,
                "A run against real data can only be compared with a baseline recorded against real data, \
                 and a synthetic run only with a synthetic baseline. The benchmark ids coincide; the corpora do not."
            ),
            BenchError::NotResultsFile(why) => write!(f, "Not a benchmark results file: {why}"),
            BenchError::SchemaVersion(found) => write!(
                f,
                "Results file is schema version {found}; this build reads version {CURRENT_SCHEMA_VERSION}."
            ),
            BenchError::NotStartupTrace(why) => write!(f, "Not a startup trace: {why}"),
            BenchError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BenchError {}

impl From<std::io::Error> for BenchError {
    fn from(e: std::io::Error) -> Self {
        BenchError::Io(e)
    }
}
