//! App-state snapshot — read-only aggregation of the full application state
//! for intent recognition. (K-Steward-2, T2-3/T2-4)

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::store::repos::{agent_profiles, providers, roles, sessions, steward, teams};
use crate::store::Db;

use super::StewardError;

/// Maximum rows to fetch per table (avoids large queries on big workspaces).
const RECENT_LIMIT: u32 = 50;

/// A point-in-time snapshot of the application state, used as context for
/// intent recognition. All fields are read-only aggregations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStateSnapshot {
    pub providers: Vec<ProviderSummary>,
    pub roles: Vec<RoleSummary>,
    pub teams: Vec<TeamSummary>,
    pub agent_profiles: Vec<AgentProfileSummary>,
    pub sessions: Vec<SessionSummary>,
    pub recent_events: Vec<EventSummary>,
    pub memory_entries: Vec<MemorySummary>,
    pub prompt_versions: Vec<PromptVersionSummary>,
    pub evolution_cycles: Vec<CycleSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleSummary {
    pub id: String,
    pub name: String,
    pub builtin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSummary {
    pub id: String,
    pub name: String,
    pub topology: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfileSummary {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSummary {
    pub id: i64,
    pub kind: String,
    pub aggregate_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySummary {
    pub id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptVersionSummary {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleSummary {
    pub id: String,
    pub phase: String,
    pub status: String,
}

/// Reads a point-in-time snapshot of the application state.
///
/// Aggregates 6+ tables with `spawn_blocking`, each limited to `RECENT_LIMIT`
/// rows to avoid large queries. Returns `StewardError::Store` if the storage
/// layer is unavailable.
pub async fn read(db_path: Arc<str>, workspace_id: &str) -> Result<AppStateSnapshot, StewardError> {
    let workspace_id = workspace_id.to_string();
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        let providers = providers::list_providers(conn)?
            .into_iter()
            .map(|p| ProviderSummary {
                id: p.id,
                name: p.name,
                protocol: format!("{:?}", p.protocol).to_lowercase(),
            })
            .collect();
        let roles = roles::list(conn)?
            .into_iter()
            .map(|r| RoleSummary {
                id: r.id,
                name: r.name,
                builtin: r.builtin,
            })
            .collect();
        let teams = teams::list(conn)?
            .into_iter()
            .map(|t| TeamSummary {
                id: t.id,
                name: t.name,
                topology: format!("{:?}", t.topology).to_lowercase(),
            })
            .collect();
        let agent_profiles = agent_profiles::list(conn)?
            .into_iter()
            .map(|a| AgentProfileSummary {
                id: a.id,
                name: a.name,
                kind: a.adapter,
            })
            .collect();
        let sessions = sessions::list(conn, &workspace_id, RECENT_LIMIT)?
            .into_iter()
            .map(|s| SessionSummary {
                id: s.id,
                title: s.title,
                kind: s.kind.as_str().to_string(),
            })
            .collect();
        let recent_events = query_recent_events(conn)?;
        let memory_entries = crate::store::repos::memory::list_user_profile(conn)?
            .into_iter()
            .map(|m| MemorySummary {
                id: m.id,
                content: m.content,
            })
            .collect();
        let prompt_versions = query_prompt_versions(conn)?;
        let evolution_cycles = steward::list_cycles(conn, RECENT_LIMIT)?
            .into_iter()
            .map(|c| CycleSummary {
                id: c.id,
                phase: c.phase.as_str().to_string(),
                status: c.status.as_str().to_string(),
            })
            .collect();
        Ok(AppStateSnapshot {
            providers,
            roles,
            teams,
            agent_profiles,
            sessions,
            recent_events,
            memory_entries,
            prompt_versions,
            evolution_cycles,
        })
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

fn query_recent_events(conn: &rusqlite::Connection) -> Result<Vec<EventSummary>, StewardError> {
    let mut stmt =
        conn.prepare("SELECT id, kind, aggregate_type FROM events ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map(rusqlite::params![RECENT_LIMIT as i64], |row| {
        Ok(EventSummary {
            id: row.get(0)?,
            kind: row.get(1)?,
            aggregate_type: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn query_prompt_versions(
    conn: &rusqlite::Connection,
) -> Result<Vec<PromptVersionSummary>, StewardError> {
    let mut stmt =
        conn.prepare("SELECT id, status FROM prompt_versions ORDER BY created_at DESC LIMIT ?1")?;
    let rows = stmt.query_map(rusqlite::params![RECENT_LIMIT as i64], |row| {
        Ok(PromptVersionSummary {
            id: row.get(0)?,
            status: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db_path() -> Arc<str> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_string_lossy().to_string();
        let db = Db::open(&path_str).unwrap();
        crate::store::migrations::run(&db.0).unwrap();
        drop(db);
        std::mem::forget(dir);
        Arc::from(path_str)
    }

    #[tokio::test]
    async fn read_returns_empty_snapshot_on_fresh_db() {
        let dbp = db_path().await;
        let snap = read(dbp, "ws1").await.unwrap();
        assert!(snap.providers.is_empty());
        assert!(snap.teams.is_empty());
        assert!(snap.sessions.is_empty());
        assert_eq!(
            snap.roles.len(),
            5,
            "5 builtin steward roles should be present"
        );
        assert!(snap.roles.iter().all(|r| r.builtin));
    }

    #[tokio::test]
    async fn read_returns_seeded_data() {
        let dbp = db_path().await;
        {
            let db = Db::open(&dbp).unwrap();
            let conn = &db.0;
            conn.execute(
                "INSERT INTO provider_configs (id, name, protocol, base_url, created_at, updated_at)
                 VALUES ('p1', 'TestProvider', 'openai_compatible', 'http://localhost', 1, 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO teams (id, name, topology, created_at, updated_at)
                 VALUES ('t1', 'TestTeam', 'pipeline', 1, 1)",
                [],
            )
            .unwrap();
        }
        let snap = read(dbp, "__migrated__").await.unwrap();
        assert!(snap.providers.len() >= 1);
        assert!(snap.teams.len() >= 1);
    }
}
