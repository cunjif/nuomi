-- 0050_task_source_and_assignee: background-task provenance columns (ADR 0029).
-- tasks gains: source ('manual' | 'schedule' | 'steward' — who created the
-- card; legacy rows default to 'manual'), created_by (steward session id /
-- schedule id / NULL) and assignee_team_id (backfilled once a dispatch is
-- approved via proposal or confirmed via direct dispatch).
-- Conventions unchanged: uuid-v7 string ids; unix-ms i64 timestamps.
-- Append-only migrations: never edit shipped files.

ALTER TABLE tasks ADD COLUMN source TEXT NOT NULL DEFAULT 'manual';
ALTER TABLE tasks ADD COLUMN assignee_team_id TEXT;
ALTER TABLE tasks ADD COLUMN created_by TEXT;
