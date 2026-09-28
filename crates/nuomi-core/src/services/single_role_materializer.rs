//! Single Role/Provider execution context materialization (module B).
//!
//! Bridges `resolve_agent` + `team_runner::materialize` into facade
//! `run_turn` consumable parameters.
//! SPEC: docs/specs/settings-integration/spec.md (AC1, AC3)

use std::path::PathBuf;
use std::sync::Arc;

use crate::domain::AgentRefKind;
use crate::providers::{LlmProvider, SecretStore};
use crate::services::{materialize, MaterializedProviders, ResolvedAgent};
use crate::{CoreError, CoreResult};

/// Role overlay parameters extracted from a [`crate::domain::Role`] and
/// applied before constructing the `LoopEngine` — system prompt 拼接 /
/// temperature 覆盖 / tool allowlist 过滤.  `None` field = no override.
#[derive(Debug, Clone)]
pub struct RoleOverlay {
    pub system_prompt: Option<String>,
    pub temperature: Option<f64>,
    pub tool_allowlist: Vec<String>,
}

/// The execution context for a single-role turn: either a materialized
/// DB provider + optional role overlay, or env fallback (no DB config).
pub enum SingleRoleContext {
    Materialized {
        provider: Arc<dyn LlmProvider>,
        model: String,
        overlay: Option<RoleOverlay>,
        /// Non-fatal skip warnings collected during materialization.
        warnings: Vec<String>,
    },
    EnvFallback,
}

/// Materializes a single-role execution context from DB configuration.
///
/// Calls the existing [`materialize`] (unchanged) to get all providers,
/// then selects the one for the resolved agent's role / CLI profile.
/// Returns [`SingleRoleContext::EnvFallback`] when the DB has no provider
/// configs at all.
pub async fn materialize_single_role(
    db_path: Arc<str>,
    secrets: Arc<dyn SecretStore>,
    cwd: Option<PathBuf>,
    resolved: Option<&ResolvedAgent>,
) -> CoreResult<SingleRoleContext> {
    tracing::info!(
        resolved_kind = ?resolved.map(|r| &r.kind),
        resolved_id = ?resolved.map(|r| r.id.as_str()),
        "materialize_single_role: start",
    );
    let materialized = materialize(db_path.clone(), secrets, cwd).await?;

    if materialized.default.is_none() && materialized.providers.is_empty() {
        tracing::warn!("materialize_single_role: EnvFallback (no providers, no default)");
        return Ok(SingleRoleContext::EnvFallback);
    }

    let warnings = materialized.warnings.clone();
    let (provider, overlay) = select_provider(&materialized, resolved, &db_path).await?;

    let Some(provider) = provider else {
        tracing::warn!(
            has_default = materialized.default.is_some(),
            provider_count = materialized.providers.len(),
            "materialize_single_role: EnvFallback (select_provider returned None)",
        );
        return Ok(SingleRoleContext::EnvFallback);
    };

    let model = provider.id().to_string();
    tracing::info!(
        provider_id = %model,
        has_overlay = overlay.is_some(),
        warning_count = warnings.len(),
        "materialize_single_role: Materialized",
    );
    Ok(SingleRoleContext::Materialized {
        provider,
        model,
        overlay,
        warnings,
    })
}

async fn select_provider(
    materialized: &MaterializedProviders,
    resolved: Option<&ResolvedAgent>,
    db_path: &Arc<str>,
) -> CoreResult<(Option<Arc<dyn LlmProvider>>, Option<RoleOverlay>)> {
    match resolved {
        Some(r) => match r.kind {
            AgentRefKind::Role => select_for_role(materialized, &r.id, db_path).await,
            AgentRefKind::Cli => {
                let provider = materialized
                    .providers
                    .get(&r.id)
                    .cloned()
                    .or_else(|| materialized.default.clone());
                Ok((provider, None))
            }
        },
        None => Ok((materialized.default.clone(), None)),
    }
}

async fn select_for_role(
    materialized: &MaterializedProviders,
    role_id: &str,
    db_path: &Arc<str>,
) -> CoreResult<(Option<Arc<dyn LlmProvider>>, Option<RoleOverlay>)> {
    let role = {
        let path = db_path.clone();
        let rid = role_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<crate::domain::Role>, CoreError> {
            let db = crate::store::Db::open(&path)?;
            Ok(crate::store::repos::roles::get(&db.0, &rid).ok())
        })
        .await
        .map_err(join_err)??
    };

    let Some(role) = role else {
        return Ok((materialized.default.clone(), None));
    };

    let provider_key = role
        .params
        .get("agent_profile_id")
        .and_then(serde_json::Value::as_str)
        .map(|s| s.to_string())
        .or(role.provider_id.clone());

    let provider = provider_key
        .as_deref()
        .and_then(|k| materialized.providers.get(k).cloned())
        .or_else(|| materialized.default.clone());

    tracing::info!(
        role_id = %role_id,
        provider_key = ?provider_key,
        found = provider.is_some(),
        has_default = materialized.default.is_some(),
        available_keys = ?materialized.providers.keys().collect::<Vec<_>>(),
        "select_for_role",
    );

    let overlay = RoleOverlay {
        system_prompt: role.system_prompt_override.clone(),
        temperature: role.temperature,
        tool_allowlist: role.tool_allowlist.clone(),
    };

    Ok((provider, Some(overlay)))
}

fn join_err(e: tokio::task::JoinError) -> CoreError {
    CoreError::Store(crate::store::StoreError::Sqlite(
        rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ProviderConfig, ProviderProtocol, Role};
    use crate::providers::MemorySecretStore;
    use crate::store::migrations;
    use serde_json::json;

    fn provider_cfg(id: &str, keyring_ref: Option<&str>, master: bool) -> ProviderConfig {
        ProviderConfig {
            id: id.into(),
            name: id.into(),
            protocol: ProviderProtocol::OpenAiCompatible,
            base_url: "http://localhost:9/v1".into(),
            keyring_ref: keyring_ref.map(str::to_string),
            capabilities: vec![],
            is_master: master,
            fallback_order: None,
            params: json!({}),
            created_at: 1,
            updated_at: 1,
        }
    }

    fn role(id: &str, provider_id: Option<&str>) -> Role {
        Role {
            id: id.into(),
            name: id.into(),
            provider_id: provider_id.map(str::to_string),
            provider_ids: vec![],
            system_prompt_override: Some("You are a coder.".into()),
            tool_allowlist: vec!["read".into(), "write".into()],
            required_capabilities: vec![],
            temperature: Some(0.1),
            max_tokens: None,
            params: json!({}),
            builtin: false,
            generated: false,
            ephemeral: false,
            source: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn temp_db() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.db");
        let conn = rusqlite::Connection::open(&file).unwrap();
        migrations::run(&conn).unwrap();
        (dir, file)
    }

    #[tokio::test]
    async fn env_fallback_when_db_has_no_providers() {
        let (_dir, file) = temp_db();
        let secrets = Arc::new(MemorySecretStore::default());
        let db_path = Arc::from(file.to_string_lossy().to_string());
        let ctx = materialize_single_role(db_path, secrets, None, None)
            .await
            .unwrap();
        assert!(matches!(ctx, SingleRoleContext::EnvFallback));
    }

    #[tokio::test]
    async fn materialized_with_default_when_no_resolved_agent() {
        let (_dir, file) = temp_db();
        {
            let conn = rusqlite::Connection::open(&file).unwrap();
            crate::store::repos::providers::insert_provider(
                &conn,
                &provider_cfg("p1", Some("kr/p1"), true),
            )
            .unwrap();
        }
        let secrets = Arc::new(MemorySecretStore::default());
        secrets.set("kr/p1", "sk-test").await.unwrap();
        let db_path = Arc::from(file.to_string_lossy().to_string());
        let ctx = materialize_single_role(db_path, secrets, None, None)
            .await
            .unwrap();
        match ctx {
            SingleRoleContext::Materialized {
                provider, overlay, ..
            } => {
                assert_eq!(provider.id(), "openai_compatible");
                assert!(overlay.is_none());
            }
            _ => panic!("expected Materialized"),
        }
    }

    #[tokio::test]
    async fn materialized_with_role_overlay() {
        let (_dir, file) = temp_db();
        {
            let conn = rusqlite::Connection::open(&file).unwrap();
            crate::store::repos::providers::insert_provider(
                &conn,
                &provider_cfg("p1", Some("kr/p1"), true),
            )
            .unwrap();
            crate::store::repos::roles::insert(&conn, &role("r1", Some("p1"))).unwrap();
        }
        let secrets = Arc::new(MemorySecretStore::default());
        secrets.set("kr/p1", "sk-test").await.unwrap();
        let db_path = Arc::from(file.to_string_lossy().to_string());
        let resolved = ResolvedAgent {
            kind: AgentRefKind::Role,
            id: "r1".into(),
            name: "r1".into(),
        };
        let ctx = materialize_single_role(db_path, secrets, None, Some(&resolved))
            .await
            .unwrap();
        match ctx {
            SingleRoleContext::Materialized {
                provider,
                model,
                overlay,
                ..
            } => {
                assert_eq!(provider.id(), "openai_compatible");
                assert_eq!(model, "openai_compatible");
                let ov = overlay.expect("should have overlay");
                assert_eq!(ov.system_prompt.as_deref(), Some("You are a coder."));
                assert_eq!(ov.temperature, Some(0.1));
                assert_eq!(
                    ov.tool_allowlist,
                    vec!["read".to_string(), "write".to_string()]
                );
            }
            _ => panic!("expected Materialized"),
        }
    }

    #[tokio::test]
    async fn role_not_found_degrades_to_no_overlay() {
        let (_dir, file) = temp_db();
        {
            let conn = rusqlite::Connection::open(&file).unwrap();
            crate::store::repos::providers::insert_provider(
                &conn,
                &provider_cfg("p1", Some("kr/p1"), true),
            )
            .unwrap();
        }
        let secrets = Arc::new(MemorySecretStore::default());
        secrets.set("kr/p1", "sk-test").await.unwrap();
        let db_path = Arc::from(file.to_string_lossy().to_string());
        let resolved = ResolvedAgent {
            kind: AgentRefKind::Role,
            id: "nonexistent".into(),
            name: "ghost".into(),
        };
        let ctx = materialize_single_role(db_path, secrets, None, Some(&resolved))
            .await
            .unwrap();
        match ctx {
            SingleRoleContext::Materialized { overlay, .. } => {
                assert!(overlay.is_none(), "Role not found → no overlay");
            }
            _ => panic!("expected Materialized"),
        }
    }
}
