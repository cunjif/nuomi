//! Steward attachment context builder (task 4.3, design §2.1.3.2).
//!
//! Validates attachments (ownership + disk presence + sha256) then assembles
//! them into agent-loop context by MIME class:
//! - text-like (md/txt/source/data): full read, budget-truncated inline block
//! - pdf: plain-text extraction via `pdf-extract` (failure → placeholder note)
//! - office docx/xlsx/pptx: zip + XML text-node extraction (failure →
//!   placeholder); legacy doc/xls/ppt are not parsable → placeholder
//! - images png/jpg/gif/webp: `ChatImagePart` when the bound provider has the
//!   Image capability tag, otherwise a name+dimensions placeholder with an
//!   instruction for the reply to disclose the descriptive fallback.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::task::spawn_blocking;

use crate::domain::Attachment;
use crate::providers::types::ChatImagePart;
use crate::store::repos::attachments as attachments_repo;
use crate::store::Db;

/// Per-attachment cap on the injected text block (chars, ~24k tokens at
/// chars/4). Oversized content is truncated with a trailing note.
pub const MAX_ATTACHMENT_BLOCK_CHARS: usize = 96_000;

/// Assembled attachment context for one user message.
#[derive(Debug, Default, Clone)]
pub struct AssembledAttachments {
    /// Text blocks appended to the user message
    /// (`StewardLoopInput.attachments`), each shaped
    /// `[附件 name]\ncontent`.
    pub text_blocks: Vec<String>,
    /// Images fed to `ChatMessage::user_with_images` (multimodal path).
    pub image_parts: Vec<ChatImagePart>,
}

/// Validation failure: the message must NOT be sent (design §2.1.3.2).
/// Carries the offending attachment name so the UI can point at the chip.
#[derive(Debug, thiserror::Error)]
#[error("附件 {name}不可用：{reason}")]
pub struct ContextBuildError {
    pub name: String,
    pub reason: String,
}

/// Loads `attachment_ids` for `session_id`, validates each one, and
/// assembles the context. Empty ids yield an empty assembly.
pub async fn build_attachment_context(
    db_path: Arc<str>,
    workspace_root: PathBuf,
    session_id: &str,
    attachment_ids: &[String],
    supports_vision: bool,
) -> Result<AssembledAttachments, ContextBuildError> {
    if attachment_ids.is_empty() {
        return Ok(AssembledAttachments::default());
    }
    let session_id = session_id.to_string();
    let ids = attachment_ids.to_vec();
    spawn_blocking(move || -> Result<AssembledAttachments, ContextBuildError> {
        let db = Db::open(&db_path).map_err(|e| ContextBuildError {
            name: String::new(),
            reason: format!("存储不可用：{e}"),
        })?;
        let owned = attachments_repo::list_by_session(&db.0, &session_id)
            .map_err(|e| ContextBuildError {
                name: String::new(),
                reason: format!("存储不可用：{e}"),
            })?
            .into_iter()
            .filter(|a| ids.contains(&a.id))
            .collect::<Vec<_>>();

        let mut out = AssembledAttachments::default();
        for id in &ids {
            let attachment =
                owned
                    .iter()
                    .find(|a| &a.id == id)
                    .ok_or_else(|| ContextBuildError {
                        name: id.clone(),
                        reason: "附件不存在或不属于当前会话".to_string(),
                    })?;
            assemble_one(&workspace_root, attachment, supports_vision, &mut out)?;
        }
        Ok(out)
    })
    .await
    .map_err(|e| ContextBuildError {
        name: String::new(),
        reason: format!("内部错误：{e}"),
    })?
}

fn assemble_one(
    workspace_root: &std::path::Path,
    attachment: &Attachment,
    supports_vision: bool,
    out: &mut AssembledAttachments,
) -> Result<(), ContextBuildError> {
    let disk_path = workspace_root.join(&attachment.rel_path);
    let data = std::fs::read(&disk_path).map_err(|_| ContextBuildError {
        name: attachment.name.clone(),
        reason: "文件已被移动或删除".to_string(),
    })?;

    // Content check: the sha256 recorded at upload must still match.
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let actual = format!("{:x}", hasher.finalize());
    if actual != attachment.sha256 {
        return Err(ContextBuildError {
            name: attachment.name.clone(),
            reason: "内容校验失败".to_string(),
        });
    }

    let name = &attachment.name;
    let mime = attachment.mime.as_str();
    if mime.starts_with("image/") {
        if supports_vision {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
            out.image_parts.push(ChatImagePart {
                mime_type: mime.to_string(),
                base64: encoded,
            });
        } else {
            let dims = image_dimensions(&data)
                .map(|(w, h)| format!("{w}x{h}"))
                .unwrap_or_else(|| "尺寸未知".to_string());
            out.text_blocks.push(format!(
                "[图片 {name}]（{dims}，{}，{} 字节）\n\
                 [系统提示] 当前模型不支持图片输入，以上传图片的文件名与尺寸描述代替；请在回复中说明当前模型以描述方式理解图片。",
                attachment.mime, data.len()
            ));
        }
        return Ok(());
    }

    let extracted = match mime {
        "application/pdf" => pdf_text(&data),
        "application/msword" | "application/vnd.ms-excel" | "application/vnd.ms-powerpoint" => None, // legacy binary formats
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            office_text(&data, mime)
        }
        _ => Some(String::from_utf8_lossy(&data).into_owned()),
    };

    let block = match extracted {
        Some(text) if !text.trim().is_empty() => {
            let text = text.trim();
            if text.chars().count() > MAX_ATTACHMENT_BLOCK_CHARS {
                let cut: String = text.chars().take(MAX_ATTACHMENT_BLOCK_CHARS).collect();
                format!("[附件 {name}]\n{cut}\n\n[附件 {name} 内容过长，已截断]")
            } else {
                format!("[附件 {name}]\n{text}")
            }
        }
        _ => format!("[附件 {name}]\n（该附件为 {mime}，无法解析出文本内容）"),
    };
    out.text_blocks.push(block);
    Ok(())
}

/// Extracts plain text from a PDF payload. `None` when parsing fails (the
/// caller turns it into a placeholder note).
fn pdf_text(data: &[u8]) -> Option<String> {
    pdf_extract::extract_text_from_mem(data).ok()
}

/// Extracts text from a modern Office payload (a zip of XML parts).
fn office_text(data: &[u8], mime: &str) -> Option<String> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).ok()?;
    match mime {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            let xml = read_zip_entry(&mut archive, "word/document.xml")?;
            Some(xml_tag_texts(&xml, "w:t", "</w:p>"))
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            let xml = read_zip_entry(&mut archive, "xl/sharedStrings.xml")?;
            Some(xml_tag_texts(&xml, "t", "</si>"))
        }
        _ => {
            let mut slides: Vec<String> = Vec::new();
            let names: Vec<String> = archive
                .file_names()
                .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
                .map(|n| n.to_string())
                .collect();
            if names.is_empty() {
                return None;
            }
            for name in names {
                let xml = read_zip_entry(&mut archive, &name)?;
                slides.push(xml_tag_texts(&xml, "a:t", "</p:sp>"));
            }
            Some(slides.join("\n\n"))
        }
    }
}

fn read_zip_entry(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    name: &str,
) -> Option<String> {
    use std::io::Read;
    let mut file = archive.by_name(name).ok()?;
    let mut xml = String::new();
    file.read_to_string(&mut xml).ok()?;
    Some(xml)
}

/// Collects the text content of every `<tag ...>…</tag>` occurrence, joining
/// with `group_close` boundaries (paragraph / shared-string / shape ends).
/// XML entities are decoded; unknown entities pass through untouched.
fn xml_tag_texts(xml: &str, tag: &str, group_close: &str) -> String {
    let open_prefix = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open_prefix) {
        let after_open = &rest[start + open_prefix.len()..];
        let Some(content_start) = after_open.find('>') else {
            break;
        };
        let content = &after_open[content_start + 1..];
        let Some(end) = content.find(&close) else {
            break;
        };
        let text = decode_xml_entities(&content[..end]);
        if !out.is_empty()
            && !out.ends_with('\n')
            && !out.ends_with(' ')
            && !text.starts_with(char::is_whitespace)
        {
            // Element boundary inside the same group: keep a soft separator
            // so words do not run together.
            out.push(' ');
        }
        out.push_str(&text);
        let after = &content[end + close.len()..];
        // A group close only counts when it precedes the next same-tag
        // element (otherwise it belongs to a later sibling group).
        let next_open = after.find(&open_prefix);
        let group_end = after.find(group_close);
        let closes_group = match (group_end, next_open) {
            (Some(g), Some(o)) => g < o,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if closes_group {
            out.push('\n');
            let g = group_end.unwrap_or(0);
            rest = &after[g + group_close.len()..];
        } else {
            rest = after;
        }
    }
    out.trim_end().to_string()
}

fn decode_xml_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let Some(semi) = tail.find(';') else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..semi];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                if let Some(num) = entity.strip_prefix('#').and_then(|n| {
                    u32::from_str_radix(
                        n.trim_start_matches('x'),
                        if n.contains('x') { 16 } else { 10 },
                    )
                    .ok()
                }) {
                    if let Some(ch) = char::from_u32(num) {
                        out.push(ch);
                    } else {
                        out.push_str(&tail[..semi + 1]);
                    }
                } else {
                    out.push_str(&tail[..semi + 1]);
                }
            }
        }
        rest = &tail[semi + 1..];
    }
    out.push_str(rest);
    out
}

/// Reads image pixel dimensions from the file header. `None` when the
/// format is unrecognised (the placeholder description then omits size).
fn image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() >= 24 && data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        return Some((w, h));
    }
    if data.len() >= 10 && (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
        let w = u16::from_le_bytes([data[6], data[7]]) as u32;
        let h = u16::from_le_bytes([data[8], data[9]]) as u32;
        return Some((w, h));
    }
    if data.len() >= 30 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        // VP8X chunk: canvas size is stored 24-bit LE, minus one.
        if &data[12..16] == b"VP8X" {
            let w = ((data[24] as u32) | ((data[25] as u32) << 8) | ((data[26] as u32) << 16)) + 1;
            let h = ((data[27] as u32) | ((data[28] as u32) << 8) | ((data[29] as u32) << 16)) + 1;
            return Some((w, h));
        }
        return None;
    }
    if data.len() >= 4 && data[0] == 0xFF && data[1] == 0xD8 {
        // JPEG: scan segment markers for a SOF frame header.
        let mut i = 2usize;
        while i + 9 < data.len() {
            if data[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = data[i + 1];
            if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC
            {
                let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
                let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
                return Some((w, h));
            }
            let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            i += 2 + len;
        }
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{now_ms, Attachment, AttachmentKind};
    use crate::store::migrations;
    use crate::store::repos::attachments;
    use std::io::Write;

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
        // `attachments.session_id` is FK-bound to `sessions`; seed the two
        // sessions used across the tests below.
        {
            let db = Db::open(&db_path).unwrap();
            for id in ["s1", "s2"] {
                let now = now_ms();
                let session = crate::domain::Session {
                    id: id.to_string(),
                    title: id.to_string(),
                    created_at: now,
                    updated_at: now,
                    kind: crate::domain::ConversationKind::Background,
                    team_id: None,
                    task_id: None,
                    schedule_id: None,
                    goal: None,
                    main_agent_id: None,
                    route_mode: None,
                    whiteboard_route_mode: None,
                    deleted_at: None,
                };
                crate::store::repos::sessions::insert(&db.0, &session).unwrap();
            }
        }
        TestEnv {
            _dir: dir,
            db_path,
            workspace_root,
        }
    }

    fn db_conn(env: &TestEnv) -> Db {
        Db::open(&env.db_path).unwrap()
    }

    /// Writes a file under the workspace at `rel_path`, then records its
    /// metadata row for `session_id`.
    fn seed_attachment(
        env: &TestEnv,
        session_id: &str,
        name: &str,
        mime: &str,
        data: &[u8],
    ) -> Attachment {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        let sha256 = format!("{:x}", hasher.finalize());
        let rel_dir = format!(".nuomi/attachments/{session_id}");
        std::fs::create_dir_all(env.workspace_root.join(&rel_dir)).unwrap();
        let ext = match mime {
            "text/markdown" => "md",
            "text/plain" => "txt",
            "application/pdf" => "pdf",
            "image/png" => "png",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
            _ => "bin",
        };
        let rel_path = format!("{rel_dir}/{sha256}.{ext}");
        std::fs::write(env.workspace_root.join(&rel_path), data).unwrap();
        let attachment = Attachment {
            id: crate::domain::new_id(),
            session_id: session_id.to_string(),
            seq: None,
            kind: if mime.starts_with("image/") {
                AttachmentKind::Image
            } else {
                AttachmentKind::File
            },
            name: name.to_string(),
            mime: mime.to_string(),
            rel_path: rel_path.clone(),
            size_bytes: data.len() as i64,
            sha256: sha256.clone(),
            created_at: now_ms(),
        };
        let db = db_conn(env);
        attachments::insert(&db.0, &attachment).unwrap();
        attachment
    }

    #[tokio::test]
    async fn empty_ids_yield_empty_assembly() {
        let env = test_env();
        let out = build_attachment_context(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "s1",
            &[],
            false,
        )
        .await
        .unwrap();
        assert!(out.text_blocks.is_empty());
        assert!(out.image_parts.is_empty());
    }

    #[tokio::test]
    async fn text_attachment_inlines_full_content() {
        let env = test_env();
        seed_attachment(
            &env,
            "s1",
            "notes.md",
            "text/markdown",
            "# 标题\n\n正文内容".as_bytes(),
        );
        let out = build_attachment_context(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "s1",
            &[seed_id(&env, "s1", "notes.md")],
            false,
        )
        .await
        .unwrap();
        assert_eq!(out.text_blocks.len(), 1);
        assert!(out.text_blocks[0].starts_with("[附件 notes.md]\n"));
        assert!(out.text_blocks[0].contains("# 标题"));
        assert!(out.text_blocks[0].contains("正文内容"));
        assert!(out.image_parts.is_empty());
    }

    #[tokio::test]
    async fn oversized_text_is_truncated_with_note() {
        let env = test_env();
        let big = "x".repeat(MAX_ATTACHMENT_BLOCK_CHARS + 100);
        seed_attachment(&env, "s1", "big.txt", "text/plain", big.as_bytes());
        let out = build_attachment_context(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "s1",
            &[seed_id(&env, "s1", "big.txt")],
            false,
        )
        .await
        .unwrap();
        assert!(out.text_blocks[0].contains("[附件 big.txt 内容过长，已截断]"));
        assert!(out.text_blocks[0].chars().count() < MAX_ATTACHMENT_BLOCK_CHARS + 200);
    }

    #[tokio::test]
    async fn binary_garbage_text_mime_falls_back_to_placeholder() {
        let env = test_env();
        // Invalid UTF-8 in a text/* payload still produces lossy text; a
        // PDF-labeled payload that pdf-extract cannot parse produces the
        // placeholder note instead.
        seed_attachment(&env, "s1", "broken.pdf", "application/pdf", b"not a pdf");
        let out = build_attachment_context(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "s1",
            &[seed_id(&env, "s1", "broken.pdf")],
            false,
        )
        .await
        .unwrap();
        assert!(out.text_blocks[0].contains("无法解析出文本内容"));
    }

    #[tokio::test]
    async fn docx_text_is_extracted_from_zip() {
        let env = test_env();
        let document_xml = b"<w:document><w:body><w:p><w:r><w:t>Hello</w:t></w:r>\
             <w:r><w:t> world</w:t></w:r></w:p><w:p><w:r><w:t>2nd para</w:t></w:r></w:p>\
             </w:body></w:document>";
        let docx = build_docx(document_xml);
        seed_attachment(
            &env,
            "s1",
            "a.docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            &docx,
        );
        let out = build_attachment_context(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "s1",
            &[seed_id(&env, "s1", "a.docx")],
            false,
        )
        .await
        .unwrap();
        assert!(out.text_blocks[0].contains("Hello world"));
        assert!(out.text_blocks[0].contains("2nd para"));
    }

    #[tokio::test]
    async fn legacy_office_mimes_get_placeholder() {
        let env = test_env();
        seed_attachment(
            &env,
            "s1",
            "old.doc",
            "application/msword",
            b"\xd0\xcf junk",
        );
        let out = build_attachment_context(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "s1",
            &[seed_id(&env, "s1", "old.doc")],
            false,
        )
        .await
        .unwrap();
        assert!(out.text_blocks[0].contains("无法解析出文本内容"));
    }

    #[tokio::test]
    async fn image_with_vision_goes_multimodal() {
        let env = test_env();
        let png = tiny_png();
        seed_attachment(&env, "s1", "shot.png", "image/png", &png);
        let out = build_attachment_context(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "s1",
            &[seed_id(&env, "s1", "shot.png")],
            true,
        )
        .await
        .unwrap();
        assert_eq!(out.image_parts.len(), 1);
        assert_eq!(out.image_parts[0].mime_type, "image/png");
        assert!(!out.image_parts[0].base64.is_empty());
        assert!(out.text_blocks.is_empty());
    }

    #[tokio::test]
    async fn image_without_vision_degrades_to_description() {
        let env = test_env();
        seed_attachment(&env, "s1", "shot.png", "image/png", &tiny_png());
        let out = build_attachment_context(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "s1",
            &[seed_id(&env, "s1", "shot.png")],
            false,
        )
        .await
        .unwrap();
        assert!(out.image_parts.is_empty());
        let block = &out.text_blocks[0];
        assert!(block.contains("[图片 shot.png]"));
        assert!(block.contains("2x2"));
        assert!(block.contains("以描述方式理解图片"));
    }

    #[tokio::test]
    async fn missing_file_reports_attachment_name() {
        let env = test_env();
        let att = seed_attachment(&env, "s1", "gone.md", "text/markdown", b"bye");
        std::fs::remove_file(env.workspace_root.join(&att.rel_path)).unwrap();
        let err = build_attachment_context(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "s1",
            &[att.id],
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err.name, "gone.md");
        assert!(err.reason.contains("移动或删除"));
    }

    #[tokio::test]
    async fn tampered_file_fails_checksum() {
        let env = test_env();
        let att = seed_attachment(&env, "s1", "tampered.md", "text/markdown", b"original");
        let disk = env.workspace_root.join(&att.rel_path);
        std::fs::write(&disk, b"tampered content").unwrap();
        let err = build_attachment_context(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "s1",
            &[att.id],
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err.name, "tampered.md");
        assert!(err.reason.contains("校验失败"));
    }

    #[tokio::test]
    async fn foreign_session_attachment_is_rejected() {
        let env = test_env();
        seed_attachment(&env, "s1", "mine.md", "text/markdown", b"mine");
        let att_id = seed_id(&env, "s1", "mine.md");
        let err = build_attachment_context(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "s2",
            &[att_id],
            false,
        )
        .await
        .unwrap_err();
        assert!(err.reason.contains("不属于当前会话"));
    }

    #[tokio::test]
    async fn unknown_id_reports_id_as_name() {
        let env = test_env();
        let err = build_attachment_context(
            env.db_path.clone(),
            env.workspace_root.clone(),
            "s1",
            &["no-such-id".to_string()],
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(err.name, "no-such-id");
    }

    // ---- helpers ----

    fn seed_id(env: &TestEnv, session_id: &str, name: &str) -> String {
        let db = db_conn(env);
        attachments::list_by_session(&db.0, session_id)
            .unwrap()
            .into_iter()
            .find(|a| a.name == name)
            .map(|a| a.id)
            .unwrap()
    }

    /// Minimal 2x2 PNG (IHDR + IDAT + IEND).
    fn tiny_png() -> Vec<u8> {
        [
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, // signature
            0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R', // IHDR chunk
            0x00, 0x00, 0x00, 0x02, // width = 2
            0x00, 0x00, 0x00, 0x02, // height = 2
            0x08, 0x02, 0x00, 0x00, 0x00, // bit depth / color type / flags
        ]
        .to_vec()
    }

    /// Wraps `document_xml` into a zip with a `word/document.xml` entry.
    fn build_docx(document_xml: &[u8]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options: zip::write::SimpleFileOptions = Default::default();
            zip.start_file("word/document.xml", options).unwrap();
            zip.write_all(document_xml).unwrap();
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn xml_entity_decoding() {
        assert_eq!(decode_xml_entities("a&amp;b"), "a&b");
        assert_eq!(decode_xml_entities("&lt;t&gt;"), "<t>");
        assert_eq!(decode_xml_entities("&#20013;&#25991;"), "中文");
        assert_eq!(decode_xml_entities("&#x4E2D;"), "中");
        assert_eq!(
            decode_xml_entities("unknown &entity; stays"),
            "unknown &entity; stays"
        );
        assert_eq!(decode_xml_entities("dangling &"), "dangling &");
    }

    #[test]
    fn image_dimensions_cover_common_formats() {
        assert_eq!(image_dimensions(&tiny_png()), Some((2, 2)));
        assert_eq!(
            image_dimensions(&[b'G', b'I', b'F', b'8', b'9', b'a', 0x20, 0x00, 0x10, 0x00]),
            Some((32, 16))
        );
        // RIFF/WEBP/VP8X canvas 1x1 stored as 0.
        assert_eq!(
            image_dimensions(&[
                b'R', b'I', b'F', b'F', 0, 0, 0, 0, b'W', b'E', b'B', b'P', b'V', b'P', b'8', b'X',
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
            ]),
            Some((1, 1))
        );
        // JPEG with SOF0: height 2, width 3.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, // SOI + APP0(len 4)
            0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x02, 0x00, 0x03, 0x01, 0x11, 0x00,
        ];
        assert_eq!(image_dimensions(&jpeg), Some((3, 2)));
        assert_eq!(image_dimensions(b"garbage"), None);
    }

    #[test]
    fn xml_tag_texts_splits_paragraphs() {
        let xml = "<w:p><w:r><w:t>one</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r></w:p>";
        assert_eq!(xml_tag_texts(xml, "w:t", "</w:p>"), "one\ntwo");
        let xml_attr = "<w:t xml:space=\"preserve\">a b</w:t>";
        assert_eq!(xml_tag_texts(xml_attr, "w:t", "</w:p>"), "a b");
    }
}
