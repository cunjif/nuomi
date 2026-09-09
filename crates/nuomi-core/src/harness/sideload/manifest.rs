//! `plugin.toml` manifest: parsing, validation, unknown-key warnings.
//!
//! Format reference: docs/plugins/plugin-format.md (ADR 0009).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::protocol::NPP_API_VERSION;

/// Manifest parse/validation failure. Never crashes boot — the loader turns
/// these into `LoadOutcome::Failed` entries.
#[derive(Debug, thiserror::Error)]
#[error("invalid plugin manifest: {0}")]
pub struct ManifestError(pub String);

fn invalid<T>(msg: impl Into<String>) -> Result<T, ManifestError> {
    Err(ManifestError(msg.into()))
}

/// `[permissions]` — v1: declared + displayed, not enforced (ADR 0009 §5).
/// TOML dotted keys (`fs.read`) nest, hence the two-level struct.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Permissions {
    #[serde(default)]
    pub fs: FsPermissions,
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub shell: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FsPermissions {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
}

/// One `[[tools]]` entry. `input` is a JSON Schema object.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolContribution {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_input_schema")]
    pub input: serde_json::Value,
    #[serde(default = "default_tool_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_input_schema() -> serde_json::Value {
    serde_json::json!({ "type": "object" })
}

fn default_tool_timeout_ms() -> u64 {
    10_000
}

/// Hook points mirror `plugins::hooks::HookPoint` but are parsed from strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPointSpec {
    PreToolCall,
    PostToolCall,
    SessionStart,
    SessionEnd,
}

impl HookPointSpec {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw {
            "pre_tool_call" => Some(Self::PreToolCall),
            "post_tool_call" => Some(Self::PostToolCall),
            "session_start" => Some(Self::SessionStart),
            "session_end" => Some(Self::SessionEnd),
            _ => None,
        }
    }

    /// Maps onto the kernel hook point for `HookRegistry::add`.
    pub fn to_kernel(self) -> crate::plugins::hooks::HookPoint {
        match self {
            Self::PreToolCall => crate::plugins::hooks::HookPoint::PreToolCall,
            Self::PostToolCall => crate::plugins::hooks::HookPoint::PostToolCall,
            Self::SessionStart => crate::plugins::hooks::HookPoint::SessionStart,
            Self::SessionEnd => crate::plugins::hooks::HookPoint::SessionEnd,
        }
    }
}

/// One `[[hooks]]` entry.
#[derive(Debug, Clone, Deserialize)]
pub struct HookContribution {
    pub point: String,
    #[serde(default = "default_hook_order")]
    pub order: u32,
}

fn default_hook_order() -> u32 {
    100
}

/// One `[[events]]` entry. Supports `*`, exact, dot-prefix (`session`) and
/// the documented trailing-wildcard spelling (`session.*`).
#[derive(Debug, Clone, Deserialize)]
pub struct EventSubscription {
    pub topic: String,
}

/// The parsed manifest. Field order follows docs/plugins/plugin-format.md.
#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    pub entry: Vec<String>,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub tools: Vec<ToolContribution>,
    #[serde(default)]
    pub hooks: Vec<HookContribution>,
    #[serde(default)]
    pub events: Vec<EventSubscription>,
}

/// Bare-key charset shared by `id` and tool names (kebab-case).
fn is_kebab(raw: &str) -> bool {
    let mut chars = raw.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

impl PluginManifest {
    /// Parses and validates `plugin.toml` inside `dir`; unknown top-level /
    /// permissions keys are warned (forward compatibility), not rejected.
    pub fn load(dir: &Path) -> Result<Self, ManifestError> {
        let path = dir.join("plugin.toml");
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| ManifestError(format!("{} unreadable: {e}", path.display())))?;
        let table: toml::Table = raw
            .parse()
            .map_err(|e| ManifestError(format!("{} is not valid TOML: {e}", path.display())))?;
        Self::warn_unknown_keys(&table);
        let manifest: PluginManifest = table
            .clone()
            .try_into()
            .map_err(|e| ManifestError(format!("{} schema mismatch: {e}", path.display())))?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn warn_unknown_keys(table: &toml::Table) {
        const KNOWN: &[&str] = &[
            "id",
            "name",
            "version",
            "api_version",
            "description",
            "authors",
            "license",
            "entry",
            "permissions",
            "tools",
            "hooks",
            "events",
        ];
        for key in table.keys() {
            if !KNOWN.contains(&key.as_str()) {
                tracing::warn!(
                    "plugin.toml: unknown top-level key '{key}' ignored (forward compat)"
                );
            }
        }
        if let Some(perms) = table.get("permissions").and_then(|v| v.as_table()) {
            for key in perms.keys() {
                if !matches!(key.as_str(), "fs" | "network" | "shell") {
                    tracing::warn!(
                        "plugin.toml: unknown permissions key '{key}' ignored (forward compat)"
                    );
                }
            }
        }
    }

    fn validate(&self) -> Result<(), ManifestError> {
        if !is_kebab(&self.id) {
            return invalid(format!("id '{}' must match ^[a-z][a-z0-9-]*$", self.id));
        }
        if self.name.trim().is_empty() {
            return invalid("name must not be empty");
        }
        if self.api_version == 0 || self.api_version > NPP_API_VERSION {
            return invalid(format!(
                "api_version {} unsupported (host speaks <= {NPP_API_VERSION})",
                self.api_version
            ));
        }
        if self.entry.is_empty() || self.entry.iter().any(String::is_empty) {
            return invalid("entry must be a non-empty argv array (no empty elements)");
        }
        for tool in &self.tools {
            if !is_kebab(&tool.name) {
                return invalid(format!("tool name '{}' must be kebab-case", tool.name));
            }
            if tool.timeout_ms == 0 {
                return invalid(format!("tool '{}' timeout_ms must be > 0", tool.name));
            }
        }
        for hook in &self.hooks {
            if HookPointSpec::parse(&hook.point).is_none() {
                return invalid(format!(
                    "hook point '{}' must be one of pre_tool_call|post_tool_call|session_start|session_end",
                    hook.point
                ));
            }
        }
        for event in &self.events {
            if event.topic.trim().is_empty() {
                return invalid("event topic must not be empty");
            }
        }
        Ok(())
    }

    /// The argv for the plugin process. Paths are used as authored: the
    /// program name resolves via PATH, and relative entry paths resolve
    /// against the plugin directory because the process is spawned with
    /// `current_dir` set to it (`PluginProcess::spawn`).
    pub fn resolved_entry(&self, _dir: &Path) -> Vec<PathBuf> {
        self.entry.iter().map(PathBuf::from).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &Path, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("plugin.toml"), body).unwrap();
    }

    #[test]
    fn parses_full_manifest() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            r#"
id = "upper"
name = "Upper"
version = "0.1.0"
api_version = 1
entry = ["node", "plugin.cjs"]
[permissions]
fs.read = ["./data/**"]
network = ["api.example.com"]
shell = false
[[tools]]
name = "upper"
description = "up"
input = { type = "object" }
timeout_ms = 2000
[[hooks]]
point = "post_tool_call"
order = 42
[[events]]
topic = "session.*"
"#,
        );
        let manifest = PluginManifest::load(dir.path()).unwrap();
        assert_eq!(manifest.id, "upper");
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].timeout_ms, 2000);
        assert_eq!(manifest.hooks[0].order, 42);
        assert_eq!(manifest.permissions.fs.read, vec!["./data/**".to_string()]);
        let entry = manifest.resolved_entry(dir.path());
        assert_eq!(entry[1], Path::new("plugin.cjs"));
    }

    #[test]
    fn unknown_keys_are_ignored_not_rejected() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            r#"
id = "fut"
name = "Future"
version = "0.1.0"
api_version = 1
entry = ["run"]
brand_new_capability = true
"#,
        );
        assert!(PluginManifest::load(dir.path()).is_ok());
    }

    #[test]
    fn rejects_bad_id_and_api_version_and_entry() {
        let base = r#"
id = "ok"
name = "X"
version = "0.1.0"
api_version = 1
entry = ["run"]
"#;
        let bodies = [
            (base.replace("id = \"ok\"", "id = \"UPPER\""), "id"),
            (
                base.replace("api_version = 1", "api_version = 99"),
                "api_version",
            ),
            (base.replace("entry = [\"run\"]", "entry = []"), "entry"),
        ];
        for (body, needle) in bodies {
            let dir = tempfile::tempdir().unwrap();
            write_manifest(dir.path(), &body);
            let err = PluginManifest::load(dir.path()).unwrap_err().to_string();
            assert!(err.contains(needle), "{err}");
        }
    }

    #[test]
    fn hook_point_must_be_known() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            r#"
id = "h"
name = "H"
version = "0.1.0"
api_version = 1
entry = ["run"]
[[hooks]]
point = "lunchtime"
"#,
        );
        let err = PluginManifest::load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("hook point"), "{err}");
    }
}
