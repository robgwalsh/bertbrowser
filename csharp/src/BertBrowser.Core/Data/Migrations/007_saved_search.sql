-- Saved searches: a query the user stored under a name, listed in the sidebar.
--
-- Keyed by the name, which is the identity the user sees, compared ignoring case so "Docs" and
-- "docs" cannot both exist. Nothing here is a path key: scope_path is the casing-preserving
-- display path of the pinned folder (only when scope = 1, Folder) and is never range-scanned
-- with PrefixBounds, so the canonicalize-every-path-column rule — whose purpose is subtree
-- scans — does not apply, and the display casing is what navigation and the sidebar need.
-- A path inside an archive is refused before it gets here (SavedSearchRules.Validate).
CREATE TABLE saved_search (
    name       TEXT    PRIMARY KEY COLLATE NOCASE,
    query      TEXT    NOT NULL,
    scope      INTEGER NOT NULL,        -- SavedSearchScope
    scope_path TEXT    NULL,            -- display path, only when scope = 1
    added_utc  TEXT    NOT NULL
) WITHOUT ROWID;
