-- 0043_selfeval_temp_dirs: 第四层应用级 Ephemeral 临时目录元数据（ADR-0025 Slice 2）。
-- Append-only — never edit shipped migrations.
--
-- 背景：ADR-0025 §4.2 正式设立系统第四层文件分层 nuomi-selfeval-{id}，
-- 与 nuomi-att-{id}、nuomi-plugin-install-{id} 统一归为应用级 Ephemeral 目录。
-- 本表跟踪每个临时目录的元数据，供两阶段 GC + 10GB 硬配额 LRU 回收决策。
--
-- 生命周期：created → active → gc_pending（评估结束待 GC）→ cleaned（已清除）。
-- GC Phase 1（评估结束）：成功 → 清源码+缓存+target，status=cleaned；
--   失败 → 清源码+缓存、保留 target 24h，status=gc_pending。
-- GC Phase 2（演进成功替换宿主后）：强制清所有残留 target，status=cleaned。
-- 配额 LRU：创建新目录前检查总容量，超 10GB 按 created_at ASC 驱逐最旧 gc_pending。

CREATE TABLE selfeval_temp_dirs (
    id          TEXT PRIMARY KEY,
    dir_path    TEXT NOT NULL UNIQUE,
    -- created | active | gc_pending | cleaned
    status      TEXT NOT NULL DEFAULT 'created',
    size_bytes  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    -- unix-ms; set when status transitions to gc_pending (Phase 1 failure path)
    gc_eligible_at INTEGER,
    -- unix-ms; set when status transitions to cleaned
    cleaned_at  INTEGER
);

CREATE INDEX idx_selfeval_temp_status ON selfeval_temp_dirs (status);
CREATE INDEX idx_selfeval_temp_created ON selfeval_temp_dirs (created_at ASC);
CREATE INDEX idx_selfeval_temp_gc_eligible ON selfeval_temp_dirs (gc_eligible_at ASC);
