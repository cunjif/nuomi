-- 0038_sandbox_task_usage: 沙箱任务资源用量列——终态快照（spec 5.7.1-5）。
-- Append-only — never edit shipped migrations.
--
-- 执行器在终态 poll 报告的用量（cpu_time_ms / peak_memory_mb 的 JSON
-- object，沿 limits / authorized_dirs 的 JSON 列先例）；沙箱服务未报时
-- 保持 NULL。审计事件（sandbox.task_state_changed）自 7.4 起已在 payload
-- 中携带 usage，本列补齐行级查询面：sandbox_get_task / sandbox_list_tasks
-- 的"查询执行记录"可完整返回资源用量（spec 5.7.1-5a / 5.7.3-3）。

ALTER TABLE sandbox_tasks ADD COLUMN usage TEXT;
