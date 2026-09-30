-- 0026_schedules_workspace: 定时任务按工作区隔离（方案 B 重建表）。
-- Append-only — never edit shipped migrations.
--
-- 前提：schedules 表无被外键引用（runs.schedule_id 为弱引用无 FK 约束），
--       可安全 DROP + RENAME。
--
-- 方案 B 四步策略（幂等可重入）：
--   1. DROP TABLE IF EXISTS schedules_new — 幂等清理上次崩溃残留
--   2. CREATE TABLE schedules_new — 含 workspace_id 列、(workspace_id, name) 复合唯一索引，
--      不含 name 全局 UNIQUE
--   3. INSERT INTO schedules_new SELECT <原列..., '__migrated__'> FROM schedules — 复制存量
--   4. 存量回收 — 聚焦工作区存在时将 __migrated__ 占位替换为聚焦 id
--   5. DROP TABLE schedules → ALTER TABLE schedules_new RENAME TO schedules
--   6. 重建既有索引 idx_schedules_due
--
-- 整过程由 migrations::run 的 BEGIN IMMEDIATE ... COMMIT 事务包裹，保证原子性。
-- 崩溃重入：若步骤 1-3 完成后崩溃（schedules_new 已建未切换），下次运行时
--   DROP TABLE IF EXISTS schedules_new 清理残留，重新执行全流程。

-- 步骤 1：幂等清理上次崩溃残留
DROP TABLE IF EXISTS schedules_new;

-- 步骤 2：新建含 workspace_id 的表（不含 name 全局 UNIQUE）
CREATE TABLE schedules_new (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    cron_expr         TEXT NOT NULL,
    task_title        TEXT NOT NULL,
    task_description  TEXT NOT NULL DEFAULT '',
    enabled           INTEGER NOT NULL DEFAULT 1,
    last_triggered_at INTEGER,
    next_trigger_at   INTEGER,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    target_kind       TEXT NOT NULL DEFAULT 'task',
    agent_kind        TEXT,
    agent_ref_id      TEXT,
    team_id           TEXT,
    session_mode      TEXT NOT NULL DEFAULT 'per_trigger',
    session_id        TEXT,
    auto_dispatch     INTEGER NOT NULL DEFAULT 1,
    workspace_id      TEXT NOT NULL DEFAULT '__migrated__'
);

CREATE UNIQUE INDEX idx_schedules_new_workspace_name ON schedules_new (workspace_id, name);
CREATE INDEX idx_schedules_new_workspace ON schedules_new (workspace_id);

-- 步骤 3：复制存量（workspace_id 填占位符，待步骤 4 回收）
INSERT INTO schedules_new
(id, name, cron_expr, task_title, task_description, enabled,
 last_triggered_at, next_trigger_at, created_at, updated_at,
 target_kind, agent_kind, agent_ref_id, team_id, session_mode, session_id, auto_dispatch,
 workspace_id)
SELECT
id, name, cron_expr, task_title, task_description, enabled,
 last_triggered_at, next_trigger_at, created_at, updated_at,
 target_kind, agent_kind, agent_ref_id, team_id, session_mode, session_id, auto_dispatch,
 '__migrated__'
FROM schedules;

-- 步骤 4：存量回收 — 聚焦工作区存在时将 __migrated__ 占位替换为聚焦 id
UPDATE schedules_new
SET workspace_id = (SELECT workspace_id FROM workspace_open_state WHERE is_focused = 1)
WHERE workspace_id = '__migrated__'
  AND EXISTS (SELECT 1 FROM workspace_open_state WHERE is_focused = 1);

-- 步骤 5：切换表
DROP TABLE schedules;
ALTER TABLE schedules_new RENAME TO schedules;

-- 步骤 6：重建既有索引（DROP TABLE 后索引已随表删除，需在新表上重建）
CREATE INDEX idx_schedules_due ON schedules (enabled, next_trigger_at);
