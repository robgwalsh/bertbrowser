-- The whole schema, fresh. Every *_utc column is a FILETIME integer (100 ns ticks since 1601 UTC);
-- 0 means unknown, which sorts below any real time so a date filter never matches it.
-- Every path_key is PathKey::canonicalize output, so subtree queries are prefix_bounds range scans.

-- Persistent file/directory name index for search. WITHOUT ROWID with path_key as the primary key,
-- so the table itself is the clustered index and a subtree search is a pure range scan. No
-- secondary index: B-trees cannot accelerate substring matching, the range scan already bounds the
-- work, and an index on a WITHOUT ROWID table carries the whole key again.
--   name_key   the name upper-cased with the same simple mapping as path keys
--   hidden     effective: the entry's own Hidden attribute OR any ancestor's, so "hidden = 0" needs
--              no ancestor lookups
--   attributes the entry's own Win32 attribute mask (0 = not recorded)
--   crawl_gen  a unix-millisecond write stamp; a completed crawl sweeps older rows in its range
CREATE TABLE fs_entry (
    path_key     TEXT    PRIMARY KEY,
    name         TEXT    NOT NULL,
    name_key     TEXT    NOT NULL,
    is_dir       INTEGER NOT NULL,
    size_bytes   INTEGER NOT NULL DEFAULT 0,
    modified_utc INTEGER NOT NULL DEFAULT 0,
    created_utc  INTEGER NOT NULL DEFAULT 0,
    attributes   INTEGER NOT NULL DEFAULT 0,
    hidden       INTEGER NOT NULL DEFAULT 0,
    crawl_gen    INTEGER NOT NULL
) WITHOUT ROWID;

-- Which subtrees have been indexed. A search root is covered iff some complete row is an
-- ancestor-or-equal of it. stale = 1 means the watcher lost track and the root needs a re-crawl.
CREATE TABLE fs_index_root (
    path_key     TEXT    PRIMARY KEY,
    display_path TEXT    NOT NULL,
    crawled_utc  INTEGER NOT NULL,
    complete     INTEGER NOT NULL DEFAULT 0,
    stale        INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

-- Recursive directory sizes from the MFT pass. A missing row means unknown, never zero.
CREATE TABLE dir_size_cache (
    path_key     TEXT    PRIMARY KEY,
    size_bytes   INTEGER NOT NULL,
    file_count   INTEGER NOT NULL,
    dir_count    INTEGER NOT NULL,
    incomplete   INTEGER NOT NULL DEFAULT 0,
    computed_utc INTEGER NOT NULL
) WITHOUT ROWID;

-- The change timeline: one row per (path, kind) burst the USN tail acted on. Written only by the
-- elevated index helper while recording is on; a rowid table because rows are appended and pruned
-- by time, never sought by key from the UI. display_path is stored because a deleted file has no
-- ancestors left to rebuild its casing from.
CREATE TABLE fs_change (
    id           INTEGER PRIMARY KEY,
    path_key     TEXT    NOT NULL,
    display_path TEXT    NOT NULL,
    old_path     TEXT    NULL,
    is_dir       INTEGER NOT NULL,
    hidden       INTEGER NOT NULL,
    kind         INTEGER NOT NULL,
    first_utc    INTEGER NOT NULL,
    last_utc     INTEGER NOT NULL,
    count        INTEGER NOT NULL DEFAULT 1
);
-- The time range and ORDER BY last_utc DESC, off one index and without a sorter.
CREATE INDEX ix_fs_change_last ON fs_change(last_utc);
-- The coalescing seek: the most recent row for this path and kind.
CREATE INDEX ix_fs_change_key ON fs_change(path_key, kind, last_utc);

CREATE TABLE bookmark (
    path_key     TEXT    PRIMARY KEY,
    display_path TEXT    NOT NULL,
    is_directory INTEGER NOT NULL,
    added_utc    INTEGER NOT NULL
) WITHOUT ROWID;

-- Saved searches, keyed by the name the user sees, compared ignoring case. scope_path is a
-- display path (never range-scanned), only when scope = 1 (Folder).
CREATE TABLE saved_search (
    name          TEXT    PRIMARY KEY COLLATE NOCASE,
    query         TEXT    NOT NULL,
    scope         INTEGER NOT NULL,
    scope_path    TEXT    NULL,
    added_utc     INTEGER NOT NULL,
    last_used_utc INTEGER NULL
) WITHOUT ROWID;

-- Saved workspaces: a pane arrangement (SessionLayout JSON) under a user-chosen name.
CREATE TABLE saved_workspace (
    name          TEXT    PRIMARY KEY COLLATE NOCASE,
    layout_json   TEXT    NOT NULL,
    added_utc     INTEGER NOT NULL,
    last_used_utc INTEGER NULL
) WITHOUT ROWID;
