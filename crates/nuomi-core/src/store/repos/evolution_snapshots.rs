//! Repository for `evolution_snapshots` table (ADR-0025 §3-§5 Slice 4).
//!
//! Stores version-tree snapshots with parent-child tracing, profile summaries,
//! and four-track evaluation metrics.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::evolution::snapshot_types::{EvaluationMetrics, ProfileSummary};
use crate::store::StoreError;

/// Trigger source for an evolution snapshot (ADR-0025 §3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    B1Accumulation,
    OnlineResearch,
    UserManual,
}

impl TriggerSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerSource::B1Accumulation => "b1_accumulation",
            TriggerSource::OnlineResearch => "online_research",
            TriggerSource::UserManual => "user_manual",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "online_research" => TriggerSource::OnlineResearch,
            "user_manual" => TriggerSource::UserManual,
            _ => TriggerSource::B1Accumulation,
        }
    }
}

/// Evolution tier (ADR-0025 §3).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum EvolutionTier {
    L0,
    L1,
    L2,
    L3,
}

impl EvolutionTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            EvolutionTier::L0 => "L0",
            EvolutionTier::L1 => "L1",
            EvolutionTier::L2 => "L2",
            EvolutionTier::L3 => "L3",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "L1" => EvolutionTier::L1,
            "L2" => EvolutionTier::L2,
            "L3" => EvolutionTier::L3,
            _ => EvolutionTier::L0,
        }
    }
}

/// Snapshot status (ADR-0025 §3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotStatus {
    Staged,
    Active,
    Reverted,
    Discarded,
}

impl SnapshotStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SnapshotStatus::Staged => "staged",
            SnapshotStatus::Active => "active",
            SnapshotStatus::Reverted => "reverted",
            SnapshotStatus::Discarded => "discarded",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "active" => SnapshotStatus::Active,
            "reverted" => SnapshotStatus::Reverted,
            "discarded" => SnapshotStatus::Discarded,
            _ => SnapshotStatus::Staged,
        }
    }
}

/// An evolution snapshot row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvolutionSnapshot {
    pub id: String,
    pub parent_snapshot_id: Option<String>,
    pub trigger_source: TriggerSource,
    pub tier: EvolutionTier,
    pub from_version: String,
    pub to_version: String,
    pub profile_summary: ProfileSummary,
    pub diff_patch_ref: String,
    pub artifact_ref: Option<String>,
    pub status: SnapshotStatus,
    pub metrics: Option<EvaluationMetrics>,
    pub created_at: i64,
}

/// Insert a new snapshot.
pub fn insert(conn: &Connection, snap: &EvolutionSnapshot) -> Result<(), StoreError> {
    let parent = snap.parent_snapshot_id.as_deref();
    let profile_json = serde_json::to_string(&snap.profile_summary)?;
    let metrics_json = match &snap.metrics {
        Some(m) => Some(serde_json::to_string(m)?),
        None => None,
    };
    let artifact = snap.artifact_ref.as_deref();
    conn.execute(
        "INSERT INTO evolution_snapshots
         (id, parent_snapshot_id, trigger_source, tier, from_version, to_version,
          profile_summary, diff_patch_ref, artifact_ref, status, metrics, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            snap.id,
            parent,
            snap.trigger_source.as_str(),
            snap.tier.as_str(),
            snap.from_version,
            snap.to_version,
            profile_json,
            snap.diff_patch_ref,
            artifact,
            snap.status.as_str(),
            metrics_json,
            snap.created_at,
        ],
    )?;
    Ok(())
}

/// Get a snapshot by ID.
pub fn get(conn: &Connection, id: &str) -> Result<EvolutionSnapshot, StoreError> {
    conn.query_row(
        "SELECT id, parent_snapshot_id, trigger_source, tier, from_version, to_version,
                profile_summary, diff_patch_ref, artifact_ref, status, metrics, created_at
         FROM evolution_snapshots WHERE id = ?1",
        [id],
        row_to_snapshot,
    )
    .map_err(StoreError::from)
}

/// List snapshots by status, ordered by creation time descending.
pub fn list_by_status(
    conn: &Connection,
    status: &SnapshotStatus,
    limit: i64,
) -> Result<Vec<EvolutionSnapshot>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_snapshot_id, trigger_source, tier, from_version, to_version,
                profile_summary, diff_patch_ref, artifact_ref, status, metrics, created_at
         FROM evolution_snapshots WHERE status = ?1
         ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![status.as_str(), limit], row_to_snapshot)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

/// List children of a parent snapshot.
pub fn list_children(
    conn: &Connection,
    parent_id: &str,
) -> Result<Vec<EvolutionSnapshot>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_snapshot_id, trigger_source, tier, from_version, to_version,
                profile_summary, diff_patch_ref, artifact_ref, status, metrics, created_at
         FROM evolution_snapshots WHERE parent_snapshot_id = ?1
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([parent_id], row_to_snapshot)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

/// Update snapshot status (e.g. staged → active, or staged → reverted).
pub fn update_status(
    conn: &Connection,
    id: &str,
    status: &SnapshotStatus,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE evolution_snapshots SET status = ?1 WHERE id = ?2",
        params![status.as_str(), id],
    )?;
    Ok(())
}

/// Update evaluation metrics for a snapshot.
pub fn update_metrics(
    conn: &Connection,
    id: &str,
    metrics: &EvaluationMetrics,
) -> Result<(), StoreError> {
    let json = serde_json::to_string(metrics)?;
    conn.execute(
        "UPDATE evolution_snapshots SET metrics = ?1 WHERE id = ?2",
        params![json, id],
    )?;
    Ok(())
}

/// Count snapshots by tier.
pub fn count_by_tier(conn: &Connection, tier: &EvolutionTier) -> Result<i64, StoreError> {
    conn.query_row(
        "SELECT count(*) FROM evolution_snapshots WHERE tier = ?1",
        [tier.as_str()],
        |r| r.get(0),
    )
    .map_err(StoreError::from)
}

fn row_to_snapshot(row: &rusqlite::Row) -> rusqlite::Result<EvolutionSnapshot> {
    let parent: Option<String> = row.get(1)?;
    let trigger_str: String = row.get(2)?;
    let tier_str: String = row.get(3)?;
    let profile_json: String = row.get(6)?;
    let status_str: String = row.get(9)?;
    let metrics_json: Option<String> = row.get(10)?;

    let profile_summary: ProfileSummary = serde_json::from_str(&profile_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let metrics: Option<EvaluationMetrics> = match metrics_json {
        Some(json) => Some(serde_json::from_str(&json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?),
        None => None,
    };

    Ok(EvolutionSnapshot {
        id: row.get(0)?,
        parent_snapshot_id: parent,
        trigger_source: TriggerSource::parse(&trigger_str),
        tier: EvolutionTier::parse(&tier_str),
        from_version: row.get(4)?,
        to_version: row.get(5)?,
        profile_summary,
        diff_patch_ref: row.get(7)?,
        artifact_ref: row.get(8)?,
        status: SnapshotStatus::parse(&status_str),
        metrics,
        created_at: row.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evolution::snapshot_types as st;
    use crate::store::migrations;

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn sample_snapshot(id: &str, parent: Option<&str>) -> EvolutionSnapshot {
        EvolutionSnapshot {
            id: id.to_string(),
            parent_snapshot_id: parent.map(|s| s.to_string()),
            trigger_source: TriggerSource::B1Accumulation,
            tier: EvolutionTier::L1,
            from_version: "v1.0".to_string(),
            to_version: "v1.1".to_string(),
            profile_summary: ProfileSummary::empty(),
            diff_patch_ref: "snapshots/s1.diff".to_string(),
            artifact_ref: None,
            status: SnapshotStatus::Staged,
            metrics: None,
            created_at: 1000,
        }
    }

    #[test]
    fn insert_and_get_roundtrip() {
        let conn = fresh_db();
        let snap = sample_snapshot("s1", None);
        insert(&conn, &snap).unwrap();
        let got = get(&conn, "s1").unwrap();
        assert_eq!(got.id, "s1");
        assert_eq!(got.tier, EvolutionTier::L1);
        assert!(got.parent_snapshot_id.is_none());
        assert!(got.metrics.is_none());
    }

    #[test]
    fn parent_child_relationship() {
        let conn = fresh_db();
        let parent = sample_snapshot("root", None);
        insert(&conn, &parent).unwrap();
        let child = sample_snapshot("c1", Some("root"));
        insert(&conn, &child).unwrap();
        let children = list_children(&conn, "root").unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, "c1");
    }

    #[test]
    fn update_status_works() {
        let conn = fresh_db();
        insert(&conn, &sample_snapshot("s1", None)).unwrap();
        update_status(&conn, "s1", &SnapshotStatus::Active).unwrap();
        let got = get(&conn, "s1").unwrap();
        assert_eq!(got.status, SnapshotStatus::Active);
    }

    #[test]
    fn update_metrics_works() {
        let conn = fresh_db();
        insert(&conn, &sample_snapshot("s1", None)).unwrap();
        let metrics = EvaluationMetrics {
            track1: st::Track1Result {
                passed: true,
                total_tests: 10,
                failures: 0,
                clippy_warnings: 0,
                typecheck_errors: 0,
                summary: "ok".into(),
            },
            track2: st::Track2Result {
                passed: true,
                skipped: true,
                replay_count: 0,
                max_degradation_pct: 0.0,
                metrics: vec![],
            },
            track3: st::Track3Result {
                verdict: "approve".into(),
                reason: "".into(),
            },
            track4: None,
            overall_pass: true,
        };
        update_metrics(&conn, "s1", &metrics).unwrap();
        let got = get(&conn, "s1").unwrap();
        assert!(got.metrics.is_some());
        assert!(got.metrics.unwrap().overall_pass);
    }

    #[test]
    fn list_by_status_filters() {
        let conn = fresh_db();
        insert(&conn, &sample_snapshot("s1", None)).unwrap();
        let mut s2 = sample_snapshot("s2", None);
        s2.status = SnapshotStatus::Active;
        insert(&conn, &s2).unwrap();
        let staged = list_by_status(&conn, &SnapshotStatus::Staged, 10).unwrap();
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].id, "s1");
        let active = list_by_status(&conn, &SnapshotStatus::Active, 10).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "s2");
    }

    #[test]
    fn count_by_tier_works() {
        let conn = fresh_db();
        insert(&conn, &sample_snapshot("s1", None)).unwrap();
        insert(&conn, &sample_snapshot("s2", None)).unwrap();
        let mut s3 = sample_snapshot("s3", None);
        s3.tier = EvolutionTier::L2;
        insert(&conn, &s3).unwrap();
        assert_eq!(count_by_tier(&conn, &EvolutionTier::L1).unwrap(), 2);
        assert_eq!(count_by_tier(&conn, &EvolutionTier::L2).unwrap(), 1);
        assert_eq!(count_by_tier(&conn, &EvolutionTier::L3).unwrap(), 0);
    }
}
