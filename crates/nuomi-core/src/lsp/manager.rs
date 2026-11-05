//! LspManager: multi-language Language Server registry + lifecycle management.
//!
//! Manages a registry of LspClient instances keyed by language. Each language
//! gets at most one LS process, started lazily on first use. The manager
//! handles discovery (PATH → SDK config → user config), health checks, and
//! graceful shutdown.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::client::LspClient;
use super::defaults;
use super::types::{LspLanguageId, LspServerConfig, LspServerStatus};
use super::LspError;

/// Maximum number of concurrent LS processes.
const MAX_CONCURRENT_LS: usize = 8;

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
}

impl LspManager {
    pub fn new() -> Self {
        Self {
            registry: Mutex::new(HashMap::new()),
            user_configs: Mutex::new(HashMap::new()),
        }
    }

    /// Register or update a user-provided LSP config for a language.
    pub async fn set_user_config(&self, config: LspServerConfig) {
        let mut configs = self.user_configs.lock().await;
        configs.insert(config.language.clone(), config);
    }

    /// Remove a user-provided LSP config (revert to default).
    pub async fn remove_user_config(&self, language: &LspLanguageId) {
        let mut configs = self.user_configs.lock().await;
        configs.remove(language);
    }

    /// Get the effective config for a language (user config > default).
    pub async fn effective_config(&self, language: &LspLanguageId) -> Option<LspServerConfig> {
        let user_configs = self.user_configs.lock().await;
        if let Some(cfg) = user_configs.get(language) {
            return Some(cfg.clone());
        }
        drop(user_configs);
        defaults::default_config(language)
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
    /// Uses the three-tier discovery strategy: user config → default config → error.
    pub async fn start(&self, language: &LspLanguageId) -> Result<Arc<LspClient>, LspError> {
        let config = self
            .effective_config(language)
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

        let mut langs: Vec<LspLanguageId> = registry.keys().cloned().collect();
        for lang in user_configs.keys() {
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
}
