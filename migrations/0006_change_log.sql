-- 0006_change_log: CDC capture for UI-facing tables (P1: change data capture).
-- AFTER triggers append one change_log row per insert/update/delete, atomically
-- in the same transaction as the source write. The high-frequency events table
-- is deliberately NOT tracked. `tasks` and `runs` (the tasks_runs domain from
-- migration 0002) are both covered. Append-only migrations: never edit shipped files.

CREATE TABLE change_log (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    table_name TEXT NOT NULL,
    row_id     TEXT NOT NULL,
    op         TEXT NOT NULL CHECK (op IN ('insert', 'update', 'delete')),
    changed_at INTEGER NOT NULL
);

CREATE TRIGGER trg_cdc_sessions_ai AFTER INSERT ON sessions
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('sessions', new.id, 'insert',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_sessions_au AFTER UPDATE ON sessions
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('sessions', new.id, 'update',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_sessions_ad AFTER DELETE ON sessions
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('sessions', old.id, 'delete',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_teams_ai AFTER INSERT ON teams
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('teams', new.id, 'insert',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_teams_au AFTER UPDATE ON teams
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('teams', new.id, 'update',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_teams_ad AFTER DELETE ON teams
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('teams', old.id, 'delete',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_roles_ai AFTER INSERT ON roles
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('roles', new.id, 'insert',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_roles_au AFTER UPDATE ON roles
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('roles', new.id, 'update',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_roles_ad AFTER DELETE ON roles
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('roles', old.id, 'delete',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_agent_profiles_ai AFTER INSERT ON agent_profiles
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('agent_profiles', new.id, 'insert',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_agent_profiles_au AFTER UPDATE ON agent_profiles
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('agent_profiles', new.id, 'update',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_agent_profiles_ad AFTER DELETE ON agent_profiles
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('agent_profiles', old.id, 'delete',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

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

CREATE TRIGGER trg_cdc_runs_ai AFTER INSERT ON runs
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('runs', new.id, 'insert',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_runs_au AFTER UPDATE ON runs
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('runs', new.id, 'update',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trg_cdc_runs_ad AFTER DELETE ON runs
BEGIN
    INSERT INTO change_log (table_name, row_id, op, changed_at)
    VALUES ('runs', old.id, 'delete',
            CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;
