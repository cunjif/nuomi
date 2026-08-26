//! Tauri command wrappers: thin delegates to commands::* impls.

use crate::commands;
use crate::ipc_error::IpcError;
use crate::state::AppState;

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
    state: tauri::State<'_, AppState>,
    session_id: String,
    input: String,
) -> Result<commands::RunResultDto, IpcError> {
    commands::impl_submit_task(&state, session_id, input).await
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
) -> Result<Vec<commands::TaskDto>, IpcError> {
    commands::impl_list_tasks(&state, status).await
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
) -> Result<Vec<commands::ApprovalDto>, IpcError> {
    commands::impl_list_pending_approvals(&state).await
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
) -> Result<Vec<commands::ScheduleDto>, IpcError> {
    commands::impl_list_schedules(&state).await
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
pub async fn get_workspace(state: tauri::State<'_, AppState>) -> Result<String, IpcError> {
    commands::impl_get_workspace(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_workspace(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<String, IpcError> {
    commands::impl_set_workspace(&state, path).await
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
) -> Result<(), IpcError> {
    commands::impl_delete_role(&state, role_id).await
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
