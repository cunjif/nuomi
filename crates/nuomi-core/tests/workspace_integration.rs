//! End-to-end workspace migration + isolation integration tests.
//!
//! Covers spec §5.4–5.7: legacy single-workspace migration, multi-workspace
//! session isolation, codebase-memory migration, remove preserves data,
//! orphan reclaim, and path blacklist.

use std::fs;
use std::path::PathBuf;

use nuomi_core::services::{
    workspace_migration, workspace_registry::WorkspaceRegistry,
};
use nuomi_core::store::{migrations, repos, Db};

fn tempdb() -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Db::open(&db_path.to_string_lossy()).unwrap();
    migrations::run(&db.0).unwrap();
    drop(db);
    (db_path, dir)
}

fn open_db(path: &PathBuf) -> Db {
    Db::open(&path.to_string_lossy()).unwrap()
}

fn make_dir(parent: &std::path::Path, name: &str) -> PathBuf {
    let p = parent.join(name);
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn legacy_single_workspace_migration_end_to_end() {
    let (db_path, dir) = tempdb();
    let ws_root = make_dir(dir.path(), "legacy-ws");

    // Seed: legacy workspace_root setting + a session with __migrated__ workspace_id.
    {
        let db = open_db(&db_path);
        repos::settings::set(&db.0, repos::settings::WORKSPACE_ROOT, &ws_root.to_string_lossy()).unwrap();
        db.0.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES ('s1','old','1','1')",
            [],
        ).unwrap();
    }

    // Run migration.
    {
        let db = open_db(&db_path);
        let outcome = workspace_migration::run_if_needed(&db.0).unwrap();
        assert!(matches!(outcome, workspace_migration::MigrationOutcome::Migrated { .. }));
    }

    // Verify: workspaces table has a real uuid entry, session reparented.
    {
        let db = open_db(&db_path);
        let entries = repos::workspaces::list(&db.0).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(uuid::Uuid::parse_str(&entries[0].id).is_ok(), "id should be uuid-v7");
        assert!(entries[0].is_active);
        let wid: String = db.0
            .query_row("SELECT workspace_id FROM sessions WHERE id='s1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(wid, entries[0].id);
    }
}

#[test]
fn multi_workspace_session_isolation() {
    let (db_path, dir) = tempdb();
    let reg = WorkspaceRegistry::new(db_path.clone());
    let ws_a = make_dir(dir.path(), "ws-a");
    let ws_b = make_dir(dir.path(), "ws-b");

    let ea = reg.register(&ws_a).unwrap();
    let eb = reg.register(&ws_b).unwrap();

    // Create a session in workspace A.
    {
        let db = open_db(&db_path);
        db.0.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES ('s-a','A','1','1')",
            [],
        ).unwrap();
        repos::sessions::set_workspace_id(&db.0, "s-a", &ea.id).unwrap();
    }

    // Activate B and verify sessions::list(B) does not contain s-a.
    reg.activate(&eb.id).unwrap();
    {
        let db = open_db(&db_path);
        let sessions_b = repos::sessions::list(&db.0, &eb.id, 100).unwrap();
        assert!(sessions_b.iter().all(|s| s.id != "s-a"), "session A must not appear in B");
        let sessions_a = repos::sessions::list(&db.0, &ea.id, 100).unwrap();
        assert!(sessions_a.iter().any(|s| s.id == "s-a"), "session A must appear in A");
    }
}

#[test]
fn remove_workspace_preserves_nuomi_dir_and_orphan_sessions() {
    let (db_path, dir) = tempdb();
    let reg = WorkspaceRegistry::new(db_path.clone());
    let ws = make_dir(dir.path(), "removable");

    let entry = reg.register(&ws).unwrap();
    // Create a session in this workspace.
    {
        let db = open_db(&db_path);
        db.0.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES ('s1','test','1','1')",
            [],
        ).unwrap();
        repos::sessions::set_workspace_id(&db.0, "s1", &entry.id).unwrap();
    }

    // Remove the workspace.
    let result = reg.remove(&entry.id).unwrap();
    assert_eq!(result.removed_id, entry.id);
    assert_eq!(result.new_active_id, None);

    // .nuomi/ directory is preserved.
    assert!(ws.join(".nuomi").is_dir(), ".nuomi/ must not be deleted");

    // Session is now an orphan (workspace_id points to removed workspace).
    {
        let db = open_db(&db_path);
        let orphans: Vec<(String, String)> = db.0
            .prepare("SELECT id, workspace_id FROM sessions WHERE workspace_id NOT IN (SELECT id FROM workspaces)")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(orphans.iter().any(|(id, wid)| id == "s1" && wid == &entry.id));
    }
}

#[test]
fn orphan_reclaim_to_new_workspace() {
    let (db_path, dir) = tempdb();
    let reg = WorkspaceRegistry::new(db_path.clone());
    let ws_a = make_dir(dir.path(), "ws-a");
    let ea = reg.register(&ws_a).unwrap();

    // Create a session in A.
    {
        let db = open_db(&db_path);
        db.0.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES ('s1','orphan','1','1')",
            [],
        ).unwrap();
        repos::sessions::set_workspace_id(&db.0, "s1", &ea.id).unwrap();
    }

    // Remove A → session becomes orphan.
    reg.remove(&ea.id).unwrap();

    // Register B and reclaim orphans into B.
    let ws_b = make_dir(dir.path(), "ws-b");
    let eb = reg.register(&ws_b).unwrap();

    {
        let db = open_db(&db_path);
        let n = db.0.execute(
            "UPDATE sessions SET workspace_id = ?1 WHERE workspace_id NOT IN (SELECT id FROM workspaces)",
            rusqlite::params![eb.id],
        ).unwrap();
        assert_eq!(n, 1, "one orphan session should be reclaimed");
    }

    // Verify session now belongs to B.
    {
        let db = open_db(&db_path);
        let sessions_b = repos::sessions::list(&db.0, &eb.id, 100).unwrap();
        assert!(sessions_b.iter().any(|s| s.id == "s1"));
    }
}

#[test]
fn codebase_memory_migration_copies_and_marks() {
    let (db_path, dir) = tempdb();
    let reg = WorkspaceRegistry::new(db_path);
    let ws = make_dir(dir.path(), "cb-ws");

    // Seed: legacy .codebase-memory/ with a file.
    let cb_dir = ws.join(".codebase-memory");
    fs::create_dir_all(&cb_dir).unwrap();
    fs::write(cb_dir.join("index.json"), r#"{"project":"test"}"#).unwrap();

    reg.register(&ws).unwrap();

    // Verify: .nuomi/codebase-memory/ has the copied file.
    let dest = ws.join(".nuomi").join("codebase-memory");
    assert!(dest.is_dir(), ".nuomi/codebase-memory/ must exist");
    let copied = fs::read_to_string(dest.join("index.json")).unwrap();
    assert!(copied.contains("test"));

    // Original is preserved (not deleted).
    assert!(cb_dir.join("index.json").exists(), "original must be preserved");
}

#[test]
fn path_blacklist_rejects_system_directories() {
    let (db_path, _dir) = tempdb();
    let reg = WorkspaceRegistry::new(db_path);

    // Try to register a blacklisted path. Use a path that definitely exists
    // but is blacklisted (e.g., the temp dir's parent on Windows is likely
    // under C:\Users which is blacklisted; on Unix / is blacklisted).
    let blacklisted = if cfg!(target_os = "windows") {
        std::path::PathBuf::from("C:\\Users")
    } else {
        std::path::PathBuf::from("/")
    };

    let result = reg.register(blacklisted.as_path());
    assert!(
        result.is_err(),
        "blacklisted path should be rejected"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, nuomi_core::services::RegistryError::Blacklisted(_)),
        "expected Blacklisted error, got: {err:?}"
    );
}
