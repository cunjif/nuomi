//! Approval gate (SPEC ui-m1 D8 / AC2): a configurable sensitive-tool list
//! intercepts `PreToolCall`; a hit creates a pending approval row and the
//! caller pauses the loop until a human resolves it.
//!
//! Persistence reuses the `memory_entries` table for the policy
//! (`kind="setting"`, marker-prefixed content — same pattern as
//! `evolution::research`) and the dedicated `approvals` table (migration 0002)
//! for decisions. Every request/decision is mirrored into the append-only
//! `events` log under aggregate `"run"`.

use std::sync::Arc;

use crate::domain::{new_id, now_ms, Approval, ApprovalDecision};
use crate::plugins::MemoryService;
use crate::store::{migrations, repos, StoreError};

use super::tools::ToolRegistry;

const POLICY_TAG: &str = "setting";
pub const SENSITIVE_TOOLS_MARKER: &str = "approval_sensitive_tools";

/// Result of a gated `PreToolCall` check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Allow,
    /// Tool is sensitive; `approval_id` identifies the pending row to resolve.
    RequireApproval {
        approval_id: String,
    },
}

/// The configured sensitive-tool list. Patterns are exact tool names or
/// wildcard prefixes ending in `*` (e.g. `"fs.write"`, `"git.*"`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SensitiveToolPolicy {
    pub patterns: Vec<String>,
}

impl SensitiveToolPolicy {
    pub fn new(patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            patterns: patterns.into_iter().map(Into::into).collect(),
        }
    }

    pub fn matches(&self, tool_name: &str) -> bool {
        self.patterns.iter().any(|p| match p.strip_suffix('*') {
            Some(prefix) => tool_name.starts_with(prefix),
            None => p == tool_name,
        })
    }
}

/// Persists the sensitive-tool list (`memory_entries`, newest entry wins).
/// Hot effect: every subsequent [`ApprovalGate::check`] reads this fresh.
pub async fn set_sensitive_tools(
    mem: &MemoryService,
    patterns: &[String],
) -> Result<(), StoreError> {
    mem.remember(
        format!(
            "{SENSITIVE_TOOLS_MARKER}={}",
            serde_json::to_string(patterns)?
        ),
        None,
        vec![POLICY_TAG.to_string()],
        "setting",
        false,
    )
    .await
    .map(|_| ())
}

/// Reads the persisted policy; `None` when nothing was ever stored.
pub async fn read_sensitive_tools(
    mem: &MemoryService,
) -> Result<Option<SensitiveToolPolicy>, StoreError> {
    let hits = mem
        .recall(
            Some(SENSITIVE_TOOLS_MARKER.to_string()),
            Some(POLICY_TAG.to_string()),
            10,
        )
        .await?;
    let prefix = format!("{SENSITIVE_TOOLS_MARKER}=");
    Ok(hits.into_iter().find_map(|m| {
        m.content
            .strip_prefix(&prefix)
            .and_then(|json| serde_json::from_str::<Vec<String>>(json).ok())
            .map(SensitiveToolPolicy::new)
    }))
}

/// Per-run gate. `fallback_policy` applies until a policy has been persisted
/// via [`set_sensitive_tools`]; once stored, the persisted list always wins
/// and takes effect immediately (hot reload by construction).
#[derive(Clone)]
pub struct ApprovalGate {
    db_path: Arc<str>,
    run_id: String,
    fallback_policy: Arc<SensitiveToolPolicy>,
}

impl ApprovalGate {
    pub fn new(
        db_path: impl Into<Arc<str>>,
        run_id: impl Into<String>,
        fallback_policy: SensitiveToolPolicy,
    ) -> Self {
        Self {
            db_path: db_path.into(),
            run_id: run_id.into(),
            fallback_policy: Arc::new(fallback_policy),
        }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    fn mem(&self) -> MemoryService {
        MemoryService::new(self.db_path.clone())
    }

    /// The PreToolCall check. Sensitive calls create a pending `approvals`
    /// row plus an `approval_requested` event BEFORE the caller pauses.
    pub async fn check(
        &self,
        tool_name: &str,
        args_json: &serde_json::Value,
    ) -> Result<GateDecision, StoreError> {
        let policy = match read_sensitive_tools(&self.mem()).await? {
            Some(persisted) => persisted,
            None => (*self.fallback_policy).clone(),
        };
        if !policy.matches(tool_name) {
            return Ok(GateDecision::Allow);
        }

        let now = now_ms();
        let approval = Approval {
            id: new_id(),
            run_id: self.run_id.clone(),
            tool_name: tool_name.to_string(),
            arguments_json: serde_json::to_string(args_json)?,
            decision: ApprovalDecision::Pending,
            decided_at: None,
            created_at: now,
        };
        let db_path = self.db_path.clone();
        let row = approval.clone();
        let payload = serde_json::json!({
            "approval_id": row.id,
            "tool": tool_name,
            "arguments": args_json,
        });
        tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
            let db = crate::store::Db::open(&db_path)?;
            migrations::run(&db.0)?;
            repos::tasks_runs::insert_approval(&db.0, &row)?;
            repos::events::append(
                &db.0,
                "run",
                &row.run_id,
                "approval_requested",
                &payload,
                now,
            )?;
            Ok(())
        })
        .await
        .map_err(join_to_store_error)??;

        Ok(GateDecision::RequireApproval {
            approval_id: approval.id,
        })
    }
}

/// Resolves a pending approval: writes `decision` + `decided_at` (guarded —
/// a decision can never be flipped) and appends an `approval_resolved` event.
pub async fn resolve_approval(
    db_path: impl Into<Arc<str>>,
    approval_id: &str,
    outcome: crate::domain::ApprovalOutcome,
) -> Result<(), StoreError> {
    let db_path = db_path.into();
    let id = approval_id.to_string();
    let now = now_ms();
    tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
        let db = crate::store::Db::open(&db_path)?;
        migrations::run(&db.0)?;
        let conn = &db.0;
        repos::tasks_runs::resolve_approval(conn, &id, outcome_decision(outcome), now)?;
        let approval = repos::tasks_runs::get_approval(conn, &id)?;
        repos::events::append(
            conn,
            "run",
            &approval.run_id,
            "approval_resolved",
            &serde_json::json!({
                "approval_id": id,
                "outcome": outcome.as_str(),
            }),
            now,
        )?;
        Ok(())
    })
    .await
    .map_err(join_to_store_error)??;
    Ok(())
}

fn outcome_decision(outcome: crate::domain::ApprovalOutcome) -> ApprovalDecision {
    match outcome {
        crate::domain::ApprovalOutcome::Approved => ApprovalDecision::Approved,
        crate::domain::ApprovalOutcome::Denied => ApprovalDecision::Denied,
    }
}

/// Executes `tool_name` through the gate. On `RequireApproval` the caller is
/// suspended at `await_decision(approval_id)` — that future is where the
/// orchestrator parks the loop after persisting its `state_changed` event
/// (iron rule) — and resumes according to the returned outcome:
/// approved → tool runs; denied → `"[denied by approval]"`.
///
/// `await_decision` implementations are expected to call
/// [`resolve_approval`] (the IPC approve/deny handler does exactly that)
/// before returning the outcome.
pub async fn execute_with_gate<F, Fut>(
    gate: &ApprovalGate,
    tools: &ToolRegistry,
    tool_name: &str,
    args_json: &serde_json::Value,
    await_decision: F,
) -> Result<String, StoreError>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = crate::domain::ApprovalOutcome>,
{
    match gate.check(tool_name, args_json).await? {
        GateDecision::Allow => tools
            .execute(tool_name, args_json)
            .await
            .map_err(|e| StoreError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))),
        GateDecision::RequireApproval { approval_id } => match await_decision(approval_id).await {
            crate::domain::ApprovalOutcome::Approved => {
                tools.execute(tool_name, args_json).await.map_err(|e| {
                    StoreError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                })
            }
            crate::domain::ApprovalOutcome::Denied => Ok("[denied by approval]".to_string()),
        },
    }
}

fn join_to_store_error(e: tokio::task::JoinError) -> StoreError {
    StoreError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{new_id, now_ms, ApprovalOutcome, Run, RunState, Task, TaskStatus};
    use crate::harness::HarnessError;
    use crate::plugins::tools::Tool;
    use crate::providers::ToolDef;
    use crate::store::Db;
    use async_trait::async_trait;
    use serde_json::json;

    struct WriteTool;

    #[async_trait]
    impl Tool for WriteTool {
        fn def(&self) -> ToolDef {
            ToolDef {
                name: "fs.write".into(),
                description: "writes a file".into(),
                parameters: json!({}),
            }
        }
        async fn execute(&self, _args: &serde_json::Value) -> Result<String, HarnessError> {
            Ok("file written".into())
        }
    }

    /// Seeds migrations + one running run row (FK target for approvals).
    /// Returns (db_path, _tempdir_guard, run_id).
    async fn env() -> (Arc<str>, tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let db_file = dir.path().join("gate.db");
        let conn = rusqlite::Connection::open(&db_file).unwrap();
        migrations::run(&conn).unwrap();
        let task = Task {
            id: new_id(),
            session_id: None,
            title: "t".into(),
            description: String::new(),
            status: TaskStatus::Running,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        repos::tasks_runs::insert_task(&conn, &task).unwrap();
        let run = Run {
            id: new_id(),
            task_id: task.id.clone(),
            session_id: "s1".into(),
            status: RunState::Running,
            heartbeat_at: now_ms(),
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        repos::tasks_runs::insert_run(&conn, &run).unwrap();
        let db_path: Arc<str> = Arc::from(db_file.to_string_lossy().to_string());
        (db_path.clone(), dir, run.id)
    }

    fn open(path: &str) -> Db {
        Db::open(path).unwrap()
    }

    #[test]
    fn policy_matches_exact_wildcard_and_misses() {
        let table: &[(&str, SensitiveToolPolicy, bool)] = &[
            ("fs.write", SensitiveToolPolicy::new(["fs.write"]), true),
            ("fs.read", SensitiveToolPolicy::new(["fs.write"]), false),
            ("git.push", SensitiveToolPolicy::new(["git.*"]), true),
            ("git", SensitiveToolPolicy::new(["git.*"]), false), // wildcard includes the dot
            ("hub.push", SensitiveToolPolicy::new(["git.*"]), false),
            (
                "anything",
                SensitiveToolPolicy::new(Vec::<String>::new()),
                false,
            ),
        ];
        for (tool, policy, expected) in table {
            assert_eq!(policy.matches(tool), *expected, "policy {policy:?}");
        }
    }

    #[tokio::test]
    async fn allow_when_tool_not_sensitive() {
        let (db_path, _dir, run_id) = env().await;
        let gate = ApprovalGate::new(
            db_path.clone(),
            run_id.clone(),
            SensitiveToolPolicy::default(),
        );
        assert_eq!(
            gate.check("fs.read", &json!({})).await.unwrap(),
            GateDecision::Allow
        );
        let conn = open(&db_path).0;
        assert!(repos::tasks_runs::list_pending_approvals(&conn)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn sensitive_check_creates_pending_row_and_event() {
        let (db_path, _dir, run_id) = env().await;
        let gate = ApprovalGate::new(
            db_path.clone(),
            run_id.clone(),
            SensitiveToolPolicy::new(["fs.write"]),
        );
        let decision = gate
            .check("fs.write", &json!({"path":"a.txt"}))
            .await
            .unwrap();
        let approval_id = match decision {
            GateDecision::RequireApproval { approval_id } => approval_id,
            other => panic!("expected RequireApproval, got {other:?}"),
        };

        let conn = open(&db_path).0;
        let row = repos::tasks_runs::get_approval(&conn, &approval_id).unwrap();
        assert_eq!(row.decision, ApprovalDecision::Pending);
        assert_eq!(row.tool_name, "fs.write");
        assert_eq!(row.run_id, run_id);

        let events = repos::events::list_by_aggregate(&conn, "run", &run_id, None).unwrap();
        assert!(events.iter().any(|ev| ev.kind == "approval_requested"
            && ev.payload["approval_id"] == approval_id.as_str()));
    }

    #[tokio::test]
    async fn policy_is_hot_effective_after_set() {
        let (db_path, _dir, run_id) = env().await;
        let mem = MemoryService::new(db_path.clone());
        let gate = ApprovalGate::new(
            db_path.clone(),
            run_id.clone(),
            SensitiveToolPolicy::default(),
        );

        // Nothing sensitive yet.
        assert_eq!(
            gate.check("net.fetch", &json!({})).await.unwrap(),
            GateDecision::Allow
        );

        // Set lands in storage and the very next check sees it — no restart.
        set_sensitive_tools(&mem, &["net.*".to_string(), "fs.write".to_string()])
            .await
            .unwrap();
        assert!(matches!(
            gate.check("net.fetch", &json!({})).await.unwrap(),
            GateDecision::RequireApproval { .. }
        ));

        // A brand-new gate instance over the same db sees it too.
        let gate2 = ApprovalGate::new(
            db_path.clone(),
            run_id.clone(),
            SensitiveToolPolicy::default(),
        );
        assert!(matches!(
            gate2.check("fs.write", &json!({})).await.unwrap(),
            GateDecision::RequireApproval { .. }
        ));
        assert_eq!(
            gate2.check("fs.read", &json!({})).await.unwrap(),
            GateDecision::Allow
        );

        // Persisted roundtrip of read_sensitive_tools.
        let policy = read_sensitive_tools(&mem).await.unwrap().unwrap();
        assert_eq!(
            policy.patterns,
            vec!["net.*".to_string(), "fs.write".to_string()]
        );
    }

    #[tokio::test]
    async fn approve_pauses_then_resumes_execution_with_iron_rule_ordering() {
        let (db_path, _dir, run_id) = env().await;
        let tools = Arc::new(crate::plugins::ToolRegistry::new());
        tools.register(Arc::new(WriteTool)).await.unwrap();
        let gate = ApprovalGate::new(
            db_path.clone(),
            run_id.clone(),
            SensitiveToolPolicy::new(["fs.write"]),
        );

        // Pause point: the loop parks here while the human decides.
        let notify_paused = Arc::new(tokio::sync::Notify::new());
        let wait_paused = notify_paused.clone();
        let resume = Arc::new(tokio::sync::Notify::new());
        let resume_for_waiter = resume.clone();

        let loop_task = tokio::spawn({
            let gate = gate.clone();
            let tools = tools.clone();
            let db_path = db_path.clone();
            let run_id = run_id.clone();
            async move {
                execute_with_gate(&gate, &tools, "fs.write", &json!({"path":"a.txt"}), {
                    move |approval_id| {
                        let wait_paused = wait_paused.clone();
                        let resume = resume_for_waiter.clone();
                        let db_path = db_path.clone();
                        let run_id = run_id.clone();
                        async move {
                            // IRON RULE: persist state_changed + transition BEFORE pausing.
                            let conn = open(&db_path).0;
                            repos::events::append(
                                &conn,
                                "run",
                                &run_id,
                                "state_changed",
                                &json!({"from": "running", "to": "awaiting_approval"}),
                                now_ms(),
                            )
                            .unwrap();
                            repos::tasks_runs::update_run_status(
                                &conn,
                                &run_id,
                                RunState::Running,
                                RunState::AwaitingApproval,
                                now_ms(),
                            )
                            .unwrap();

                            wait_paused.notify_one(); // loop is now parked
                            resume.notified().await; // ...until the human decides

                            let conn = open(&db_path).0;
                            let row = repos::tasks_runs::get_approval(&conn, &approval_id).unwrap();
                            let outcome = if row.decision == ApprovalDecision::Approved {
                                ApprovalOutcome::Approved
                            } else {
                                ApprovalOutcome::Denied
                            };

                            // IRON RULE again: persist state_changed BEFORE resuming work.
                            repos::events::append(
                                &conn,
                                "run",
                                &run_id,
                                "state_changed",
                                &json!({"from": "awaiting_approval", "to": "running"}),
                                now_ms(),
                            )
                            .unwrap();
                            repos::tasks_runs::update_run_status(
                                &conn,
                                &run_id,
                                RunState::AwaitingApproval,
                                RunState::Running,
                                now_ms(),
                            )
                            .unwrap();
                            outcome
                        }
                    }
                })
                .await
                .unwrap()
            }
        });

        notify_paused.notified().await;
        {
            // While parked: run row shows awaiting_approval, approval pending.
            let conn = open(&db_path).0;
            let run = repos::tasks_runs::get_run(&conn, &run_id).unwrap();
            assert_eq!(run.status, RunState::AwaitingApproval);
            assert_eq!(
                repos::tasks_runs::list_pending_approvals(&conn)
                    .unwrap()
                    .len(),
                1
            );
        }

        // Human approves: persist decision first, then wake the loop.
        resolve_approval(
            db_path.clone(),
            pending_id(&db_path).as_str(),
            ApprovalOutcome::Approved,
        )
        .await
        .unwrap();
        resume.notify_one();

        let result = tokio::join!(loop_task).0.unwrap();
        assert_eq!(result, "file written");

        // Back to running, and event order proves event-before-side-effect.
        let conn = open(&db_path).0;
        let run = repos::tasks_runs::get_run(&conn, &run_id).unwrap();
        assert_eq!(run.status, RunState::Running);
        let events = repos::events::list_by_aggregate(&conn, "run", &run_id, None).unwrap();
        let kinds: Vec<&str> = events.iter().map(|ev| ev.kind.as_str()).collect();
        let requested = kinds
            .iter()
            .position(|k| *k == "approval_requested")
            .unwrap();
        let changed = kinds.iter().position(|k| *k == "state_changed").unwrap();
        let resolved = kinds
            .iter()
            .position(|k| *k == "approval_resolved")
            .unwrap();
        assert!(requested < changed && changed < resolved);
    }

    #[tokio::test]
    async fn deny_returns_denied_result_and_loop_continues() {
        let (db_path, _dir, run_id) = env().await;
        let tools = Arc::new(crate::plugins::ToolRegistry::new());
        tools.register(Arc::new(WriteTool)).await.unwrap();
        let gate = ApprovalGate::new(
            db_path.clone(),
            run_id.clone(),
            SensitiveToolPolicy::new(["fs.write"]),
        );

        let result = execute_with_gate(&gate, &tools, "fs.write", &json!({}), |approval_id| {
            let db_path = db_path.clone();
            async move {
                // Deny path: resolve then report the outcome back to the loop.
                resolve_approval(db_path, &approval_id, ApprovalOutcome::Denied)
                    .await
                    .unwrap();
                ApprovalOutcome::Denied
            }
        })
        .await
        .unwrap();
        assert_eq!(result, "[denied by approval]");

        // The decision is final in storage.
        let conn = open(&db_path).0;
        let rows = repos::tasks_runs::list_approvals_by_run(&conn, &run_id).unwrap();
        assert_eq!(rows[0].decision, ApprovalDecision::Denied);
        assert!(rows[0].decided_at.is_some());
    }

    #[tokio::test]
    async fn resolve_is_guarded_against_double_decision() {
        let (db_path, _dir, run_id) = env().await;
        let gate = ApprovalGate::new(
            db_path.clone(),
            run_id.clone(),
            SensitiveToolPolicy::new(["fs.write"]),
        );
        let id = match gate.check("fs.write", &json!({})).await.unwrap() {
            GateDecision::RequireApproval { approval_id } => approval_id,
            other => panic!("unexpected {other:?}"),
        };
        resolve_approval(db_path.clone(), &id, ApprovalOutcome::Approved)
            .await
            .unwrap();
        // Second resolve must fail (guarded UPDATE), not silently flip.
        assert!(
            resolve_approval(db_path.clone(), &id, ApprovalOutcome::Denied)
                .await
                .is_err()
        );
    }

    fn pending_id(db_path: &str) -> String {
        let conn = open(db_path).0;
        repos::tasks_runs::list_pending_approvals(&conn).unwrap()[0]
            .id
            .clone()
    }
}
