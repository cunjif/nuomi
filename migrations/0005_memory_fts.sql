-- 0005_memory_fts: FTS5 full-text index over memory_entries (P1: memory search).
-- Uses the trigram tokenizer so CJK substring queries work; writes are kept in
-- sync via AFTER triggers (external-content FTS5 pattern). Append-only
-- migrations: never edit shipped files.

CREATE VIRTUAL TABLE memory_fts USING fts5(
    content,
    content='memory_entries',
    content_rowid='rowid',
    tokenize='trigram'
);

CREATE TRIGGER trg_memory_fts_ai AFTER INSERT ON memory_entries
BEGIN
    INSERT INTO memory_fts (rowid, content) VALUES (new.rowid, new.content);
END;

CREATE TRIGGER trg_memory_fts_au AFTER UPDATE ON memory_entries
BEGIN
    INSERT INTO memory_fts (memory_fts, rowid, content)
    VALUES ('delete', old.rowid, old.content);
    INSERT INTO memory_fts (rowid, content) VALUES (new.rowid, new.content);
END;

CREATE TRIGGER trg_memory_fts_ad AFTER DELETE ON memory_entries
BEGIN
    INSERT INTO memory_fts (memory_fts, rowid, content)
    VALUES ('delete', old.rowid, old.content);
END;
