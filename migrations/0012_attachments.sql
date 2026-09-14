-- 0012_attachments: per-session attachment metadata (content-addressed on disk).
--   kind = 'file' | 'image' | 'paste' | 'text'
--   seq  — events.seq of the bound user message (NULL until submitted)
--   rel_path — relative to workspace root, e.g. .nuomi/attachments/<sid>/<sha>.png
CREATE TABLE attachments (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq         INTEGER,
    kind        TEXT NOT NULL CHECK (kind IN ('file', 'image', 'paste', 'text')),
    name        TEXT NOT NULL,
    mime        TEXT NOT NULL,
    rel_path    TEXT NOT NULL,
    size_bytes  INTEGER NOT NULL,
    sha256      TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE INDEX idx_attachments_session ON attachments (session_id, created_at);
CREATE INDEX idx_attachments_seq     ON attachments (seq);
