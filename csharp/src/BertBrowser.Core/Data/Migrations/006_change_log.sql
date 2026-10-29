-- The change timeline: one row per (path, kind) burst the USN tail acted on.
--
-- Written only by the elevated index helper, and only while the user has turned recording on
-- (off by default — see ChangeLogPolicy); read by the app's "What changed" window. A rowid table
-- on purpose, unlike fs_entry: rows are appended and pruned by time, never sought by key from the
-- UI, and INTEGER PRIMARY KEY keeps the two secondary indexes small.
--
-- display_path is stored (fs_entry keeps only the uppercased key) because a deleted file has no
-- ancestors left to rebuild its casing from. Timestamps are "O" text, so BINARY collation is
-- chronological and a range is a plain string comparison.
CREATE TABLE fs_change (
    id           INTEGER PRIMARY KEY,
    path_key     TEXT    NOT NULL,
    display_path TEXT    NOT NULL,
    old_path     TEXT    NULL,
    is_dir       INTEGER NOT NULL,
    hidden       INTEGER NOT NULL,
    kind         INTEGER NOT NULL,
    first_utc    TEXT    NOT NULL,
    last_utc     TEXT    NOT NULL,
    count        INTEGER NOT NULL DEFAULT 1
);

-- The time range and the ORDER BY last_utc DESC, off one index and without a sorter.
CREATE INDEX ix_fs_change_last ON fs_change(last_utc);

-- The coalescing seek: the most recent row for this path and kind.
CREATE INDEX ix_fs_change_key ON fs_change(path_key, kind, last_utc);
