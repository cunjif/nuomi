-- 0024_workspace_open_state: 多工作区开启集合 + 布局快照 + 最近使用列表 + 固定标记。
-- Append-only — never edit shipped migrations.
--
-- 将"单一活跃工作区（is_active 至多一条）"模型重构为"多工作区同时开启并行运行 +
-- 唯一聚焦"模型。workspace_open_state 记录开启集合与聚焦态（is_focused 全表至多一条），
-- workspace_layout_snapshot 持久化布局快照（单行表），workspace_recent 维护最近使用
-- 列表（LRU 淘汰）。workspaces.is_pinned 标记固定工作区（启动时必恢复）。
-- 存量迁移：将 workspaces.is_active = 1 的工作区写入 workspace_open_state，
-- 保证重构后原活跃工作区自动进入开启集合并成为初始聚焦。

-- 固定标记列
ALTER TABLE workspaces ADD COLUMN is_pinned INTEGER NOT NULL DEFAULT 0;

-- 开启状态表：记录哪些工作区当前处于开启集合，以及唯一聚焦态
CREATE TABLE workspace_open_state (
    workspace_id     TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    opened_at        INTEGER NOT NULL,
    last_focused_at  INTEGER NOT NULL,
    is_focused       INTEGER NOT NULL DEFAULT 0 CHECK (is_focused IN (0, 1))
);

CREATE INDEX idx_open_state_focused ON workspace_open_state (is_focused) WHERE is_focused = 1;
CREATE INDEX idx_open_state_last_focused ON workspace_open_state (last_focused_at DESC);

-- 布局快照单行表：持久化当前布局模式与分屏配置
CREATE TABLE workspace_layout_snapshot (
    id                   INTEGER PRIMARY KEY CHECK (id = 1),
    mode                 TEXT NOT NULL DEFAULT 'single' CHECK (mode IN ('single', 'split', 'overview')),
    split_workspace_ids  TEXT,
    focused_workspace_id TEXT,
    captured_at          INTEGER NOT NULL
);

-- 最近使用列表：LRU 淘汰，固定工作区置顶
CREATE TABLE workspace_recent (
    workspace_id  TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    last_used_at  INTEGER NOT NULL,
    is_pinned     INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_recent_last_used ON workspace_recent (last_used_at DESC);

-- 存量数据搬迁：将 is_active = 1 的工作区写入开启集合，设为初始聚焦。
-- NOT EXISTS 守卫保证幂等（重复执行不产生重复行）。
INSERT INTO workspace_open_state (workspace_id, opened_at, last_focused_at, is_focused)
SELECT
    id,
    strftime('%s', 'now') * 1000,
    strftime('%s', 'now') * 1000,
    1
FROM workspaces
WHERE is_active = 1
  AND NOT EXISTS (SELECT 1 FROM workspace_open_state WHERE workspace_id = workspaces.id);
