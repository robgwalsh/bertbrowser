-- When each saved search was last run, for the Saved searches page of Settings. Null until it
-- first is, for the reason 010 gives: "never" is truer than inventing a time.
ALTER TABLE saved_search ADD COLUMN last_used_utc TEXT;
