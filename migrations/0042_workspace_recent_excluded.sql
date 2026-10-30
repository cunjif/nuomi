-- 0042_workspace_recent_excluded: 持久化"用户已从 recent 列表显式移除"标记。
-- Append-only — never edit shipped migrations.
--
-- 背景：workspace_recent 是"最近使用缓存"，remove_recent 只 DELETE 缓存行，
-- workspaces 实体保留。启动 restore 时 workspace_layout::restore_from_snapshot
-- 遍历 workspaces 表逐个 open → workspace_recent::touch（upsert）重新填充
-- 缓存，导致用户删除的 recent 项复活（删除未持久化"已删除"语义）。
--
-- 本表记录"用户已把此 workspace 从 recent 显式移除"。touch 前检查该标记并
-- 跳过（restore 路径）；用户主动重新 open/focus 时清除标记并重新加入 recent
-- （"重新使用"覆盖"曾显式删除"）。ON DELETE CASCADE 对齐 workspace_recent：
-- 工作区实体删除时排除标记一并清除。

CREATE TABLE workspace_recent_excluded (
    workspace_id  TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    excluded_at   INTEGER NOT NULL
);

CREATE INDEX idx_recent_excluded_at ON workspace_recent_excluded (excluded_at DESC);
