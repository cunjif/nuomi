-- 0027_steward_sessions: 管家会话域 — 独立表隔离，不修改 sessions.kind CHECK 约束。
-- Append-only — never edit shipped migrations.
--
-- 设计权衡（design.md §2.3.2.2）：sessions.kind 的 CHECK 约束不含 'steward'，
-- 重建大表成本高且违反"只增不改"精神。改用独立 steward_sessions 表（id 引用 sessions.id），
-- 管家会话在 sessions 表中 kind 仍记为 'background'（语义最接近），
-- 但 steward_sessions 行的存在是判别管家会话的权威。零侵入现有 sessions 表。

CREATE TABLE steward_sessions (
    id          TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    steward_id  TEXT NOT NULL,
    title       TEXT NOT NULL DEFAULT '',
    goal        TEXT,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX idx_steward_sessions_updated ON steward_sessions (updated_at DESC);
