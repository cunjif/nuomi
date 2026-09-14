-- 0017_sessions_workspace: 会话按工作区隔离 + 存量会话归属迁移占位。
-- Append-only — never edit shipped migrations.
--
-- sessions 表新增 workspace_id 列，按工作区隔离会话查询。存量会话的
-- workspace_id 填充为 '__migrated__' 占位符，由 Rust 端 WorkspaceMigration
-- 在启动时替换为迁移出的工作区真实 id（UPDATE sessions SET workspace_id =
-- <migrated_workspace_id> WHERE workspace_id = '__migrated__'）。迁移完成后
-- __migrated__ 不再出现于新会话（新建会话写入真实 workspace_id）。

ALTER TABLE sessions ADD COLUMN workspace_id TEXT NOT NULL DEFAULT '__migrated__';

CREATE INDEX idx_sessions_workspace ON sessions (workspace_id);
