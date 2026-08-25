-- 0002_tasks_runs: UI M1 execution-domain tables (SPEC ui-m1 D2/D3).
-- Conventions unchanged from 0001: uuid-v7 string ids; unix-ms i64 timestamps;
-- JSON payloads as TEXT. Append-only migrations: never edit shipped files.

CREATE TABLE tasks (
    id          TEXT PRIMARY KEY,
    session_id  TEXT REFERENCES sessions(id),
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL DEFAULT 'queued'
                CHECK (status IN ('backlog', 'queued', 'running', 'done', 'cancelled')),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX idx_tasks_status ON tasks (status);
CREATE INDEX idx_tasks_session ON tasks (session_id);

CREATE TABLE runs (
    id           TEXT PRIMARY KEY,
    task_id      TEXT NOT NULL REFERENCES tasks(id),
    session_id   TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'queued'
                 CHECK (status IN ('queued', 'running', 'awaiting_approval', 'succeeded',
                                   'failed', 'timed_out', 'cancelled', 'interrupted')),
    heartbeat_at INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE INDEX idx_runs_task ON runs (task_id);
CREATE INDEX idx_runs_status ON runs (status);

CREATE TABLE approvals (
    id             TEXT PRIMARY KEY,
    run_id         TEXT NOT NULL REFERENCES runs(id),
    tool_name      TEXT NOT NULL,
    arguments_json TEXT NOT NULL,
    decision       TEXT NOT NULL DEFAULT 'pending'
                   CHECK (decision IN ('pending', 'approved', 'denied')),
    decided_at     INTEGER,
    created_at     INTEGER NOT NULL
);

CREATE INDEX idx_approvals_run ON approvals (run_id);
CREATE INDEX idx_approvals_pending ON approvals (decision);

CREATE TABLE schedules (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL UNIQUE,
    cron_expr         TEXT NOT NULL,
    task_title        TEXT NOT NULL,
    task_description  TEXT NOT NULL DEFAULT '',
    enabled           INTEGER NOT NULL DEFAULT 1,
    last_triggered_at INTEGER,
    next_trigger_at   INTEGER,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
);

CREATE INDEX idx_schedules_due ON schedules (enabled, next_trigger_at);
