-- 0016_workspaces: 多工作区注册表 + 存量单工作区迁移占位。
-- Append-only — never edit shipped migrations.
--
-- workspaces 表存放所有已纳管工作区的注册项。root_path 规范化绝对路径带
-- UNIQUE 约束；is_active 全表至多一条为 1（由应用层 + 部分索引共同维护）。
-- 存量迁移：若 app_settings.workspace_root 存在且 workspaces 表为空，插入
-- 占位行（临时 id、默认色 paper-yellow、is_active=1），保证迁移幂等。路径
-- 规范化、uuid-v7 id、哈希取色由 Rust 端 WorkspaceMigration 在迁移 runner
-- 执行后（启动时）修正，避免在 SQL 中实现 FNV-1a 哈希与路径规范化。

CREATE TABLE workspaces (
    id          TEXT PRIMARY KEY,           -- uuid-v7（占位行为临时 hex）
    root_path   TEXT NOT NULL UNIQUE,       -- 规范化绝对路径
    color_tag   TEXT NOT NULL,              -- 色板键名
    created_at  INTEGER NOT NULL,           -- unix-ms
    is_active   INTEGER NOT NULL DEFAULT 0  -- 0/1, 全表至多一条为 1
);

CREATE INDEX idx_workspaces_active ON workspaces (is_active) WHERE is_active = 1;

-- 存量迁移占位 INSERT：从 app_settings.workspace_root 迁移为注册项并设激活。
-- NOT EXISTS 守卫保证幂等（重复执行不产生第二行）。
INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active)
SELECT
    lower(hex(randomblob(16))),             -- 临时 id, 由 Rust 端迁移后用 uuid-v7 替换
    value,
    'paper-yellow',                          -- 默认色, Rust 端迁移后用哈希取色修正
    strftime('%s', 'now') * 1000,
    1
FROM app_settings
WHERE key = 'workspace_root'
  AND NOT EXISTS (SELECT 1 FROM workspaces);
