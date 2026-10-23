//! Persistence for the `fs_entry` / `fs_index_root` search index. The compiled predicate is a filter
//! on a clustered range scan, never a seek: there is no index on size or dates, deliberately, since a
//! secondary index on a WITHOUT ROWID table carries the whole key again and doubles every build's
//! write cost. Spec: Core/Data/FsIndexRepository.cs, Core/Models/{FsEntryRow,FsIndexRoot,FsSubtreeRow,SearchHit}.cs.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bb_paths::{PathKey, fold_upper, parent_key};
use rusqlite::{Connection, ToSql, params};

use super::{Db, Error, IN_CHUNK, Result, placeholders};
use crate::FileTime;
use crate::cancel::CancelToken;
use crate::search::{ContentMatch, SearchCandidate, SearchQuery, SqlPredicate};

/// A row destined for (or read from) the `fs_entry` search index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsEntryRow {
    pub path_key: String,
    pub name: String,
    pub is_directory: bool,
    pub size_bytes: i64,
    pub modified_utc: FileTime,
    /// The entry's *effective* hidden state: its own attribute or that of any ancestor.
    pub hidden: bool,
    /// The entry's *own* Win32 attribute mask, uninherited. Zero means not recorded.
    pub attributes: u32,
    /// Unknown (zero) for the names-only build path.
    pub created_utc: FileTime,
}

/// An indexed subtree root (`fs_index_root` row).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsIndexRoot {
    pub path_key: String,
    pub display_path: String,
    pub crawled_utc: FileTime,
    pub complete: bool,
    pub stale: bool,
}

/// One indexed entry inside a subtree, addressed relative to that subtree's root. A whole display
/// path is deliberately not stored per row; it is rebuilt from the ancestors' names on demand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsSubtreeRow {
    /// The canonical key with the root sliced off: upper-cased, no leading separator.
    pub relative_key: String,
    /// The leaf name in the casing the index recorded.
    pub name: String,
    pub is_directory: bool,
    pub size_bytes: i64,
    pub modified_utc: FileTime,
}

/// One search result. `relative_dir_display` is the parent directory relative to the search root
/// ("" for direct children), display-cased; for a whole-PC search it is the full parent path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub display_path: String,
    pub relative_dir_display: String,
    pub name: String,
    pub is_directory: bool,
    pub size_bytes: i64,
    pub modified_utc: FileTime,
    pub hidden: bool,
    /// Zero means not recorded rather than none.
    pub attributes: u32,
    pub created_utc: FileTime,
    /// Where a `content:` term found its needle, when one did.
    pub content_match: Option<ContentMatch>,
}

/// The shortlist a duplicate scan starts from, with the two counts that tell a sizeless index from
/// an empty folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateShortlist {
    pub files: Vec<SearchHit>,
    pub files_in_scope: i64,
    pub sized_files_in_scope: i64,
}

/// One row read back out of `fs_entry`, before its display path is reassembled.
struct EntryRow {
    key: String,
    name: String,
    is_dir: bool,
    size: i64,
    modified: FileTime,
    hidden: bool,
    attributes: u32,
    created: FileTime,
}

/// The drive root prefix length in a canonical key: "C:\".
const DRIVE_ROOT_LENGTH: usize = 3;

pub struct FsIndexRepository {
    db: Arc<Db>,
}

impl FsIndexRepository {
    pub fn new(db: Arc<Db>) -> Self {
        FsIndexRepository { db }
    }

    /// Bulk-upserts one chunk of crawled entries in a single transaction.
    pub fn upsert_entries(&self, rows: &[FsEntryRow], crawl_gen: i64) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut conn = self.db.open()?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO fs_entry(path_key, name, name_key, is_dir, size_bytes, modified_utc, hidden,
                                      attributes, created_utc, crawl_gen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(path_key) DO UPDATE SET
                     name = excluded.name, name_key = excluded.name_key, is_dir = excluded.is_dir,
                     size_bytes = excluded.size_bytes, modified_utc = excluded.modified_utc,
                     hidden = excluded.hidden, attributes = excluded.attributes,
                     created_utc = excluded.created_utc, crawl_gen = excluded.crawl_gen;",
            )?;
            for row in rows {
                stmt.execute(params![
                    row.path_key,
                    row.name,
                    fold_upper(&row.name),
                    row.is_directory as i64,
                    row.size_bytes,
                    row.modified_utc.0,
                    row.hidden as i64,
                    i64::from(row.attributes),
                    row.created_utc.0,
                    crawl_gen
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Removes entries under `root_key` that a completed crawl did not touch (their crawl_gen
    /// predates the crawl): entries that vanished from disk.
    pub fn sweep_vanished(&self, root_key: &str, crawl_gen: i64) -> Result<()> {
        let (lo, hi) = PathKey::prefix_bounds(root_key)?;
        let conn = self.db.open()?;
        conn.execute(
            "DELETE FROM fs_entry WHERE path_key >= ?1 AND path_key < ?2 AND crawl_gen < ?3;",
            params![lo, hi, crawl_gen],
        )?;
        Ok(())
    }

    /// Registers (or refreshes) an indexed root; always clears the stale flag.
    pub fn upsert_root(
        &self,
        root_key: &str,
        display_path: &str,
        crawled_utc: FileTime,
        complete: bool,
    ) -> Result<()> {
        let conn = self.db.open()?;
        conn.execute(
            "INSERT INTO fs_index_root(path_key, display_path, crawled_utc, complete, stale)
             VALUES (?1, ?2, ?3, ?4, 0)
             ON CONFLICT(path_key) DO UPDATE SET
                 display_path = excluded.display_path, crawled_utc = excluded.crawled_utc,
                 complete = excluded.complete, stale = 0;",
            params![root_key, display_path, crawled_utc.0, complete as i64],
        )?;
        Ok(())
    }

    /// Flags an indexed root as needing a re-crawl (e.g. watcher buffer overflow).
    pub fn mark_root_stale(&self, root_key: &str) -> Result<()> {
        let conn = self.db.open()?;
        conn.execute(
            "UPDATE fs_index_root SET stale = 1 WHERE path_key = ?1;",
            params![root_key],
        )?;
        Ok(())
    }

    /// Finds the completely-indexed root that covers `path` (ancestor-or-equal), preferring
    /// non-stale, then deepest. None if uncovered.
    pub fn find_covering_root(&self, path: &str) -> Result<Option<FsIndexRoot>> {
        let mut chain = Vec::new();
        let mut key = Some(PathKey::canonicalize(path)?);
        while let Some(k) = key {
            key = parent_key(&k);
            chain.push(k);
        }
        let conn = self.db.open()?;
        let sql = format!(
            "SELECT path_key, display_path, crawled_utc, complete, stale FROM fs_index_root
             WHERE complete = 1 AND path_key IN ({});",
            placeholders(chain.len())
        );
        let mut stmt = conn.prepare(&sql)?;
        let candidates = stmt
            .query_map(rusqlite::params_from_iter(chain.iter()), |r| {
                Ok(FsIndexRoot {
                    path_key: r.get(0)?,
                    display_path: r.get(1)?,
                    crawled_utc: FileTime(r.get(2)?),
                    complete: r.get::<_, i64>(3)? != 0,
                    stale: r.get::<_, i64>(4)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // Fresh roots first, then deepest.
        Ok(candidates
            .into_iter()
            .min_by_key(|r| (r.stale, std::cmp::Reverse(r.path_key.len()))))
    }

    /// Range-scans the subtree under `root_path` applying the query, capped at `cap` hits. No ORDER
    /// BY: LIMIT lets SQLite stop scanning as soon as enough matches are found. Relative display
    /// paths are reconstructed from the ancestor directory rows.
    pub fn search(
        &self,
        root_path: &str,
        query: &SearchQuery,
        cap: usize,
        include_hidden: bool,
    ) -> Result<(Vec<SearchHit>, bool)> {
        let root_key = PathKey::canonicalize(root_path)?;
        let root_display = PathKey::normalize_display(root_path)?;
        let (lo, hi) = PathKey::prefix_bounds(&root_key)?;
        let conn = self.db.open()?;
        let predicate = query.compile();
        let hidden_filter = if include_hidden {
            ""
        } else {
            "AND hidden = 0 "
        };
        let sql = format!(
            "SELECT path_key, name, is_dir, size_bytes, modified_utc, hidden, name_key, attributes, created_utc
             FROM fs_entry
             WHERE path_key >= :lo AND path_key < :hi AND ({}) {hidden_filter} {};",
            predicate.sql,
            limit_clause(&predicate)
        );
        let mut bindings = predicate.bindings();
        let limit = cap as i64 + 1;
        bindings.push((":lo", &lo));
        bindings.push((":hi", &hi));
        if predicate.complete {
            bindings.push((":limit", &limit));
        }
        let (mut rows, truncated) = read_matching_rows(&conn, &sql, &bindings, query, cap)?;
        if truncated {
            rows.pop();
        }
        let ancestors = lookup_ancestor_names(&conn, &rows, lo.len())?;
        let hits = rows
            .into_iter()
            .map(|row| {
                let rel_dir = build_relative_dir(&row.key, lo.len(), &ancestors);
                hit(join(&root_display, &rel_dir, &row.name), rel_dir, row)
            })
            .collect();
        Ok((hits, truncated))
    }

    /// Every indexed entry under a subtree, keyed relative to it. Its own row is not returned. A
    /// clustered range scan over the prefix bounds; `ORDER BY path_key` costs nothing because that is
    /// already the table's order. `exclude` is applied to the raw path key: one check covers a whole
    /// held folder, because its contents carry its name in their own keys.
    pub fn subtree(
        &self,
        root_path: &str,
        include_hidden: bool,
        cap: usize,
        exclude: Option<&dyn Fn(&str) -> bool>,
        cancel: &CancelToken,
    ) -> Result<(Vec<FsSubtreeRow>, bool)> {
        let (lo, hi) = PathKey::prefix_bounds(&PathKey::canonicalize(root_path)?)?;
        let conn = self.db.open()?;
        let sql = format!(
            "SELECT path_key, name, is_dir, size_bytes, modified_utc FROM fs_entry
             WHERE path_key >= ?1 AND path_key < ?2 {} ORDER BY path_key;",
            if include_hidden {
                ""
            } else {
                "AND hidden = 0 "
            }
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![lo, hi])?;
        let mut out = Vec::new();
        let mut truncated = false;
        while let Some(r) = rows.next()? {
            cancel.check().map_err(|_| Error::Cancelled)?;
            let path_key: String = r.get(0)?;
            if exclude.is_some_and(|f| f(&path_key)) {
                continue;
            }
            if out.len() == cap {
                truncated = true;
                break;
            }
            out.push(FsSubtreeRow {
                relative_key: path_key[lo.len()..].to_string(),
                name: r.get(1)?,
                is_directory: r.get::<_, i64>(2)? != 0,
                size_bytes: r.get(3)?,
                modified_utc: FileTime(r.get(4)?),
            });
        }
        Ok((out, truncated))
    }

    /// Whether anything in scope carries a real byte length; None for the whole index. The names-only
    /// fallback build records every row at size 0, so a size or date filter over such a volume
    /// matches nothing however the disk looks, and "no results" would be a lie.
    pub fn has_size_data(&self, root_path: Option<&str>) -> Result<bool> {
        let conn = self.db.open()?;
        let found: i64 = match root_path {
            None => conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM fs_entry WHERE size_bytes > 0);",
                [],
                |r| r.get(0),
            )?,
            Some(root) => {
                let (lo, hi) = PathKey::prefix_bounds(&PathKey::canonicalize(root)?)?;
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM fs_entry WHERE path_key >= ?1 AND path_key < ?2 AND size_bytes > 0);",
                    params![lo, hi],
                    |r| r.get(0),
                )?
            }
        };
        Ok(found != 0)
    }

    /// Whole-index ("This PC") search: the same predicate with no subtree bound. Results carry full
    /// display paths reconstructed from each row's ancestors up to its drive root.
    pub fn search_global(
        &self,
        query: &SearchQuery,
        cap: usize,
        include_hidden: bool,
    ) -> Result<(Vec<SearchHit>, bool)> {
        let conn = self.db.open()?;
        let predicate = query.compile();
        let hidden_filter = if include_hidden {
            ""
        } else {
            "AND hidden = 0 "
        };
        let sql = format!(
            "SELECT path_key, name, is_dir, size_bytes, modified_utc, hidden, name_key, attributes, created_utc
             FROM fs_entry WHERE ({}) {hidden_filter} {};",
            predicate.sql,
            limit_clause(&predicate)
        );
        let mut bindings = predicate.bindings();
        let limit = cap as i64 + 1;
        if predicate.complete {
            bindings.push((":limit", &limit));
        }
        let (mut rows, truncated) = read_matching_rows(&conn, &sql, &bindings, query, cap)?;
        if truncated {
            rows.pop();
        }
        let ancestors = lookup_ancestor_names(&conn, &rows, DRIVE_ROOT_LENGTH)?;
        let hits = rows
            .into_iter()
            .map(|row| global_hit(row, &ancestors))
            .collect();
        Ok((hits, truncated))
    }

    /// The biggest files under `root_path` (or across every indexed volume when None), largest first.
    /// This one does order, so LIMIT bounds the sorter rather than the scan. Directories are excluded
    /// by the query: a folder's total lives in dir_size_cache.
    pub fn largest_files(
        &self,
        root_path: Option<&str>,
        limit: usize,
        include_hidden: bool,
    ) -> Result<Vec<SearchHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let scope = Scope::of(root_path)?;
        let conn = self.db.open()?;
        let sql = format!(
            "SELECT path_key, name, is_dir, size_bytes, modified_utc, hidden, attributes, created_utc
             FROM fs_entry WHERE {}is_dir = 0 {} ORDER BY size_bytes DESC LIMIT ?3;",
            scope.clause(),
            if include_hidden { "" } else { "AND hidden = 0 " }
        );
        let mut stmt = conn.prepare(&sql)?;
        let (lo, hi) = scope.bounds();
        let mut rows = stmt.query(params![lo, hi, limit as i64])?;
        let mut entries = Vec::new();
        while let Some(r) = rows.next()? {
            entries.push(read_entry(r)?);
        }
        build_hits(&conn, entries, &scope)
    }

    /// Watcher apply: removes an entry and (for directories) its whole subtree.
    pub fn delete_subtree(&self, path_key: &str) -> Result<()> {
        let (lo, hi) = PathKey::prefix_bounds(path_key)?;
        let mut conn = self.db.open()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM fs_entry WHERE path_key = ?1;",
            params![path_key],
        )?;
        tx.execute(
            "DELETE FROM fs_entry WHERE path_key >= ?1 AND path_key < ?2;",
            params![lo, hi],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Watcher apply: renames an entry, rewriting descendant keys in place. Whatever previously
    /// existed at the target is removed first (overwrite-moves).
    pub fn rename(
        &self,
        old_key: &str,
        new_key: &str,
        new_name: &str,
        crawl_gen: i64,
    ) -> Result<()> {
        let mut conn = self.db.open()?;
        let tx = conn.transaction()?;
        let name_key = fold_upper(new_name);
        if old_key != new_key {
            let (old_lo, old_hi) = PathKey::prefix_bounds(old_key)?;
            let (new_lo, new_hi) = PathKey::prefix_bounds(new_key)?;
            tx.execute(
                "DELETE FROM fs_entry WHERE path_key = ?1;",
                params![new_key],
            )?;
            tx.execute(
                "DELETE FROM fs_entry WHERE path_key >= ?1 AND path_key < ?2;",
                params![new_lo, new_hi],
            )?;
            // length(?2) is evaluated by SQLite so character counting matches substr's.
            tx.execute(
                "UPDATE fs_entry SET path_key = ?1 || substr(path_key, length(?2) + 1), crawl_gen = ?3
                 WHERE path_key >= ?4 AND path_key < ?5;",
                params![new_key, old_key, crawl_gen, old_lo, old_hi],
            )?;
            tx.execute(
                "UPDATE fs_entry SET path_key = ?1, name = ?2, name_key = ?3, crawl_gen = ?4 WHERE path_key = ?5;",
                params![new_key, new_name, name_key, crawl_gen, old_key],
            )?;
        } else {
            // Case-only rename: the key is unchanged, only the display name moves.
            tx.execute(
                "UPDATE fs_entry SET name = ?1, name_key = ?2, crawl_gen = ?3 WHERE path_key = ?4;",
                params![new_name, name_key, crawl_gen, old_key],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Every file at or above `min_size_bytes` that shares its byte length with at least one other:
    /// the shortlist a duplicate scan starts from, with no file opened. Two streaming scans and no
    /// GROUP BY, because grouping by size would materialise a temp B-tree over every (size, key)
    /// pair. `exclude` is applied in both passes so an excluded file cannot prop up a size group.
    /// The two counts describe what the index knows and are taken before the floor and the exclusion.
    pub fn duplicate_candidates(
        &self,
        root_path: Option<&str>,
        min_size_bytes: i64,
        include_hidden: bool,
        exclude: Option<&dyn Fn(&str) -> bool>,
        cancel: &CancelToken,
    ) -> Result<DuplicateShortlist> {
        // Zero would sweep in every file the sizeless build path could not measure.
        let floor = min_size_bytes.max(1);
        let scope = Scope::of(root_path)?;
        let hidden_filter = if include_hidden {
            ""
        } else {
            "AND hidden = 0 "
        };
        let conn = self.db.open()?;
        let (lo, hi) = scope.bounds();

        // Pass one: which byte lengths occur more than once.
        let mut counts: HashMap<i64, u32> = HashMap::new();
        let mut files_in_scope = 0i64;
        let mut sized_files_in_scope = 0i64;
        {
            let sql = format!(
                "SELECT size_bytes, path_key FROM fs_entry WHERE {}is_dir = 0 {hidden_filter};",
                scope.clause()
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query(params![lo, hi])?;
            while let Some(r) = rows.next()? {
                cancel.check().map_err(|_| Error::Cancelled)?;
                let size: i64 = r.get(0)?;
                files_in_scope += 1;
                if size > 0 {
                    sized_files_in_scope += 1;
                }
                if size < floor {
                    continue;
                }
                if exclude.is_some_and(|f| f(&r.get::<_, String>(1).unwrap_or_default())) {
                    continue;
                }
                *counts.entry(size).or_insert(0) += 1;
            }
        }
        let colliding: HashSet<i64> = counts
            .into_iter()
            .filter(|&(_, n)| n > 1)
            .map(|(s, _)| s)
            .collect();
        if colliding.is_empty() {
            return Ok(DuplicateShortlist {
                files: Vec::new(),
                files_in_scope,
                sized_files_in_scope,
            });
        }

        // Pass two: the rows at those lengths.
        let mut entries = Vec::new();
        {
            let sql = format!(
                "SELECT path_key, name, is_dir, size_bytes, modified_utc, hidden, attributes, created_utc
                 FROM fs_entry WHERE {}is_dir = 0 AND size_bytes >= ?3 {hidden_filter};",
                scope.clause()
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query(params![lo, hi, floor])?;
            while let Some(r) = rows.next()? {
                cancel.check().map_err(|_| Error::Cancelled)?;
                let size: i64 = r.get(3)?;
                if !colliding.contains(&size) {
                    continue;
                }
                let key: String = r.get(0)?;
                if exclude.is_some_and(|f| f(&key)) {
                    continue;
                }
                entries.push(read_entry(r)?);
            }
        }
        Ok(DuplicateShortlist {
            files: build_hits(&conn, entries, &scope)?,
            files_in_scope,
            sized_files_in_scope,
        })
    }
}

/// A subtree scope, or the whole index. The two bound parameters are always bound (`?1`, `?2`) so
/// the statements have one shape; an unscoped query ignores them.
struct Scope {
    scoped: bool,
    root_display: String,
    lo: String,
    hi: String,
}

impl Scope {
    fn of(root_path: Option<&str>) -> Result<Scope> {
        match root_path.filter(|p| !p.is_empty()) {
            Some(root) => {
                let key = PathKey::canonicalize(root)?;
                let (lo, hi) = PathKey::prefix_bounds(&key)?;
                Ok(Scope {
                    scoped: true,
                    root_display: PathKey::normalize_display(root)?,
                    lo,
                    hi,
                })
            }
            None => Ok(Scope {
                scoped: false,
                root_display: String::new(),
                lo: String::new(),
                hi: String::new(),
            }),
        }
    }

    fn clause(&self) -> &'static str {
        if self.scoped {
            "path_key >= ?1 AND path_key < ?2 AND "
        } else {
            "?1 IS NOT NULL AND ?2 IS NOT NULL AND "
        }
    }

    fn bounds(&self) -> (&str, &str) {
        (&self.lo, &self.hi)
    }
}

/// The LIMIT clause for a compiled predicate, and only when it is exact. An incomplete predicate is
/// a superset, re-checked per row; letting SQLite stop at `cap + 1` rows would cap the wrong
/// population.
fn limit_clause(predicate: &SqlPredicate) -> &'static str {
    if predicate.complete {
        "LIMIT :limit"
    } else {
        ""
    }
}

/// Reads rows, keeping only those the query really matches, and stops one past `cap`. **This
/// re-check is what makes the SQL an optimisation rather than a second implementation.** Delete it
/// and a regex search returns every row in the subtree.
fn read_matching_rows(
    conn: &Connection,
    sql: &str,
    bindings: &[(&str, &dyn ToSql)],
    query: &SearchQuery,
    cap: usize,
) -> Result<(Vec<EntryRow>, bool)> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(bindings)?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let key: String = r.get(0)?;
        let name: String = r.get(1)?;
        let is_dir = r.get::<_, i64>(2)? != 0;
        let size: i64 = r.get(3)?;
        let modified = FileTime(r.get(4)?);
        let hidden = r.get::<_, i64>(5)? != 0;
        let name_key: String = r.get(6)?;
        let attributes = r.get::<_, i64>(7)? as u32;
        let created = FileTime(r.get(8)?);
        let candidate = SearchCandidate {
            name_key: &name_key,
            path_key: &key,
            is_directory: is_dir,
            size_bytes: size,
            modified_utc: modified,
            hidden,
            attributes,
            created_utc: created,
            content: None,
        };
        if !query.matches(&candidate) {
            continue;
        }
        out.push(EntryRow {
            key,
            name,
            is_dir,
            size,
            modified,
            hidden,
            attributes,
            created,
        });
        if out.len() > cap {
            return Ok((out, true));
        }
    }
    Ok((out, false))
}

/// The eight-column projection the non-search scans select.
fn read_entry(r: &rusqlite::Row<'_>) -> rusqlite::Result<EntryRow> {
    Ok(EntryRow {
        key: r.get(0)?,
        name: r.get(1)?,
        is_dir: r.get::<_, i64>(2)? != 0,
        size: r.get(3)?,
        modified: FileTime(r.get(4)?),
        hidden: r.get::<_, i64>(5)? != 0,
        attributes: r.get::<_, i64>(6)? as u32,
        created: FileTime(r.get(7)?),
    })
}

/// Display names for every distinct ancestor directory (strictly between the search root and each
/// hit). Each ancestor is itself an `fs_entry` row keyed by a prefix of the hit's key.
fn lookup_ancestor_names(
    conn: &Connection,
    rows: &[EntryRow],
    lo_length: usize,
) -> Result<HashMap<String, String>> {
    let mut keys: HashSet<&str> = HashSet::new();
    for row in rows {
        for (i, _) in row.key.match_indices('\\') {
            if i >= lo_length {
                keys.insert(&row.key[..i]);
            }
        }
    }
    let keys: Vec<&str> = keys.into_iter().collect();
    let mut names = HashMap::with_capacity(keys.len());
    for chunk in keys.chunks(IN_CHUNK) {
        let sql = format!(
            "SELECT path_key, name FROM fs_entry WHERE path_key IN ({});",
            placeholders(chunk.len())
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(chunk.iter()))?;
        while let Some(r) = rows.next()? {
            names.insert(r.get::<_, String>(0)?, r.get::<_, String>(1)?);
        }
    }
    Ok(names)
}

/// The hit's parent directory relative to the root, from the ancestors' display names, falling back
/// to the upper-cased key segment where an ancestor row is missing.
fn build_relative_dir(
    path_key: &str,
    lo_length: usize,
    ancestors: &HashMap<String, String>,
) -> String {
    let Some(last_sep) = path_key.rfind('\\') else {
        return String::new();
    };
    if last_sep < lo_length {
        return String::new(); // direct child of the search root
    }
    let mut segments = Vec::new();
    for (i, _) in path_key.match_indices('\\') {
        if i < lo_length {
            continue;
        }
        let ancestor_key = &path_key[..i];
        segments.push(match ancestors.get(ancestor_key) {
            Some(name) => name.clone(),
            None => bb_paths::file_name(ancestor_key).to_string(),
        });
    }
    segments.join("\\")
}

fn join(root: &str, rel_dir: &str, name: &str) -> String {
    let mut out = root.trim_end_matches('\\').to_string();
    if !rel_dir.is_empty() {
        out.push('\\');
        out.push_str(rel_dir);
    }
    out.push('\\');
    out.push_str(name);
    out
}

fn hit(display_path: String, relative_dir_display: String, row: EntryRow) -> SearchHit {
    SearchHit {
        display_path,
        relative_dir_display,
        name: row.name,
        is_directory: row.is_dir,
        size_bytes: row.size,
        modified_utc: row.modified,
        hidden: row.hidden,
        attributes: row.attributes,
        created_utc: row.created,
        content_match: None,
    }
}

/// A whole-PC hit: the full parent path from the drive root.
fn global_hit(row: EntryRow, ancestors: &HashMap<String, String>) -> SearchHit {
    let drive_root = &row.key[..DRIVE_ROOT_LENGTH.min(row.key.len())]; // "C:\"
    let rel_dir = build_relative_dir(&row.key, DRIVE_ROOT_LENGTH, ancestors);
    let parent_full = if rel_dir.is_empty() {
        drive_root.to_string()
    } else {
        format!("{drive_root}{rel_dir}")
    };
    let display = if parent_full.ends_with('\\') {
        format!("{parent_full}{}", row.name)
    } else {
        format!("{parent_full}\\{}", row.name)
    };
    hit(display, parent_full, row)
}

/// Turns raw rows into hits with display paths rebuilt from their ancestors' names.
fn build_hits(conn: &Connection, rows: Vec<EntryRow>, scope: &Scope) -> Result<Vec<SearchHit>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let prefix_length = if scope.scoped {
        scope.lo.len()
    } else {
        DRIVE_ROOT_LENGTH
    };
    let ancestors = lookup_ancestor_names(conn, &rows, prefix_length)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            if scope.scoped {
                let rel_dir = build_relative_dir(&row.key, prefix_length, &ancestors);
                hit(join(&scope.root_display, &rel_dir, &row.name), rel_dir, row)
            } else {
                global_hit(row, &ancestors)
            }
        })
        .collect())
}
