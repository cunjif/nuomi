-- 0007_session_cache_scope
-- Adds the session cache-lineage scope (hermes-style prompt-cache routing
-- root). Empty string means "unset": callers fall back to the session id.
-- NOTE: registration in store/migrations.rs is owned by the main session
-- (pending registration — see P1 report).
ALTER TABLE sessions ADD COLUMN cache_scope TEXT NOT NULL DEFAULT '';
