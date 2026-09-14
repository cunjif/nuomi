-- 0014_conversation_meta.sql
-- Conversation metadata: goal, main agent, route modes, participants, todos.
-- Append-only — never edit shipped migrations.

-- Sessions gain optional metadata columns (all nullable for backward compat).
ALTER TABLE sessions ADD COLUMN goal TEXT;
ALTER TABLE sessions ADD COLUMN main_agent_id TEXT;
ALTER TABLE sessions ADD COLUMN route_mode TEXT;
ALTER TABLE sessions ADD COLUMN whiteboard_route_mode TEXT;

-- Many-to-many: which agents participate in a conversation.
CREATE TABLE IF NOT EXISTS conversation_participants (
    session_id    TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    agent_kind    TEXT    NOT NULL,
    agent_ref_id  TEXT    NOT NULL,
    joined_at     INTEGER NOT NULL,
    PRIMARY KEY (session_id, agent_kind, agent_ref_id)
);

-- Per-conversation todo items (agent-summarized, user-editable).
CREATE TABLE IF NOT EXISTS conversation_todos (
    id           TEXT    PRIMARY KEY,
    session_id   TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    description  TEXT    NOT NULL,
    completed    INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);
