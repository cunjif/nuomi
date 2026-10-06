//! Steward AI domain enums — mirror 1:1 onto SQLite CHECK constraints in
//! migrations 0027-0031. String literals must match exactly.
//!
//! Conventions follow `entities::ConversationKind`: `as_str()` / `parse()`
//! roundtrip, `#[serde(rename_all = "snake_case")]`.

use serde::{Deserialize, Serialize};

/// 5 built-in dev-team roles (migration 0028 `steward_dev_role_bindings.role_kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevRoleKind {
    Researcher,
    Designer,
    Developer,
    Tester,
    Verifier,
}

impl DevRoleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DevRoleKind::Researcher => "researcher",
            DevRoleKind::Designer => "designer",
            DevRoleKind::Developer => "developer",
            DevRoleKind::Tester => "tester",
            DevRoleKind::Verifier => "verifier",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "researcher" => Some(DevRoleKind::Researcher),
            "designer" => Some(DevRoleKind::Designer),
            "developer" => Some(DevRoleKind::Developer),
            "tester" => Some(DevRoleKind::Tester),
            "verifier" => Some(DevRoleKind::Verifier),
            _ => None,
        }
    }

    /// All 5 variants in canonical order.
    pub fn all() -> [DevRoleKind; 5] {
        [
            DevRoleKind::Researcher,
            DevRoleKind::Designer,
            DevRoleKind::Developer,
            DevRoleKind::Tester,
            DevRoleKind::Verifier,
        ]
    }
}

/// Evolution cycle phase (migration 0029 `evolution_cycles.phase`).
/// Flow: cleanse → research → design → develop → test → verify → gate → merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CyclePhase {
    Cleanse,
    Research,
    Design,
    Develop,
    Test,
    Verify,
    Gate,
    Merge,
}

impl CyclePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            CyclePhase::Cleanse => "cleanse",
            CyclePhase::Research => "research",
            CyclePhase::Design => "design",
            CyclePhase::Develop => "develop",
            CyclePhase::Test => "test",
            CyclePhase::Verify => "verify",
            CyclePhase::Gate => "gate",
            CyclePhase::Merge => "merge",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cleanse" => Some(CyclePhase::Cleanse),
            "research" => Some(CyclePhase::Research),
            "design" => Some(CyclePhase::Design),
            "develop" => Some(CyclePhase::Develop),
            "test" => Some(CyclePhase::Test),
            "verify" => Some(CyclePhase::Verify),
            "gate" => Some(CyclePhase::Gate),
            "merge" => Some(CyclePhase::Merge),
            _ => None,
        }
    }

    /// Next phase in the canonical flow, or None if at terminal `merge`.
    pub fn next(self) -> Option<CyclePhase> {
        match self {
            CyclePhase::Cleanse => Some(CyclePhase::Research),
            CyclePhase::Research => Some(CyclePhase::Design),
            CyclePhase::Design => Some(CyclePhase::Develop),
            CyclePhase::Develop => Some(CyclePhase::Test),
            CyclePhase::Test => Some(CyclePhase::Verify),
            CyclePhase::Verify => Some(CyclePhase::Gate),
            CyclePhase::Gate => Some(CyclePhase::Merge),
            CyclePhase::Merge => None,
        }
    }
}

/// Evolution cycle status (migration 0029 `evolution_cycles.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CycleStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

impl CycleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CycleStatus::Running => "running",
            CycleStatus::Completed => "completed",
            CycleStatus::Cancelled => "cancelled",
            CycleStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "running" => Some(CycleStatus::Running),
            "completed" => Some(CycleStatus::Completed),
            "cancelled" => Some(CycleStatus::Cancelled),
            "failed" => Some(CycleStatus::Failed),
            _ => None,
        }
    }
}

/// Evolution task phase (migration 0029 `evolution_tasks.phase`).
/// Subset of `CyclePhase` — tasks only run in research..verify stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    Research,
    Design,
    Develop,
    Test,
    Verify,
}

impl TaskPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskPhase::Research => "research",
            TaskPhase::Design => "design",
            TaskPhase::Develop => "develop",
            TaskPhase::Test => "test",
            TaskPhase::Verify => "verify",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "research" => Some(TaskPhase::Research),
            "design" => Some(TaskPhase::Design),
            "develop" => Some(TaskPhase::Develop),
            "test" => Some(TaskPhase::Test),
            "verify" => Some(TaskPhase::Verify),
            _ => None,
        }
    }
}

/// Evolution artifact type (migration 0029 `evolution_artifacts.artifact_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    ResearchReport,
    DesignProposal,
    PromptCandidate,
    ConfigChange,
    NewRole,
    NewTeam,
    TestReport,
    Verification,
}

impl ArtifactType {
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactType::ResearchReport => "research_report",
            ArtifactType::DesignProposal => "design_proposal",
            ArtifactType::PromptCandidate => "prompt_candidate",
            ArtifactType::ConfigChange => "config_change",
            ArtifactType::NewRole => "new_role",
            ArtifactType::NewTeam => "new_team",
            ArtifactType::TestReport => "test_report",
            ArtifactType::Verification => "verification",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "research_report" => Some(ArtifactType::ResearchReport),
            "design_proposal" => Some(ArtifactType::DesignProposal),
            "prompt_candidate" => Some(ArtifactType::PromptCandidate),
            "config_change" => Some(ArtifactType::ConfigChange),
            "new_role" => Some(ArtifactType::NewRole),
            "new_team" => Some(ArtifactType::NewTeam),
            "test_report" => Some(ArtifactType::TestReport),
            "verification" => Some(ArtifactType::Verification),
            _ => None,
        }
    }
}

/// Evolution artifact status (migration 0029 `evolution_artifacts.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    PendingReview,
    Approved,
    Rejected,
    NeedsRevision,
}

impl ArtifactStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactStatus::PendingReview => "pending_review",
            ArtifactStatus::Approved => "approved",
            ArtifactStatus::Rejected => "rejected",
            ArtifactStatus::NeedsRevision => "needs_revision",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending_review" => Some(ArtifactStatus::PendingReview),
            "approved" => Some(ArtifactStatus::Approved),
            "rejected" => Some(ArtifactStatus::Rejected),
            "needs_revision" => Some(ArtifactStatus::NeedsRevision),
            _ => None,
        }
    }
}

/// Evolution task status (migration 0029 `evolution_tasks.status`).
/// Superset of `CycleStatus` plus `Pending` (tasks wait on dependencies).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(TaskStatus::Pending),
            "running" => Some(TaskStatus::Running),
            "completed" => Some(TaskStatus::Completed),
            "failed" => Some(TaskStatus::Failed),
            "cancelled" => Some(TaskStatus::Cancelled),
            _ => None,
        }
    }
}

/// Gate decision kind (migration 0030 `evolution_gate_decisions.decision`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateDecisionKind {
    Approve,
    Reject,
    RequestChanges,
}

impl GateDecisionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            GateDecisionKind::Approve => "approve",
            GateDecisionKind::Reject => "reject",
            GateDecisionKind::RequestChanges => "request_changes",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "approve" => Some(GateDecisionKind::Approve),
            "reject" => Some(GateDecisionKind::Reject),
            "request_changes" => Some(GateDecisionKind::RequestChanges),
            _ => None,
        }
    }
}

/// Evolution trigger source (migration 0029 `evolution_cycles.trigger_source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    User,
    Scheduled,
    SelfReflect,
}

impl TriggerSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerSource::User => "user",
            TriggerSource::Scheduled => "scheduled",
            TriggerSource::SelfReflect => "self_reflect",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "user" => Some(TriggerSource::User),
            "scheduled" => Some(TriggerSource::Scheduled),
            "self_reflect" => Some(TriggerSource::SelfReflect),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_role_kind_roundtrip() {
        for kind in DevRoleKind::all() {
            assert_eq!(DevRoleKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(DevRoleKind::parse("bogus"), None);
        assert_eq!(DevRoleKind::all().len(), 5);
    }

    #[test]
    fn cycle_phase_roundtrip() {
        let all = [
            CyclePhase::Cleanse,
            CyclePhase::Research,
            CyclePhase::Design,
            CyclePhase::Develop,
            CyclePhase::Test,
            CyclePhase::Verify,
            CyclePhase::Gate,
            CyclePhase::Merge,
        ];
        for p in all {
            assert_eq!(CyclePhase::parse(p.as_str()), Some(p));
        }
        assert_eq!(CyclePhase::parse("bogus"), None);
    }

    #[test]
    fn cycle_phase_next_flow() {
        assert_eq!(CyclePhase::Cleanse.next(), Some(CyclePhase::Research));
        assert_eq!(CyclePhase::Research.next(), Some(CyclePhase::Design));
        assert_eq!(CyclePhase::Design.next(), Some(CyclePhase::Develop));
        assert_eq!(CyclePhase::Develop.next(), Some(CyclePhase::Test));
        assert_eq!(CyclePhase::Test.next(), Some(CyclePhase::Verify));
        assert_eq!(CyclePhase::Verify.next(), Some(CyclePhase::Gate));
        assert_eq!(CyclePhase::Gate.next(), Some(CyclePhase::Merge));
        assert_eq!(CyclePhase::Merge.next(), None);
    }

    #[test]
    fn cycle_status_roundtrip() {
        let all = [
            CycleStatus::Running,
            CycleStatus::Completed,
            CycleStatus::Cancelled,
            CycleStatus::Failed,
        ];
        for s in all {
            assert_eq!(CycleStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(CycleStatus::parse("bogus"), None);
    }

    #[test]
    fn task_phase_roundtrip() {
        let all = [
            TaskPhase::Research,
            TaskPhase::Design,
            TaskPhase::Develop,
            TaskPhase::Test,
            TaskPhase::Verify,
        ];
        for p in all {
            assert_eq!(TaskPhase::parse(p.as_str()), Some(p));
        }
        assert_eq!(TaskPhase::parse("cleanse"), None);
        assert_eq!(TaskPhase::parse("gate"), None);
        assert_eq!(TaskPhase::parse("merge"), None);
    }

    #[test]
    fn artifact_type_roundtrip() {
        let all = [
            ArtifactType::ResearchReport,
            ArtifactType::DesignProposal,
            ArtifactType::PromptCandidate,
            ArtifactType::ConfigChange,
            ArtifactType::NewRole,
            ArtifactType::NewTeam,
            ArtifactType::TestReport,
            ArtifactType::Verification,
        ];
        for t in all {
            assert_eq!(ArtifactType::parse(t.as_str()), Some(t));
        }
        assert_eq!(ArtifactType::parse("bogus"), None);
    }

    #[test]
    fn artifact_status_roundtrip() {
        let all = [
            ArtifactStatus::PendingReview,
            ArtifactStatus::Approved,
            ArtifactStatus::Rejected,
            ArtifactStatus::NeedsRevision,
        ];
        for s in all {
            assert_eq!(ArtifactStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(ArtifactStatus::parse("bogus"), None);
    }

    #[test]
    fn trigger_source_roundtrip() {
        let all = [
            TriggerSource::User,
            TriggerSource::Scheduled,
            TriggerSource::SelfReflect,
        ];
        for t in all {
            assert_eq!(TriggerSource::parse(t.as_str()), Some(t));
        }
        assert_eq!(TriggerSource::parse("bogus"), None);
    }

    #[test]
    fn task_status_roundtrip() {
        let all = [
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::Completed,
            TaskStatus::Failed,
            TaskStatus::Cancelled,
        ];
        for s in all {
            assert_eq!(TaskStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(TaskStatus::parse("bogus"), None);
    }

    #[test]
    fn gate_decision_kind_roundtrip() {
        let all = [
            GateDecisionKind::Approve,
            GateDecisionKind::Reject,
            GateDecisionKind::RequestChanges,
        ];
        for d in all {
            assert_eq!(GateDecisionKind::parse(d.as_str()), Some(d));
        }
        assert_eq!(GateDecisionKind::parse("bogus"), None);
    }
}
