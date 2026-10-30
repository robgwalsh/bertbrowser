//! Persistence for `fs_change`, the change timeline. The writer is the index helper's USN tail and
//! the reader is the app, in two processes over one WAL database. Spec: Core/Data/ChangeLogRepository.cs.

use std::sync::Arc;
use std::time::Duration;

use bb_paths::PathKey;
use rusqlite::{ToSql, params};

use super::{Db, Result};
use crate::FileTime;
use crate::changes::{
    ChangeEvent, ChangeKind, ChangeLogRules, ChangeLogStamp, ChangeQuery, ChangeRow,
};
use crate::time::duration_ticks;

pub struct ChangeLogRepository {
    db: Arc<Db>,
}

impl ChangeLogRepository {
    pub fn new(db: Arc<Db>) -> Self {
        ChangeLogRepository { db }
    }

    /// Writes one drained batch in one transaction, folding each event into the most recent row for
    /// the same path and kind when that row is inside the coalesce window.
    ///
    /// An empty batch opens no connection. Every write here is itself a filesystem change the tail
    /// will see on its next poll, and a writer that touched the database for nothing would tick the
    /// journal for ever.
    pub fn record(&self, events: &[ChangeEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let window = duration_ticks(ChangeLogRules::COALESCE_WINDOW);
        let mut conn = self.db.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        {
            let mut fold = tx.prepare(
                "UPDATE fs_change SET count = count + 1, last_utc = ?1
                 WHERE id = (SELECT id FROM fs_change
                             WHERE path_key = ?2 AND kind = ?3 AND last_utc >= ?4
                             ORDER BY last_utc DESC LIMIT 1);",
            )?;
            let mut insert = tx.prepare(
                "INSERT INTO fs_change(path_key, display_path, old_path, is_dir, hidden, kind, first_utc, last_utc, count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1);",
            )?;
            for ev in events {
                let folded = fold.execute(params![
                    ev.utc.0,
                    ev.path_key,
                    ev.kind as i64,
                    ev.utc.0 - window
                ])?;
                if folded > 0 {
                    continue;
                }
                insert.execute(params![
                    ev.path_key,
                    ev.display_path,
                    ev.old_display_path,
                    ev.is_directory as i64,
                    ev.hidden as i64,
                    ev.kind as i64,
                    ev.utc.0
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Drops rows older than `retention`, then anything past the cap, newest kept. Idempotent.
    pub fn prune(&self, now_utc: FileTime, retention: Duration, max_rows: i64) -> Result<()> {
        let mut conn = self.db.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM fs_change WHERE last_utc < ?1;",
            params![(now_utc - retention).0],
        )?;

        // Ids only ever grow, so the span from the oldest to the newest is at least the row count:
        // a table whose span is inside the cap has nothing past it. Both ends are one step into the
        // rowid tree, where the OFFSET below walks every row it skips.
        let span: Option<i64> = tx.query_row(
            "SELECT (SELECT max(id) FROM fs_change) - (SELECT min(id) FROM fs_change) + 1;",
            [],
            |r| r.get(0),
        )?;
        if span.is_none_or(|s| s <= max_rows) {
            tx.commit()?;
            return Ok(());
        }

        // The subselect is NULL while the table is under the cap, and a comparison with NULL deletes
        // nothing.
        tx.execute(
            "DELETE FROM fs_change WHERE id < (SELECT id FROM fs_change ORDER BY id DESC LIMIT 1 OFFSET ?1);",
            params![(max_rows - 1).max(0)],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The privacy switch: everything, gone.
    pub fn clear(&self) -> Result<()> {
        let conn = self.db.open()?;
        conn.execute("DELETE FROM fs_change;", [])?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self.db.open()?;
        Ok(conn.query_row("SELECT count(*) FROM fs_change;", [], |r| r.get(0))?)
    }

    /// Changes whenever a row is added *or folded*; the live view polls this rather than re-running
    /// its query.
    pub fn stamp(&self) -> Result<ChangeLogStamp> {
        let conn = self.db.open()?;
        let (max_id, last): (Option<i64>, Option<i64>) =
            conn.query_row("SELECT max(id), max(last_utc) FROM fs_change;", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        Ok(ChangeLogStamp {
            max_id: max_id.unwrap_or(0),
            last_utc: last.map(FileTime),
        })
    }

    /// Newest first, bounded below by the range and above by the limit. The time bound and the
    /// ordering both come off `ix_fs_change_last`, so the scan stops the moment the limit is full;
    /// scope, kind and hidden are filters on rows already in range, never a second index.
    pub fn query(&self, query: &ChangeQuery) -> Result<(Vec<ChangeRow>, bool)> {
        let (predicate, values) = predicate(query)?;
        let limit = query.limit as i64 + 1;
        let sql = format!(
            "SELECT id, path_key, display_path, old_path, is_dir, hidden, kind, first_utc, last_utc, count
             {predicate} ORDER BY last_utc DESC LIMIT {limit};"
        );
        let conn = self.db.open()?;
        let mut stmt = conn.prepare(&sql)?;
        let bound: Vec<&dyn ToSql> = values.iter().map(|v| v as &dyn ToSql).collect();
        let mut rows = stmt.query(bound.as_slice())?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            if out.len() == query.limit {
                return Ok((out, true));
            }
            out.push(ChangeRow {
                id: r.get(0)?,
                path_key: r.get(1)?,
                display_path: r.get(2)?,
                old_display_path: r.get(3)?,
                is_directory: r.get::<_, i64>(4)? != 0,
                hidden: r.get::<_, i64>(5)? != 0,
                kind: ChangeKind::from_i64(r.get(6)?),
                first_utc: FileTime(r.get(7)?),
                last_utc: FileTime(r.get(8)?),
                count: r.get(9)?,
            });
        }
        Ok((out, false))
    }

    /// The planner's account of [`query`](Self::query), for the test that keeps it a range scan.
    pub fn explain_query(&self, query: &ChangeQuery) -> Result<Vec<String>> {
        let (predicate, values) = predicate(query)?;
        let limit = query.limit as i64 + 1;
        let sql = format!(
            "EXPLAIN QUERY PLAN SELECT id {predicate} ORDER BY last_utc DESC LIMIT {limit};"
        );
        let conn = self.db.open()?;
        let mut stmt = conn.prepare(&sql)?;
        let bound: Vec<&dyn ToSql> = values.iter().map(|v| v as &dyn ToSql).collect();
        let columns = stmt.column_count();
        let mut rows = stmt.query(bound.as_slice())?;
        let mut lines = Vec::new();
        while let Some(r) = rows.next()? {
            lines.push(r.get::<_, String>(columns - 1)?);
        }
        Ok(lines)
    }
}

/// The FROM/WHERE part with positional parameters, and their values in order. Pinned to
/// `ix_fs_change_last`: given the scope bounds the planner prefers `ix_fs_change_key` and then sorts,
/// which materialises every row in range before the LIMIT can stop anything.
fn predicate(query: &ChangeQuery) -> Result<(String, Vec<rusqlite::types::Value>)> {
    use rusqlite::types::Value;
    let mut sql = String::from(" FROM fs_change INDEXED BY ix_fs_change_last WHERE last_utc >= ?1");
    let mut values = vec![Value::Integer(query.since_utc.0)];

    if let Some(scope) = &query.scope_path_key {
        let (lo, hi) = PathKey::prefix_bounds(scope)?;
        sql.push_str(" AND path_key >= ?2 AND path_key < ?3");
        values.push(Value::Text(lo));
        values.push(Value::Text(hi));
    }

    // Fewer than all four: a literal list, since the values are this enum's own integers.
    if query.kinds.len() < 4 {
        if query.kinds.is_empty() {
            sql.push_str(" AND 0");
        } else {
            let mut kinds: Vec<i64> = query.kinds.iter().map(|k| *k as i64).collect();
            kinds.sort_unstable();
            let list = kinds
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            sql.push_str(&format!(" AND kind IN ({list})"));
        }
    }

    if !query.include_hidden {
        sql.push_str(" AND hidden = 0");
    }
    Ok((sql, values))
}
