//! Shadow Team creation for RoleAgent self-dispatch (ADR-0030).
//!
//! Creates a single-member Team with a cloned "shadow Role" that mirrors
//! the parent RoleAgent's configuration. Bypasses `team_former::validate_plan`
//! (which requires >= 2 members) because self-dispatch is an explicit
//! single-role execution, not LLM-planned team composition.

use std::sync::Arc;

use crate::domain::{new_id, now_ms, Role, Team, TeamTopology};
use crate::store::{repos, Db};
use crate::{CoreError, CoreResult};

/// Outcome of [`create_shadow_team`]: the ids needed to dispatch a task
/// onto the newly created single-member team.
#[derive(Debug, Clone)]
pub struct ShadowTeam {
    pub team_id: String,
    pub shadow_role_id: String,
}

/// Creates a shadow Role + single-member Pipeline Team for self-dispatch.
///
/// 1. Reads the parent Role from DB.
/// 2. Creates a shadow Role (ephemeral=true, name=`{original}-subagent`)
///    with the same provider binding, system prompt, tool allowlist,
///    temperature, and params.
/// 3. Creates a Pipeline Team with the shadow Role as its sole member.
///
/// Does NOT call `team_former::draft_team_plan` or `validate_plan` —
/// self-dispatch is explicit, not LLM-planned.
pub async fn create_shadow_team(db_path: Arc<str>, role_id: &str) -> CoreResult<ShadowTeam> {
    let role_id = role_id.to_string();
    let db_path_clone = Arc::clone(&db_path);
    let role = tokio::task::spawn_blocking(move || -> CoreResult<Role> {
        let db = Db::open(&db_path_clone)?;
        repos::roles::get(&db.0, &role_id).map_err(CoreError::from)
    })
    .await
    .map_err(|e| {
        CoreError::Store(crate::store::StoreError::Sqlite(
            rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
        ))
    })??;

    let now = now_ms();
    let shadow_role_id = new_id();
    let shadow_role = Role {
        id: shadow_role_id.clone(),
        name: format!("{}-subagent", role.name),
        provider_id: role.provider_id.clone(),
        provider_ids: role.provider_ids.clone(),
        system_prompt_override: role.system_prompt_override.clone(),
        tool_allowlist: role.tool_allowlist.clone(),
        required_capabilities: role.required_capabilities.clone(),
        temperature: role.temperature,
        max_tokens: role.max_tokens,
        params: role.params.clone(),
        builtin: false,
        generated: false,
        ephemeral: true,
        source: Some(serde_json::json!({
            "kind": "shadow",
            "parent_role_id": role.id,
            "parent_role_name": role.name,
        })),
        created_at: now,
        updated_at: now,
    };

    let team_id = new_id();
    let team = Team {
        id: team_id.clone(),
        name: format!("{}-self-dispatch", role.name),
        topology: TeamTopology::Pipeline,
        member_role_ids: vec![shadow_role_id.clone()],
        config: serde_json::json!({}),
        created_at: now,
        updated_at: now,
    };

    let db_path_clone = Arc::clone(&db_path);
    tokio::task::spawn_blocking(move || -> CoreResult<()> {
        let db = Db::open(&db_path_clone)?;
        repos::roles::insert(&db.0, &shadow_role)?;
        repos::teams::insert(&db.0, &team)?;
        Ok(())
    })
    .await
    .map_err(|e| {
        CoreError::Store(crate::store::StoreError::Sqlite(
            rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
        ))
    })??;

    Ok(ShadowTeam {
        team_id,
        shadow_role_id,
    })
}
