-- Records whether each indexed entry is hidden (its own Hidden attribute, OR it lives
-- inside a hidden subtree — the crawler propagates the flag down so a simple
-- "hidden = 0" filter matches Explorer's "don't show hidden items" behavior without
-- any ancestor lookups at query time).
--
-- Existing rows default to 0 (visible); the next re-crawl of each root stamps the real
-- value. New crawls/watcher writes always set it explicitly.
ALTER TABLE fs_entry ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
