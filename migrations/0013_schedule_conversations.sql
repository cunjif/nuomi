-- 0013_schedule_conversations: schedules upgraded from "only enqueue Task" to
-- "create a typed conversation and optionally auto-dispatch".
-- Append-only: new columns on `schedules` only.
--   target_kind   — 'task' | 'chat' | 'group'
--   agent_kind/agent_ref_id — execution body for chat targets
--   team_id       — execution body for group targets
--   session_mode  — 'per_trigger' (new session each fire) | 'reuse' (one session)
--   session_id    — reuse mode: the shared session
--   auto_dispatch — 1 = auto-run on trigger; 0 = only create task/session
ALTER TABLE schedules ADD COLUMN target_kind   TEXT NOT NULL DEFAULT 'task'
  CHECK (target_kind IN ('task', 'chat', 'group'));
ALTER TABLE schedules ADD COLUMN agent_kind    TEXT;
ALTER TABLE schedules ADD COLUMN agent_ref_id  TEXT;
ALTER TABLE schedules ADD COLUMN team_id       TEXT;
ALTER TABLE schedules ADD COLUMN session_mode  TEXT NOT NULL DEFAULT 'per_trigger'
  CHECK (session_mode IN ('per_trigger', 'reuse'));
ALTER TABLE schedules ADD COLUMN session_id    TEXT;
ALTER TABLE schedules ADD COLUMN auto_dispatch INTEGER NOT NULL DEFAULT 1;
