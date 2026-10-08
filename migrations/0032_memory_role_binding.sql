-- 0032: Per-agent memory isolation — add role_id to memory_entries.
-- NULL role_id = global memory (preserved for backward compatibility).
-- Non-NULL role_id = per-agent private memory scoped to that role.
-- No FK constraint: role_id is a logical reference, not enforced by the DB
-- (consistent with how role_id is stored in events payload).

ALTER TABLE memory_entries ADD COLUMN role_id TEXT;

CREATE INDEX idx_memory_role ON memory_entries(role_id);
