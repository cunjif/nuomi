-- 0037_sandbox_tasks: 沙箱执行任务记录——生命周期审计（ADR-0022）。
-- Append-only — never edit shipped migrations.
--
-- 沙箱任务（代码运行 run_code / 应用构建 build_app）的持久化载体：
-- 每次状态变迁先落 `sandbox.task_state_changed` 审计事件（events 表）再
-- 产生外部副作用（对齐 Run 状态机铁律），本表承载任务的终态快照与
-- 输出/产物定位。authorized_dirs 为 JSON 数组（提交时内核校验 ⊆ 当前
-- 工作区工作目录集）；limits 为 JSON 对象（timeout_secs/cpu_percent/
-- memory_mb/network_enabled）。超大输出落 .nuomi/sandbox/<task_id>/
-- 产物目录、行内只存摘要与定位（截断策略由执行器控制）——本表只存
-- 全量文本列，超大场景由 7.4 执行器写产物目录后置摘要引用。

CREATE TABLE sandbox_tasks (
    id              TEXT PRIMARY KEY,            -- uuid-v7
    task_type       TEXT NOT NULL CHECK (task_type IN ('run_code', 'build_app')),
    runtime         TEXT NOT NULL,               -- 执行语言/运行时（插件声明清单内）
    status          TEXT NOT NULL CHECK (status IN ('queued','running','succeeded','failed','timed_out','cancelled')),
    source          TEXT NOT NULL,               -- 代码片段/目标文件/构建目标描述
    authorized_dirs TEXT NOT NULL,               -- JSON array（⊆ 工作区工作目录）
    limits          TEXT NOT NULL,               -- JSON object（资源限制）
    stdout          TEXT NOT NULL DEFAULT '',
    stderr          TEXT NOT NULL DEFAULT '',
    artifact_ref    TEXT,                        -- 产物定位（沙箱内路径或回传路径）
    artifact_retryable INTEGER NOT NULL DEFAULT 0, -- 产物回传失败可重试标记
    exit_info       TEXT,                        -- 退出码/信号/中止原因
    duration_ms     INTEGER,
    created_at      INTEGER NOT NULL,            -- unix-ms
    finished_at     INTEGER
);

CREATE INDEX idx_sandbox_tasks_created ON sandbox_tasks (created_at DESC);
