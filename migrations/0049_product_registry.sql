-- Unified product registry: indexes all temporary artifacts produced by
-- the six modules (sandbox, workbench, terminal, chat, steward, evolution)
-- so they can be discovered and consumed across module boundaries.
--
-- This table is NOT append-only (unlike `events`): rows are inserted when a
-- product is created and deleted by GC when `expires_at` passes. The `events`
-- table remains the authoritative audit trail; this table is a file-system
-- index for cross-module sharing.

CREATE TABLE product_registry (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    kind         TEXT    NOT NULL,  -- tool_result|subagent|search|sandbox_output|sandbox_artifact|terminal_log|evolution_snapshot
    producer     TEXT    NOT NULL,  -- loop:<run_id>|steward:<session_id>|sandbox:<task_id>|terminal:<term_id>|evolution:<cycle_id>|cross_workspace
    workspace_id TEXT,               -- owning workspace (nullable for global products)
    session_id   TEXT,               -- owning session (nullable)
    run_id       TEXT,               -- owning run (nullable)
    key          TEXT    NOT NULL,  -- business key (tool_call_id / task_id / search_query_hash / ...)
    abs_path     TEXT    NOT NULL,  -- absolute path of the product file
    size_bytes   INTEGER NOT NULL,
    sha256       TEXT,
    created_at   INTEGER NOT NULL,  -- unix-ms
    expires_at   INTEGER,           -- unix-ms; NULL = no expiry
    metadata     TEXT               -- JSON (mime / encoding / source_tool / ...)
);

CREATE INDEX idx_product_kind_ws   ON product_registry(kind, workspace_id);
CREATE INDEX idx_product_producer  ON product_registry(producer);
CREATE INDEX idx_product_expires   ON product_registry(expires_at);
CREATE INDEX idx_product_session   ON product_registry(session_id);
