//! Saved searches, keyed by name ignoring case. Spec: Core/Data/SavedSearchRepository.cs,
//! Core/Models/SavedSearch.cs.

use std::sync::Arc;

use bb_paths::PathKey;
use rusqlite::{OptionalExtension, params};

use super::{Db, Result};
use crate::FileTime;

/// Where a saved search runs when it is opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum SavedSearchScope {
    /// A template: the folder the tab is showing at the time, whatever that is.
    CurrentFolder = 0,
    /// Pinned: always `scope_path`, navigating there first.
    Folder = 1,
    /// The whole-PC (index-backed) search.
    ThisPc = 2,
}

impl SavedSearchScope {
    pub fn from_i64(v: i64) -> SavedSearchScope {
        match v {
            1 => SavedSearchScope::Folder,
            2 => SavedSearchScope::ThisPc,
            _ => SavedSearchScope::CurrentFolder,
        }
    }
}

/// A search query stored under a user-chosen name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedSearch {
    /// The identity; unique ignoring case.
    pub name: String,
    /// Text in the search language, exactly as it would be typed.
    pub query: String,
    pub scope: SavedSearchScope,
    /// The pinned folder, present iff `scope` is `Folder`. A display path, not a path key.
    pub scope_path: Option<String>,
    /// When it was first saved. None only for one not yet stored.
    pub created_utc: Option<FileTime>,
    /// When it was last run; None if it never has been.
    pub last_used_utc: Option<FileTime>,
}

impl SavedSearch {
    pub fn new(
        name: &str,
        query: &str,
        scope: SavedSearchScope,
        scope_path: Option<&str>,
    ) -> SavedSearch {
        SavedSearch {
            name: name.to_string(),
            query: query.to_string(),
            scope,
            scope_path: scope_path.map(str::to_string),
            created_utc: None,
            last_used_utc: None,
        }
    }
}

pub struct SavedSearchRepository {
    db: Arc<Db>,
}

impl SavedSearchRepository {
    pub fn new(db: Arc<Db>) -> Self {
        SavedSearchRepository { db }
    }

    /// Every saved search, ordered by name ignoring case: the sidebar's order.
    pub fn get_all(&self) -> Result<Vec<SavedSearch>> {
        let conn = self.db.open()?;
        let mut stmt = conn.prepare(
            "SELECT name, query, scope, scope_path, added_utc, last_used_utc
             FROM saved_search ORDER BY name COLLATE NOCASE;",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(SavedSearch {
                name: r.get(0)?,
                query: r.get(1)?,
                scope: SavedSearchScope::from_i64(r.get(2)?),
                scope_path: r.get(3)?,
                created_utc: r.get::<_, Option<i64>>(4)?.map(FileTime),
                last_used_utc: r.get::<_, Option<i64>>(5)?.map(FileTime),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Records that the search was just run. A missing row is not an error: it may have been
    /// deleted in the meantime.
    pub fn mark_used(&self, name: &str, utc: FileTime) -> Result<()> {
        let conn = self.db.open()?;
        conn.execute(
            "UPDATE saved_search SET last_used_utc = ?1 WHERE name = ?2;",
            params![utc.0, name],
        )?;
        Ok(())
    }

    /// Creates the search, or replaces the query and scope of the one already stored under that
    /// name (in any case). The original added_utc is kept on a replace; the stored name takes the
    /// casing of the row that was there first, since NOCASE treats them as one key.
    pub fn save(&self, search: &SavedSearch) -> Result<()> {
        let scope_path = match &search.scope_path {
            Some(p) => Some(PathKey::normalize_display(p)?),
            None => None,
        };
        let conn = self.db.open()?;
        conn.execute(
            "INSERT INTO saved_search(name, query, scope, scope_path, added_utc)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(name) DO UPDATE SET
                 query = excluded.query,
                 scope = excluded.scope,
                 scope_path = excluded.scope_path;",
            params![
                search.name,
                search.query,
                search.scope as i64,
                scope_path,
                FileTime::now().0
            ],
        )?;
        Ok(())
    }

    /// Renames a search. False, changing nothing, when there is no such row or when `new_name`
    /// already belongs to a different row. A change of case only is a rename of the same row.
    pub fn rename(&self, old_name: &str, new_name: &str) -> Result<bool> {
        rename_row(&self.db, "saved_search", old_name, new_name)
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        let conn = self.db.open()?;
        conn.execute("DELETE FROM saved_search WHERE name = ?1;", params![name])?;
        Ok(())
    }

    pub fn exists(&self, name: &str) -> Result<bool> {
        exists_row(&self.db, "saved_search", name)
    }
}

pub(super) fn rename_row(db: &Db, table: &str, old_name: &str, new_name: &str) -> Result<bool> {
    let mut conn = db.open()?;
    let tx = conn.transaction()?;
    let same_row = bb_paths::fold_upper(old_name) == bb_paths::fold_upper(new_name);
    if !same_row {
        let taken: Option<i64> = tx
            .query_row(
                &format!("SELECT 1 FROM {table} WHERE name = ?1 LIMIT 1;"),
                params![new_name],
                |r| r.get(0),
            )
            .optional()?;
        if taken.is_some() {
            return Ok(false);
        }
    }
    let moved = tx.execute(
        &format!("UPDATE {table} SET name = ?1 WHERE name = ?2;"),
        params![new_name, old_name],
    )? > 0;
    tx.commit()?;
    Ok(moved)
}

pub(super) fn exists_row(db: &Db, table: &str, name: &str) -> Result<bool> {
    let conn = db.open()?;
    let found: Option<i64> = conn
        .query_row(
            &format!("SELECT 1 FROM {table} WHERE name = ?1 LIMIT 1;"),
            params![name],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}
