-- 0025_tasks_runs_workspace: 任务与运行按工作区隔离。
-- Append-only — never edit shipped migrations.
--
-- tasks/runs 新增 workspace_id 列，按工作区隔离查询。存量任务/运行的
-- workspace_id 填充为 '__migrated__' 占位符，由 Rust 端 WorkspaceMigration
-- 在启动时替换为当前聚焦工作区真实 id。迁移完成后 __migrated__ 不再出现于
-- 新任务（新建任务写入真实 workspace_id）。

ALTER TABLE tasks ADD COLUMN workspace_id TEXT NOT NULL DEFAULT '__migrated__';
ALTER TABLE runs ADD COLUMN workspace_id TEXT NOT NULL DEFAULT '__migrated__';

CREATE INDEX idx_tasks_workspace ON tasks (workspace_id);
CREATE INDEX idx_runs_workspace ON runs (workspace_id);

-- 存量任务/运行绑定：将 __migrated__ 占位符替换为当前聚焦工作区 id。
-- 若无聚焦工作区（workspace_open_state 无 is_focused=1 行），保留占位符待首次开启时回收。
UPDATE tasks
SET workspace_id = (SELECT workspace_id FROM workspace_open_state WHERE is_focused = 1)
WHERE workspace_id = '__migrated__'
  AND EXISTS (SELECT 1 FROM workspace_open_state WHERE is_focused = 1);

UPDATE runs
SET workspace_id = (SELECT workspace_id FROM workspace_open_state WHERE is_focused = 1)
WHERE workspace_id = '__migrated__'
  AND EXISTS (SELECT 1 FROM workspace_open_state WHERE is_focused = 1);
