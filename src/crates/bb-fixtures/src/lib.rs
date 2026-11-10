//! Test fixtures that need nothing of the app: a unique temp path, and the metadata fixtures copied
//! verbatim from MetadataFixtures. A dev-dependency only. Apart from bb-test-support (which re-exports
//! all of it) so that a crate below bb-core can test without bb-core in its test build.

pub mod metadata_fixtures;
pub mod metadata_fixtures_data;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique name under %TEMP%, as the C# tests built theirs.
pub fn unique_temp_path(prefix: &str, suffix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{n}-{nanos:x}{suffix}",
        std::process::id()
    ))
}
