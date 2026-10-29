CREATE TABLE tag (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL COLLATE NOCASE UNIQUE,
    color       TEXT NULL,
    created_utc TEXT NOT NULL
);

CREATE TABLE file (
    id           INTEGER PRIMARY KEY,
    path_key     TEXT NOT NULL UNIQUE,
    display_path TEXT NOT NULL
);

CREATE TABLE file_tag (
    file_id INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tag(id)  ON DELETE CASCADE,
    PRIMARY KEY (file_id, tag_id)
) WITHOUT ROWID;

CREATE INDEX ix_file_tag_tag ON file_tag(tag_id, file_id);

CREATE TABLE dir_size_cache (
    path_key     TEXT PRIMARY KEY,
    size_bytes   INTEGER NOT NULL,
    file_count   INTEGER NOT NULL,
    dir_count    INTEGER NOT NULL,
    incomplete   INTEGER NOT NULL DEFAULT 0,
    computed_utc TEXT NOT NULL
) WITHOUT ROWID;
