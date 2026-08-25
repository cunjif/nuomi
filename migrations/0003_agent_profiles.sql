-- 0003_agent_profiles: M-CLI1 CLI agent registry (SPEC cli-agents-m1 D7).
-- Conventions unchanged from 0001/0002. Append-only migrations: never edit shipped files.

CREATE TABLE agent_profiles (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    adapter     TEXT NOT NULL DEFAULT 'cli' CHECK (adapter IN ('cli')),
    flavor      TEXT NOT NULL CHECK (flavor IN ('claude_code', 'codex', 'plain')),
    command     TEXT NOT NULL,
    args        TEXT NOT NULL DEFAULT '[]',
    env         TEXT NOT NULL DEFAULT '{}',
    working_dir TEXT,
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);
