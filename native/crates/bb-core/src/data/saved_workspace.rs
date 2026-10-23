//! Saved workspaces, keyed by name ignoring case. Spec: Core/Data/SavedWorkspaceRepository.cs,
//! Core/Models/SavedWorkspace.cs.

use std::sync::Arc;

use rusqlite::params;

use super::saved_search::{exists_row, rename_row};
use super::{Db, Result};
use crate::FileTime;
use crate::layout::SessionLayout;

/// A pane arrangement stored under a user-chosen name.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedWorkspace {
    /// The identity; unique ignoring case.
    pub name: String,
    /// The captured pane tree: splits, tabs, sort and columns.
    pub layout: SessionLayout,
    pub created_utc: Option<FileTime>,
    pub last_used_utc: Option<FileTime>,
}

impl SavedWorkspace {
    pub fn new(name: &str, layout: SessionLayout) -> SavedWorkspace {
        SavedWorkspace {
            name: name.to_string(),
            layout,
            created_utc: None,
            last_used_utc: None,
        }
    }
}

pub struct SavedWorkspaceRepository {
    db: Arc<Db>,
}

impl SavedWorkspaceRepository {
    pub fn new(db: Arc<Db>) -> Self {
        SavedWorkspaceRepository { db }
    }

    /// Every saved workspace, ordered by name ignoring case. A row whose layout_json fails to parse
    /// (hand-edited DB, a future format change) is skipped rather than failing the whole list.
    pub fn get_all(&self) -> Result<Vec<SavedWorkspace>> {
        let conn = self.db.open()?;
        let mut stmt = conn.prepare(
            "SELECT name, layout_json, added_utc, last_used_utc FROM saved_workspace ORDER BY name COLLATE NOCASE;",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
            ))
        })?;
        let mut list = Vec::new();
        for row in rows {
            let (name, json, added, used) = row?;
            let Ok(layout) = serde_json::from_str::<SessionLayout>(&json) else {
                continue;
            };
            list.push(SavedWorkspace {
                name,
                layout,
                created_utc: added.map(FileTime),
                last_used_utc: used.map(FileTime),
            });
        }
        Ok(list)
    }

    /// Creates the workspace, or replaces the layout of the one already stored under that name (in
    /// any case). The original added_utc is kept on a replace.
    pub fn save(&self, workspace: &SavedWorkspace) -> Result<()> {
        let json = serde_json::to_string(&workspace.layout)?;
        let conn = self.db.open()?;
        conn.execute(
            "INSERT INTO saved_workspace(name, layout_json, added_utc) VALUES (?1, ?2, ?3)
             ON CONFLICT(name) DO UPDATE SET layout_json = excluded.layout_json;",
            params![workspace.name, json, FileTime::now().0],
        )?;
        Ok(())
    }

    pub fn rename(&self, old_name: &str, new_name: &str) -> Result<bool> {
        rename_row(&self.db, "saved_workspace", old_name, new_name)
    }

    /// Records that the workspace was just switched to. A missing row is not an error.
    pub fn mark_used(&self, name: &str, utc: FileTime) -> Result<()> {
        let conn = self.db.open()?;
        conn.execute(
            "UPDATE saved_workspace SET last_used_utc = ?1 WHERE name = ?2;",
            params![utc.0, name],
        )?;
        Ok(())
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        let conn = self.db.open()?;
        conn.execute(
            "DELETE FROM saved_workspace WHERE name = ?1;",
            params![name],
        )?;
        Ok(())
    }

    pub fn exists(&self, name: &str) -> Result<bool> {
        exists_row(&self.db, "saved_workspace", name)
    }
}
