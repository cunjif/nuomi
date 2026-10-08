-- 0033: Event rollbacks marker table (message rollback/undo).
-- Events are append-only (trg_events_no_update/no_delete); rollback is a
-- soft-hide via an independent marker table, not a physical delete.
CREATE TABLE IF NOT EXISTS event_rollbacks (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    aggregate_type  TEXT    NOT NULL,
    aggregate_id    TEXT    NOT NULL,
    from_seq        INTEGER NOT NULL,
    to_seq          INTEGER NOT NULL,
    rolled_back_at  INTEGER NOT NULL,
    reason          TEXT
);

CREATE INDEX IF NOT EXISTS idx_event_rollbacks_agg
    ON event_rollbacks (aggregate_type, aggregate_id, from_seq, to_seq);
