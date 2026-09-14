-- 0015_context_injections.sql
-- Context injection records: references to other sessions, rules, or custom
-- system prompts that a user has injected into a conversation for extra context.
-- Append-only — never edit shipped migrations.

CREATE TABLE IF NOT EXISTS context_injections (
    id          TEXT    PRIMARY KEY,
    session_id  TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    type        TEXT    NOT NULL,  -- 'session' | 'rule' | 'system_prompt'
    ref_id      TEXT,               -- referenced session/rule id (NULL for system_prompt)
    text        TEXT,               -- inline text (for system_prompt type)
    status      TEXT    NOT NULL DEFAULT 'active',  -- 'active' | 'removed'
    created_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_context_injections_session
    ON context_injections(session_id) WHERE status = 'active';
