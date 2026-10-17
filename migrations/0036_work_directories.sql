-- 0036_work_directories: 工作区两级模型——工作目录表 + 存量迁移（ADR-0021）。
-- Append-only — never edit shipped migrations.
--
-- 将"一个物理目录 = 一个工作区"模型重构为"工作区（一级、唯一打开单位）
-- ⊃ 工作目录（多个挂载）"两级模型。work_directories 存放各工作区挂载的
-- 工作目录：abs_path 为规范化绝对路径，UNIQUE(workspace_id, abs_path) 做
-- 库内兜底；跨工作区唯一归属（一个目录不能同时挂多个工作区）由 repo 层
-- 事务保证（ADR-0021 决策 2）。
-- 存量迁移：每个 workspaces.root_path 非空行生成首个工作目录（NOT EXISTS
-- 守卫幂等，mounted=1、added_at=created_at）。id 用临时 hex 占位、
-- display_name 用 root_path 占位——精确修正（uuid-v7、跨平台分隔符、尾部
-- 斜杠、同工作区内重名消歧）由 Rust 端 workspace_migration 在迁移 runner
-- 执行后完成，对齐 0016 的"SQL 占位 + Rust 修正"既有模式。

CREATE TABLE work_directories (
    id           TEXT PRIMARY KEY,            -- uuid-v7（占位行为临时 hex）
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    abs_path     TEXT NOT NULL,               -- 规范化绝对路径
    display_name TEXT NOT NULL,               -- 目录名（Rust 端修正）
    mounted      INTEGER NOT NULL DEFAULT 1 CHECK (mounted IN (0, 1)),
    added_at     INTEGER NOT NULL,            -- unix-ms
    UNIQUE (workspace_id, abs_path)
);

CREATE INDEX idx_work_directories_ws ON work_directories (workspace_id, added_at);

-- 存量迁移：为每个 root_path 非空的工作区生成首个工作目录。
-- NOT EXISTS 守卫保证幂等（重复执行不产生第二行）。
INSERT INTO work_directories (id, workspace_id, abs_path, display_name, mounted, added_at)
SELECT
    lower(hex(randomblob(16))),               -- 临时 id, 由 Rust 端迁移后用 uuid-v7 替换
    id,
    root_path,                                 -- 占位, Rust 端规范化后修正
    root_path,                                 -- 占位 display_name, Rust 端修正为目录名
    1,
    created_at
FROM workspaces
WHERE root_path <> ''
  AND NOT EXISTS (SELECT 1 FROM work_directories WHERE workspace_id = workspaces.id);
