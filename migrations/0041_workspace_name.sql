-- 0041_workspace_name: 工作区用户命名列（ADR-0024 Part A）。
-- Append-only — never edit shipped migrations.
--
-- ADR-0021 决策③原为"标识派生不落库"；ADR-0024 修订为"用户命名落库 +
-- 派生兜底"。name 列存放用户创建工作区时输入的命名（非空 trim 后的值）；
-- NULL 表示存量工作区或用户未命名，展示回退派生 label（derive_label）。
-- 零数据搬移：存量行 name=NULL，展示优先级 name > label > id。

ALTER TABLE workspaces ADD COLUMN name TEXT;
