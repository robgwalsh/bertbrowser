//! Sidebar bookmarks, keyed by canonical path. Spec: Core/Data/BookmarkRepository.cs.

use std::sync::Arc;

use bb_paths::PathKey;
use rusqlite::{OptionalExtension, params};

use super::{Db, Result};
use crate::FileTime;

/// A user-pinned file or directory shown in the sidebar's Bookmarks section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    /// Casing-preserving normalized path.
    pub display_path: String,
    pub is_directory: bool,
}

pub struct BookmarkRepository {
    db: Arc<Db>,
}

impl BookmarkRepository {
    pub fn new(db: Arc<Db>) -> Self {
        BookmarkRepository { db }
    }

    /// All bookmarks, ordered for display (folders before files, then by name).
    pub fn get_all(&self) -> Result<Vec<Bookmark>> {
        let conn = self.db.open()?;
        let mut stmt = conn.prepare(
            "SELECT display_path, is_directory FROM bookmark ORDER BY is_directory DESC, display_path COLLATE NOCASE;",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Bookmark {
                display_path: r.get(0)?,
                is_directory: r.get::<_, i64>(1)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Adds a bookmark. False if the path was already bookmarked.
    pub fn add(&self, path: &str, is_directory: bool) -> Result<bool> {
        let conn = self.db.open()?;
        let changed = conn.execute(
            "INSERT OR IGNORE INTO bookmark(path_key, display_path, is_directory, added_utc) VALUES (?1, ?2, ?3, ?4);",
            params![
                PathKey::canonicalize(path)?,
                PathKey::normalize_display(path)?,
                is_directory as i64,
                FileTime::now().0
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn remove(&self, path: &str) -> Result<()> {
        let conn = self.db.open()?;
        conn.execute(
            "DELETE FROM bookmark WHERE path_key = ?1;",
            params![PathKey::canonicalize(path)?],
        )?;
        Ok(())
    }

    pub fn exists(&self, path: &str) -> Result<bool> {
        let conn = self.db.open()?;
        let found: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM bookmark WHERE path_key = ?1 LIMIT 1;",
                params![PathKey::canonicalize(path)?],
                |r| r.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }
}
