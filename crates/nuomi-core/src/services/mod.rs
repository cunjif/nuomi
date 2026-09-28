//! Out-of-kernel domain services for the UI M1 surfaces: sandboxed
//! workspace file access, whitelisted git subprocesses, scheduler.
//! Each service owns its own `thiserror` error type.

pub mod ai_commit_service;
pub mod capability_router;
pub mod codebase_memory_migrator;
pub mod conversation_service;
pub mod cross_workspace;
pub mod debug_event_publisher;
pub mod git_service;
pub mod nuomi_dir;
pub mod presets;
pub mod reference_pre_check;
pub mod role_director;
pub mod scheduler_service;
pub mod single_role_materializer;
pub mod team_former;
pub mod team_runner;
pub mod workspace;
pub mod workspace_guard;
pub mod workspace_layout;
pub mod workspace_migration;
pub mod workspace_open_set;
pub mod workspace_palette;
pub mod workspace_registry;

pub use crate::store::repos::workspace_layout_snapshot::LayoutMode;
pub use ai_commit_service::{
    generate as ai_commit_generate, get_default_agent as ai_commit_get_default_agent,
    list_commit_agents as ai_commit_list_commit_agents,
    set_default_agent as ai_commit_set_default_agent, AiCommitError, AiCommitResult,
    CommitAgentOption, AI_COMMIT_DEFAULT_AGENT_KEY, AI_COMMIT_DIFF_LIMIT, AI_COMMIT_TIMEOUT_MS,
};
pub use capability_router::{
    cleanup_expired_temps, cleanup_temp, load_routing_rules, route, save_routing_rules,
    RouteOutcome, RouteRequest, RoutingError, RoutingRules, ROUTING_RULES_KEY,
};
pub use codebase_memory_migrator::{
    is_migrated, migrate as migrate_codebase_memory, MigrationError, MigrationResult,
};
pub use conversation_service::{
    compose_user_message, create_conversation, is_role_ready, name_agent_ref, resolve_agent,
    resolve_default_agent, resolve_participants, AgentRef, ResolvedAgent, DEFAULT_AGENT_KEY,
};
pub use cross_workspace::{
    CrossSearchOutcome, CrossSearchResultGroup, CrossWorkspaceError, CrossWorkspaceService,
    DiffResult, FileMatch, FileReference, MatchType,
};
pub use debug_event_publisher::{
    emit_env_fallback, emit_materialize_warnings, emit_materialized, emit_provider_missing,
    emit_role_applied,
};
pub use git_service::{
    create_worktree, merge_base_into_worktree, merge_worktree_into_base, recover_merges,
    remove_worktree, worktree_path, CommitInfo, GitError, GitService, MergeIntent, RecoveryItem,
    StageAOutcome, StageBResult, StatusEntry, WorktreeInfo, WorktreeMergeError, BRANCH_PREFIX,
    NUOMI_META_DIR, WORKTREES_DIR,
};
pub use nuomi_dir::{codebase_memory_dir, ensure_nuomi_dir, NuomiDirError, NUOMI_DIR_NAME};
pub use presets::{seed_builtin_roles, SeedReport, PRESET_ROLES};
pub use reference_pre_check::{
    check_provider_refs, check_role_refs, delete_and_nullify_provider_refs,
    delete_and_nullify_role_refs, detect_missing_provider, EntityRefs, MissingProviderHint,
};
pub use role_director::{
    generate_role, get_role_director_binding, set_role_director_binding, GeneratedRoleDraft,
    RoleDirectorBinding, RoleDirectorBindingMode, RoleDirectorError,
};
pub use scheduler_service::{
    next_after, parse_schedule, CronExpr, FieldSet, ScheduleSpec, SchedulerError, SchedulerHandle,
    SchedulerRunner,
};
pub use single_role_materializer::{materialize_single_role, RoleOverlay, SingleRoleContext};
pub use team_former::{form_team, preview_team, FormedTeam, TeamPlan, TeamPlanMember};
pub use team_runner::{materialize, run_team, MaterializedProviders, TeamRunOutcome};
pub use workspace::{FileEntry, WorkspaceError, WorkspaceService};
pub use workspace_guard::{canonicalize as guard_canonicalize, is_blacklisted, PathGuardError};
pub use workspace_layout::{LayoutError, RestoreOutcome, WorkspaceLayoutService};
pub use workspace_migration::{
    run_if_needed as run_workspace_migration, MigrationOrchestrationError, MigrationOutcome,
};
pub use workspace_open_set::{
    OpenSetError, OpenSetProvider, WorkspaceOpenSetService, MAX_OPEN_WORKSPACES,
};
pub use workspace_palette::{color_for, hash as palette_hash, ColorTag, PALETTE};
pub use workspace_registry::{
    RegistryError, RemoveResult as WorkspaceRemoveResult, WorkspaceEntryWithPresence,
    WorkspaceRegistry,
};
