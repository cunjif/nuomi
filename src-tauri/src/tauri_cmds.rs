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
    quoted_event_seq: Option<i64>,
) -> Result<commands::RunResultDto, IpcError> {
    commands::impl_submit_task(app_handle, &state, session_id, input, quoted_event_seq).await
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
pub async fn list_proposals(
    state: tauri::State<'_, AppState>,
    workspace_id: Option<String>,
) -> Result<Vec<commands::ProposalDto>, IpcError> {
    commands::impl_list_proposals(&state, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn resolve_proposal(
    state: tauri::State<'_, AppState>,
    proposal_id: String,
    approved: bool,
) -> Result<(), IpcError> {
    commands::impl_resolve_proposal(&state, proposal_id, approved).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_dir(
    state: tauri::State<'_, AppState>,
    path: String,
    workspace_id: Option<String>,
) -> Result<Vec<commands::FileEntryDto>, IpcError> {
    commands::impl_list_dir(&state, path, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn read_file(
    state: tauri::State<'_, AppState>,
    path: String,
    workspace_id: Option<String>,
) -> Result<String, IpcError> {
    commands::impl_read_file(&state, path, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn write_file(
    state: tauri::State<'_, AppState>,
    path: String,
    content: String,
    workspace_id: Option<String>,
) -> Result<(), IpcError> {
    commands::impl_write_file(&state, path, content, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_file(
    state: tauri::State<'_, AppState>,
    path: String,
    content: String,
    workspace_id: Option<String>,
) -> Result<(), IpcError> {
    commands::impl_create_file(&state, path, content, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_dir(
    state: tauri::State<'_, AppState>,
    path: String,
    workspace_id: Option<String>,
) -> Result<(), IpcError> {
    commands::impl_create_dir(&state, path, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete(
    state: tauri::State<'_, AppState>,
    path: String,
    workspace_id: Option<String>,
) -> Result<(), IpcError> {
    commands::impl_delete(&state, path, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn rename(
    state: tauri::State<'_, AppState>,
    from: String,
    to: String,
    workspace_id: Option<String>,
) -> Result<(), IpcError> {
    commands::impl_rename(&state, from, to, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn copy(
    state: tauri::State<'_, AppState>,
    from: String,
    to: String,
    workspace_id: Option<String>,
) -> Result<(), IpcError> {
    commands::impl_copy(&state, from, to, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_status(
    state: tauri::State<'_, AppState>,
    workspace_id: Option<String>,
) -> Result<Vec<commands::GitStatusDto>, IpcError> {
    commands::impl_git_status(&state, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_log(
    state: tauri::State<'_, AppState>,
    limit: u32,
    workspace_id: Option<String>,
) -> Result<Vec<commands::GitCommitDto>, IpcError> {
    commands::impl_git_log(&state, limit, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_stage(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
    workspace_id: Option<String>,
) -> Result<(), IpcError> {
    commands::impl_git_stage(&state, paths, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_commit(
    state: tauri::State<'_, AppState>,
    message: String,
    workspace_id: Option<String>,
) -> Result<String, IpcError> {
    commands::impl_git_commit(&state, message, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_push(
    state: tauri::State<'_, AppState>,
    remote: String,
    branch: String,
    workspace_id: Option<String>,
) -> Result<String, IpcError> {
    commands::impl_git_push(&state, remote, branch, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_worktrees(
    state: tauri::State<'_, AppState>,
    workspace_id: Option<String>,
) -> Result<Vec<commands::GitWorktreeDto>, IpcError> {
    commands::impl_git_worktrees(&state, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_diff(
    state: tauri::State<'_, AppState>,
    path: String,
    staged: bool,
    workspace_id: Option<String>,
) -> Result<String, IpcError> {
    commands::impl_git_diff(&state, path, staged, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_staged_diff(
    state: tauri::State<'_, AppState>,
    workspace_id: Option<String>,
) -> Result<String, IpcError> {
    commands::impl_git_staged_diff(&state, workspace_id).await
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

// ---- Two-level workspace commands (ADR-0021) ----

#[tauri::command]
#[specta::specta]
pub async fn workspace_add_directory(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<commands::WorkspaceEntryDto, IpcError> {
    commands::impl_workspace_add_directory(&state, path).await
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_remove_directory(
    state: tauri::State<'_, AppState>,
    workspace_id: String,
    directory_id: String,
) -> Result<commands::RemoveDirectoryResultDto, IpcError> {
    commands::impl_workspace_remove_directory(&state, workspace_id, directory_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_mount_directory(
    state: tauri::State<'_, AppState>,
    workspace_id: String,
    directory_id: String,
) -> Result<(), IpcError> {
    commands::impl_workspace_mount_directory(&state, workspace_id, directory_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_unmount_directory(
    state: tauri::State<'_, AppState>,
    workspace_id: String,
    directory_id: String,
) -> Result<(), IpcError> {
    commands::impl_workspace_unmount_directory(&state, workspace_id, directory_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_close(
    state: tauri::State<'_, AppState>,
    workspace_id: String,
) -> Result<bool, IpcError> {
    commands::impl_workspace_close(&state, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_switch(
    state: tauri::State<'_, AppState>,
    workspace_id: String,
) -> Result<commands::WorkspaceEntryDto, IpcError> {
    commands::impl_workspace_switch(&state, workspace_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_get_open(
    state: tauri::State<'_, AppState>,
) -> Result<Option<commands::WorkspaceEntryDto>, IpcError> {
    commands::impl_workspace_get_open(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_focus_directory(
    state: tauri::State<'_, AppState>,
    workspace_id: String,
    directory_id: String,
) -> Result<commands::WorkspaceEntryDto, IpcError> {
    commands::impl_workspace_focus_directory(&state, workspace_id, directory_id).await
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
    quoted_event_seq: Option<i64>,
) -> Result<commands::RunResultDto, IpcError> {
    commands::impl_submit_message(
        &state,
        session_id,
        text,
        attachment_ids,
        route_target_agent_ids,
        context_injection_ids,
        quoted_event_seq,
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
pub async fn remember_memory(
    state: tauri::State<'_, AppState>,
    content: String,
    tags: Vec<String>,
    kind: String,
    user_profile: bool,
    role_id: Option<String>,
    source_session_id: Option<String>,
) -> Result<commands::MemoryEntryDto, IpcError> {
    commands::impl_remember_memory(
        &state,
        content,
        tags,
        kind,
        user_profile,
        role_id,
        source_session_id,
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn recall_memory(
    state: tauri::State<'_, AppState>,
    query: Option<String>,
    tag: Option<String>,
    role_id: Option<String>,
    limit: u32,
) -> Result<Vec<commands::MemoryEntryDto>, IpcError> {
    commands::impl_recall_memory(&state, query, tag, role_id, limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_memory(
    state: tauri::State<'_, AppState>,
    role_id: Option<String>,
    limit: u32,
) -> Result<Vec<commands::MemoryEntryDto>, IpcError> {
    commands::impl_list_memory(&state, role_id, limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_memory(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<bool, IpcError> {
    commands::impl_delete_memory(&state, id).await
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

// ---------- steward ----------

#[tauri::command]
#[specta::specta]
pub async fn steward_create_session(
    state: tauri::State<'_, AppState>,
    title: String,
) -> Result<commands::StewardSessionDto, IpcError> {
    commands::impl_steward_create_session(&state, title).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_list_sessions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::StewardSessionDto>, IpcError> {
    commands::impl_steward_list_sessions(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_ensure_singleton_session(
    state: tauri::State<'_, AppState>,
) -> Result<commands::StewardSessionDto, IpcError> {
    commands::impl_steward_ensure_singleton_session(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_get_attachment_limit(
    state: tauri::State<'_, AppState>,
) -> Result<i64, IpcError> {
    commands::impl_steward_get_attachment_limit(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_get_settings(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<commands::StewardSettingsDto, IpcError> {
    commands::impl_steward_get_settings(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_update_soul(
    state: tauri::State<'_, AppState>,
    session_id: String,
    content: String,
) -> Result<(), IpcError> {
    commands::impl_steward_update_soul(&state, session_id, content).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_update_binding(
    state: tauri::State<'_, AppState>,
    session_id: String,
    mode: commands::BindingModeDto,
    provider_id: Option<String>,
    agent_profile_id: Option<String>,
) -> Result<(), IpcError> {
    commands::impl_steward_update_binding(&state, session_id, mode, provider_id, agent_profile_id)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_clear_messages(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<(), IpcError> {
    commands::impl_steward_clear_messages(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_batch_delete_messages(
    state: tauri::State<'_, AppState>,
    session_id: String,
    message_ids: Vec<String>,
) -> Result<(), IpcError> {
    commands::impl_steward_batch_delete_messages(&state, session_id, message_ids).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_list_session_messages(
    state: tauri::State<'_, AppState>,
    session_id: String,
    limit: u32,
) -> Result<Vec<commands::StewardMessageDto>, IpcError> {
    commands::impl_steward_list_session_messages(&state, session_id, limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_send_message(
    state: tauri::State<'_, AppState>,
    session_id: String,
    text: String,
    attachment_ids: Option<Vec<String>>,
) -> Result<commands::StewardReplyDto, IpcError> {
    commands::impl_steward_send_message(&state, session_id, text, attachment_ids).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_save_attachment(
    state: tauri::State<'_, AppState>,
    session_id: String,
    name: String,
    mime: String,
    base64: String,
) -> Result<commands::StewardAttachmentDto, IpcError> {
    commands::impl_steward_save_attachment(&state, session_id, name, mime, base64).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_get_context_usage(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<commands::StewardContextUsageDto, IpcError> {
    commands::impl_steward_get_context_usage(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_optimize_prompt(
    state: tauri::State<'_, AppState>,
    session_id: String,
    draft: String,
) -> Result<commands::StewardPromptOptimizeResultDto, IpcError> {
    commands::impl_steward_optimize_prompt(&state, session_id, draft).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_context_usage(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<commands::ContextUsageDto, IpcError> {
    commands::impl_get_context_usage(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn optimize_prompt(
    state: tauri::State<'_, AppState>,
    session_id: String,
    draft: String,
) -> Result<commands::PromptOptimizeResultDto, IpcError> {
    commands::impl_optimize_prompt(&state, session_id, draft).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_get_app_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<commands::AppStateSnapshotDto, IpcError> {
    commands::impl_steward_get_app_snapshot(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_propose_config_change(
    state: tauri::State<'_, AppState>,
    target: commands::ConfigTargetInput,
    intent: String,
) -> Result<commands::ConfigProposalDto, IpcError> {
    commands::impl_steward_propose_config_change(&state, target, intent).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_confirm_config_change(
    state: tauri::State<'_, AppState>,
    proposal_id: String,
) -> Result<commands::ConfigChangeResultDto, IpcError> {
    commands::impl_steward_confirm_config_change(&state, proposal_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_cancel_config_change(
    state: tauri::State<'_, AppState>,
    session_id: String,
    proposal_id: String,
) -> Result<(), IpcError> {
    commands::impl_steward_cancel_config_change(&state, session_id, proposal_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_rollback_config_change(
    state: tauri::State<'_, AppState>,
    snapshot_id: String,
) -> Result<(), IpcError> {
    commands::impl_steward_rollback_config_change(&state, snapshot_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_get_dev_team(
    state: tauri::State<'_, AppState>,
) -> Result<commands::DevTeamDto, IpcError> {
    commands::impl_steward_get_dev_team(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_set_dev_role_binding(
    state: tauri::State<'_, AppState>,
    binding: commands::DevRoleBindingInput,
) -> Result<(), IpcError> {
    commands::impl_steward_set_dev_role_binding(&state, binding).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_list_cycle_tasks(
    state: tauri::State<'_, AppState>,
    cycle_id: String,
) -> Result<Vec<commands::EvolutionTaskDto>, IpcError> {
    commands::impl_steward_list_cycle_tasks(&state, cycle_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_cleanse_data(
    state: tauri::State<'_, AppState>,
    scope: commands::CleanseScopeInput,
    rules: commands::CleanseRulesInput,
) -> Result<commands::CleanseReportDto, IpcError> {
    commands::impl_steward_cleanse_data(&state, scope, rules).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_list_data_pools(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::EvolutionDataPoolDto>, IpcError> {
    commands::impl_steward_list_data_pools(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_trigger_evolution(
    state: tauri::State<'_, AppState>,
    instruction: String,
) -> Result<commands::EvolutionCycleDto, IpcError> {
    commands::impl_steward_trigger_evolution(&state, instruction).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_list_cycles(
    state: tauri::State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<commands::EvolutionCycleDto>, IpcError> {
    commands::impl_steward_list_cycles(&state, status).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_get_cycle(
    state: tauri::State<'_, AppState>,
    cycle_id: String,
) -> Result<commands::CycleDetailDto, IpcError> {
    commands::impl_steward_get_cycle(&state, cycle_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_cancel_cycle(
    state: tauri::State<'_, AppState>,
    cycle_id: String,
) -> Result<(), IpcError> {
    commands::impl_steward_cancel_cycle(&state, cycle_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_list_artifacts(
    state: tauri::State<'_, AppState>,
    cycle_id: Option<String>,
    status: Option<String>,
) -> Result<Vec<commands::EvolutionArtifactDto>, IpcError> {
    commands::impl_steward_list_artifacts(&state, cycle_id, status).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_get_artifact(
    state: tauri::State<'_, AppState>,
    artifact_id: String,
) -> Result<commands::EvolutionArtifactDto, IpcError> {
    commands::impl_steward_get_artifact(&state, artifact_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_resolve_gate(
    state: tauri::State<'_, AppState>,
    artifact_id: String,
    decision: commands::GateDecisionDto,
) -> Result<commands::ResolveOutcomeDto, IpcError> {
    commands::impl_steward_resolve_gate(&state, artifact_id, decision).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_resolve_gate_batch(
    state: tauri::State<'_, AppState>,
    cycle_id: String,
    decision: commands::GateDecisionDto,
) -> Result<Vec<commands::ResolveOutcomeDto>, IpcError> {
    commands::impl_steward_resolve_gate_batch(&state, cycle_id, decision).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_list_gate_pending(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::EvolutionArtifactDto>, IpcError> {
    commands::impl_steward_list_gate_pending(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_list_events(
    state: tauri::State<'_, AppState>,
    aggregate_id: Option<String>,
    kind_prefix: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<commands::StewardEventDto>, IpcError> {
    commands::impl_steward_list_events(&state, aggregate_id, kind_prefix, limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn steward_set_online_authorization(
    state: tauri::State<'_, AppState>,
    authorized: bool,
) -> Result<(), IpcError> {
    commands::impl_steward_set_online_authorization(&state, authorized).await
}

#[tauri::command]
#[specta::specta]
pub async fn rollback_session_events(
    state: tauri::State<'_, AppState>,
    session_id: String,
    to_seq: i64,
) -> Result<commands::RollbackResultDto, IpcError> {
    commands::impl_rollback_session_events(&state, session_id, to_seq).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_rollback_points(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<Vec<commands::RollbackPointDto>, IpcError> {
    commands::impl_list_rollback_points(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn sandbox_run_code(
    state: tauri::State<'_, AppState>,
    request: commands::SandboxRunCodeRequest,
) -> Result<commands::SandboxTaskDto, IpcError> {
    commands::impl_sandbox_run_code(&state, request).await
}

#[tauri::command]
#[specta::specta]
pub async fn sandbox_build_app(
    state: tauri::State<'_, AppState>,
    request: commands::SandboxBuildAppRequest,
) -> Result<commands::SandboxTaskDto, IpcError> {
    commands::impl_sandbox_build_app(&state, request).await
}

#[tauri::command]
#[specta::specta]
pub async fn sandbox_list_tasks(
    state: tauri::State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<commands::SandboxTaskDto>, IpcError> {
    commands::impl_sandbox_list_tasks(&state, limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn sandbox_get_task(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<commands::SandboxTaskDto, IpcError> {
    commands::impl_sandbox_get_task(&state, task_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn sandbox_cancel_task(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<(), IpcError> {
    commands::impl_sandbox_cancel_task(&state, task_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn sandbox_retry_artifact(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<commands::SandboxTaskDto, IpcError> {
    commands::impl_sandbox_retry_artifact(&state, task_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn sandbox_list_runtimes(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<commands::SandboxRuntimeDto>, IpcError> {
    commands::impl_sandbox_list_runtimes(&state).await
}

/// Frontend heartbeat: called once after the React root mounts to signal that
/// the webview is alive. If this never arrives within the watchdog grace
/// period, the shell auto-reloads the webview (see `shell_resilience`).
#[tauri::command]
#[specta::specta]
pub fn __nuomi_heartbeat(state: tauri::State<'_, crate::shell_resilience::WatchdogState>) {
    state.mark_ready();
}
