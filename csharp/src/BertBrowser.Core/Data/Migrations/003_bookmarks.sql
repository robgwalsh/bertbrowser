CREATE TABLE bookmark (
    path_key     TEXT PRIMARY KEY,
    display_path TEXT NOT NULL,
    is_directory INTEGER NOT NULL,
    added_utc    TEXT NOT NULL
) WITHOUT ROWID;
