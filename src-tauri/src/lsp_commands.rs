//! LSP IPC commands (ADR-0027 L2): thin validation + forwarding to LspManager.
//!
//! All commands follow the same pattern as commands.rs: `impl_*` free functions
//! testable without a Tauri runtime, returning `Result<T, IpcError>`.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use nuomi_core::lsp::{
    types::{LspLanguageId, LspServerConfig, LspServerStatus},
    ConfigSource, LspClient,
};

use crate::ipc_error::IpcError;
use crate::state::AppState;

// ---------- DTOs ----------

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct LspServerConfigDto {
    pub language: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default = "default_init_timeout")]
    pub init_timeout_secs: u64,
}

fn default_true() -> bool {
    true
}

fn default_init_timeout() -> u64 {
    30
}

impl From<LspServerConfig> for LspServerConfigDto {
    fn from(cfg: LspServerConfig) -> Self {
        Self {
            language: cfg.language.as_str().to_string(),
            command: cfg.command,
            args: cfg.args,
            enabled: cfg.enabled,
            env: cfg.env,
            working_dir: cfg.working_dir,
            init_timeout_secs: cfg.init_timeout_secs,
        }
    }
}

impl From<LspServerConfigDto> for LspServerConfig {
    fn from(dto: LspServerConfigDto) -> Self {
        Self {
            language: LspLanguageId::new(dto.language),
            command: dto.command,
            args: dto.args,
            enabled: dto.enabled,
            env: dto.env,
            working_dir: dto.working_dir,
            init_timeout_secs: dto.init_timeout_secs,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct LspServerStatusDto {
    pub language: String,
    pub status: String,
}

impl From<(LspLanguageId, LspServerStatus)> for LspServerStatusDto {
    fn from((lang, status): (LspLanguageId, LspServerStatus)) -> Self {
        Self {
            language: lang.as_str().to_string(),
            status: status.to_string(),
        }
    }
}

// ---------- config commands ----------

pub async fn impl_lsp_get_configs(state: &AppState) -> Result<Vec<LspServerConfigDto>, IpcError> {
    let manager = &state.lsp_manager;
    let languages = manager.known_languages().await;
    let mut configs = Vec::new();
    for lang in languages {
        if let Some(cfg) = manager.effective_config(&lang).await {
            configs.push(cfg.into());
        }
    }
    Ok(configs)
}

pub async fn impl_lsp_set_config(
    state: &AppState,
    config: LspServerConfigDto,
) -> Result<(), IpcError> {
    state.lsp_manager.set_user_config(config.into()).await;
    Ok(())
}

pub async fn impl_lsp_get_statuses(state: &AppState) -> Result<Vec<LspServerStatusDto>, IpcError> {
    let statuses = state.lsp_manager.all_statuses().await;
    Ok(statuses.into_iter().map(Into::into).collect())
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ConfigSourceDto {
    pub language: String,
    pub source: String,
}

impl From<(LspLanguageId, ConfigSource)> for ConfigSourceDto {
    fn from((lang, source): (LspLanguageId, ConfigSource)) -> Self {
        let source = match source {
            ConfigSource::User => "user",
            ConfigSource::AutoDetected => "autodetected",
            ConfigSource::Default => "default",
            ConfigSource::None => "none",
        };
        Self {
            language: lang.as_str().to_string(),
            source: source.to_string(),
        }
    }
}

/// Force re-detection for a language, returning the newly detected config.
pub async fn impl_lsp_redetect(
    state: &AppState,
    language: String,
) -> Result<Option<LspServerConfigDto>, IpcError> {
    let lang = LspLanguageId::new(language);
    Ok(state
        .lsp_manager
        .redetect(&lang)
        .await
        .map(Into::into))
}

/// Get the config source for a language (user / autodetected / default / none).
pub async fn impl_lsp_config_source(
    state: &AppState,
    language: String,
) -> Result<ConfigSourceDto, IpcError> {
    let lang = LspLanguageId::new(language);
    Ok((lang.clone(), state.lsp_manager.config_source(&lang).await).into())
}

// ---------- lifecycle commands ----------

pub async fn impl_lsp_start(
    state: &AppState,
    language: String,
    root_uri: String,
) -> Result<(), IpcError> {
    let lang = LspLanguageId::new(language);
    state
        .lsp_manager
        .ensure_started(&lang, &root_uri)
        .await
        .map_err(IpcError::from)?;
    Ok(())
}

pub async fn impl_lsp_stop(state: &AppState, language: String) -> Result<(), IpcError> {
    let lang = LspLanguageId::new(language);
    state
        .lsp_manager
        .stop(&lang)
        .await
        .map_err(IpcError::from)?;
    Ok(())
}

// ---------- request-response commands (forward to LS) ----------

async fn get_client(
    state: &AppState,
    language: &str,
    root_uri: &str,
) -> Result<Arc<LspClient>, IpcError> {
    let lang = LspLanguageId::new(language);
    state
        .lsp_manager
        .ensure_started(&lang, root_uri)
        .await
        .map_err(IpcError::from)
}

pub async fn impl_lsp_completion(
    state: &AppState,
    language: String,
    root_uri: String,
    uri: String,
    line: u32,
    character: u32,
) -> Result<serde_json::Value, IpcError> {
    let client = get_client(state, &language, &root_uri).await?;
    client
        .completion(&uri, line, character)
        .await
        .map_err(IpcError::from)
}

pub async fn impl_lsp_hover(
    state: &AppState,
    language: String,
    root_uri: String,
    uri: String,
    line: u32,
    character: u32,
) -> Result<serde_json::Value, IpcError> {
    let client = get_client(state, &language, &root_uri).await?;
    client
        .hover(&uri, line, character)
        .await
        .map_err(IpcError::from)
}

pub async fn impl_lsp_definition(
    state: &AppState,
    language: String,
    root_uri: String,
    uri: String,
    line: u32,
    character: u32,
) -> Result<serde_json::Value, IpcError> {
    let client = get_client(state, &language, &root_uri).await?;
    client
        .definition(&uri, line, character)
        .await
        .map_err(IpcError::from)
}

pub async fn impl_lsp_references(
    state: &AppState,
    language: String,
    root_uri: String,
    uri: String,
    line: u32,
    character: u32,
) -> Result<serde_json::Value, IpcError> {
    let client = get_client(state, &language, &root_uri).await?;
    client
        .references(&uri, line, character)
        .await
        .map_err(IpcError::from)
}

pub async fn impl_lsp_signature_help(
    state: &AppState,
    language: String,
    root_uri: String,
    uri: String,
    line: u32,
    character: u32,
) -> Result<serde_json::Value, IpcError> {
    let client = get_client(state, &language, &root_uri).await?;
    client
        .signature_help(&uri, line, character)
        .await
        .map_err(IpcError::from)
}

pub async fn impl_lsp_formatting(
    state: &AppState,
    language: String,
    root_uri: String,
    uri: String,
) -> Result<serde_json::Value, IpcError> {
    let client = get_client(state, &language, &root_uri).await?;
    client.formatting(&uri).await.map_err(IpcError::from)
}

pub async fn impl_lsp_document_symbols(
    state: &AppState,
    language: String,
    root_uri: String,
    uri: String,
) -> Result<serde_json::Value, IpcError> {
    let client = get_client(state, &language, &root_uri).await?;
    client.document_symbols(&uri).await.map_err(IpcError::from)
}

// ---------- notification commands (fire-and-forget to LS) ----------

pub async fn impl_lsp_did_open(
    state: &AppState,
    language: String,
    root_uri: String,
    uri: String,
    language_id: String,
    text: String,
) -> Result<(), IpcError> {
    let client = get_client(state, &language, &root_uri).await?;
    client
        .did_open(&uri, &language_id, &text)
        .await
        .map_err(IpcError::from)
}

pub async fn impl_lsp_did_change(
    state: &AppState,
    language: String,
    root_uri: String,
    uri: String,
    version: i32,
    text: String,
) -> Result<(), IpcError> {
    let client = get_client(state, &language, &root_uri).await?;
    client
        .did_change(&uri, version, &text)
        .await
        .map_err(IpcError::from)
}

pub async fn impl_lsp_did_close(
    state: &AppState,
    language: String,
    root_uri: String,
    uri: String,
) -> Result<(), IpcError> {
    let client = get_client(state, &language, &root_uri).await?;
    client.did_close(&uri).await.map_err(IpcError::from)
}

pub async fn impl_lsp_did_save(
    state: &AppState,
    language: String,
    root_uri: String,
    uri: String,
    text: String,
) -> Result<(), IpcError> {
    let client = get_client(state, &language, &root_uri).await?;
    client.did_save(&uri, &text).await.map_err(IpcError::from)
}
