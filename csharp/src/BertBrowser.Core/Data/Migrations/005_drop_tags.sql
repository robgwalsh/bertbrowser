-- Drops the file-tagging feature. file_tag first (it references both parents), then
-- the tag catalog, then the file table — which existed solely to give tag links a
-- stable path-keyed row. dir_size_cache and the search index are untouched.
DROP INDEX IF EXISTS ix_file_tag_tag;
DROP TABLE IF EXISTS file_tag;
DROP TABLE IF EXISTS tag;
DROP TABLE IF EXISTS file;
