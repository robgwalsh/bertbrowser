-- Saved workspaces: a pane arrangement the user stored under a name, listed in the sidebar.
--
-- Keyed by the name, which is the identity the user sees, compared ignoring case so "Docs" and
-- "docs" cannot both exist. layout_json is a SessionLayout (the same shape settings.json stores
-- for the single unnamed session) serialized whole, since a pane tree has no scalar columns to
-- split it into and is never queried by anything but name.
CREATE TABLE saved_workspace (
    name        TEXT    PRIMARY KEY COLLATE NOCASE,
    layout_json TEXT    NOT NULL,
    added_utc   TEXT    NOT NULL
) WITHOUT ROWID;
