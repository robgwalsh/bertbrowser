-- Persistent file/directory name index for search.
--
-- fs_entry is WITHOUT ROWID with path_key as the primary key, so the table itself
-- is the clustered index and a subtree search is a pure PrefixBounds range scan.
-- There is deliberately no secondary index on name: B-trees cannot accelerate
-- *substring* matching, the range scan already bounds the work, and an extra index
-- would double the write cost of crawls for no query benefit.
--
-- name_key is the entry name uppercased in C# (ToUpperInvariant) — SQLite's upper()
-- only folds ASCII, so all case folding stays in C#, matching PathKey semantics.
--
-- crawl_gen is a unix-millisecond write stamp: a crawl stamps every upsert with its
-- start time and, on successful completion, deletes rows in its range with an older
-- stamp ("vanish sweep"). Watcher writes stamp the current time so they survive
-- sweeps by concurrent crawls.
CREATE TABLE fs_entry (
    path_key     TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    name_key     TEXT NOT NULL,
    is_dir       INTEGER NOT NULL,
    size_bytes   INTEGER NOT NULL DEFAULT 0,
    modified_utc TEXT NOT NULL,
    crawl_gen    INTEGER NOT NULL
) WITHOUT ROWID;

-- Which subtrees have been indexed. A search root is covered iff some row with
-- complete = 1 is an ancestor-or-equal of it. stale = 1 means the watcher lost
-- track of changes (e.g. buffer overflow) and the root needs a re-crawl.
CREATE TABLE fs_index_root (
    path_key     TEXT PRIMARY KEY,
    display_path TEXT NOT NULL,
    crawled_utc  TEXT NOT NULL,
    complete     INTEGER NOT NULL DEFAULT 0,
    stale        INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;
