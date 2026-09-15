-- 0018_task_failed_status: add 'failed' to tasks.status CHECK constraint.
-- Scheduler dispatch failures mark the Task as 'failed' (AC8, ADR 0011).
-- SQLite cannot ALTER a CHECK constraint in place, so we rebuild the table.

CREATE TABLE tasks_new (
    id          TEXT PRIMARY KEY,
    session_id  TEXT REFERENCES sessions(id),
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL DEFAULT 'queued'
                CHECK (status IN ('backlog', 'queued', 'running', 'done', 'cancelled', 'failed')),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

INSERT INTO tasks_new (id, session_id, title, description, status, created_at, updated_at)
SELECT id, session_id, title, description, status, created_at, updated_at FROM tasks;

DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;

CREATE INDEX idx_tasks_status ON tasks (status);
CREATE INDEX idx_tasks_session ON tasks (session_id);

-- Restore CDC triggers lost when the original tasks table was dropped.
CREATE TRIGGER trg_cdc_tasks_ai AFTER INSERT ON tasks
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('tasks', new.id, 'insert',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_tasks_au AFTER UPDATE ON tasks
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('tasks', new.id, 'update',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_tasks_ad AFTER DELETE ON tasks
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('tasks', old.id, 'delete',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;
