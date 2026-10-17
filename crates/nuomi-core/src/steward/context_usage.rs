//! Context usage aggregation for the steward workbench (task 4.5, design
//! §2.2.2-A3): the data source behind the usage indicator and the prune
//! popup. Read-only, no side effects.
//!
//! Estimation mirrors `prune_view` (`providers/context.rs`): byte length
//! divided by 4 (CHARS_PER_TOKEN). Sections:
//! - `SystemPrompt` — SOUL persona override + the steward base prompt
//!   (what the next send would assemble; the snapshot-unavailable note is
//!   a one-liner and not counted);
//! - `History` — the persisted session history (`message_received` /
//!   `message_sent` events rebuilt via `list_session_messages`, same
//!   HISTORY_LIMIT window the loop uses);
//! - `ToolResults` — accumulated `steward.tool_result` event payloads of
//!   the session. Tool messages live only inside an agent loop (never in
//!   the rebuilt history), so this section reports the session-wide tool
//!   result volume the prune pressure comes from.
//!
//! Prune records read `steward.context_pruned` events (when the prune
//! happened and what it collapsed — PruneStats snapshot payload).

use std::sync::Arc;

use crate::steward::events::{self, StewardEventKind};
use crate::steward::kernel::{self, HISTORY_LIMIT};
use crate::steward::llm_loop::{CONTEXT_BUDGET_TOKENS, STEWARD_BASE_PROMPT};
use crate::steward::settings;
use crate::steward::StewardError;
use crate::store::Db;
use tokio::task::spawn_blocking;

/// Cap on tool-result / prune-record event rows scanned per query.
const EVENT_SCAN_LIMIT: u32 = 500;

#[derive(Debug)]
pub struct ContextSectionUsage {
    /// `SystemPrompt` | `History` | `ToolResults`.
    pub name: String,
    pub tokens: i64,
}

#[derive(Debug)]
pub struct PruneRecord {
    /// Event `created_at` (ms since epoch) — when the prune happened.
    pub occurred_at: i64,
    pub budget_tokens: i64,
    pub approx_tokens: i64,
    pub duplicates_collapsed: i64,
    pub results_summarized: i64,
    pub folded_messages: i64,
}

#[derive(Debug)]
pub struct StewardContextUsage {
    pub sections: Vec<ContextSectionUsage>,
    pub total_tokens: i64,
    pub budget_tokens: i64,
    /// Oldest first (append-only event order).
    pub prune_records: Vec<PruneRecord>,
}

/// Aggregates the context usage of one steward session.
/// Returns `StewardError::NotFound` when the session does not exist.
pub async fn get_context_usage(
    db_path: Arc<str>,
    session_id: &str,
) -> Result<StewardContextUsage, StewardError> {
    // Session existence check rides list_session_messages (NotFound).
    let history =
        kernel::list_session_messages(Arc::clone(&db_path), session_id, HISTORY_LIMIT).await?;

    let session_id = session_id.to_string();
    let tool_records = spawn_blocking(move || -> Result<_, StewardError> {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        let tools = events::list(
            conn,
            Some(&session_id),
            Some("steward.tool_result"),
            EVENT_SCAN_LIMIT,
        )?;
        let prunes = events::list(
            conn,
            Some(&session_id),
            Some(StewardEventKind::ContextPruned.as_str()),
            EVENT_SCAN_LIMIT,
        )?;
        let soul = settings::read_soul_override(conn);
        Ok((tools, prunes, soul))
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))??;

    let (tool_records, prune_records_raw, soul) = tool_records;

    let system_prompt_len = soul.map(|s| s.len()).unwrap_or(0) + STEWARD_BASE_PROMPT.len();
    let history_len: usize = history.iter().map(|m| m.text.len()).sum();
    let tool_results_len: usize = tool_records
        .iter()
        .map(|e| e.payload.to_string().len())
        .sum();

    let sections = vec![
        ContextSectionUsage {
            name: "SystemPrompt".into(),
            tokens: approx_tokens_len(system_prompt_len),
        },
        ContextSectionUsage {
            name: "History".into(),
            tokens: approx_tokens_len(history_len),
        },
        ContextSectionUsage {
            name: "ToolResults".into(),
            tokens: approx_tokens_len(tool_results_len),
        },
    ];
    let total_tokens: i64 = sections.iter().map(|s| s.tokens).sum();

    let mut prune_records = Vec::with_capacity(prune_records_raw.len());
    for e in &prune_records_raw {
        let p = &e.payload;
        prune_records.push(PruneRecord {
            occurred_at: e.created_at,
            budget_tokens: p.get("budget_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            approx_tokens: p.get("approx_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            duplicates_collapsed: p
                .get("duplicates_collapsed")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            results_summarized: p
                .get("results_summarized")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            folded_messages: p
                .get("folded_messages")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        });
    }

    Ok(StewardContextUsage {
        sections,
        total_tokens,
        budget_tokens: CONTEXT_BUDGET_TOKENS as i64,
        prune_records,
    })
}

fn approx_tokens_len(len: usize) -> i64 {
    (len / 4) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steward::events::{log, StewardEventKind};
    use crate::store::migrations;
    use serde_json::json;

    async fn db_path() -> Arc<str> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.db");
        let path_str = path.to_string_lossy().to_string();
        let db = Db::open(&path_str).unwrap();
        migrations::run(&db.0).unwrap();
        drop(db);
        // keep tempdir alive for the process lifetime (tests are short)
        std::mem::forget(dir);
        Arc::from(path_str)
    }

    fn seed_steward_session(conn: &rusqlite::Connection, id: &str) {
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at, kind)
             VALUES (?1, 'test', 1, 1, 'background')",
            [id],
        )
        .unwrap();
        crate::store::repos::steward::insert_steward_session(
            conn,
            &crate::domain::StewardSession {
                id: id.into(),
                steward_id: "steward".into(),
                title: "test".into(),
                goal: None,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
    }

    #[tokio::test]
    async fn unknown_session_is_not_found() {
        let dbp = db_path().await;
        let err = get_context_usage(dbp, "nope").await.unwrap_err();
        assert!(matches!(err, StewardError::NotFound { .. }));
    }

    #[tokio::test]
    async fn sections_aggregate_and_total_matches() {
        let dbp = db_path().await;
        {
            let db = Db::open(&dbp).unwrap();
            seed_steward_session(&db.0, "s1");
            log(
                &db.0,
                "s1",
                StewardEventKind::MessageReceived,
                &json!({"session_id": "s1", "text": "hello world!"}),
            )
            .unwrap();
            crate::store::repos::events::append(
                &db.0,
                "steward",
                "s1",
                "steward.message_sent",
                &json!({"session_id": "s1", "text": "hi there, steward back"}),
                crate::domain::now_ms(),
            )
            .unwrap();
            log(
                &db.0,
                "s1",
                StewardEventKind::ToolResult,
                &json!({"tool_call_id": "c1", "ok": true, "suspended": false, "detail": "x".repeat(40)}),
            )
            .unwrap();
        }

        let usage = get_context_usage(dbp, "s1").await.unwrap();
        assert_eq!(usage.sections.len(), 3);
        let by = |name: &str| -> i64 {
            usage
                .sections
                .iter()
                .find(|s| s.name == name)
                .map(|s| s.tokens)
                .unwrap()
        };
        // SystemPrompt = base prompt (no soul override seeded) — chars/4.
        assert_eq!(by("SystemPrompt"), (STEWARD_BASE_PROMPT.len() / 4) as i64);
        // History = "hello world!" + "hi there, steward back" byte lens / 4.
        let expected_history = (("hello world!".len() + "hi there, steward back".len()) / 4) as i64;
        assert_eq!(by("History"), expected_history);
        // ToolResults = tool_result payload JSON length / 4 (> 0 here).
        assert!(by("ToolResults") > 0);
        assert_eq!(
            usage.total_tokens,
            by("SystemPrompt") + by("History") + by("ToolResults")
        );
        assert_eq!(usage.budget_tokens, CONTEXT_BUDGET_TOKENS as i64);
        assert!(usage.prune_records.is_empty());
    }

    #[tokio::test]
    async fn prune_records_read_context_pruned_events() {
        let dbp = db_path().await;
        {
            let db = Db::open(&dbp).unwrap();
            seed_steward_session(&db.0, "s2");
            log(
                &db.0,
                "s2",
                StewardEventKind::ContextPruned,
                &json!({
                    "budget_tokens": 32768,
                    "approx_tokens": 31000,
                    "duplicates_collapsed": 2,
                    "results_summarized": 3,
                    "folded_messages": 5,
                }),
            )
            .unwrap();
            // An unrelated steward event on the same session must not leak in.
            log(
                &db.0,
                "s2",
                StewardEventKind::ToolCall,
                &json!({"name": "read_app_state"}),
            )
            .unwrap();
            // A prune on another session must not leak in.
            log(
                &db.0,
                "s9",
                StewardEventKind::ContextPruned,
                &json!({"budget_tokens": 1, "approx_tokens": 1, "duplicates_collapsed": 0,
                        "results_summarized": 0, "folded_messages": 0}),
            )
            .unwrap();
        }

        let usage = get_context_usage(dbp, "s2").await.unwrap();
        assert_eq!(usage.prune_records.len(), 1);
        let rec = &usage.prune_records[0];
        assert_eq!(rec.budget_tokens, 32768);
        assert_eq!(rec.approx_tokens, 31000);
        assert_eq!(rec.duplicates_collapsed, 2);
        assert_eq!(rec.results_summarized, 3);
        assert_eq!(rec.folded_messages, 5);
        assert!(rec.occurred_at > 0);
    }
}
