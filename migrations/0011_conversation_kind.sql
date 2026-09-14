-- 0011_conversation_kind: Conversation = Session + kind + bindings.
-- Append-only: new columns on `sessions` only; no existing column is touched.
--   kind         — discriminant: 'chat' | 'group' | 'background' | 'scheduled'
--   agent_kind   — 'cli' | 'role' (NULL = default resolution chain)
--   agent_ref_id — agent_profiles.id or roles.id
--   team_id      — group: the Team
--   task_id      — background: the Task anchor
--   schedule_id  — scheduled: the Schedule anchor
ALTER TABLE sessions ADD COLUMN kind TEXT NOT NULL DEFAULT 'chat'
  CHECK (kind IN ('chat', 'group', 'background', 'scheduled'));
ALTER TABLE sessions ADD COLUMN agent_kind   TEXT;
ALTER TABLE sessions ADD COLUMN agent_ref_id TEXT;
ALTER TABLE sessions ADD COLUMN team_id      TEXT;
ALTER TABLE sessions ADD COLUMN task_id      TEXT;
ALTER TABLE sessions ADD COLUMN schedule_id  TEXT;

CREATE INDEX idx_sessions_kind     ON sessions (kind, updated_at DESC);
CREATE INDEX idx_sessions_task     ON sessions (task_id);
CREATE INDEX idx_sessions_schedule ON sessions (schedule_id);
