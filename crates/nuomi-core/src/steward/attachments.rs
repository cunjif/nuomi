//! Steward-scoped attachment intake (task 4.2, design §2.2.2-A2).
//!
//! Entry-scoped MIME whitelist + configurable size cap + steward-session
//! ownership check, then content-addressed storage reusing the shared
//! `attachments` table and the `.nuomi/attachments/{session}/{sha256}.{ext}`
//! layout also used by the chat composer. The steward whitelist differs
//! from the chat one: it adds common Office formats and restricts images
//! to png/jpg/gif/webp (exact list, no `image/*`/`text/*` prefixes).

use std::path::PathBuf;
use std::sync::Arc;

use rusqlite::Connection;
use tokio::task::spawn_blocking;

use crate::domain::{now_ms, Attachment, AttachmentKind};
use crate::store::repos::{attachments, settings, steward};
use crate::store::{Db, StoreError};

/// `app_settings` key for the per-file size cap in bytes.
pub const ATTACHMENT_MAX_BYTES_KEY: &str = "steward_attachment_max_bytes";
/// Default per-file cap: 10 MB (design §2.1 config table).
pub const DEFAULT_ATTACHMENT_MAX_BYTES: usize = 10 * 1024 * 1024;

/// Steward entrypoint MIME whitelist (spec 5.3): md/txt, pdf, common
/// Office formats, common source/data extensions, png/jpg/gif/webp.
pub const STEWARD_ALLOWED_MIMES: &[&str] = &[
    "text/plain",
    "text/markdown",
    "application/pdf",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-powerpoint",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/json",
    "application/yaml",
    "application/x-yaml",
    "application/javascript",
    "application/typescript",
    "application/x-sh",
    "application/x-python",
    "application/x-rust",
    "application/x-go",
    "application/x-toml",
    "application/xml",
    "application/csv",
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
];

/// True when `mime` is on the steward whitelist (exact match).
pub fn is_steward_allowed_mime(mime: &str) -> bool {
    STEWARD_ALLOWED_MIMES.contains(&mime)
}

/// File extension for a whitelisted steward MIME type. Unrecognised values
/// fall back to `bin` — never the raw MIME substring (defence in depth:
/// the value lands in a filesystem path).
pub fn steward_file_ext_for_mime(mime: &str) -> &'static str {
    match mime {
        "text/plain" => "txt",
        "text/markdown" => "md",
        "application/pdf" => "pdf",
        "application/msword" => "doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.ms-excel" => "xls",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "application/vnd.ms-powerpoint" => "ppt",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
        "application/json" => "json",
        "application/yaml" | "application/x-yaml" => "yaml",
        "application/javascript" | "application/typescript" => "txt",
        "application/x-sh" => "sh",
        "application/x-python" => "py",
        "application/x-rust" => "rs",
        "application/x-go" => "go",
        "application/x-toml" => "toml",
        "application/xml" => "xml",
        "application/csv" => "csv",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "bin",
    }
}

/// Loads the per-file cap from `app_settings`; missing or invalid values
/// fall back to the default (best-effort read, mirrors `StewardLoopConfig`).
pub fn load_max_bytes(conn: &Connection) -> usize {
    settings::get(conn, ATTACHMENT_MAX_BYTES_KEY)
        .ok()
        .flatten()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_ATTACHMENT_MAX_BYTES)
}

/// Validation and persistence failures for steward attachments. Every
/// business variant carries the attachment name so the UI can point at the
/// offending chip (design §2.2.2-A2).
#[derive(Debug, thiserror::Error)]
pub enum StewardAttachmentError {
    #[error("附件 {name} 的文件类型不受支持（{mime}）")]
    MimeRejected { name: String, mime: String },

    #[error("附件 {name} 大小 {size} 字节，超过上限 {max} 字节")]
    TooLarge {
        name: String,
        size: usize,
        max: usize,
    },

    #[error("附件 {name} 内容为空")]
    Empty { name: String },

    #[error("附件 {name} 所属会话无效（{session_id}）")]
    SessionInvalid { name: String, session_id: String },

    #[error("store error: {0}")]
    Store(String),

    #[error("io error: {0}")]
    Io(String),
}

impl From<StoreError> for StewardAttachmentError {
    fn from(e: StoreError) -> Self {
        StewardAttachmentError::Store(e.to_string())
    }
}

/// Validates and persists a steward attachment: whitelist, size cap
/// (`steward_attachment_max_bytes`, default 10 MB), steward-session
/// ownership, then a content-addressed write plus a metadata row. Returns
/// the stored row.
pub async fn save_steward_attachment(
    db_path: Arc<str>,
    workspace_root: PathBuf,
    session_id: &str,
    name: &str,
    mime: &str,
    data: Vec<u8>,
) -> Result<Attachment, StewardAttachmentError> {
    if !is_steward_allowed_mime(mime) {
        return Err(StewardAttachmentError::MimeRejected {
            name: name.to_string(),
            mime: mime.to_string(),
        });
    }
    if data.is_empty() {
        return Err(StewardAttachmentError::Empty {
            name: name.to_string(),
        });
    }
    let session_id = session_id.to_string();
    let name = name.to_string();
    let mime = mime.to_string();
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        let max = load_max_bytes(conn);
        if data.len() > max {
            return Err(StewardAttachmentError::TooLarge {
                name,
                size: data.len(),
                max,
            });
        }
        // Ownership: the attachment must belong to a live steward session.
        if steward::get_steward_session(conn, &session_id).is_err() {
            return Err(StewardAttachmentError::SessionInvalid { name, session_id });
        }

        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let sha256 = format!("{:x}", hasher.finalize());
        let kind = if mime.starts_with("image/") {
            AttachmentKind::Image
        } else {
            AttachmentKind::File
        };
        let ext = steward_file_ext_for_mime(&mime);
        let rel_path = format!(".nuomi/attachments/{session_id}/{sha256}.{ext}");
        let attachment = Attachment {
            id: crate::domain::new_id(),
            session_id: session_id.clone(),
            seq: None,
            kind,
            name: name.clone(),
            mime: mime.clone(),
            rel_path: rel_path.clone(),
            size_bytes: data.len() as i64,
            sha256,
            created_at: now_ms(),
        };

        let disk_dir = workspace_root.join(format!(".nuomi/attachments/{session_id}"));
        std::fs::create_dir_all(&disk_dir)
            .map_err(|e| StewardAttachmentError::Io(e.to_string()))?;
        let disk_path = workspace_root.join(&rel_path);
        // Defence in depth: the write must stay inside the session dir even
        // if a future refactor lets an unsanitised segment through.
        if !disk_path.starts_with(&disk_dir) {
            return Err(StewardAttachmentError::Io(
                "attachment path escapes the session directory".to_string(),
            ));
        }
        std::fs::write(&disk_path, &data).map_err(|e| StewardAttachmentError::Io(e.to_string()))?;
        attachments::insert(conn, &attachment)?;
        Ok(attachment)
    })
    .await
    .map_err(|e| StewardAttachmentError::Store(format!("join error: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ConversationKind, Session, StewardAi, StewardSession};
    use crate::store::migrations;
    use crate::store::repos::sessions as sessions_repo;

    struct TestEnv {
        _dir: tempfile::TempDir,
        db_path: Arc<str>,
        workspace_root: PathBuf,
    }

    fn test_env() -> TestEnv {
        let dir = tempfile::tempdir().unwrap();
        let db_file = dir.path().join("s.db");
        let db_path: Arc<str> = Arc::from(db_file.to_string_lossy().to_string());
        {
            let db = Db::open(&db_path).unwrap();
            migrations::run(&db.0).unwrap();
        }
        let workspace_root = dir.path().join("ws");
        std::fs::create_dir_all(&workspace_root).unwrap();
        TestEnv {
            _dir: dir,
            db_path,
            workspace_root,
        }
    }

    fn with_conn(env: &TestEnv, f: impl FnOnce(&Connection)) {
        let db = Db::open(&env.db_path).unwrap();
        f(&db.0);
    }

    fn insert_steward_session(env: &TestEnv, id: &str) {
        with_conn(env, |conn| {
            let now = now_ms();
            steward::upsert_steward_ai(
                conn,
                &StewardAi {
                    id: "steward-1".to_string(),
                    ready: false,
                    online_authorized: false,
                    created_at: now,
                },
            )
            .unwrap();
            let session = Session {
                id: id.to_string(),
                title: "steward".to_string(),
                created_at: now,
                updated_at: now,
                kind: ConversationKind::Background,
                team_id: None,
                task_id: None,
                schedule_id: None,
                goal: None,
                main_agent_id: None,
                route_mode: None,
                whiteboard_route_mode: None,
                deleted_at: None,
            };
            sessions_repo::insert(conn, &session).unwrap();
            sessions_repo::set_workspace_id(conn, id, "__migrated__").unwrap();
            steward::insert_steward_session(
                conn,
                &StewardSession {
                    id: id.to_string(),
                    steward_id: "steward-1".to_string(),
                    title: "steward".to_string(),
                    goal: None,
                    created_at: now,
                    updated_at: now,
                },
            )
            .unwrap();
        });
    }

    #[test]
    fn whitelist_accepts_steward_scopes_and_rejects_others() {
        for mime in [
            "text/markdown",
            "text/plain",
            "application/pdf",
            "application/msword",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "application/x-rust",
            "image/png",
            "image/jpeg",
            "image/gif",
            "image/webp",
        ] {
            assert!(is_steward_allowed_mime(mime), "should allow {mime}");
        }
        // Executables, vector images and free-text HTML are not on the list.
        for mime in [
            "application/x-msdownload",
            "application/octet-stream",
            "image/svg+xml",
            "text/html",
            "",
        ] {
            assert!(!is_steward_allowed_mime(mime), "should reject {mime}");
        }
    }

    #[test]
    fn file_ext_maps_office_and_images() {
        assert_eq!(steward_file_ext_for_mime("application/msword"), "doc");
        assert_eq!(
            steward_file_ext_for_mime(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            ),
            "docx"
        );
        assert_eq!(
            steward_file_ext_for_mime(
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            ),
            "xlsx"
        );
        assert_eq!(steward_file_ext_for_mime("image/jpeg"), "jpg");
        assert_eq!(steward_file_ext_for_mime("image/png"), "png");
        assert_eq!(steward_file_ext_for_mime("text/markdown"), "md");
        assert_eq!(steward_file_ext_for_mime("mystery/unknown"), "bin");
    }

    #[test]
    fn load_max_bytes_defaults_and_overrides() {
        let env = test_env();
        with_conn(&env, |conn| {
            assert_eq!(load_max_bytes(conn), DEFAULT_ATTACHMENT_MAX_BYTES);
            settings::set(conn, ATTACHMENT_MAX_BYTES_KEY, "1024").unwrap();
            assert_eq!(load_max_bytes(conn), 1024);
            // Invalid values fall back to the default.
            settings::set(conn, ATTACHMENT_MAX_BYTES_KEY, "not-a-number").unwrap();
            assert_eq!(load_max_bytes(conn), DEFAULT_ATTACHMENT_MAX_BYTES);
            settings::set(conn, ATTACHMENT_MAX_BYTES_KEY, "0").unwrap();
            assert_eq!(load_max_bytes(conn), DEFAULT_ATTACHMENT_MAX_BYTES);
        });
    }

    #[tokio::test]
    async fn rejects_executable_mime_with_attachment_name() {
        let env = test_env();
        insert_steward_session(&env, "sess-exe");
        let err = save_steward_attachment(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "sess-exe",
            "setup.exe",
            "application/x-msdownload",
            b"MZ..".to_vec(),
        )
        .await
        .unwrap_err();
        match err {
            StewardAttachmentError::MimeRejected { name, mime } => {
                assert_eq!(name, "setup.exe");
                assert_eq!(mime, "application/x-msdownload");
            }
            other => panic!("expected MimeRejected, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rejects_oversized_attachment_from_configured_cap() {
        let env = test_env();
        insert_steward_session(&env, "sess-big");
        with_conn(&env, |conn| {
            settings::set(conn, ATTACHMENT_MAX_BYTES_KEY, "8").unwrap();
        });
        let err = save_steward_attachment(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "sess-big",
            "big.md",
            "text/markdown",
            vec![b'x'; 9],
        )
        .await
        .unwrap_err();
        match err {
            StewardAttachmentError::TooLarge { name, size, max } => {
                assert_eq!(name, "big.md");
                assert_eq!(size, 9);
                assert_eq!(max, 8);
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rejects_empty_payload() {
        let env = test_env();
        insert_steward_session(&env, "sess-empty");
        let err = save_steward_attachment(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "sess-empty",
            "empty.txt",
            "text/plain",
            Vec::new(),
        )
        .await
        .unwrap_err();
        match err {
            StewardAttachmentError::Empty { name } => assert_eq!(name, "empty.txt"),
            other => panic!("expected Empty, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rejects_unknown_and_non_steward_sessions() {
        let env = test_env();
        // Unknown session id.
        let err = save_steward_attachment(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "no-such-session",
            "a.md",
            "text/markdown",
            b"hello".to_vec(),
        )
        .await
        .unwrap_err();
        match err {
            StewardAttachmentError::SessionInvalid { name, session_id } => {
                assert_eq!(name, "a.md");
                assert_eq!(session_id, "no-such-session");
            }
            other => panic!("expected SessionInvalid, got {other:?}"),
        }

        // A plain (non-steward) chat session is not a valid owner.
        with_conn(&env, |conn| {
            let now = now_ms();
            let session = Session {
                id: "chat-only".to_string(),
                title: "chat".to_string(),
                created_at: now,
                updated_at: now,
                kind: ConversationKind::Chat,
                team_id: None,
                task_id: None,
                schedule_id: None,
                goal: None,
                main_agent_id: None,
                route_mode: None,
                whiteboard_route_mode: None,
                deleted_at: None,
            };
            sessions_repo::insert(conn, &session).unwrap();
            sessions_repo::set_workspace_id(conn, "chat-only", "__migrated__").unwrap();
        });
        let err = save_steward_attachment(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "chat-only",
            "b.md",
            "text/markdown",
            b"hello".to_vec(),
        )
        .await
        .unwrap_err();
        match err {
            StewardAttachmentError::SessionInvalid { name, session_id } => {
                assert_eq!(name, "b.md");
                assert_eq!(session_id, "chat-only");
            }
            other => panic!("expected SessionInvalid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn saves_metadata_row_and_content_addressed_file() {
        let env = test_env();
        insert_steward_session(&env, "sess-ok");
        let saved = save_steward_attachment(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "sess-ok",
            "notes.md",
            "text/markdown",
            b"# hello".to_vec(),
        )
        .await
        .unwrap();

        assert_eq!(saved.session_id, "sess-ok");
        assert_eq!(saved.name, "notes.md");
        assert_eq!(saved.mime, "text/markdown");
        assert_eq!(saved.kind, AttachmentKind::File);
        assert_eq!(saved.size_bytes, 7);
        assert!(saved.rel_path.starts_with(".nuomi/attachments/sess-ok/"));
        assert!(saved.rel_path.ends_with(".md"));
        assert_eq!(saved.sha256.len(), 64);

        // File exists on disk at the content-addressed path.
        let disk_path = env.workspace_root.join(&saved.rel_path);
        assert_eq!(std::fs::read(&disk_path).unwrap(), b"# hello");

        // Metadata row is persisted.
        with_conn(&env, |conn| {
            let rows = attachments::list_by_session(conn, "sess-ok").unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].id, saved.id);
            assert_eq!(rows[0].sha256, saved.sha256);
        });
    }

    #[tokio::test]
    async fn saves_image_attachment_with_image_kind() {
        let env = test_env();
        insert_steward_session(&env, "sess-img");
        let saved = save_steward_attachment(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "sess-img",
            "shot.png",
            "image/png",
            vec![0x89, b'P', b'N', b'G'],
        )
        .await
        .unwrap();
        assert_eq!(saved.kind, AttachmentKind::Image);
        assert!(saved.rel_path.ends_with(".png"));
    }
}
