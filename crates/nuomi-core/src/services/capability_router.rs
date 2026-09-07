//! Capability router: resolves a capability requirement
//! (`RouteRequest`) onto the best Role/Provider binding in the catalog.
//!
//! Algorithm (§1-§3 of the capability-routing spec):
//! 1. **Bound roles win** — roles whose `required_capabilities` cover the
//!    request AND whose bound providers actually carry those capabilities
//!    are ranked by provider `priority` weight; the top match is returned.
//!    `prefer_role_id` short-circuits ranking when it matches.
//! 2. **Temp role creation** — when no role matches but some provider
//!    covers the request, an ephemeral role (`temp-<caps>-<short id>`,
//!    `ephemeral = true`, bound to that provider) is created and returned;
//!    the caller GCs it via [`cleanup_temp`] / the run-end
//!    [`cleanup_expired_temps`] hook.
//! 3. **No match** — [`RoutingError::NoCapability`] with the missing set.
//!
//! Rules (`RoutingRules`) persist in `app_settings` under
//! [`ROUTING_RULES_KEY`]: `prefer_local` biases ranking toward
//! localhost endpoints; `capability_overrides` pins a capability to a
//! specific provider id.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::domain::{new_id, now_ms, Capability, ProviderConfig, Role};
use crate::store::{migrations, repos, Db, StoreError};

/// `app_settings` key holding the serialized [`RoutingRules`].
pub const ROUTING_RULES_KEY: &str = "routing_rules";

/// A routing request: which capabilities are needed and an optional role
/// preference.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RouteRequest {
    pub required_capabilities: Vec<Capability>,
    #[serde(default)]
    pub prefer_role_id: Option<String>,
}

/// User-configurable routing rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutingRules {
    /// Bias ranking toward localhost / loopback endpoints (local-first).
    #[serde(default)]
    pub prefer_local: bool,
    /// Pin a capability to a specific provider id (overrides ranking).
    #[serde(default)]
    pub capability_overrides: BTreeMap<Capability, String>,
}

/// Routing failure with the unmatched capability set.
#[derive(Debug, thiserror::Error)]
pub enum RoutingError {
    #[error("no provider/role covers capabilities: {0}")]
    NoCapability(String),
    #[error("store: {0}")]
    Store(String),
}

impl From<StoreError> for RoutingError {
    fn from(e: StoreError) -> Self {
        RoutingError::Store(e.to_string())
    }
}

/// Result of a successful route.
#[derive(Debug, Clone)]
pub struct RouteOutcome {
    pub role: Role,
    /// True when [`route`] created an ephemeral temp role for this request.
    pub created_temp: bool,
}

/// Loads [`RoutingRules`] from `app_settings`; missing/malformed → default.
pub fn load_routing_rules(conn: &Connection) -> Result<RoutingRules, RoutingError> {
    match repos::settings::get(conn, ROUTING_RULES_KEY)? {
        None => Ok(RoutingRules::default()),
        Some(json) => Ok(serde_json::from_str(&json).unwrap_or_default()),
    }
}

/// Persists [`RoutingRules`] into `app_settings` (UPSERT).
pub fn save_routing_rules(conn: &Connection, rules: &RoutingRules) -> Result<(), RoutingError> {
    let json = serde_json::to_string(rules).map_err(|e| RoutingError::Store(e.to_string()))?;
    repos::settings::set(conn, ROUTING_RULES_KEY, &json)?;
    Ok(())
}

/// Runs the routing algorithm against the live catalog (read-mostly; writes
/// only in the temp-role branch). Runs migrations first.
pub fn route(conn: &mut Connection, request: &RouteRequest) -> Result<RouteOutcome, RoutingError> {
    migrations::run(conn)?;
    let rules = load_routing_rules(conn)?;
    let providers = repos::providers::list_providers(conn)?;
    let roles = repos::roles::list(conn)?;
    route_catalog(conn, request, &rules, &providers, &roles)
}

/// Catalog-injected core of [`route`] (pure ranking logic + temp insert).
pub fn route_catalog(
    conn: &Connection,
    request: &RouteRequest,
    rules: &RoutingRules,
    providers: &[ProviderConfig],
    roles: &[Role],
) -> Result<RouteOutcome, RoutingError> {
    let required = &request.required_capabilities;
    if required.is_empty() {
        return Err(RoutingError::NoCapability(String::new()));
    }

    // §1: roles whose typed capabilities cover the request (request ⊆ role
    // capabilities) and whose bound providers actually carry them.
    let mut best: Option<(&Role, f64)> = None;
    for role in roles {
        if !required
            .iter()
            .all(|c| role.required_capabilities.contains(c))
        {
            continue;
        }
        let score = binding_score(role, required, rules, providers);
        let Some(score) = score else { continue };
        if request
            .prefer_role_id
            .as_deref()
            .is_some_and(|pref| pref == role.id)
        {
            return Ok(RouteOutcome {
                role: role.clone(),
                created_temp: false,
            });
        }
        if best.is_none_or(|(_, s)| score > s) {
            best = Some((role, score));
        }
    }
    if let Some((role, _)) = best {
        return Ok(RouteOutcome {
            role: role.clone(),
            created_temp: false,
        });
    }

    // §2: no role covers it — auto-create an ephemeral temp role bound to
    // the best covering provider (overrides first, then priority).
    let provider = pick_provider(required, rules, providers)
        .ok_or_else(|| RoutingError::NoCapability(display_caps(required)))?;
    let role = create_temp_role(conn, required, provider)?;
    Ok(RouteOutcome {
        role,
        created_temp: true,
    })
}

/// Score of a role binding: max provider priority among its bound providers
/// that cover `required` (`None` = no covering provider). `prefer_local`
/// adds a +1 bonus for loopback endpoints; overrides pin +2.
fn binding_score(
    role: &Role,
    required: &[Capability],
    rules: &RoutingRules,
    providers: &[ProviderConfig],
) -> Option<f64> {
    let mut ids = role.provider_ids.clone();
    if let Some(legacy) = role.provider_id.as_ref() {
        if !ids.contains(legacy) {
            ids.push(legacy.clone());
        }
    }
    ids.into_iter()
        .filter_map(|pid| providers.iter().find(|p| p.id == pid))
        .filter(|p| p.covers(required))
        .map(|p| provider_score(p, required, rules))
        .max_by(|a, b| a.total_cmp(b))
}

/// Ranking score of one provider for `required`.
fn provider_score(provider: &ProviderConfig, required: &[Capability], rules: &RoutingRules) -> f64 {
    let settings = crate::domain::entities::ProviderSettings::from_params(&provider.params);
    let mut score = settings.priority.unwrap_or(5.0);
    if rules.prefer_local && is_local(&provider.base_url) {
        score += 1.0;
    }
    if required.iter().any(|c| {
        rules
            .capability_overrides
            .get(c)
            .is_some_and(|pid| pid == &provider.id)
    }) {
        score += 2.0;
    }
    score
}

/// Picks the best covering provider for temp-role creation: overrides first
/// (only when that provider covers everything), then priority score.
fn pick_provider<'a>(
    required: &[Capability],
    rules: &RoutingRules,
    providers: &'a [ProviderConfig],
) -> Option<&'a ProviderConfig> {
    let covering: Vec<&ProviderConfig> = providers
        .iter()
        .filter(|p| p.enabled_setting() && p.covers(required))
        .collect();
    if covering.is_empty() {
        return None;
    }
    for cap in required {
        if let Some(pid) = rules.capability_overrides.get(cap) {
            if let Some(p) = covering.iter().find(|p| p.id == *pid) {
                return Some(p);
            }
        }
    }
    covering.into_iter().max_by(|a, b| {
        provider_score(a, required, rules).total_cmp(&provider_score(b, required, rules))
    })
}

fn is_local(base_url: &str) -> bool {
    let host = base_url
        .strip_prefix("http://")
        .or_else(|| base_url.strip_prefix("https://"))
        .unwrap_or(base_url);
    let host = host.split(['/', ':']).next().unwrap_or(host);
    matches!(
        host,
        "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0" | "host.docker.internal"
    )
}

/// Inserts the ephemeral role (`temp-<caps>-<8-char id>`) bound to
/// `provider` (both the multi-binding and the legacy single column).
fn create_temp_role(
    conn: &Connection,
    required: &[Capability],
    provider: &ProviderConfig,
) -> Result<Role, RoutingError> {
    let id = new_id();
    let caps_label: Vec<&str> = required.iter().map(|c| c.as_str()).collect();
    let role = Role {
        id: id.clone(),
        name: format!("temp-{}-{}", caps_label.join("-"), &id[..8.min(id.len())]),
        provider_id: Some(provider.id.clone()),
        provider_ids: vec![provider.id.clone()],
        system_prompt_override: Some(format!(
            "You are a temporary auto-routed assistant covering the {} capability. Complete the \
given task directly and concisely.",
            caps_label.join("/")
        )),
        tool_allowlist: vec![],
        required_capabilities: required.to_vec(),
        temperature: None,
        max_tokens: None,
        params: serde_json::json!({ "description": "capability-router temp role" }),
        builtin: false,
        generated: false,
        ephemeral: true,
        source: None,
        created_at: now_ms(),
        updated_at: now_ms(),
    };
    repos::roles::insert(conn, &role)?;
    Ok(role)
}

/// Removes one temp role by id — but only when it really is ephemeral.
/// Returns whether a row was removed.
pub fn cleanup_temp(conn: &Connection, role_id: &str) -> Result<bool, RoutingError> {
    let deleted = match repos::roles::get(conn, role_id) {
        Ok(role) if role.ephemeral => repos::roles::delete(conn, role_id)?,
        Ok(_) => false,
        Err(StoreError::NotFound { .. }) => false,
        Err(e) => return Err(e.into()),
    };
    Ok(deleted)
}

/// Run-end GC hook: deletes every ephemeral temp role. Wired at run
/// settlement (src-tauri commands `spawn_team_run`) and available for the
/// CLI/run-loop integration (TODO: kernel run loop in nuomi-cli once the
/// kernel exposes a run-finished callback).
pub fn cleanup_expired_temps(conn: &Connection) -> Result<usize, RoutingError> {
    Ok(repos::roles::delete_ephemeral(conn)?)
}

fn display_caps(caps: &[Capability]) -> String {
    caps.iter()
        .map(|c| c.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

// ------------------------------------------------- orchestrator integration

/// Adapter implementing the orchestrator's [`CapabilityFallback`] trait
/// (`orchestrator::router`) on top of the live database. Requested string
/// tags are parsed into system capabilities; unparseable tags (legacy
/// free-form labels like `"code"`) are ignored by the capability model and
/// simply cannot be served by the fallback.
pub struct DbCapabilityFallback {
    db_path: std::sync::Arc<str>,
}

impl DbCapabilityFallback {
    pub fn new(db_path: std::sync::Arc<str>) -> Self {
        Self { db_path }
    }
}

impl crate::orchestrator::router::CapabilityFallback for DbCapabilityFallback {
    fn resolve(
        &self,
        required: &[String],
    ) -> Result<Option<crate::orchestrator::router::FallbackSelection>, String> {
        let caps: Vec<Capability> = required
            .iter()
            .filter_map(|c| Capability::parse(c))
            .collect();
        if caps.is_empty() {
            return Ok(None);
        }
        let mut conn = Db::open(&self.db_path).map_err(|e| e.to_string())?;
        let outcome = route(
            &mut conn.0,
            &RouteRequest {
                required_capabilities: caps,
                prefer_role_id: None,
            },
        )
        .map_err(|e| e.to_string())?;
        Ok(Some(crate::orchestrator::router::FallbackSelection {
            role: outcome.role,
            temp: outcome.created_temp,
        }))
    }

    fn cleanup(&self, role_id: &str) {
        // Best-effort: failures are left to the run-end ephemeral GC.
        match Db::open(&self.db_path) {
            Ok(conn) => {
                if let Err(e) = cleanup_temp(&conn.0, role_id) {
                    tracing::warn!(error = %e, role = %role_id, "temp role cleanup failed");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, role = %role_id, "temp role cleanup open failed")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::ProviderSettings;
    use crate::domain::ProviderProtocol;

    fn db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("t.db")).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrations::run(&conn).unwrap();
        (dir, conn)
    }

    fn provider(
        id: &str,
        name: &str,
        caps: &[Capability],
        priority: f64,
        base: &str,
    ) -> ProviderConfig {
        let settings = ProviderSettings {
            models: vec![crate::domain::ModelEntry::with_caps("m-1", caps)],
            priority: Some(priority),
            ..ProviderSettings::default()
        };
        ProviderConfig {
            id: id.into(),
            name: name.into(),
            protocol: ProviderProtocol::OpenAiCompatible,
            base_url: base.into(),
            keyring_ref: None,
            capabilities: vec![],
            is_master: false,
            fallback_order: None,
            params: settings.into_params(serde_json::json!({})),
            created_at: 1,
            updated_at: 1,
        }
    }

    fn role(id: &str, name: &str, caps: &[Capability], provider_ids: Vec<String>) -> Role {
        Role {
            id: id.into(),
            name: name.into(),
            provider_id: provider_ids.first().cloned(),
            provider_ids,
            system_prompt_override: None,
            tool_allowlist: vec![],
            required_capabilities: caps.to_vec(),
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({}),
            builtin: false,
            generated: false,
            ephemeral: false,
            source: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    /// Persists the fixture providers: temp-role creation inserts rows that
    /// carry a provider_configs FK.
    fn persist(conn: &Connection, providers: &[ProviderConfig]) {
        for p in providers {
            repos::providers::insert_provider(conn, p).unwrap();
        }
    }

    fn request(caps: &[Capability]) -> RouteRequest {
        RouteRequest {
            required_capabilities: caps.to_vec(),
            prefer_role_id: None,
        }
    }

    #[test]
    fn binds_existing_role_by_priority_weight() {
        let (_dir, conn) = db();
        let providers = vec![
            provider("p-low", "low", &[Capability::Reasoning], 1.0, "http://x"),
            provider(
                "p-high",
                "high",
                &[Capability::Reasoning, Capability::Image],
                9.0,
                "http://x",
            ),
        ];
        let roles = vec![
            role(
                "r-weak",
                "weak",
                &[Capability::Reasoning],
                vec!["p-low".into()],
            ),
            role(
                "r-strong",
                "strong",
                &[Capability::Reasoning, Capability::Image],
                vec!["p-high".into()],
            ),
        ];
        // Both cover reasoning; the higher-priority binding wins.
        let outcome = route_catalog(
            &conn,
            &request(&[Capability::Reasoning]),
            &RoutingRules::default(),
            &providers,
            &roles,
        )
        .unwrap();
        assert_eq!(outcome.role.name, "strong");
        assert!(!outcome.created_temp);
    }

    #[test]
    fn prefer_role_id_short_circuits_when_it_covers() {
        let (_dir, conn) = db();
        let providers = vec![provider("p1", "p", &[Capability::Voice], 5.0, "http://x")];
        let roles = vec![role(
            "r-preferred",
            "mine",
            &[Capability::Voice],
            vec!["p1".into()],
        )];
        let mut req = request(&[Capability::Voice]);
        req.prefer_role_id = Some("r-preferred".into());
        let outcome =
            route_catalog(&conn, &req, &RoutingRules::default(), &providers, &roles).unwrap();
        assert_eq!(outcome.role.id, "r-preferred");
    }

    #[test]
    fn role_without_capable_provider_is_not_selected() {
        let (_dir, conn) = db();
        let providers = vec![provider(
            "p1",
            "p",
            &[Capability::Reasoning],
            5.0,
            "http://x",
        )];
        let roles = vec![role(
            "r1",
            "vision",
            &[Capability::Image],
            vec!["p1".into()],
        )];
        let err = route_catalog(
            &conn,
            &request(&[Capability::Image]),
            &RoutingRules::default(),
            &providers,
            &roles,
        )
        .unwrap_err();
        assert!(matches!(err, RoutingError::NoCapability(_)));
    }

    #[test]
    fn creates_ephemeral_temp_role_bound_to_best_provider() {
        let (_dir, conn) = db();
        let providers = vec![
            provider("p-low", "low", &[Capability::Video], 1.0, "http://x"),
            provider("p-high", "high", &[Capability::Video], 8.0, "http://x"),
        ];
        persist(&conn, &providers);
        let roles: Vec<Role> = vec![];
        let outcome = route_catalog(
            &conn,
            &request(&[Capability::Video]),
            &RoutingRules::default(),
            &providers,
            &roles,
        )
        .unwrap();
        assert!(outcome.created_temp);
        assert!(outcome.role.ephemeral);
        assert!(outcome.role.name.starts_with("temp-video-"));
        assert_eq!(outcome.role.provider_id.as_deref(), Some("p-high"));
        // Persisted and then removable via the GC hooks.
        let stored = repos::roles::get(&conn, &outcome.role.id).unwrap();
        assert!(stored.ephemeral);
        assert!(cleanup_temp(&conn, &stored.id).unwrap());
        assert!(!cleanup_temp(&conn, &stored.id).unwrap());
    }

    #[test]
    fn capability_override_pins_provider_and_cleanup_gcs_all_temps() {
        let (_dir, conn) = db();
        let providers = vec![
            provider("p-a", "a", &[Capability::Reasoning], 9.0, "http://x"),
            provider("p-b", "b", &[Capability::Reasoning], 1.0, "http://x"),
        ];
        persist(&conn, &providers);
        let mut rules = RoutingRules::default();
        rules
            .capability_overrides
            .insert(Capability::Reasoning, "p-b".into());
        let outcome = route_catalog(
            &conn,
            &request(&[Capability::Reasoning]),
            &rules,
            &providers,
            &[],
        )
        .unwrap();
        assert_eq!(outcome.role.provider_id.as_deref(), Some("p-b"));

        // Round-trip the rules through app_settings.
        save_routing_rules(&conn, &rules).unwrap();
        let loaded = load_routing_rules(&conn).unwrap();
        assert_eq!(
            loaded
                .capability_overrides
                .get(&Capability::Reasoning)
                .map(String::as_str),
            Some("p-b")
        );

        // GC: the override temp above + two more, but never a normal role.
        create_temp_role(&conn, &[Capability::Voice], &providers[0]).unwrap();
        create_temp_role(&conn, &[Capability::Image], &providers[0]).unwrap();
        repos::roles::insert(&conn, &role("keep", "keep", &[], vec![])).unwrap();
        assert_eq!(cleanup_expired_temps(&conn).unwrap(), 3);
        assert!(repos::roles::get(&conn, "keep").is_ok());
    }

    #[test]
    fn prefer_local_and_enabled_flags_affect_ranking() {
        let (_dir, conn) = db();
        let providers = vec![
            provider(
                "remote",
                "remote",
                &[Capability::Reasoning],
                5.0,
                "http://api.example.com",
            ),
            provider(
                "local",
                "local",
                &[Capability::Reasoning],
                5.0,
                "http://localhost:11434/v1",
            ),
        ];
        persist(&conn, &providers);
        let rules = RoutingRules {
            prefer_local: true,
            ..RoutingRules::default()
        };
        let outcome = route_catalog(
            &conn,
            &request(&[Capability::Reasoning]),
            &rules,
            &providers,
            &[],
        )
        .unwrap();
        assert_eq!(outcome.role.provider_id.as_deref(), Some("local"));

        // A disabled provider (enabled=false in settings) is never picked.
        let mut disabled = provider("off", "off", &[Capability::Voice], 10.0, "http://x");
        let mut settings = ProviderSettings::from_params(&disabled.params);
        settings.enabled = false;
        disabled.params = settings.into_params(disabled.params);
        let providers = vec![disabled];
        let err = route_catalog(
            &conn,
            &request(&[Capability::Voice]),
            &RoutingRules::default(),
            &providers,
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, RoutingError::NoCapability(_)));
    }

    #[test]
    fn empty_request_is_rejected() {
        let (_dir, conn) = db();
        let err =
            route_catalog(&conn, &request(&[]), &RoutingRules::default(), &[], &[]).unwrap_err();
        assert!(matches!(err, RoutingError::NoCapability(_)));
    }
}
