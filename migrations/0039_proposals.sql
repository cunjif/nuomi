-- 0039_proposals: advisory team-formation proposals (P2-F 组 2, ADR-0023).
-- Third inbox row family next to approvals: an LLM-planned team draft that a
-- producer (scheduled auto-formation / idle backlog scan / future manual
-- save) records for a human to approve before any Role/Team row is written.
-- Conventions unchanged: uuid-v7 string ids; unix-ms i64 timestamps; JSON
-- payloads as TEXT. Append-only migrations: never edit shipped files.

CREATE TABLE proposals (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    source       TEXT NOT NULL,
    plan_json    TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending'
                 CHECK (status IN ('pending', 'approved', 'denied')),
    workspace_id TEXT NOT NULL,
    resolved_at  INTEGER,
    created_at   INTEGER NOT NULL
);

CREATE INDEX idx_proposals_pending ON proposals (status);
CREATE INDEX idx_proposals_source_status ON proposals (source, status);
