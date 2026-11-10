//! Shared test helpers: temp databases and trees under %TEMP%, in-memory pipe pairs, fixtures copied
//! verbatim from MetadataFixtures and ArchiveFixtures, in-memory probes. A dev-dependency only.

// Re-exported from bb-fixtures and bb-archive-fixtures, which a crate below bb-core can use without
// bb-core.
pub use bb_archive_fixtures::archive_support;
pub use bb_fixtures::{
    archive_fixtures, fake_recycle_bin, metadata_fixtures, metadata_fixtures_data,
    try_create_junction, unique_temp_path,
};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bb_core::data::Db;

pub mod duplex;
pub mod pipe_fakes;

/// A migrated database in a temp file, deleted with its WAL and SHM on drop.
pub struct TempDb {
    pub path: PathBuf,
    pub db: Arc<Db>,
}

impl TempDb {
    pub fn new() -> TempDb {
        let path = unique_temp_path("bertbrowser-test", ".db");
        let db = Arc::new(Db::new(path.clone()));
        db.migrate().expect("migrate");
        TempDb { path, db }
    }

    /// A database file that was never migrated: no tables at all.
    pub fn bare() -> TempDb {
        let path = unique_temp_path("bertbrowser-test", ".db");
        TempDb {
            path: path.clone(),
            db: Arc::new(Db::new(path)),
        }
    }

    pub fn db(&self) -> Arc<Db> {
        self.db.clone()
    }
}

impl Default for TempDb {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        self.db.clear_pool();
        remove_db_files(&self.path);
    }
}

/// Deletes `path` and its `-wal`/`-shm` siblings, ignoring what is not there.
pub fn remove_db_files(path: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        let _ = std::fs::remove_file(p);
    }
}
