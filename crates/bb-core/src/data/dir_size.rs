//! `dir_size_cache`: recursive directory totals from the MFT pass. Spec: Core/Data/DirSizeRepository.cs.

use std::collections::HashMap;
use std::sync::Arc;

use bb_paths::PathKey;
use rusqlite::params;

use super::{Db, IN_CHUNK, Result, placeholders};
use crate::FileTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirSizeResult {
    pub path_key: String,
    pub size_bytes: i64,
    pub file_count: i32,
    pub dir_count: i32,
    pub incomplete: bool,
    pub computed_utc: FileTime,
}

pub struct DirSizeRepository {
    db: Arc<Db>,
}

impl DirSizeRepository {
    pub fn new(db: Arc<Db>) -> Self {
        DirSizeRepository { db }
    }

    /// Upserts an entire scanned subtree in one transaction.
    pub fn upsert_many<'a>(
        &self,
        results: impl IntoIterator<Item = &'a DirSizeResult>,
    ) -> Result<()> {
        let mut conn = self.db.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO dir_size_cache(path_key, size_bytes, file_count, dir_count, incomplete, computed_utc)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(path_key) DO UPDATE SET
                     size_bytes = excluded.size_bytes,
                     file_count = excluded.file_count,
                     dir_count = excluded.dir_count,
                     incomplete = excluded.incomplete,
                     computed_utc = excluded.computed_utc;",
            )?;
            for r in results {
                stmt.execute(params![
                    r.path_key,
                    r.size_bytes,
                    r.file_count,
                    r.dir_count,
                    r.incomplete as i64,
                    r.computed_utc.0
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Cached sizes for a set of directories (typically the children of the current one), keyed by
    /// canonical path key.
    pub fn get_many(&self, paths: &[&str]) -> Result<HashMap<String, DirSizeResult>> {
        let mut result = HashMap::new();
        if paths.is_empty() {
            return Ok(result);
        }
        let keys = paths
            .iter()
            .map(|p| PathKey::canonicalize(p))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let conn = self.db.open()?;
        for chunk in keys.chunks(IN_CHUNK) {
            let sql = format!(
                "SELECT path_key, size_bytes, file_count, dir_count, incomplete, computed_utc
                 FROM dir_size_cache WHERE path_key IN ({});",
                placeholders(chunk.len())
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                Ok(DirSizeResult {
                    path_key: r.get(0)?,
                    size_bytes: r.get(1)?,
                    file_count: r.get(2)?,
                    dir_count: r.get(3)?,
                    incomplete: r.get::<_, i64>(4)? != 0,
                    computed_utc: FileTime(r.get(5)?),
                })
            })?;
            for row in rows {
                let row = row?;
                result.insert(row.path_key.clone(), row);
            }
        }
        Ok(result)
    }

    pub fn get(&self, path: &str) -> Result<Option<DirSizeResult>> {
        let key = PathKey::canonicalize(path)?;
        let mut rows = self.get_many(&[key.as_str()])?;
        Ok(rows.remove(&key))
    }
}
