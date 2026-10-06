//! Steward AI domain entities — mirror 1:1 onto SQLite schema in migrations
//! 0027-0031. Conventions follow `entities.rs`: uuid-v7 string ids, unix-ms
//! i64 `*At` timestamps, JSON payloads as `serde_json::Value`.

use serde::{Deserialize, Serialize};

use super::entities::AgentRefKind;
use super::steward_enums::{
    ArtifactStatus, ArtifactType, CyclePhase, CycleStatus, DevRoleKind, GateDecisionKind,
    TaskPhase, TaskStatus, TriggerSource,
};

/// Steward AI singleton (migration 0028 `steward_ai`). One row per workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StewardAi {
    pub id: String,
    /// Whether the dev team has been initialized (5 role bindings ready).
    #[serde(default)]
    pub ready: bool,
    /// Whether online research is authorized (shared with evolution::research).
    #[serde(default)]
    pub online_authorized: bool,
    pub created_at: i64,
}

/// Steward session — the "meta conversation" about the app itself (migration 0027).
/// The `id` mirrors `sessions.id` (kind='background'); `steward_sessions` row
/// existence is the authoritative marker of a steward session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StewardSession {
    pub id: String,
    pub steward_id: String,
    #[serde(default)]
    pub title: String,
    pub goal: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Dev team singleton (migration 0028 `steward_dev_team`). Conventional id:
/// `steward_dev_team`. Never deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevTeam {
    pub id: String,
    pub steward_id: String,
    pub created_at: i64,
}

/// A dev-role binding (migration 0028 `steward_dev_role_bindings`).
/// PK = role_kind. Users may replace but never delete bindings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevRoleBinding {
    pub role_kind: DevRoleKind,
    pub agent_kind: AgentRefKind,
    pub agent_ref_id: String,
    pub updated_at: i64,
}

/// An evolution cycle (migration 0029 `evolution_cycles`).
/// Phase flow: cleanse → research → design → develop → test → verify → gate → merge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionCycle {
    pub id: String,
    pub trigger_source: TriggerSource,
    #[serde(default)]
    pub trigger_context: String,
    #[serde(default = "default_cycle_phase")]
    pub phase: CyclePhase,
    #[serde(default = "default_cycle_status")]
    pub status: CycleStatus,
    pub created_at: i64,
    pub ended_at: Option<i64>,
}

fn default_cycle_phase() -> CyclePhase {
    CyclePhase::Cleanse
}

fn default_cycle_status() -> CycleStatus {
    CycleStatus::Running
}

/// A task within an evolution cycle (migration 0029 `evolution_tasks`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionTask {
    pub id: String,
    pub cycle_id: String,
    pub phase: TaskPhase,
    pub dev_role: DevRoleKind,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default = "default_task_status")]
    pub status: TaskStatus,
    pub acceptance_criteria: String,
    pub trigger_source: String,
    pub created_at: i64,
    pub updated_at: i64,
}

fn default_task_status() -> TaskStatus {
    TaskStatus::Pending
}

/// An artifact produced by the dev team (migration 0029 `evolution_artifacts`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionArtifact {
    pub id: String,
    pub task_id: String,
    pub produced_by_role: String,
    pub artifact_type: ArtifactType,
    pub content: serde_json::Value,
    #[serde(default = "default_artifact_status")]
    pub status: ArtifactStatus,
    pub diff_preview: Option<String>,
    pub rollback_plan: Option<serde_json::Value>,
    pub created_at: i64,
}

fn default_artifact_status() -> ArtifactStatus {
    ArtifactStatus::PendingReview
}

/// A cleansed data pool (migration 0030 `evolution_data_pools`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionDataPool {
    pub id: String,
    pub scope: serde_json::Value,
    pub rules_id: String,
    pub product: serde_json::Value,
    pub created_at: i64,
}

/// A gate decision on an artifact (migration 0030 `evolution_gate_decisions`).
/// UNIQUE(artifact_id) — at most one decision per artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateDecision {
    pub id: String,
    pub artifact_id: String,
    pub decision: GateDecisionKind,
    pub reason: Option<String>,
    pub decided_at: i64,
}

/// A before/after snapshot for config-change rollback (migration 0030
/// `steward_change_snapshots`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StewardChangeSnapshot {
    pub id: String,
    pub proposal_id: String,
    pub target_type: String,
    pub target_id: String,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steward_ai_roundtrip() {
        let ai = StewardAi {
            id: "s1".into(),
            ready: true,
            online_authorized: false,
            created_at: 1,
        };
        let json = serde_json::to_value(&ai).unwrap();
        let back: StewardAi = serde_json::from_value(json).unwrap();
        assert_eq!(back.id, "s1");
        assert!(back.ready);
        assert!(!back.online_authorized);
    }

    #[test]
    fn steward_session_roundtrip() {
        let s = StewardSession {
            id: "s1".into(),
            steward_id: "st".into(),
            title: "t".into(),
            goal: Some("g".into()),
            created_at: 1,
            updated_at: 2,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: StewardSession = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "s1");
        assert_eq!(back.goal.as_deref(), Some("g"));
    }

    #[test]
    fn dev_team_roundtrip() {
        let t = DevTeam {
            id: "steward_dev_team".into(),
            steward_id: "s1".into(),
            created_at: 1,
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: DevTeam = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "steward_dev_team");
    }

    #[test]
    fn dev_role_binding_roundtrip() {
        let b = DevRoleBinding {
            role_kind: DevRoleKind::Researcher,
            agent_kind: AgentRefKind::Role,
            agent_ref_id: "role_x".into(),
            updated_at: 1,
        };
        let json = serde_json::to_string(&b).unwrap();
        let back: DevRoleBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role_kind, DevRoleKind::Researcher);
        assert_eq!(back.agent_kind, AgentRefKind::Role);
    }

    #[test]
    fn evolution_cycle_roundtrip() {
        let c = EvolutionCycle {
            id: "c1".into(),
            trigger_source: TriggerSource::User,
            trigger_context: "ctx".into(),
            phase: CyclePhase::Research,
            status: CycleStatus::Running,
            created_at: 1,
            ended_at: None,
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: EvolutionCycle = serde_json::from_str(&json).unwrap();
        assert_eq!(back.phase, CyclePhase::Research);
        assert_eq!(back.trigger_source, TriggerSource::User);
    }

    #[test]
    fn evolution_task_roundtrip() {
        let t = EvolutionTask {
            id: "t1".into(),
            cycle_id: "c1".into(),
            phase: TaskPhase::Design,
            dev_role: DevRoleKind::Designer,
            depends_on: vec!["t0".into()],
            status: TaskStatus::Pending,
            acceptance_criteria: "must pass".into(),
            trigger_source: "user".into(),
            created_at: 1,
            updated_at: 2,
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: EvolutionTask = serde_json::from_str(&json).unwrap();
        assert_eq!(back.depends_on, vec!["t0".to_string()]);
        assert_eq!(back.status, TaskStatus::Pending);
    }

    #[test]
    fn evolution_artifact_roundtrip() {
        let a = EvolutionArtifact {
            id: "a1".into(),
            task_id: "t1".into(),
            produced_by_role: "researcher".into(),
            artifact_type: ArtifactType::ResearchReport,
            content: serde_json::json!({"summary": "ok"}),
            status: ArtifactStatus::PendingReview,
            diff_preview: Some("diff".into()),
            rollback_plan: None,
            created_at: 1,
        };
        let json = serde_json::to_string(&a).unwrap();
        let back: EvolutionArtifact = serde_json::from_str(&json).unwrap();
        assert_eq!(back.artifact_type, ArtifactType::ResearchReport);
        assert_eq!(back.content["summary"], "ok");
    }

    #[test]
    fn evolution_data_pool_roundtrip() {
        let p = EvolutionDataPool {
            id: "p1".into(),
            scope: serde_json::json!({"range": "7d"}),
            rules_id: "r1".into(),
            product: serde_json::json!({"patterns": []}),
            created_at: 1,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: EvolutionDataPool = serde_json::from_str(&json).unwrap();
        assert_eq!(back.rules_id, "r1");
    }

    #[test]
    fn gate_decision_roundtrip() {
        let d = GateDecision {
            id: "d1".into(),
            artifact_id: "a1".into(),
            decision: GateDecisionKind::Approve,
            reason: Some("looks good".into()),
            decided_at: 1,
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: GateDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(back.decision, GateDecisionKind::Approve);
    }

    #[test]
    fn steward_change_snapshot_roundtrip() {
        let s = StewardChangeSnapshot {
            id: "snap1".into(),
            proposal_id: "a1".into(),
            target_type: "role".into(),
            target_id: "r1".into(),
            before: serde_json::json!({"name": "old"}),
            after: serde_json::json!({"name": "new"}),
            created_at: 1,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: StewardChangeSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.before["name"], "old");
        assert_eq!(back.after["name"], "new");
    }
}
