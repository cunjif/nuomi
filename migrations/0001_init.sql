-- 0001_init: core schema for the nuomi harness kernel.
-- Conventions: uuid-v7 string ids; unix-ms i64 timestamps (`*At`); JSON payloads as TEXT.
-- Events and whiteboard notes are append-only (guarded by triggers).

CREATE TABLE sessions (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    aggregate_type  TEXT NOT NULL,
    aggregate_id    TEXT NOT NULL,
    kind            TEXT NOT NULL,
    payload         TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    created_at      INTEGER NOT NULL
);

CREATE INDEX idx_events_aggregate ON events (aggregate_type, aggregate_id, seq);

CREATE TRIGGER trg_events_no_update BEFORE UPDATE ON events
BEGIN
    SELECT RAISE(ABORT, 'events is append-only');
END;

CREATE TRIGGER trg_events_no_delete BEFORE DELETE ON events
BEGIN
    SELECT RAISE(ABORT, 'events is append-only');
END;

CREATE TABLE provider_configs (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL UNIQUE,
    protocol       TEXT NOT NULL CHECK (protocol IN ('openai_compatible', 'anthropic_compatible')),
    base_url       TEXT NOT NULL,
    keyring_ref    TEXT,
    capabilities   TEXT NOT NULL DEFAULT '[]',
    is_master      INTEGER NOT NULL DEFAULT 0,
    fallback_order INTEGER,
    params_json    TEXT NOT NULL DEFAULT '{}',
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE TABLE roles (
    id                      TEXT PRIMARY KEY,
    name                    TEXT NOT NULL UNIQUE,
    provider_id             TEXT REFERENCES provider_configs(id),
    system_prompt_override  TEXT,
    tool_allowlist          TEXT NOT NULL DEFAULT '[]',
    temperature             REAL,
    max_tokens              INTEGER,
    params_json             TEXT NOT NULL DEFAULT '{}',
    created_at              INTEGER NOT NULL,
    updated_at              INTEGER NOT NULL
);

CREATE TABLE teams (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    topology        TEXT NOT NULL CHECK (topology IN ('pipeline', 'router', 'group_chat')),
    member_role_ids TEXT NOT NULL DEFAULT '[]',
    config_json     TEXT NOT NULL DEFAULT '{}',
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE TABLE memory_entries (
    id                TEXT PRIMARY KEY,
    content           TEXT NOT NULL,
    source_session_id TEXT REFERENCES sessions(id),
    tags              TEXT NOT NULL DEFAULT '[]',
    kind              TEXT NOT NULL DEFAULT 'note',
    user_profile      INTEGER NOT NULL DEFAULT 0,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
);

CREATE INDEX idx_memory_tags ON memory_entries (tags);
CREATE INDEX idx_memory_created ON memory_entries (created_at DESC);

CREATE TABLE whiteboard_notes (
    id             TEXT PRIMARY KEY,
    session_id     TEXT NOT NULL REFERENCES sessions(id),
    author_role_id TEXT REFERENCES roles(id),
    note_type      TEXT NOT NULL,
    body           TEXT NOT NULL,
    refs_json      TEXT NOT NULL DEFAULT '{}',
    seq            INTEGER NOT NULL,
    created_at     INTEGER NOT NULL
);

CREATE INDEX idx_whiteboard_session ON whiteboard_notes (session_id, seq);

CREATE TRIGGER trg_whiteboard_no_update BEFORE UPDATE ON whiteboard_notes
BEGIN
    SELECT RAISE(ABORT, 'whiteboard_notes is append-only');
END;

CREATE TRIGGER trg_whiteboard_no_delete BEFORE DELETE ON whiteboard_notes
BEGIN
    SELECT RAISE(ABORT, 'whiteboard_notes is append-only');
END;

CREATE TABLE prompt_versions (
    id             TEXT PRIMARY KEY,
    plugin         TEXT NOT NULL DEFAULT 'system_prompt',
    version        INTEGER NOT NULL,
    status         TEXT NOT NULL DEFAULT 'candidate' CHECK (status IN ('candidate', 'active', 'retired')),
    content        TEXT NOT NULL,
    diff_text      TEXT,
    parent_version INTEGER,
    activated_at   INTEGER,
    created_at     INTEGER NOT NULL,
    UNIQUE (plugin, version)
);

CREATE TABLE artifacts (
    id              TEXT PRIMARY KEY,
    session_id      TEXT REFERENCES sessions(id),
    path            TEXT NOT NULL,
    retention_until INTEGER,
    created_at      INTEGER NOT NULL
);
