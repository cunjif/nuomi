//! WhiteBoard service: append-only shared blackboard for group chats (SPEC D8).
//!
//! SQLite-backed via [`crate::store::repos::whiteboard`]; every call opens a
//! short-lived blocking connection inside `spawn_blocking`.

use std::sync::Arc;

use serde_json::Value;

use crate::domain::{new_id, now_ms, WhiteBoardNote};
use crate::store::repos;
use crate::store::StoreError;

/// Service surface for the shared blackboard.
#[derive(Clone)]
pub struct WhiteBoardService {
    db_path: Arc<str>,
}

impl WhiteBoardService {
    pub fn new(db_path: impl Into<Arc<str>>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    /// Runs `f` with a fresh blocking connection to the shared database.
    async fn with_db<T: Send + 'static>(
        &self,
        f: impl FnOnce(&rusqlite::Connection) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let path = self.db_path.clone();
        tokio::task::spawn_blocking(move || {
            let conn = crate::store::Db::open(&path)?;
            crate::store::migrations::run(&conn.0)?;
            f(&conn.0)
        })
        .await
        .map_err(|e| StoreError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?
    }

    /// Appends a note; `seq` is assigned monotonically within the session.
    pub async fn post(
        &self,
        session_id: &str,
        author_role_id: Option<String>,
        note_type: &str,
        body: String,
        refs: Value,
    ) -> Result<WhiteBoardNote, StoreError> {
        let note = WhiteBoardNote {
            id: new_id(),
            session_id: session_id.to_string(),
            author_role_id,
            note_type: note_type.to_string(),
            body,
            refs,
            seq: 0, // assigned by the repository
            created_at: now_ms(),
        };
        self.with_db({
            let note = note.clone();
            move |c| repos::whiteboard::append(c, &note)
        })
        .await?;
        Ok(note)
    }

    /// All notes of a session in append order.
    pub async fn read_all(&self, session_id: &str) -> Result<Vec<WhiteBoardNote>, StoreError> {
        let sid = session_id.to_string();
        self.with_db(move |c| repos::whiteboard::list_by_session(c, &sid))
            .await
    }

    /// Compact text digest of the board for injection into agent context.
    pub async fn to_context_digest(&self, session_id: &str) -> Result<String, StoreError> {
        let notes = self.read_all(session_id).await?;
        Ok(notes
            .iter()
            .map(|n| {
                format!(
                    "[#{}] {} ({}): {}",
                    n.seq,
                    n.author_role_id.as_deref().unwrap_or("system"),
                    n.note_type,
                    n.body
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// Records one agent turn durably: a whiteboard note plus an append-only
/// `message` event on the session aggregate.
pub(crate) async fn record_turn(
    wb: &WhiteBoardService,
    session_id: &str,
    role: &crate::domain::Role,
    note_type: &str,
    body: &str,
) -> Result<(), super::OrchestratorError> {
    wb.post(
        session_id,
        Some(role.id.clone()),
        note_type,
        body.to_string(),
        serde_json::json!({}),
    )
    .await
    .map_err(|e| super::OrchestratorError::Store(e.to_string()))?;

    let payload = serde_json::json!({
        "role_id": role.id,
        "role_name": role.name,
        "content": body,
    });
    let sid = session_id.to_string();
    wb.with_db(move |c| repos::events::append(c, "session", &sid, "message", &payload, now_ms()))
        .await
        .map_err(|e| super::OrchestratorError::Store(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SESSION: &str = "wb-s1";
    const ROLE_A: &str = "role-a";
    const ROLE_B: &str = "role-b";

    /// Creates the database file and seeds session + role rows (FK targets).
    fn seed_db(path: &str) {
        let db = crate::store::Db::open(path).unwrap();
        crate::store::migrations::run(&db.0).unwrap();
        db.0.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES (?1, '', 1, 1)",
            (SESSION,),
        )
        .unwrap();
        for rid in [ROLE_A, ROLE_B] {
            db.0.execute(
                "INSERT INTO roles (id, name, created_at, updated_at) VALUES (?1, ?1, 1, 1)",
                (rid,),
            )
            .unwrap();
        }
    }

    #[tokio::test]
    async fn post_then_read_all_is_ordered_and_digest_contains_bodies() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wb.db").to_string_lossy().to_string();
        seed_db(&path);

        let wb = WhiteBoardService::new(path.as_str());
        wb.post(
            SESSION,
            Some(ROLE_A.into()),
            "finding",
            "alpha finding".into(),
            json!({}),
        )
        .await
        .unwrap();
        wb.post(
            SESSION,
            Some(ROLE_B.into()),
            "decision",
            "beta decision".into(),
            json!({}),
        )
        .await
        .unwrap();

        let notes = wb.read_all(SESSION).await.unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes.iter().map(|n| n.seq).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(notes[0].body, "alpha finding");
        assert_eq!(notes[1].author_role_id.as_deref(), Some(ROLE_B));

        let digest = wb.to_context_digest(SESSION).await.unwrap();
        assert!(digest.contains("alpha finding"));
        assert!(digest.contains("beta decision"));
        assert!(digest.contains(ROLE_A));
    }

    #[tokio::test]
    async fn sessions_are_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wb2.db").to_string_lossy().to_string();
        seed_db(&path);
        // second session row for isolation check
        let db = crate::store::Db::open(&path).unwrap();
        db.0.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES ('wb-s2', '', 1, 1)",
            (),
        )
        .unwrap();

        let wb = WhiteBoardService::new(path.as_str());
        wb.post(SESSION, None, "finding", "for s1".into(), json!({}))
            .await
            .unwrap();
        wb.post("wb-s2", None, "finding", "for s2".into(), json!({}))
            .await
            .unwrap();

        assert_eq!(wb.read_all(SESSION).await.unwrap().len(), 1);
        assert_eq!(wb.read_all("wb-s2").await.unwrap().len(), 1);
    }
}
