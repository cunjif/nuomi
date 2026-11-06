//! LspManager: multi-language Language Server registry + lifecycle management.
//!
//! Manages a registry of LspClient instances keyed by language. Each language
//! gets at most one LS process, started lazily on first use. The manager
//! handles discovery (user config → persisted autodetect → live detect →
//! default), health checks, and graceful shutdown.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rusqlite::Connection;
use serde::Serialize;
use tokio::sync::Mutex;

use super::client::LspClient;
use super::defaults;
use super::detector;
use super::types::{LspLanguageId, LspServerConfig, LspServerStatus};
use super::LspError;

/// Maximum number of concurrent LS processes.
const MAX_CONCURRENT_LS: usize = 8;

/// app_settings key prefix for user-edited configs.
const KEY_USER_CONFIG: &str = "lsp_config.";
/// app_settings key prefix for auto-detected configs.
const KEY_AUTODETECT: &str = "lsp_autodetect.";

/// The source of an effective LSP config, for UI display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    /// User manually configured in Settings.
    User,
    /// Auto-detected and persisted.
    AutoDetected,
    /// Hardcoded default (detection found nothing).
    Default,
    /// No config available at all.
    None,
}

/// Entry in the manager's registry.
struct RegistryEntry {
    client: Option<Arc<LspClient>>,
    status: LspServerStatus,
    config: LspServerConfig,
}

/// Manages Language Server processes for multiple languages.
pub struct LspManager {
    registry: Mutex<HashMap<LspLanguageId, RegistryEntry>>,
    /// User-provided configs that override defaults.
    user_configs: Mutex<HashMap<LspLanguageId, LspServerConfig>>,
    /// Auto-detected configs persisted across restarts.
    autodetected_configs: Mutex<HashMap<LspLanguageId, LspServerConfig>>,
    /// Database path for config persistence. `None` = in-memory only (tests).
    db_path: Option<PathBuf>,
}

impl LspManager {
    pub fn new() -> Self {
        Self {
            registry: Mutex::new(HashMap::new()),
            user_configs: Mutex::new(HashMap::new()),
            autodetected_configs: Mutex::new(HashMap::new()),
            db_path: None,
        }
    }

    /// Creates a manager backed by a SQLite database for config persistence.
    /// Loads previously saved user and auto-detected configs from `app_settings`.
    pub fn with_db(db_path: PathBuf) -> Self {
        let mut user_configs = HashMap::new();
        let mut autodetected_configs = HashMap::new();
        if let Ok(conn) = Connection::open(&db_path) {
            let _ = crate::store::migrations::run(&conn);
            load_configs(&conn, KEY_USER_CONFIG, &mut user_configs);
            load_configs(&conn, KEY_AUTODETECT, &mut autodetected_configs);
        }
        Self {
            registry: Mutex::new(HashMap::new()),
            user_configs: Mutex::new(user_configs),
            autodetected_configs: Mutex::new(autodetected_configs),
            db_path: Some(db_path),
        }
    }

    /// Register or update a user-provided LSP config for a language.
    /// Persists to `app_settings` when a db_path is configured.
    pub async fn set_user_config(&self, config: LspServerConfig) {
        let lang = config.language.clone();
        {
            let mut configs = self.user_configs.lock().await;
            configs.insert(lang.clone(), config.clone());
        }
        self.persist(KEY_USER_CONFIG, &lang, &config).await;
    }

    /// Remove a user-provided LSP config (revert to auto-detect or default).
    pub async fn remove_user_config(&self, language: &LspLanguageId) {
        {
            let mut configs = self.user_configs.lock().await;
            configs.remove(language);
        }
        self.remove_persisted(KEY_USER_CONFIG, language).await;
    }

    /// Force re-detection for a language: clears the cached auto-detected config
    /// and runs a fresh PATH + SDK probe. Returns the newly detected config if any.
    pub async fn redetect(&self, language: &LspLanguageId) -> Option<LspServerConfig> {
        {
            let mut autodetected = self.autodetected_configs.lock().await;
            autodetected.remove(language);
        }
        self.remove_persisted(KEY_AUTODETECT, language).await;

        if let Some(cfg) = detector::detect_config(language) {
            self.set_autodetected_config(language.clone(), cfg.clone()).await;
            return Some(cfg);
        }
        None
    }

    /// Returns the source of the effective config for UI display.
    pub async fn config_source(&self, language: &LspLanguageId) -> ConfigSource {
        {
            let user_configs = self.user_configs.lock().await;
            if user_configs.contains_key(language) {
                return ConfigSource::User;
            }
        }
        {
            let autodetected = self.autodetected_configs.lock().await;
            if autodetected.contains_key(language) {
                return ConfigSource::AutoDetected;
            }
        }
        if defaults::default_config(language).is_some() {
            ConfigSource::Default
        } else {
            ConfigSource::None
        }
    }

    /// Get the effective config for a language (user > autodetected > default).
    /// Side-effect-free — does not trigger live detection.
    pub async fn effective_config(&self, language: &LspLanguageId) -> Option<LspServerConfig> {
        {
            let user_configs = self.user_configs.lock().await;
            if let Some(cfg) = user_configs.get(language) {
                return Some(cfg.clone());
            }
        }
        {
            let autodetected = self.autodetected_configs.lock().await;
            if let Some(cfg) = autodetected.get(language) {
                return Some(cfg.clone());
            }
        }
        defaults::default_config(language)
    }

    /// Resolve config with live detection (user > autodetected > detect > default).
    /// Triggers PATH + SDK probing on first call for a language, persisting the
    /// result for subsequent calls.
    async fn resolve_config(&self, language: &LspLanguageId) -> Option<LspServerConfig> {
        {
            let user_configs = self.user_configs.lock().await;
            if let Some(cfg) = user_configs.get(language) {
                return Some(cfg.clone());
            }
        }
        {
            let autodetected = self.autodetected_configs.lock().await;
            if let Some(cfg) = autodetected.get(language) {
                return Some(cfg.clone());
            }
        }
        if let Some(cfg) = detector::detect_config(language) {
            self.set_autodetected_config(language.clone(), cfg.clone()).await;
            return Some(cfg);
        }
        defaults::default_config(language)
    }

    async fn set_autodetected_config(&self, lang: LspLanguageId, cfg: LspServerConfig) {
        {
            let mut autodetected = self.autodetected_configs.lock().await;
            autodetected.insert(lang.clone(), cfg.clone());
        }
        self.persist(KEY_AUTODETECT, &lang, &cfg).await;
    }

    async fn persist(&self, prefix: &str, lang: &LspLanguageId, config: &LspServerConfig) {
        let Some(ref db_path) = self.db_path else { return };
        let key = format!("{prefix}{lang}");
        let Ok(value) = serde_json::to_string(config) else { return };
        let path = db_path.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let Ok(conn) = Connection::open(&path) else { return };
            let _ = crate::store::migrations::run(&conn);
            let _ = crate::store::repos::settings::set(&conn, &key, &value);
        })
        .await;
    }

    async fn remove_persisted(&self, prefix: &str, lang: &LspLanguageId) {
        let Some(ref db_path) = self.db_path else { return };
        let key = format!("{prefix}{lang}");
        let path = db_path.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let Ok(conn) = Connection::open(&path) else { return };
            let _ = crate::store::migrations::run(&conn);
            let _ = conn.execute(
                "DELETE FROM app_settings WHERE key = ?1",
                rusqlite::params![&key],
            );
        })
        .await;
    }

    /// Get the status of a language server.
    pub async fn status(&self, language: &LspLanguageId) -> LspServerStatus {
        let registry = self.registry.lock().await;
        registry
            .get(language)
            .map(|e| e.status.clone())
            .unwrap_or(LspServerStatus::NotStarted)
    }

    /// Start a Language Server for the given language (lazy, if not already running).
    ///
    /// Uses the four-tier discovery strategy: user config → persisted autodetect
    /// → live PATH/SDK detection → default config → error.
    pub async fn start(&self, language: &LspLanguageId) -> Result<Arc<LspClient>, LspError> {
        let config = self
            .resolve_config(language)
            .await
            .ok_or_else(|| LspError::NotAvailable(language.to_string()))?;

        if !config.enabled {
            return Err(LspError::NotAvailable(format!(
                "{language} (disabled by user config)"
            )));
        }

        let mut registry = self.registry.lock().await;

        if let Some(entry) = registry.get_mut(language) {
            if let Some(ref client) = entry.client {
                if client.is_alive().await {
                    return Ok(Arc::clone(client));
                }
            }
            entry.status = LspServerStatus::Failed("process not alive".into());
        }

        let registry_count = registry.len();
        if registry_count >= MAX_CONCURRENT_LS {
            let active: Vec<String> = registry
                .iter()
                .filter(|(_, e)| e.client.is_some())
                .map(|(k, _)| k.to_string())
                .collect();
            return Err(LspError::NotAvailable(format!(
                "max {MAX_CONCURRENT_LS} concurrent LS reached (active: {})",
                active.join(", ")
            )));
        }

        let entry = registry
            .entry(language.clone())
            .or_insert_with(|| RegistryEntry {
                client: None,
                status: LspServerStatus::NotStarted,
                config: config.clone(),
            });

        entry.status = LspServerStatus::Initializing;
        entry.config = config.clone();

        match LspClient::spawn(config).await {
            Ok(client) => {
                let client = Arc::new(client);
                entry.client = Some(Arc::clone(&client));
                entry.status = LspServerStatus::Ready;
                Ok(client)
            }
            Err(e) => {
                entry.status = LspServerStatus::Failed(e.to_string());
                Err(e)
            }
        }
    }

    /// Ensure a Language Server is started and initialized for the given language.
    pub async fn ensure_started(
        &self,
        language: &LspLanguageId,
        root_uri: &str,
    ) -> Result<Arc<LspClient>, LspError> {
        let client = self.start(language).await?;

        if !client.is_initialized() {
            client
                .initialize(root_uri)
                .await
                .map_err(|e| LspError::InitializeFailed(e.to_string()))?;
        }

        Ok(client)
    }

    /// Stop a Language Server for the given language.
    pub async fn stop(&self, language: &LspLanguageId) -> Result<(), LspError> {
        let mut registry = self.registry.lock().await;
        if let Some(entry) = registry.get_mut(language) {
            if let Some(client) = entry.client.take() {
                if let Some(client) = Arc::into_inner(client) {
                    client.shutdown().await?;
                }
            }
            entry.status = LspServerStatus::NotStarted;
        }
        Ok(())
    }

    /// Stop all running Language Servers.
    pub async fn stop_all(&self) {
        let mut registry = self.registry.lock().await;
        for (_, entry) in registry.iter_mut() {
            if let Some(client) = entry.client.take() {
                if let Some(client) = Arc::into_inner(client) {
                    let _ = client.shutdown().await;
                }
            }
            entry.status = LspServerStatus::NotStarted;
        }
    }

    /// Get the list of all known (configured) languages.
    pub async fn known_languages(&self) -> Vec<LspLanguageId> {
        let registry = self.registry.lock().await;
        let user_configs = self.user_configs.lock().await;
        let autodetected = self.autodetected_configs.lock().await;

        let mut langs: Vec<LspLanguageId> = registry.keys().cloned().collect();
        for lang in user_configs.keys() {
            if !langs.contains(lang) {
                langs.push(lang.clone());
            }
        }
        for lang in autodetected.keys() {
            if !langs.contains(lang) {
                langs.push(lang.clone());
            }
        }
        for cfg in defaults::all_defaults() {
            if !langs.contains(&cfg.language) {
                langs.push(cfg.language);
            }
        }
        langs
    }

    /// Get statuses for all known languages.
    pub async fn all_statuses(&self) -> Vec<(LspLanguageId, LspServerStatus)> {
        let registry = self.registry.lock().await;
        let user_configs = self.user_configs.lock().await;
        let autodetected = self.autodetected_configs.lock().await;

        let mut result = Vec::new();

        for lang in registry.keys() {
            let status = registry
                .get(lang)
                .map(|e| e.status.clone())
                .unwrap_or(LspServerStatus::NotStarted);
            result.push((lang.clone(), status));
        }

        for lang in user_configs.keys() {
            if !result.iter().any(|(l, _)| l == lang) {
                let cfg = user_configs.get(lang);
                if cfg.map(|c| c.enabled).unwrap_or(true) {
                    result.push((lang.clone(), LspServerStatus::NotStarted));
                } else {
                    result.push((lang.clone(), LspServerStatus::Disabled));
                }
            }
        }

        for lang in autodetected.keys() {
            if !result.iter().any(|(l, _)| l == lang) {
                result.push((lang.clone(), LspServerStatus::NotStarted));
            }
        }

        for cfg in defaults::all_defaults() {
            if !result.iter().any(|(l, _)| l == &cfg.language) {
                result.push((cfg.language, LspServerStatus::NotStarted));
            }
        }

        result
    }

    /// Drain all pending server notifications from every running client.
    /// Called by the shell's diagnostic forwarder task (ADR-0027 L5).
    pub async fn drain_notifications(
        &self,
    ) -> Vec<(LspLanguageId, super::client::ServerNotification)> {
        let registry = self.registry.lock().await;
        let mut result = Vec::new();
        for (lang, entry) in registry.iter() {
            if let Some(ref client) = entry.client {
                while let Some(notif) = client.try_recv_notification().await {
                    result.push((lang.clone(), notif));
                }
            }
        }
        result
    }
}

/// Loads all configs from `app_settings` whose key starts with `prefix`,
/// deserializing each value as `LspServerConfig` into `out`.
fn load_configs(
    conn: &Connection,
    prefix: &str,
    out: &mut HashMap<LspLanguageId, LspServerConfig>,
) {
    let sql = "SELECT key, value FROM app_settings WHERE key LIKE ?1";
    let pattern = format!("{prefix}%");
    let Ok(mut rows) = conn.prepare(sql) else {
        return;
    };
    let Ok(iter) = rows.query_map(rusqlite::params![&pattern], |row| {
        let key: String = row.get(0)?;
        let value: String = row.get(1)?;
        Ok((key, value))
    }) else {
        return;
    };
    for row in iter.flatten() {
        let lang = row.0.strip_prefix(prefix).unwrap_or("");
        if let Ok(cfg) = serde_json::from_str::<LspServerConfig>(&row.1) {
            out.insert(LspLanguageId::new(lang), cfg);
        }
    }
}

impl Default for LspManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_manager_has_no_running_servers() {
        let mgr = LspManager::new();
        assert_eq!(
            mgr.status(&LspLanguageId::new("python")).await,
            LspServerStatus::NotStarted
        );
    }

    #[tokio::test]
    async fn user_config_overrides_default() {
        let mgr = LspManager::new();
        let custom =
            LspServerConfig::new("python", "my-custom-pylsp").with_args(vec!["--stdio".into()]);
        mgr.set_user_config(custom).await;

        let cfg = mgr.effective_config(&LspLanguageId::new("python")).await;
        assert!(cfg.is_some());
        assert_eq!(cfg.unwrap().command, "my-custom-pylsp");
    }

    #[tokio::test]
    async fn remove_user_config_reverts_to_default() {
        let mgr = LspManager::new();
        mgr.set_user_config(LspServerConfig::new("python", "custom-lsp"))
            .await;
        assert!(mgr
            .effective_config(&LspLanguageId::new("python"))
            .await
            .is_some());

        mgr.remove_user_config(&LspLanguageId::new("python")).await;
        let cfg = mgr.effective_config(&LspLanguageId::new("python")).await;
        assert_eq!(cfg.unwrap().command, "pyright-langserver");
    }

    #[tokio::test]
    async fn effective_config_for_unknown_language() {
        let mgr = LspManager::new();
        assert!(mgr
            .effective_config(&LspLanguageId::new("brainfuck"))
            .await
            .is_none());
    }

    #[tokio::test]
    async fn disabled_config_returns_not_available() {
        let mgr = LspManager::new();
        let disabled = LspServerConfig::new("python", "pyright-langserver").with_enabled(false);
        mgr.set_user_config(disabled).await;

        let result = mgr.start(&LspLanguageId::new("python")).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            LspError::NotAvailable(_) => {}
            other => panic!("expected NotAvailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn known_languages_includes_defaults_and_user_configs() {
        let mgr = LspManager::new();
        mgr.set_user_config(LspServerConfig::new("lua", "lua-lsp"))
            .await;

        let langs = mgr.known_languages().await;
        assert!(langs.iter().any(|l| l.as_str() == "lua"));
        assert!(langs.iter().any(|l| l.as_str() == "python"));
        assert!(langs.iter().any(|l| l.as_str() == "rust"));
    }

    #[tokio::test]
    async fn all_statuses_covers_defaults() {
        let mgr = LspManager::new();
        let statuses = mgr.all_statuses().await;
        assert!(statuses.len() >= 8);
        assert!(statuses
            .iter()
            .any(|(l, s)| l.as_str() == "python" && *s == LspServerStatus::NotStarted));
    }

    #[tokio::test]
    async fn stop_all_clears_registry() {
        let mgr = LspManager::new();
        mgr.stop_all().await;
        assert_eq!(
            mgr.status(&LspLanguageId::new("python")).await,
            LspServerStatus::NotStarted
        );
    }

    #[tokio::test]
    async fn config_source_returns_none_for_unknown() {
        let mgr = LspManager::new();
        assert_eq!(
            mgr.config_source(&LspLanguageId::new("brainfuck")).await,
            ConfigSource::None
        );
    }

    #[tokio::test]
    async fn config_source_returns_default_for_known_language() {
        let mgr = LspManager::new();
        assert_eq!(
            mgr.config_source(&LspLanguageId::new("python")).await,
            ConfigSource::Default
        );
    }

    #[tokio::test]
    async fn config_source_returns_user_after_set_user_config() {
        let mgr = LspManager::new();
        mgr.set_user_config(LspServerConfig::new("python", "custom-pylsp"))
            .await;
        assert_eq!(
            mgr.config_source(&LspLanguageId::new("python")).await,
            ConfigSource::User
        );
    }

    #[tokio::test]
    async fn config_source_returns_autodetected_after_set_autodetected() {
        let mgr = LspManager::new();
        mgr.set_autodetected_config(
            LspLanguageId::new("python"),
            LspServerConfig::new("python", "detected-pylsp"),
        )
        .await;
        assert_eq!(
            mgr.config_source(&LspLanguageId::new("python")).await,
            ConfigSource::AutoDetected
        );
    }

    #[tokio::test]
    async fn effective_config_prefers_user_over_autodetected() {
        let mgr = LspManager::new();
        mgr.set_autodetected_config(
            LspLanguageId::new("python"),
            LspServerConfig::new("python", "detected-pylsp"),
        )
        .await;
        mgr.set_user_config(LspServerConfig::new("python", "user-pylsp"))
            .await;
        let cfg = mgr.effective_config(&LspLanguageId::new("python")).await;
        assert_eq!(cfg.unwrap().command, "user-pylsp");
    }

    #[tokio::test]
    async fn effective_config_uses_autodetected_when_no_user() {
        let mgr = LspManager::new();
        mgr.set_autodetected_config(
            LspLanguageId::new("python"),
            LspServerConfig::new("python", "detected-pylsp"),
        )
        .await;
        let cfg = mgr.effective_config(&LspLanguageId::new("python")).await;
        assert_eq!(cfg.unwrap().command, "detected-pylsp");
    }

    #[tokio::test]
    async fn redetect_clears_and_reprobes() {
        let mgr = LspManager::new();
        mgr.set_autodetected_config(
            LspLanguageId::new("python"),
            LspServerConfig::new("python", "old-detected"),
        )
        .await;
        assert_eq!(
            mgr.config_source(&LspLanguageId::new("python")).await,
            ConfigSource::AutoDetected
        );
        // redetect may or may not find something depending on PATH, but it
        // always clears the old autodetected entry.
        let _ = mgr.redetect(&LspLanguageId::new("python")).await;
        // After redetect, if detection found nothing, source falls back to Default.
        let source = mgr.config_source(&LspLanguageId::new("python")).await;
        assert!(source == ConfigSource::AutoDetected || source == ConfigSource::Default);
    }

    #[tokio::test]
    async fn with_db_loads_persisted_configs() {
        use crate::store::migrations;
        let dir = std::env::temp_dir().join("nuomi_lsp_manager_with_db_test");
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");
        std::fs::remove_file(&db_path).ok();

        // First: create a manager, set a user config, which persists.
        {
            let conn = Connection::open(&db_path).unwrap();
            migrations::run(&conn).unwrap();
            let cfg = LspServerConfig::new("python", "persisted-pylsp")
                .with_args(vec!["--stdio".into()]);
            let value = serde_json::to_string(&cfg).unwrap();
            crate::store::repos::settings::set(&conn, "lsp_config.python", &value).unwrap();
        }

        // Second: create a new manager with_db — it should load the config.
        let mgr = LspManager::with_db(db_path.clone());
        let cfg = mgr.effective_config(&LspLanguageId::new("python")).await;
        assert_eq!(cfg.unwrap().command, "persisted-pylsp");
        assert_eq!(
            mgr.config_source(&LspLanguageId::new("python")).await,
            ConfigSource::User
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn set_user_config_persists_to_db() {
        let dir = std::env::temp_dir().join("nuomi_lsp_manager_persist_test");
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");
        std::fs::remove_file(&db_path).ok();

        let mgr = LspManager::with_db(db_path.clone());
        mgr.set_user_config(
            LspServerConfig::new("java", "my-jdtls").with_args(vec!["--stdio".into()]),
        )
        .await;

        // Verify persisted by opening a fresh connection.
        let conn = Connection::open(&db_path).unwrap();
        let value =
            crate::store::repos::settings::get(&conn, "lsp_config.java").unwrap().unwrap();
        let cfg: LspServerConfig = serde_json::from_str(&value).unwrap();
        assert_eq!(cfg.command, "my-jdtls");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn remove_user_config_deletes_from_db() {
        let dir = std::env::temp_dir().join("nuomi_lsp_manager_remove_test");
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");
        std::fs::remove_file(&db_path).ok();

        let mgr = LspManager::with_db(db_path.clone());
        mgr.set_user_config(LspServerConfig::new("java", "my-jdtls")).await;
        mgr.remove_user_config(&LspLanguageId::new("java")).await;

        let conn = Connection::open(&db_path).unwrap();
        let value = crate::store::repos::settings::get(&conn, "lsp_config.java").unwrap();
        assert!(value.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }
}
