-- 0023_message_queue: ADR 0015 对话消息队列。
--   message_queue 表存储排队等待处理的消息；
--   sessions.agent_busy 持久化 RoleAgent 忙碌状态。

CREATE TABLE message_queue (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL,
    text        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'queued',
    seq         INTEGER NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE INDEX idx_queue_session ON message_queue (session_id, status, seq);

ALTER TABLE sessions ADD COLUMN agent_busy INTEGER NOT NULL DEFAULT 0;
