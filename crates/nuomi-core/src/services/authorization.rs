//! File-operation authorization core (chat_rich_bind spec 5.4/6.2, design
//! §2.1.3.1): RoleAgents read/search freely INSIDE the session's bound work
//! directory; write/delete (and, conservatively, reads OUTSIDE the bound
//! directory — D7) raise an in-bubble authorization request the user must
//! resolve (grant_once / grant_session / deny). Group chats attribute each
//! request to the RoleAgent that raised it — never to bystanders.
//!
//! Iron rule: every state transition appends its `authorization.*` event and
//! updates the `file_authorizations` row inside ONE transaction; external
//! side effects (executing the tool, waking the loop) happen only after
//! commit. Wakers are tokio oneshots held in a process-global hub so the IPC
//! resolve command (shell layer) can reach a pending turn's loop.
//!
//! D4 boundary: only HOST-executed tools (native-provider loop engine) pass
//! through here; CLI agents execute inside their own subprocess and never
//! surface authorization bubbles this release.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::oneshot;

use crate::store::repos::agent_auth_policies::{self, AgentAuthPolicyValue};
use crate::store::repos::file_authorizations::{
    self, AuthorizationOp, AuthorizationState, FileAuthorizationEntry,
};
use crate::store::repos::sessions;
use crate::store::repos::settings;
use crate::store::{migrations, repos, Db, StoreError};

/// Host tool names whose execution counts as a write/delete (intercepted
/// wherever the path lands). The registry is deployment-populated; matching
/// stays exact-name (no wildcards — the set is ours, not user config).
/// Process-global hub singleton (same pattern as `integrations::CLIENT`):
/// the per-turn gates and the IPC resolvers must share one waiter table,
/// and the shell AppState has no other injection path into the kernel's
/// turn execution. One process = one app = one hub.
static GLOBAL_HUB: OnceLock<Arc<AuthorizationHub>> = OnceLock::new();

/// The process-wide authorization hub.
pub fn global_hub() -> &'static Arc<AuthorizationHub> {
    GLOBAL_HUB.get_or_init(AuthorizationHub::new)
}

pub const WRITE_DELETE_TOOLS: &[&str] = &[
    "write_file",
    "create_file",
    "delete_file",
    "fs.write",
    "fs.delete",
];

/// Host read/search tools: free inside the bound directory, authorized
/// outside it (D7 conservative).
pub const READ_TOOLS: &[&str] = &[
    "read_file",
    "search_files",
    "list_dir",
    "glob",
    "grep",
    "fs.read",
    "fs.search",
];

/// App-settings key for the timeout (default 120s, valid range 30–600).
pub const TIMEOUT_SETTING_KEY: &str = "authorization.timeout_seconds";
pub const TIMEOUT_DEFAULT_SECS: i64 = 120;
pub const TIMEOUT_MIN_SECS: i64 = 30;
pub const TIMEOUT_MAX_SECS: i64 = 600;
/// Sweeper cadence: 5s — ≈4% error on the 120s default (spec 4.1-6).
pub const SWEEP_INTERVAL_MS: u64 = 5_000;

/// User decision on a pending authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationDecision {
    GrantOnce,
    GrantSession,
    Deny,
}

impl AuthorizationDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            AuthorizationDecision::GrantOnce => "grant_once",
            AuthorizationDecision::GrantSession => "grant_session",
            AuthorizationDecision::Deny => "deny",
        }
    }
}

/// Outcome handed back to the loop engine after authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateOutcome {
    /// No authorization needed (non-gated tool, in-bound read, cache hit).
    Allow,
    /// The user granted — execute this call.
    Grant,
    /// The user denied (or the request was suppressed as a repeat).
    Deny { reason: String },
}

/// Process-global coordination state: pending waiters (oneshot senders the
/// IPC resolver or the sweeper fires), session-grant cache, and per-session
/// denial suppression. Held by the shell AppState; every per-turn gate
/// clones the Arc.
#[derive(Default)]
pub struct AuthorizationHub {
    /// authorization_id -> waker for the waiting loop.
    waiters: Mutex<HashMap<String, oneshot::Sender<AuthorizationDecision>>>,
    /// (session_id, role_id, op) granted for the whole session.
    grants: Mutex<HashSet<(String, String, String)>>,
    /// (session_id, role_id, path) denied — repeat suppression (5.4.3-2).
    denials: Mutex<HashSet<(String, String, String)>>,
}

impl AuthorizationHub {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn register_waiter(&self, id: &str) -> oneshot::Receiver<AuthorizationDecision> {
        let (tx, rx) = oneshot::channel();
        self.waiters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id.to_string(), tx);
        rx
    }

    fn take_waiter(&self, id: &str) -> Option<oneshot::Sender<AuthorizationDecision>> {
        self.waiters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(id)
    }

    /// Cache-hit check for granted_session authorizations.
    fn has_session_grant(&self, session_id: &str, role_id: &str, op: &str) -> bool {
        self.grants
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&(session_id.to_string(), role_id.to_string(), op.to_string()))
    }

    fn insert_session_grant(&self, session_id: &str, role_id: &str, op: &str) {
        self.grants
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert((session_id.to_string(), role_id.to_string(), op.to_string()));
    }

    fn is_denied_once(&self, session_id: &str, role_id: &str, path: &str) -> bool {
        self.denials
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&(
                session_id.to_string(),
                role_id.to_string(),
                path.to_string(),
            ))
    }

    fn insert_denial(&self, session_id: &str, role_id: &str, path: &str) {
        self.denials
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert((
                session_id.to_string(),
                role_id.to_string(),
                path.to_string(),
            ));
    }

    /// Drops every cached grant/denial of a session (session closed).
    pub fn forget_session(&self, session_id: &str) {
        self.grants
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|(sid, _, _)| sid != session_id);
        self.denials
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|(sid, _, _)| sid != session_id);
    }
}

fn extract_target_path(args: &serde_json::Value) -> Option<String> {
    const KEYS: [&str; 3] = ["path", "file", "abs_path"];
    let obj = args.as_object()?;
    for key in KEYS {
        if let Some(value) = obj.get(key) {
            if let Some(s) = value.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// Resolves the session's bound work directory (absolute path) — `None`
/// when unbound or the binding is stale/unavailable.
fn bound_directory(conn: &rusqlite::Connection, session_id: &str) -> Option<String> {
    sessions::get_work_directory_binding(conn, session_id)
        .ok()
        .flatten()
        .filter(|b| b.available)
        .and_then(|b| b.directory.map(|d| d.abs_path))
}

fn path_within(base: &str, target: &str) -> bool {
    let normalize = |p: &str| {
        let unified = p.replace('\\', "/");
        let trimmed = unified.trim_end_matches('/');
        if cfg!(windows) || cfg!(target_os = "macos") {
            trimmed.to_lowercase()
        } else {
            trimmed.to_string()
        }
    };
    let base_n = normalize(base);
    let target_n = normalize(target);
    if target_n == base_n {
        return true;
    }
    target_n.starts_with(&format!("{base_n}/"))
}

/// Per-turn gate used by the loop engine (native-provider RoleAgents only,
/// D4). `session_id` scopes authorization rows and the grant cache;
/// `role_id` attributes requests to exactly the agent that raised them.
#[derive(Clone)]
pub struct AuthorizationGate {
    db_path: Arc<str>,
    hub: Arc<AuthorizationHub>,
    session_id: String,
    role_id: String,
}

impl AuthorizationGate {
    pub fn new(
        db_path: impl Into<Arc<str>>,
        hub: Arc<AuthorizationHub>,
        session_id: impl Into<String>,
        role_id: impl Into<String>,
    ) -> Self {
        Self {
            db_path: db_path.into(),
            hub,
            session_id: session_id.into(),
            role_id: role_id.into(),
        }
    }

    /// Full gate: classify (fast paths: non-gated tool, in-bound read,
    /// session-grant cache, repeat suppression), then for write/delete and
    /// out-of-bound reads persist the pending row + `authorization.requested`
    /// event (one transaction) and await the user's decision.
    pub async fn authorize(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<GateOutcome, StoreError> {
        let Some(path) = extract_target_path(args) else {
            return Ok(GateOutcome::Allow);
        };
        let is_write = WRITE_DELETE_TOOLS.contains(&tool_name);
        let is_read = READ_TOOLS.contains(&tool_name);
        if !is_write && !is_read {
            return Ok(GateOutcome::Allow);
        }
        // Both gate kinds ride op='write' rows (the CHECK constraint admits
        // only write/delete; D7 out-of-bound reads are authorization events
        // all the same — resolution notes the decision, not the op nuance).
        let op = AuthorizationOp::Write;

        if !is_write {
            // Reads: free inside the bound directory; gated outside (D7);
            // unbound legacy sessions stay ungated (spec 4.5-3).
            let db_path = self.db_path.clone();
            let sid = self.session_id.clone();
            let bound =
                tokio::task::spawn_blocking(move || -> Result<Option<String>, StoreError> {
                    let db = Db::open(&db_path)?;
                    Ok(bound_directory(&db.0, &sid))
                })
                .await
                .map_err(join_to_store_error)??;
            match bound {
                Some(base) if path_within(&base, &path) => return Ok(GateOutcome::Allow),
                Some(_) => {
                    if self
                        .hub
                        .is_denied_once(&self.session_id, &self.role_id, &path)
                    {
                        return Ok(GateOutcome::Deny {
                            reason: "read outside the bound directory was denied earlier".into(),
                        });
                    }
                }
                None => return Ok(GateOutcome::Allow),
            }
        } else {
            // Write/delete: session-grant cache hit → allow.
            if self
                .hub
                .has_session_grant(&self.session_id, &self.role_id, op.as_str())
            {
                return Ok(GateOutcome::Allow);
            }
            // Repeat suppression (spec 5.4.3-2).
            if self
                .hub
                .is_denied_once(&self.session_id, &self.role_id, &path)
            {
                return Ok(GateOutcome::Deny {
                    reason: "denied earlier this session".into(),
                });
            }
        }

        // Persist pending row + requested event in one transaction.
        let db_path = self.db_path.clone();
        let (sid, rid) = (self.session_id.clone(), self.role_id.clone());
        let tool = tool_name.to_string();
        let target = path.clone();
        let timeout_ms = read_timeout_ms(self.db_path.clone()).await?;
        let entry =
            tokio::task::spawn_blocking(move || -> Result<FileAuthorizationEntry, StoreError> {
                let db = Db::open(&db_path)?;
                migrations::run(&db.0)?;
                let conn = &db.0;
                let entry =
                    file_authorizations::insert_pending(conn, &sid, &rid, op, &target, timeout_ms)?;
                repos::events::append(
                    conn,
                    "session",
                    &sid,
                    "authorization.requested",
                    &serde_json::json!({
                        "authorizationId": entry.id,
                        "roleId": rid,
                        "op": op.as_str(),
                        "tool": tool,
                        "targetPath": target,
                        "requestedAt": entry.requested_at,
                        "expiresAt": entry.expires_at,
                    }),
                    entry.requested_at,
                )?;
                Ok(entry)
            })
            .await
            .map_err(join_to_store_error)??;

        // Await the user's decision (oneshot waker registered globally).
        let rx = self.hub.register_waiter(&entry.id);
        match rx.await {
            Ok(decision) => match decision {
                AuthorizationDecision::GrantOnce => Ok(GateOutcome::Grant),
                AuthorizationDecision::GrantSession => {
                    self.hub
                        .insert_session_grant(&self.session_id, &self.role_id, op.as_str());
                    Ok(GateOutcome::Grant)
                }
                AuthorizationDecision::Deny => {
                    self.hub
                        .insert_denial(&self.session_id, &self.role_id, &path);
                    Ok(GateOutcome::Deny {
                        reason: "denied by user".into(),
                    })
                }
            },
            Err(_) => Ok(GateOutcome::Deny {
                reason: "authorization request closed".into(),
            }),
        }
    }
}

fn join_to_store_error(e: tokio::task::JoinError) -> StoreError {
    StoreError::Io(std::io::Error::other(e.to_string()))
}

/// Reads the timeout setting (clamped to 30–600s, default 120).
pub async fn read_timeout_ms(db_path: Arc<str>) -> Result<i64, StoreError> {
    let raw = db_path;
    let secs = tokio::task::spawn_blocking(move || -> Result<i64, StoreError> {
        let db = Db::open(&raw)?;
        let value = settings::get(&db.0, TIMEOUT_SETTING_KEY)?;
        let parsed = value
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(TIMEOUT_DEFAULT_SECS);
        Ok(parsed.clamp(TIMEOUT_MIN_SECS, TIMEOUT_MAX_SECS))
    })
    .await
    .map_err(join_to_store_error)??;
    Ok(secs * 1000)
}

/// Typed resolution failures surfaced to the IPC layer.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("authorization {0} not found")]
    NotFound(String),
    #[error("authorization {0} already resolved")]
    AlreadyResolved(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Resolves a pending (or timeout-notified) authorization: state transition
/// plus the `authorization.resolved` event in one transaction, THEN wakes
/// the waiting loop (iron rule: side effects only after commit).
pub async fn resolve_authorization(
    db_path: &str,
    hub: &AuthorizationHub,
    authorization_id: &str,
    decision: AuthorizationDecision,
) -> Result<(), ResolveError> {
    let db_path = db_path.to_string();
    let id = authorization_id.to_string();
    let now = crate::domain::now_ms();
    let target_state = match decision {
        AuthorizationDecision::GrantOnce => AuthorizationState::GrantedOnce,
        AuthorizationDecision::GrantSession => AuthorizationState::GrantedSession,
        AuthorizationDecision::Deny => AuthorizationState::Denied,
    };
    let payload = serde_json::json!({
        "authorizationId": id,
        "decision": decision.as_str(),
        "resolvedAt": now,
    });
    let entry =
        tokio::task::spawn_blocking(move || -> Result<FileAuthorizationEntry, ResolveError> {
            let db = Db::open(&db_path)?;
            let conn = &db.0;
            let current = file_authorizations::find_by_id(conn, &id)
                .map_err(ResolveError::Store)?
                .ok_or_else(|| ResolveError::NotFound(id.clone()))?;
            if !current.state.can_transition(target_state) {
                return Err(ResolveError::AlreadyResolved(id));
            }
            file_authorizations::transition_with_event(
                conn,
                &id,
                target_state,
                Some(decision.as_str()),
                "authorization.resolved",
                &payload,
                now,
            )
            .map_err(ResolveError::Store)
        })
        .await
        .map_err(|e| ResolveError::Store(join_to_store_error(e)))??;
    // Side effect AFTER commit: wake the loop.
    if let Some(sender) = hub.take_waiter(&entry.id) {
        let _ = sender.send(decision);
    }
    Ok(())
}

/// Sweeper tick: expires pending rows past `expires_at`. With a default
/// policy the request resolves by that policy (`timeout_default` event +
/// waker — authorize → Grant, deny → Deny); without one it stays pending
/// and ONE `timeout_notified` event drives the UI reminder (spec 5.4.1-7/8).
/// Returns how many rows the policy path resolved (observability/tests).
pub async fn sweep_once(db_path: &str, hub: &AuthorizationHub) -> Result<usize, StoreError> {
    let db_path = db_path.to_string();
    let now = crate::domain::now_ms();
    let expired = {
        let db_path = db_path.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<FileAuthorizationEntry>, StoreError> {
            let db = Db::open(&db_path)?;
            let conn = &db.0;
            file_authorizations::scan_expired_pending(conn, now)
        })
        .await
        .map_err(join_to_store_error)??
    };
    let mut resolved_by_policy = 0;
    for entry in expired {
        let policy = tokio::task::spawn_blocking({
            let db_path = db_path.clone();
            let role_id = entry.role_id.clone();
            move || -> Result<Option<AgentAuthPolicyValue>, StoreError> {
                let db = Db::open(&db_path)?;
                agent_auth_policies::find_by_role(&db.0, &role_id)
            }
        })
        .await
        .map_err(join_to_store_error)??;
        match policy {
            Some(policy) => {
                let decision = match policy {
                    AgentAuthPolicyValue::Authorize => AuthorizationDecision::GrantOnce,
                    AgentAuthPolicyValue::Deny => AuthorizationDecision::Deny,
                };
                let transition = tokio::task::spawn_blocking({
                    let db_path = db_path.clone();
                    let id = entry.id.clone();
                    let policy_str = policy.as_str();
                    let decision_str = decision.as_str();
                    move || -> Result<(), ResolveError> {
                        let db = Db::open(&db_path)?;
                        let conn = &db.0;
                        let already = repos::events::list_by_aggregate(
                            conn,
                            "session",
                            &entry_session(&db.0, &id),
                            None,
                        )
                        .map(|events| {
                            events.iter().any(|e| {
                                e.kind == "authorization.timeout_default"
                                    && e.payload.get("authorizationId").and_then(|v| v.as_str())
                                        == Some(id.as_str())
                            })
                        })
                        .unwrap_or(false);
                        if already {
                            return Ok(());
                        }
                        file_authorizations::transition_with_event(
                            conn,
                            &id,
                            AuthorizationState::TimeoutDefault,
                            Some(decision_str),
                            "authorization.timeout_default",
                            &serde_json::json!({
                                "authorizationId": id,
                                "policy": policy_str,
                                "executedAt": now,
                            }),
                            now,
                        )
                        .map_err(ResolveError::Store)?;
                        Ok(())
                    }
                })
                .await;
                match transition {
                    Ok(_) => {
                        if let Some(sender) = hub.take_waiter(&entry.id) {
                            let _ = sender.send(decision);
                        }
                        resolved_by_policy += 1;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "sweep policy transition failed");
                    }
                }
            }
            None => {
                // No policy: keep pending, notify once.
                let db_path = db_path.clone();
                let id = entry.id.clone();
                let sid = entry.session_id.clone();
                let result = tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
                    let db = Db::open(&db_path)?;
                    let conn = &db.0;
                    let already = repos::events::list_by_aggregate(conn, "session", &sid, None)
                        .map(|events| {
                            events.iter().any(|e| {
                                e.kind == "authorization.timeout_notified"
                                    && e.payload.get("authorizationId").and_then(|v| v.as_str())
                                        == Some(id.as_str())
                            })
                        })
                        .unwrap_or(false);
                    if already {
                        return Ok(());
                    }
                    repos::events::append(
                        conn,
                        "session",
                        &sid,
                        "authorization.timeout_notified",
                        &serde_json::json!({ "authorizationId": id, "notifiedAt": now }),
                        now,
                    )?;
                    Ok(())
                })
                .await;
                if let Err(e) = result {
                    tracing::warn!(error = %e, "sweep notify failed");
                }
            }
        }
    }
    Ok(resolved_by_policy)
}

/// Helper for the sweep policy branch: finds a row's session id (the entry
/// already knows it — this exists because the closure captures the entry).
fn entry_session(conn: &rusqlite::Connection, id: &str) -> String {
    file_authorizations::find_by_id(conn, id)
        .ok()
        .flatten()
        .map(|e| e.session_id)
        .unwrap_or_default()
}

/// Voids a pending authorization (requester left the conversation / session
/// deleted — spec 5.4.3-1): `authorization.voided` event + state move, then
/// wake the waiter with a denial so the loop unblocks.
pub async fn void_authorization(
    db_path: &str,
    hub: &AuthorizationHub,
    authorization_id: &str,
    reason: &str,
) -> Result<(), ResolveError> {
    let db_path = db_path.to_string();
    let id = authorization_id.to_string();
    let reason = reason.to_string();
    let now = crate::domain::now_ms();
    let entry =
        tokio::task::spawn_blocking(move || -> Result<FileAuthorizationEntry, ResolveError> {
            let db = Db::open(&db_path)?;
            file_authorizations::transition_with_event(
                &db.0,
                &id,
                AuthorizationState::Voided,
                Some("voided"),
                "authorization.voided",
                &serde_json::json!({ "authorizationId": id, "reason": reason }),
                now,
            )
            .map_err(ResolveError::Store)
        })
        .await
        .map_err(|e| ResolveError::Store(join_to_store_error(e)))??;
    if let Some(sender) = hub.take_waiter(&entry.id) {
        let _ = sender.send(AuthorizationDecision::Deny);
    }
    Ok(())
}

/// Records a policy change (`authorization.policy_changed` event, spec 6.3).
/// The event rides the `role` aggregate (the policy is per-RoleAgent GLOBAL,
/// not per-session).
pub async fn set_policy(
    db_path: &str,
    role_id: &str,
    policy: Option<AgentAuthPolicyValue>,
) -> Result<(), StoreError> {
    let db_path = db_path.to_string();
    let role_id = role_id.to_string();
    let now = crate::domain::now_ms();
    tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        match policy {
            Some(value) => agent_auth_policies::upsert(conn, &role_id, value)?,
            None => agent_auth_policies::unset(conn, &role_id)?,
        }
        repos::events::append(
            conn,
            "role",
            &role_id,
            "authorization.policy_changed",
            &serde_json::json!({ "roleId": role_id, "policy": policy.map(|p| p.as_str()) }),
            now,
        )?;
        Ok(())
    })
    .await
    .map_err(join_to_store_error)??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::repos::workspaces::{self, WorkspaceEntry};

    fn seed_bound_session(conn: &rusqlite::Connection, sid: &str, dir_abs: &str) {
        crate::store::repos::sessions::insert(
            conn,
            &crate::domain::Session::new_chat(sid.into(), "t".into(), 1),
        )
        .unwrap();
        crate::store::repos::sessions::set_workspace_id(conn, sid, "w1").unwrap();
        workspaces::insert(
            conn,
            &WorkspaceEntry {
                id: "w1".into(),
                root_path: dir_abs.into(),
                color_tag: "paper-yellow".into(),
                created_at: 1,
                is_active: false,
                is_pinned: false,
            },
        )
        .unwrap();
        crate::store::repos::work_directories::add(conn, "w1", dir_abs, "dir").unwrap();
        crate::store::repos::sessions::set_work_directory(
            conn,
            sid,
            &crate::store::repos::work_directories::find_by_path(conn, dir_abs)
                .unwrap()
                .unwrap()
                .id,
        )
        .unwrap();
    }

    #[test]
    fn path_within_normalizes_separators_and_case() {
        assert!(path_within("C:\\proj\\alpha", "C:/proj/alpha/src/main.rs"));
        assert!(path_within("C:\\proj\\alpha", "C:\\proj\\alpha"));
        assert!(!path_within("C:\\proj\\alpha", "C:\\proj\\alpha-evil\\x"));
        assert!(!path_within("C:\\proj\\alpha", "C:\\proj\\beta\\x"));
    }

    #[tokio::test]
    async fn read_inside_bound_directory_is_allowed_without_request() {
        let tmp = tempfile::tempdir().unwrap();
        let db_file = tmp.path().join("k.db");
        let db_path: Arc<str> = db_file.to_string_lossy().to_string().into();
        let seed = Db::open(&db_path).unwrap();
        migrations::run(&seed.0).unwrap();
        let dir = tmp.path().join("proj");
        std::fs::create_dir_all(&dir).unwrap();
        let dir_abs = dir.to_string_lossy().to_string();
        seed_bound_session(&seed.0, "s1", &dir_abs);

        let hub = AuthorizationHub::new();
        let gate = AuthorizationGate::new(db_path.clone(), hub, "s1", "role_1");
        let inside = format!("{dir_abs}\\src\\main.rs");
        let outcome = gate
            .authorize("read_file", &serde_json::json!({ "path": inside }))
            .await
            .unwrap();
        assert_eq!(outcome, GateOutcome::Allow);
        // No authorization row was created.
        let conn = rusqlite::Connection::open(&db_file).unwrap();
        assert!(file_authorizations::list_by_session(&conn, "s1")
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn write_request_persists_pending_then_grants() {
        let tmp = tempfile::tempdir().unwrap();
        let db_file = tmp.path().join("k.db");
        let db_path: Arc<str> = db_file.to_string_lossy().to_string().into();
        let seed = Db::open(&db_path).unwrap();
        migrations::run(&seed.0).unwrap();
        crate::store::repos::sessions::insert(
            &seed.0,
            &crate::domain::Session::new_chat("s1".into(), "t".into(), 1),
        )
        .unwrap();

        let hub = AuthorizationHub::new();
        let gate = AuthorizationGate::new(db_path.clone(), hub.clone(), "s1", "role_1");
        let target = tmp.path().join("out.txt").to_string_lossy().to_string();
        let args = serde_json::json!({ "path": target, "content": "x" });
        let authorize = gate.authorize("write_file", &args);
        let db_path_str: String = db_path.to_string();
        let hub2 = hub.clone();
        let file = db_file.clone();
        let resolver = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            // Resolve the first pending row: grant_once.
            let conn = rusqlite::Connection::open(&file).unwrap();
            let pending = file_authorizations::list_by_session(&conn, "s1").unwrap();
            assert_eq!(pending.len(), 1);
            drop(conn);
            resolve_authorization(
                &db_path_str,
                &hub2,
                &pending[0].id,
                AuthorizationDecision::GrantOnce,
            )
            .await
            .unwrap();
        });
        let outcome = authorize.await.unwrap();
        resolver.await.unwrap();
        assert_eq!(outcome, GateOutcome::Grant);
        // requested + resolved events both on the log.
        let conn = rusqlite::Connection::open(&db_file).unwrap();
        let events = repos::events::list_by_aggregate(&conn, "session", "s1", None).unwrap();
        let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"authorization.requested"));
        assert!(kinds.contains(&"authorization.resolved"));
    }

    #[tokio::test]
    async fn deny_then_repeat_is_suppressed() {
        let tmp = tempfile::tempdir().unwrap();
        let db_file = tmp.path().join("k.db");
        let db_path: Arc<str> = db_file.to_string_lossy().to_string().into();
        let seed = Db::open(&db_path).unwrap();
        migrations::run(&seed.0).unwrap();
        crate::store::repos::sessions::insert(
            &seed.0,
            &crate::domain::Session::new_chat("s1".into(), "t".into(), 1),
        )
        .unwrap();

        let hub = AuthorizationHub::new();
        let gate = AuthorizationGate::new(db_path.clone(), hub.clone(), "s1", "role_1");
        let target = tmp.path().join("denied.txt").to_string_lossy().to_string();
        let args = serde_json::json!({ "path": target });

        let authorize = gate.authorize("write_file", &args);
        let db_path_str: String = db_path.to_string();
        let hub2 = hub.clone();
        let file = db_file.clone();
        let resolver = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            let conn = rusqlite::Connection::open(&file).unwrap();
            let pending = file_authorizations::list_by_session(&conn, "s1").unwrap();
            drop(conn);
            resolve_authorization(
                &db_path_str,
                &hub2,
                &pending[0].id,
                AuthorizationDecision::Deny,
            )
            .await
            .unwrap();
        });
        let first = authorize.await.unwrap();
        resolver.await.unwrap();
        assert!(matches!(first, GateOutcome::Deny { .. }));

        // Same (role, path) again: suppressed without a new row.
        let second = gate.authorize("write_file", &args).await.unwrap();
        assert!(matches!(second, GateOutcome::Deny { .. }));
        let conn = rusqlite::Connection::open(&db_file).unwrap();
        assert_eq!(
            file_authorizations::list_by_session(&conn, "s1")
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn session_grant_caches_subsequent_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let db_file = tmp.path().join("k.db");
        let db_path: Arc<str> = db_file.to_string_lossy().to_string().into();
        let seed = Db::open(&db_path).unwrap();
        migrations::run(&seed.0).unwrap();
        crate::store::repos::sessions::insert(
            &seed.0,
            &crate::domain::Session::new_chat("s1".into(), "t".into(), 1),
        )
        .unwrap();

        let hub = AuthorizationHub::new();
        let gate = AuthorizationGate::new(db_path.clone(), hub.clone(), "s1", "role_1");
        let target = tmp.path().join("a.txt").to_string_lossy().to_string();
        let args = serde_json::json!({ "path": target });
        let authorize = gate.authorize("write_file", &args);
        let db_path_str: String = db_path.to_string();
        let hub2 = hub.clone();
        let file = db_file.clone();
        let resolver = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            let conn = rusqlite::Connection::open(&file).unwrap();
            let pending = file_authorizations::list_by_session(&conn, "s1").unwrap();
            drop(conn);
            resolve_authorization(
                &db_path_str,
                &hub2,
                &pending[0].id,
                AuthorizationDecision::GrantSession,
            )
            .await
            .unwrap();
        });
        let first = authorize.await.unwrap();
        resolver.await.unwrap();
        assert_eq!(first, GateOutcome::Grant);

        // Any other path now allowed without a new request.
        let other = tmp.path().join("b.txt").to_string_lossy().to_string();
        let second = gate
            .authorize("write_file", &serde_json::json!({ "path": other }))
            .await
            .unwrap();
        assert_eq!(second, GateOutcome::Allow);
        let conn = rusqlite::Connection::open(&db_file).unwrap();
        assert_eq!(
            file_authorizations::list_by_session(&conn, "s1")
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn sweeper_resolves_by_policy_or_notifies() {
        let tmp = tempfile::tempdir().unwrap();
        let db_file = tmp.path().join("k.db");
        let db_path: Arc<str> = db_file.to_string_lossy().to_string().into();
        let conn = rusqlite::Connection::open(&db_file).unwrap();
        migrations::run(&conn).unwrap();
        crate::store::repos::sessions::insert(
            &conn,
            &crate::domain::Session::new_chat("s1".into(), "t".into(), 1),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO roles (id, name, created_at, updated_at) VALUES ('role_1', 'R', 1, 1)",
            [],
        )
        .unwrap();
        agent_auth_policies::upsert(&conn, "role_1", AgentAuthPolicyValue::Authorize).unwrap();
        // Row WITH a default policy (authorize) already expired.
        file_authorizations::insert_pending(
            &conn,
            "s1",
            "role_1",
            AuthorizationOp::Write,
            "C:\\x.txt",
            -1_000,
        )
        .unwrap();
        // Row WITHOUT a policy already expired.
        file_authorizations::insert_pending(
            &conn,
            "s1",
            "role_2",
            AuthorizationOp::Delete,
            "C:\\y.txt",
            -1_000,
        )
        .unwrap();
        drop(conn);

        let hub = AuthorizationHub::new();
        let db_path_str: String = db_path.to_string();
        let resolved = sweep_once(&db_path_str, &hub).await.unwrap();
        assert_eq!(resolved, 1, "policy row resolves");

        let conn = rusqlite::Connection::open(&db_file).unwrap();
        let rows = file_authorizations::list_by_session(&conn, "s1").unwrap();
        let by_role = |r: &str| rows.iter().find(|e| e.role_id == r).unwrap();
        assert_eq!(by_role("role_1").state, AuthorizationState::TimeoutDefault);
        assert_eq!(by_role("role_2").state, AuthorizationState::Pending);
        let events = repos::events::list_by_aggregate(&conn, "session", "s1", None).unwrap();
        assert!(events
            .iter()
            .any(|e| e.kind == "authorization.timeout_default"));
        assert!(events
            .iter()
            .any(|e| e.kind == "authorization.timeout_notified"));

        // Second sweep: no duplicate notify for the policy-less row.
        let resolved2 = sweep_once(&db_path_str, &hub).await.unwrap();
        assert_eq!(resolved2, 0);
        let events2 = repos::events::list_by_aggregate(&conn, "session", "s1", None).unwrap();
        let notified = events2
            .iter()
            .filter(|e| e.kind == "authorization.timeout_notified")
            .count();
        assert_eq!(notified, 1);
    }

    #[tokio::test]
    async fn resolve_twice_is_already_resolved() {
        let tmp = tempfile::tempdir().unwrap();
        let db_file = tmp.path().join("k.db");
        let db_path: Arc<str> = db_file.to_string_lossy().to_string().into();
        let conn = rusqlite::Connection::open(&db_file).unwrap();
        migrations::run(&conn).unwrap();
        crate::store::repos::sessions::insert(
            &conn,
            &crate::domain::Session::new_chat("s1".into(), "t".into(), 1),
        )
        .unwrap();
        let entry = file_authorizations::insert_pending(
            &conn,
            "s1",
            "role_1",
            AuthorizationOp::Write,
            "C:\\x",
            120_000,
        )
        .unwrap();
        drop(conn);
        let hub = AuthorizationHub::new();
        let db_path_str: String = db_path.to_string();
        resolve_authorization(&db_path_str, &hub, &entry.id, AuthorizationDecision::Deny)
            .await
            .unwrap();
        let err = resolve_authorization(&db_path_str, &hub, &entry.id, AuthorizationDecision::Deny)
            .await
            .unwrap_err();
        assert!(matches!(err, ResolveError::AlreadyResolved(_)));
        // Unknown id: NotFound.
        let err = resolve_authorization(&db_path_str, &hub, "ghost", AuthorizationDecision::Deny)
            .await
            .unwrap_err();
        assert!(matches!(err, ResolveError::NotFound(_)));
    }

    #[tokio::test]
    async fn void_wakes_waiter_with_deny() {
        let tmp = tempfile::tempdir().unwrap();
        let db_file = tmp.path().join("k.db");
        let db_path: Arc<str> = db_file.to_string_lossy().to_string().into();
        let conn = rusqlite::Connection::open(&db_file).unwrap();
        migrations::run(&conn).unwrap();
        crate::store::repos::sessions::insert(
            &conn,
            &crate::domain::Session::new_chat("s1".into(), "t".into(), 1),
        )
        .unwrap();
        let entry = file_authorizations::insert_pending(
            &conn,
            "s1",
            "role_1",
            AuthorizationOp::Write,
            "C:\\x",
            120_000,
        )
        .unwrap();
        drop(conn);
        let hub = AuthorizationHub::new();
        let db_path_str: String = db_path.to_string();
        void_authorization(&db_path_str, &hub, &entry.id, "requester removed")
            .await
            .unwrap();
        let conn = rusqlite::Connection::open(&db_file).unwrap();
        let stored = file_authorizations::find_by_id(&conn, &entry.id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.state, AuthorizationState::Voided);
        let events = repos::events::list_by_aggregate(&conn, "session", "s1", None).unwrap();
        assert!(events.iter().any(|e| e.kind == "authorization.voided"));
    }
}
