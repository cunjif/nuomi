-- 0022_session_soft_delete: ADR 0014 — sessions 表软删除（tombstone）。
--   deleted_at IS NULL 表示未删除；非 NULL 表示软删除时间戳（unix-ms）。
--   events 表 append-only 铁律不允许物理删除，故会话删除改为标记。

ALTER TABLE sessions ADD COLUMN deleted_at INTEGER;
