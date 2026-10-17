-- 0034_steward_settings_schema: 管家设置面板的存储基础。
-- Append-only — never edit shipped migrations.
--
-- 1. steward_sessions.cleared_at: 清除点时间戳。NULL = 未清空；非 NULL = 清空时间戳。
--    list_session_messages 读取时过滤 created_at <= cleared_at 的事件。
--    清空不可逆：update_cleared_at 禁止重置为 NULL。
--
-- 2. steward_deleted_events: 批量删除消息的删除标记表。
--    events 表有 trg_events_no_delete / trg_events_no_update 触发器（append-only），
--    不能直接 DELETE events 行，故用标记表绕过：list_session_messages LEFT JOIN 排除已标记的 event_id。
--
-- 不触碰 events 表触发器，append-only 约束保留。

ALTER TABLE steward_sessions ADD COLUMN cleared_at INTEGER;

CREATE TABLE steward_deleted_events (
    session_id  TEXT NOT NULL,
    event_id    TEXT NOT NULL,
    deleted_at  INTEGER NOT NULL,
    PRIMARY KEY (session_id, event_id)
);

CREATE INDEX idx_deleted_events_session ON steward_deleted_events(session_id);
