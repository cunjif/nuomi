-- 0030_evolution_gate_and_pool: 验收门决策 + 数据池 + 配置变更快照。
-- Append-only — never edit shipped migrations.
--
-- evolution_gate_decisions: 进化产物验收决策，UNIQUE(artifact_id) 保证每产物至多一条决策。
--   决策三选一：approve/reject/request_changes（spec §5.7.1 规则 2）。
-- evolution_data_pools: 清洗后的结构化数据池（脱敏 + 聚合产物）。
-- steward_change_snapshots: 配置变更前/后快照，用于回滚。

CREATE TABLE evolution_gate_decisions (
    id          TEXT PRIMARY KEY,
    artifact_id TEXT NOT NULL REFERENCES evolution_artifacts(id),
    decision    TEXT NOT NULL CHECK (decision IN ('approve','reject','request_changes')),
    reason      TEXT,
    decided_at  INTEGER NOT NULL,
    UNIQUE (artifact_id)
);

CREATE TABLE evolution_data_pools (
    id          TEXT PRIMARY KEY,
    scope_json  TEXT NOT NULL,
    rules_id    TEXT NOT NULL,
    product_json TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX idx_pools_created ON evolution_data_pools (created_at DESC);

CREATE TABLE steward_change_snapshots (
    id          TEXT PRIMARY KEY,
    proposal_id TEXT NOT NULL REFERENCES evolution_artifacts(id),
    target_type TEXT NOT NULL,
    target_id   TEXT NOT NULL,
    before_json TEXT NOT NULL,
    after_json  TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX idx_snapshots_proposal ON steward_change_snapshots (proposal_id);
