//! Tauri command wrappers: thin delegates to commands::* impls.

use crate::commands;
use crate::ipc_error::IpcError;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub fn plugin_list() -> Result<commands::PluginListResultDto, IpcError> {
    Ok(commands::impl_plugin_list())
}

#[tauri::command]
#[specta::specta]
pub fn plugin_install_from_path(path: String) -> Result<commands::PluginInfoDto, IpcError> {
    commands::impl_plugin_install_from_path(path)
}

#[tauri::command]
#[specta::specta]
pub fn plugin_uninstall(plugin_id: String) -> Result<(), IpcError> {
    commands::impl_plugin_uninstall(plugin_id)
}

#[tauri::command]
#[specta::specta]
pub fn plugin_open_dir() -> Result<(), IpcError> {
    commands::impl_plugin_open_dir()
}

#[tauri::command]
#[specta::specta]
pub async fn plugin_editor_call(
    state: tauri::State<'_, AppState>,
    plugin_id: String,
    method: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    commands::impl_plugin_editor_call(&state, plugin_id, method, params).await
}

#[tauri::command]
#[specta::specta]
pub async fn app_setting_get(
    state: tauri::State<'_, AppState>,
    key: String,
) -> Result<Option<String>, IpcError> {
    commands::impl_app_setting_get(&state, key).await
}

#[tauri::command]
#[specta::specta]
pub async fn app_setting_set(
    state: tauri::State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), IpcError> {
    commands::impl_app_setting_set(&state, key, value).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_view_scope(
    state: tauri::State<'_, AppState>,
    surface: String,
) -> Result<String, IpcError> {
    commands::impl_get_view_scope(&state, surface).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_view_scope(
    state: tauri::State<'_, AppState>,
    surface: String,
    scope: String,
) -> Result<(), IpcError> {
    commands::impl_set_view_scope(&state, surface, scope).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_session(
    state: tauri::State<'_, AppState>,
) -> Result<commands::SessionDto, IpcError> {
    commands::impl_create_session(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::SessionDto>, IpcError> {
    commands::impl_list_sessions(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn resume_session(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<(), IpcError> {
    commands::impl_resume_session(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_events(
    state: tauri::State<'_, AppState>,
    session_id: String,
    after_seq: i64,
) -> Result<Vec<commands::EventDto>, IpcError> {
    commands::impl_list_events(&state, session_id, after_seq).await
}

#[tauri::command]
#[specta::specta]
pub async fn submit_task(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: String,
    input: String,
) -> Result<commands::RunResultDto, IpcError> {
    commands::impl_submit_task(app_handle, &state, session_id, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn enqueue_message(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: String,
    input: String,
) -> Result<commands::MessageQueueItemDto, IpcError> {
    commands::impl_enqueue_message(app_handle, &state, session_id, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_message_queue(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<Vec<commands::MessageQueueItemDto>, IpcError> {
    commands::impl_list_message_queue(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_message_queue_item(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), IpcError> {
    commands::impl_cancel_message_queue_item(&state, id).await
}

#[tauri::command]
#[specta::specta]
pub async fn clear_message_queue(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<usize, IpcError> {
    commands::impl_clear_message_queue(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_task(
    state: tauri::State<'_, AppState>,
    title: String,
    description: String,
) -> Result<commands::TaskDto, IpcError> {
    commands::impl_create_task(&state, title, description).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_tasks(
    state: tauri::State<'_, AppState>,
    status: Option<String>,
    workspace_id: Option<String>,
) -> Result<Vec<commands::TaskDto>, IpcError> {
    commands::impl_list_tasks(&state, status, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_task_status(
    state: tauri::State<'_, AppState>,
    task_id: String,
    status: String,
) -> Result<(), IpcError> {
    commands::impl_update_task_status(&state, task_id, status).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_task(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<(), IpcError> {
    commands::impl_delete_task(&state, task_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_run(
    state: tauri::State<'_, AppState>,
    run_id: String,
) -> Result<commands::RunDto, IpcError> {
    commands::impl_get_run(&state, run_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_runs_by_task(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<Vec<commands::RunDto>, IpcError> {
    commands::impl_list_runs_by_task(&state, task_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_pending_approvals(
    state: tauri::State<'_, AppState>,
    workspace_id: Option<String>,
) -> Result<Vec<commands::ApprovalDto>, IpcError> {
    commands::impl_list_pending_approvals(&state, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn resolve_approval(
    state: tauri::State<'_, AppState>,
    approval_id: String,
    approved: bool,
) -> Result<(), IpcError> {
    commands::impl_resolve_approval(&state, approval_id, approved).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_dir(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<Vec<commands::FileEntryDto>, IpcError> {
    commands::impl_list_dir(&state, path).await
}

#[tauri::command]
#[specta::specta]
pub async fn read_file(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<String, IpcError> {
    commands::impl_read_file(&state, path).await
}

#[tauri::command]
#[specta::specta]
pub async fn write_file(
    state: tauri::State<'_, AppState>,
    path: String,
    content: String,
) -> Result<(), IpcError> {
    commands::impl_write_file(&state, path, content).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_status(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::GitStatusDto>, IpcError> {
    commands::impl_git_status(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_log(
    state: tauri::State<'_, AppState>,
    limit: u32,
) -> Result<Vec<commands::GitCommitDto>, IpcError> {
    commands::impl_git_log(&state, limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_stage(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
) -> Result<(), IpcError> {
    commands::impl_git_stage(&state, paths).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_commit(
    state: tauri::State<'_, AppState>,
    message: String,
) -> Result<String, IpcError> {
    commands::impl_git_commit(&state, message).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_push(
    state: tauri::State<'_, AppState>,
    remote: String,
    branch: String,
) -> Result<String, IpcError> {
    commands::impl_git_push(&state, remote, branch).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_worktrees(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::GitWorktreeDto>, IpcError> {
    commands::impl_git_worktrees(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_diff(
    state: tauri::State<'_, AppState>,
    path: String,
    staged: bool,
) -> Result<String, IpcError> {
    commands::impl_git_diff(&state, path, staged).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_staged_diff(state: tauri::State<'_, AppState>) -> Result<String, IpcError> {
    commands::impl_git_staged_diff(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_commit_agents(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::CommitAgentOptionDto>, IpcError> {
    commands::impl_list_commit_agents(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn ai_commit_generate(
    state: tauri::State<'_, AppState>,
    role_agent: Option<commands::AgentRefInput>,
) -> Result<commands::AiCommitResultDto, IpcError> {
    commands::impl_ai_commit_generate(&state, role_agent).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_schedule(
    state: tauri::State<'_, AppState>,
    name: String,
    cron_expr: String,
    task_title: String,
    task_description: String,
) -> Result<commands::ScheduleDto, IpcError> {
    commands::impl_create_schedule(&state, name, cron_expr, task_title, task_description).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_schedules(
    state: tauri::State<'_, AppState>,
    workspace_id: Option<String>,
) -> Result<Vec<commands::ScheduleDto>, IpcError> {
    commands::impl_list_schedules(&state, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn toggle_schedule(
    state: tauri::State<'_, AppState>,
    schedule_id: String,
    enabled: bool,
) -> Result<(), IpcError> {
    commands::impl_toggle_schedule(&state, schedule_id, enabled).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_schedule(
    state: tauri::State<'_, AppState>,
    schedule_id: String,
) -> Result<(), IpcError> {
    commands::impl_delete_schedule(&state, schedule_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn upsert_provider(
    state: tauri::State<'_, AppState>,
    provider: commands::ProviderInput,
) -> Result<(), IpcError> {
    commands::impl_upsert_provider(&state, provider).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_providers(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::ProviderDto>, IpcError> {
    commands::impl_list_providers(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_provider(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    force: Option<bool>,
) -> Result<(), IpcError> {
    commands::impl_delete_provider(&state, provider_id, force.unwrap_or(false)).await
}

#[tauri::command]
#[specta::specta]
pub async fn test_provider_connection(
    state: tauri::State<'_, AppState>,
    input: commands::TestProviderConnectionInput,
) -> Result<commands::TestProviderConnectionDto, IpcError> {
    commands::impl_test_provider_connection(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_provider_models(
    state: tauri::State<'_, AppState>,
    input: commands::ListProviderModelsInput,
) -> Result<commands::ListProviderModelsDto, IpcError> {
    commands::impl_list_provider_models(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_sensitive_tools(
    state: tauri::State<'_, AppState>,
    patterns: Vec<String>,
) -> Result<(), IpcError> {
    commands::impl_set_sensitive_tools(&state, patterns).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_sensitive_tools(
    state: tauri::State<'_, AppState>,
) -> Result<Option<Vec<String>>, IpcError> {
    commands::impl_get_sensitive_tools(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_online_authorized(
    state: tauri::State<'_, AppState>,
    authorized: bool,
) -> Result<(), IpcError> {
    commands::impl_set_online_authorized(&state, authorized).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_online_authorized(state: tauri::State<'_, AppState>) -> Result<bool, IpcError> {
    commands::impl_get_online_authorized(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_evolution_settings(
    state: tauri::State<'_, AppState>,
) -> Result<commands::EvolutionSettingsDto, IpcError> {
    commands::impl_get_evolution_settings(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_evolution_settings(
    state: tauri::State<'_, AppState>,
    settings: commands::EvolutionSettingsDto,
) -> Result<(), IpcError> {
    commands::impl_set_evolution_settings(&state, settings).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_workspace(
    state: tauri::State<'_, AppState>,
) -> Result<commands::WorkspaceInfo, IpcError> {
    commands::impl_get_workspace(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_workspace(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<commands::WorkspaceInfo, IpcError> {
    commands::impl_set_workspace(&state, path).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_workspaces(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::WorkspaceEntryDto>, IpcError> {
    commands::impl_list_workspaces(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn add_workspace(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<commands::WorkspaceEntryDto, IpcError> {
    commands::impl_add_workspace(&state, path).await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_workspace(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<commands::RemoveWorkspaceResult, IpcError> {
    commands::impl_remove_workspace(&state, id).await
}

#[tauri::command]
#[specta::specta]
pub async fn activate_workspace(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<commands::WorkspaceEntryDto, IpcError> {
    commands::impl_activate_workspace(&state, id).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_active_workspace(
    state: tauri::State<'_, AppState>,
) -> Result<Option<commands::WorkspaceEntryDto>, IpcError> {
    commands::impl_get_active_workspace(&state).await
}

// ---- Multi-workspace open-set commands ----

#[tauri::command]
#[specta::specta]
pub async fn open_workspace(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<commands::OpenWorkspaceResult, IpcError> {
    commands::impl_open_workspace(&state, id).await
}

#[tauri::command]
#[specta::specta]
pub async fn close_workspace(
    state: tauri::State<'_, AppState>,
    id: String,
    force: bool,
) -> Result<commands::CloseWorkspaceResult, IpcError> {
    commands::impl_close_workspace(&state, id, force).await
}

#[tauri::command]
#[specta::specta]
pub async fn focus_workspace(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<commands::FocusWorkspaceResult, IpcError> {
    commands::impl_focus_workspace(&state, id).await
}

#[tauri::command]
#[specta::specta]
pub async fn close_all_workspaces(
    state: tauri::State<'_, AppState>,
    exclude_pinned: bool,
) -> Result<Vec<commands::CloseWorkspaceResult>, IpcError> {
    commands::impl_close_all_workspaces(&state, exclude_pinned).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_open_set(
    state: tauri::State<'_, AppState>,
) -> Result<commands::OpenSetDto, IpcError> {
    commands::impl_get_open_set(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn pin_workspace(state: tauri::State<'_, AppState>, id: String) -> Result<(), IpcError> {
    commands::impl_pin_workspace(&state, id).await
}

#[tauri::command]
#[specta::specta]
pub async fn unpin_workspace(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), IpcError> {
    commands::impl_unpin_workspace(&state, id).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_layout_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<Option<commands::LayoutSnapshotDto>, IpcError> {
    commands::impl_get_layout_snapshot(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_layout_snapshot(
    state: tauri::State<'_, AppState>,
    mode: String,
    split_workspace_ids: Option<[String; 2]>,
) -> Result<(), IpcError> {
    commands::impl_set_layout_snapshot(&state, mode, split_workspace_ids).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_recent_workspaces(
    state: tauri::State<'_, AppState>,
    limit: u32,
) -> Result<Vec<commands::RecentWorkspaceDto>, IpcError> {
    commands::impl_get_recent_workspaces(&state, limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn cross_workspace_search(
    state: tauri::State<'_, AppState>,
    query: String,
    match_content: bool,
) -> Result<commands::CrossSearchOutcomeDto, IpcError> {
    commands::impl_cross_workspace_search(&state, query, match_content).await
}

#[tauri::command]
#[specta::specta]
pub async fn cross_workspace_reference(
    state: tauri::State<'_, AppState>,
    source_workspace_id: String,
    file_path: String,
) -> Result<commands::FileReferenceDto, IpcError> {
    commands::impl_cross_workspace_reference(&state, source_workspace_id, file_path).await
}

#[tauri::command]
#[specta::specta]
pub async fn cross_workspace_compare(
    state: tauri::State<'_, AppState>,
    workspace_a: String,
    file_a: String,
    workspace_b: String,
    file_b: String,
) -> Result<commands::DiffResultDto, IpcError> {
    commands::impl_cross_workspace_compare(&state, workspace_a, file_a, workspace_b, file_b).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_orphan_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::OrphanSessionDto>, IpcError> {
    commands::impl_list_orphan_sessions(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn detect_isolation_violations(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::IsolationViolationDto>, IpcError> {
    commands::impl_detect_isolation_violations(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn reclaim_orphan_sessions(
    state: tauri::State<'_, AppState>,
    workspace_id: String,
) -> Result<u64, IpcError> {
    commands::impl_reclaim_orphan_sessions(&state, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_agent_profiles(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::AgentProfileDto>, IpcError> {
    commands::impl_list_agent_profiles(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn upsert_agent_profile(
    state: tauri::State<'_, AppState>,
    profile: commands::AgentProfileInput,
) -> Result<commands::AgentProfileDto, IpcError> {
    commands::impl_upsert_agent_profile(&state, profile).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_agent_profile(
    state: tauri::State<'_, AppState>,
    profile_id: String,
) -> Result<(), IpcError> {
    commands::impl_delete_agent_profile(&state, profile_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn check_cli_agent(
    state: tauri::State<'_, AppState>,
    profile_id: String,
) -> Result<commands::CliAgentCheckDto, IpcError> {
    commands::impl_check_cli_agent(&state, profile_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_roles(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::RoleDto>, IpcError> {
    commands::impl_list_roles(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn upsert_role(
    state: tauri::State<'_, AppState>,
    role: commands::RoleInput,
) -> Result<commands::RoleDto, IpcError> {
    commands::impl_upsert_role(&state, role).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_role(
    state: tauri::State<'_, AppState>,
    role_id: String,
    force: Option<bool>,
) -> Result<(), IpcError> {
    commands::impl_delete_role(&state, role_id, force.unwrap_or(false)).await
}

// ---------- role capability system (presets / director / routing) ----------

#[tauri::command]
#[specta::specta]
pub async fn seed_builtin_roles(
    state: tauri::State<'_, AppState>,
) -> Result<commands::SeedRolesDto, IpcError> {
    commands::impl_seed_builtin_roles(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn generate_role(
    state: tauri::State<'_, AppState>,
    description: String,
    binding: commands::RoleDirectorBindingDto,
) -> Result<commands::RoleDto, IpcError> {
    commands::impl_generate_role(&state, description, binding).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_role_director_binding(
    state: tauri::State<'_, AppState>,
) -> Result<Option<commands::RoleDirectorBindingDto>, IpcError> {
    commands::impl_get_role_director_binding(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_role_director_binding(
    state: tauri::State<'_, AppState>,
    binding: commands::RoleDirectorBindingDto,
) -> Result<(), IpcError> {
    commands::impl_set_role_director_binding(&state, binding).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_routing_rules(
    state: tauri::State<'_, AppState>,
) -> Result<commands::RoutingRulesDto, IpcError> {
    commands::impl_get_routing_rules(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_routing_rules(
    state: tauri::State<'_, AppState>,
    rules: commands::RoutingRulesDto,
) -> Result<(), IpcError> {
    commands::impl_set_routing_rules(&state, rules).await
}

#[tauri::command]
#[specta::specta]
pub async fn route_capability(
    state: tauri::State<'_, AppState>,
    request: commands::RouteRequestDto,
) -> Result<commands::RouteResultDto, IpcError> {
    commands::impl_route_capability(&state, request).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_teams(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::TeamDto>, IpcError> {
    commands::impl_list_teams(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn upsert_team(
    state: tauri::State<'_, AppState>,
    team: commands::TeamInput,
) -> Result<commands::TeamDto, IpcError> {
    commands::impl_upsert_team(&state, team).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_team(
    state: tauri::State<'_, AppState>,
    team_id: String,
) -> Result<(), IpcError> {
    commands::impl_delete_team(&state, team_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_whiteboard_notes(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<Vec<commands::WhiteBoardNoteDto>, IpcError> {
    commands::impl_list_whiteboard_notes(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn form_team(
    state: tauri::State<'_, AppState>,
    task: String,
    session_id: Option<String>,
) -> Result<commands::TeamDto, IpcError> {
    commands::impl_form_team(&state, task, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn preview_team(
    state: tauri::State<'_, AppState>,
    task: String,
) -> Result<commands::TeamPlanDto, IpcError> {
    commands::impl_preview_team(&state, task).await
}

#[tauri::command]
#[specta::specta]
pub async fn run_team_on_task(
    state: tauri::State<'_, AppState>,
    task_id: String,
    team_id: String,
) -> Result<commands::RunDto, IpcError> {
    commands::impl_run_team_on_task(&state, task_id, team_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn run_team_session(
    state: tauri::State<'_, AppState>,
    session_id: String,
    team_id: String,
    task: String,
) -> Result<commands::TeamRunResultDto, IpcError> {
    commands::impl_run_team_session(&state, session_id, team_id, task).await
}

// ---------- integrations (SPEC bots-telemetry-m1 B4) ----------

#[tauri::command]
#[specta::specta]
pub async fn list_integrations(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::IntegrationDto>, IpcError> {
    commands::impl_list_integrations(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn upsert_integration(
    state: tauri::State<'_, AppState>,
    input: commands::IntegrationInput,
) -> Result<commands::IntegrationDto, IpcError> {
    commands::impl_upsert_integration(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_integration(
    state: tauri::State<'_, AppState>,
    integration_id: String,
) -> Result<(), IpcError> {
    commands::impl_delete_integration(&state, integration_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn test_integration(
    state: tauri::State<'_, AppState>,
    integration_id: String,
) -> Result<commands::TestIntegrationDto, IpcError> {
    commands::impl_test_integration(&state, integration_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn journal_rollback(state: tauri::State<'_, AppState>, seq: u64) -> Result<(), IpcError> {
    commands::impl_journal_rollback(&state, seq).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_conversation(
    state: tauri::State<'_, AppState>,
    input: commands::ConversationInput,
) -> Result<commands::ConversationDto, IpcError> {
    commands::impl_create_conversation(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_conversations(
    state: tauri::State<'_, AppState>,
    kind: Option<String>,
    workspace_id: Option<String>,
) -> Result<Vec<commands::ConversationDto>, IpcError> {
    commands::impl_list_conversations(&state, kind, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_conversation(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<commands::ConversationDto, IpcError> {
    commands::impl_get_conversation(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_conversation_agent(
    state: tauri::State<'_, AppState>,
    session_id: String,
    agent: Option<commands::AgentRefInput>,
) -> Result<commands::ConversationDto, IpcError> {
    commands::impl_set_conversation_agent(&state, session_id, agent).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_agent_options(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::AgentOptionDto>, IpcError> {
    commands::impl_list_agent_options(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_conversation(
    state: tauri::State<'_, AppState>,
    session_id: String,
    input: commands::ConversationUpdateInput,
) -> Result<commands::ConversationDto, IpcError> {
    commands::impl_update_conversation(&state, session_id, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn add_conversation_agent(
    state: tauri::State<'_, AppState>,
    session_id: String,
    agent: commands::AgentRefInput,
) -> Result<commands::ConversationDto, IpcError> {
    commands::impl_add_conversation_agent(&state, session_id, agent).await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_conversation_agent(
    state: tauri::State<'_, AppState>,
    session_id: String,
    agent: commands::AgentRefInput,
) -> Result<commands::ConversationDto, IpcError> {
    commands::impl_remove_conversation_agent(&state, session_id, agent).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_conversation(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<(), IpcError> {
    commands::impl_delete_conversation(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn clear_conversations(
    state: tauri::State<'_, AppState>,
    workspace_id: Option<String>,
) -> Result<usize, IpcError> {
    commands::impl_clear_conversations(&state, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_agent_detail(
    state: tauri::State<'_, AppState>,
    agent_kind: String,
    agent_id: String,
) -> Result<commands::AgentDetailDto, IpcError> {
    commands::impl_get_agent_detail(&state, agent_kind, agent_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn submit_message(
    state: tauri::State<'_, AppState>,
    session_id: String,
    text: String,
    attachment_ids: Vec<String>,
    route_target_agent_ids: Option<Vec<String>>,
    context_injection_ids: Option<Vec<String>>,
) -> Result<commands::RunResultDto, IpcError> {
    commands::impl_submit_message(
        &state,
        session_id,
        text,
        attachment_ids,
        route_target_agent_ids,
        context_injection_ids,
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_conversation(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<(), IpcError> {
    commands::impl_stop_conversation(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_active_runs(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::RunDto>, IpcError> {
    commands::impl_list_active_runs(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_run(state: tauri::State<'_, AppState>, run_id: String) -> Result<(), IpcError> {
    commands::impl_cancel_run(&state, run_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_attachment(
    state: tauri::State<'_, AppState>,
    session_id: String,
    name: String,
    mime: String,
    data_base64: String,
) -> Result<commands::AttachmentDto, IpcError> {
    commands::impl_save_attachment(&state, session_id, name, mime, data_base64).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_attachments(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<Vec<commands::AttachmentDto>, IpcError> {
    commands::impl_list_attachments(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_attachment(
    state: tauri::State<'_, AppState>,
    attachment_id: String,
) -> Result<(), IpcError> {
    commands::impl_delete_attachment(&state, attachment_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn upsert_schedule(
    state: tauri::State<'_, AppState>,
    input: commands::ScheduleInput,
) -> Result<commands::ScheduleDto, IpcError> {
    commands::impl_upsert_schedule(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_schedule(
    state: tauri::State<'_, AppState>,
    schedule_id: String,
    input: commands::ScheduleInput,
) -> Result<commands::ScheduleDto, IpcError> {
    commands::impl_update_schedule(&state, schedule_id, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn inject_context(
    state: tauri::State<'_, AppState>,
    session_id: String,
    input: commands::ContextInjectionInput,
) -> Result<commands::ContextInjectionDto, IpcError> {
    commands::impl_inject_context(&state, session_id, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_injectable_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::InjectableSessionDto>, IpcError> {
    commands::impl_list_injectable_sessions(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_injectable_rules(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::InjectableRuleDto>, IpcError> {
    commands::impl_list_injectable_rules(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn transcribe_audio(
    state: tauri::State<'_, AppState>,
    audio_base64: String,
    model_source: Option<String>,
) -> Result<String, IpcError> {
    commands::impl_transcribe_audio(&state, audio_base64, model_source).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_asr_models(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::AsrModelDto>, IpcError> {
    commands::impl_list_asr_models(&state).await
}

/// Frontend heartbeat: called once after the React root mounts to signal that
/// the webview is alive. If this never arrives within the watchdog grace
/// period, the shell auto-reloads the webview (see `shell_resilience`).
#[tauri::command]
#[specta::specta]
pub fn __nuomi_heartbeat(state: tauri::State<'_, crate::shell_resilience::WatchdogState>) {
    state.mark_ready();
}
