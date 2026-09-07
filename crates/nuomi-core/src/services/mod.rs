//! Out-of-kernel domain services for the UI M1 surfaces: sandboxed
//! workspace file access, whitelisted git subprocesses, scheduler.
//! Each service owns its own `thiserror` error type.

pub mod capability_router;
pub mod git_service;
pub mod presets;
pub mod role_director;
pub mod scheduler_service;
pub mod team_former;
pub mod team_runner;
pub mod workspace;

pub use capability_router::{
    cleanup_expired_temps, cleanup_temp, load_routing_rules, route, save_routing_rules,
    RouteOutcome, RouteRequest, RoutingError, RoutingRules, ROUTING_RULES_KEY,
};
pub use git_service::{
    create_worktree, merge_base_into_worktree, merge_worktree_into_base, recover_merges,
    remove_worktree, worktree_path, CommitInfo, GitError, GitService, MergeIntent, RecoveryItem,
    StageAOutcome, StageBResult, StatusEntry, WorktreeInfo, WorktreeMergeError, BRANCH_PREFIX,
    NUOMI_META_DIR, WORKTREES_DIR,
};
pub use presets::{seed_builtin_roles, SeedReport, PRESET_ROLES};
pub use role_director::{generate_role, GeneratedRoleDraft, RoleDirectorError};
pub use scheduler_service::{
    next_after, parse_schedule, CronExpr, FieldSet, ScheduleSpec, SchedulerError, SchedulerHandle,
    SchedulerRunner,
};
pub use team_former::{form_team, preview_team, FormedTeam, TeamPlan, TeamPlanMember};
pub use team_runner::{materialize, run_team, MaterializedProviders, TeamRunOutcome};
pub use workspace::{FileEntry, WorkspaceError, WorkspaceService};
