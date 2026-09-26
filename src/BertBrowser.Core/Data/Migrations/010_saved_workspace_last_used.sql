-- When each saved workspace was last switched to, for the Workspaces page of Settings. Null until
-- it first is: a workspace saved before this column existed has simply never been opened as far as
-- anything can tell, and saying "never" is truer than inventing its creation time.
ALTER TABLE saved_workspace ADD COLUMN last_used_utc TEXT;
