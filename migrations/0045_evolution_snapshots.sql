-- 0045_evolution_snapshots.sql
-- Evolution snapshot tree (ADR-0025 §3-§5 Slice 4).
-- Supports version-tree time travel and parent-child branch tracing.
-- Strong-type contracts:
--   profile_summary → nuomi_core::evolution::ProfileSummary
--   metrics         → nuomi_core::evolution::EvaluationMetrics

CREATE TABLE IF NOT EXISTS evolution_snapshots (
    id                  TEXT PRIMARY KEY NOT NULL,
    parent_snapshot_id  TEXT REFERENCES evolution_snapshots(id),
    trigger_source      TEXT NOT NULL,    -- 'b1_accumulation' | 'online_research' | 'user_manual'
    tier                TEXT NOT NULL,    -- 'L0' | 'L1' | 'L2' | 'L3'
    from_version        TEXT NOT NULL,
    to_version          TEXT NOT NULL,
    profile_summary     TEXT NOT NULL,    -- JSON (ProfileSummary)
    diff_patch_ref      TEXT NOT NULL,    -- relative path to diff patch artifact
    artifact_ref        TEXT,             -- L3 binary archive path (NULL for L0-L2)
    status              TEXT NOT NULL CHECK(status IN ('staged', 'active', 'reverted', 'discarded')),
    metrics             TEXT,             -- JSON (EvaluationMetrics, NULL until evaluation completes)
    created_at          INTEGER NOT NULL  -- epoch millis
);

CREATE INDEX IF NOT EXISTS idx_evolution_snapshots_status  ON evolution_snapshots(status);
CREATE INDEX IF NOT EXISTS idx_evolution_snapshots_created ON evolution_snapshots(created_at);
CREATE INDEX IF NOT EXISTS idx_evolution_snapshots_parent  ON evolution_snapshots(parent_snapshot_id);
CREATE INDEX IF NOT EXISTS idx_evolution_snapshots_tier    ON evolution_snapshots(tier);
