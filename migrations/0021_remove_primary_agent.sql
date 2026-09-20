-- 0021_remove_primary_agent: ADR 0013 —
-- 祛除主 Agent 概念，统一为 IM 式单聊/群聊模型。
-- 把 sessions.agent_kind/agent_ref_id 数据迁移到 conversation_participants 表，
-- 然后置 NULL（列保留兼容但不再读写）。
-- Append-only: never edit shipped migrations.

-- 把 sessions.agent_kind/agent_ref_id 非空的行迁移到 conversation_participants。
-- INSERT OR IGNORE 去重：若 conversation_participants 已有相同 (session_id, agent_kind, agent_ref_id) 则跳过。
-- joined_at 用 sessions.updated_at 填充（无更精确的加入时间）。
INSERT OR IGNORE INTO conversation_participants (session_id, agent_kind, agent_ref_id, joined_at)
    SELECT id, agent_kind, agent_ref_id, updated_at
    FROM sessions
    WHERE agent_kind IS NOT NULL AND agent_ref_id IS NOT NULL;

-- 迁移完成后置 NULL，标记废弃（列保留避免 SQLite DROP COLUMN 兼容问题）。
UPDATE sessions SET agent_kind = NULL, agent_ref_id = NULL
    WHERE agent_kind IS NOT NULL OR agent_ref_id IS NOT NULL;
