-- 0048_sandbox_audit_columns: 沙箱任务审计与输出分流列（ADR-0026）。
-- Append-only — never edit shipped migrations.
--
-- 五列分两组：
-- 1. 提交人审计（spec 5.7.1-5）：submitter 记录任务来源
--    （"workbench" / "selfeval" / "repl" / 未来用户 id），NULL 兼容存量行。
-- 2. 大输出产物目录分流（ADR-0022 §6）：stdout_ref / stderr_ref 指向
--    .nuomi/sandbox/<task_id>/ 下的全量输出文件；stdout_truncated /
--    stderr_truncated 标记行内是否已截断（0=完整，1=截断，全量在 ref）。
--    未截断时 ref 为 NULL、行内即完整输出；截断时行内为前 64KB 摘要。

ALTER TABLE sandbox_tasks ADD COLUMN submitter TEXT;
ALTER TABLE sandbox_tasks ADD COLUMN stdout_ref TEXT;
ALTER TABLE sandbox_tasks ADD COLUMN stderr_ref TEXT;
ALTER TABLE sandbox_tasks ADD COLUMN stdout_truncated INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sandbox_tasks ADD COLUMN stderr_truncated INTEGER NOT NULL DEFAULT 0;
