//! Test fixtures that need nothing above bb-files: a unique temp path, the metadata fixtures copied
//! verbatim from MetadataFixtures, and a Recycle Bin that really moves files. A dev-dependency only.
//! Apart from bb-test-support (which re-exports all of it) so that a crate below bb-core can test
//! without bb-core in its test build.

pub mod fake_recycle_bin;
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

/// A directory junction, which, unlike a symlink, an ordinary account may create. Shelled out
/// because the standard library exposes no API for one. False when it could not be made.
pub fn try_create_junction(link: &std::path::Path, target: &std::path::Path) -> bool {
    use bb_base::attributes::{DIRECTORY, REPARSE_POINT};
    use std::os::windows::fs::MetadataExt;
    let _ = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output();
    match std::fs::symlink_metadata(link) {
        // The attribute bits: `FileType::is_dir` is false for a junction, which it reads as a symlink.
        Ok(meta) => {
            meta.file_attributes() & (DIRECTORY | REPARSE_POINT) == (DIRECTORY | REPARSE_POINT)
        }
        Err(_) => false,
    }
}
