//! Tool registry: the set of tools an agent may call.

use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::providers::ToolDef;

use super::super::harness::{Event, HarnessError};

/// A callable tool. MCP tools, slave-provider delegations and builtins
/// all implement this.
#[async_trait]
pub trait Tool: Send + Sync {
    fn def(&self) -> ToolDef;
    async fn execute(&self, arguments: &serde_json::Value) -> Result<String, HarnessError>;
}

/// Registry shared via the plugin Context.
#[derive(Default)]
pub struct ToolRegistry {
    tools: tokio::sync::RwLock<BTreeMap<String, Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, tool: Arc<dyn Tool>) -> Result<(), HarnessError> {
        let name = tool.def().name.clone();
        let mut tools = self.tools.write().await;
        if tools.contains_key(&name) {
            return Err(HarnessError::DuplicateService {
                name: format!("tool#{name}"),
                owner: "registry".into(),
            });
        }
        tools.insert(name, tool);
        Ok(())
    }

    pub async fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().await.get(name).cloned()
    }

    /// Tool definitions, optionally filtered by a role allowlist
    /// (empty allowlist = unrestricted).
    pub async fn defs_for(&self, allowlist: &[String]) -> Vec<ToolDef> {
        let tools = self.tools.read().await;
        tools
            .values()
            .filter(|t| allowlist.is_empty() || allowlist.iter().any(|a| *a == t.def().name))
            .map(|t| t.def())
            .collect()
    }

    /// Executes a tool by name with JSON arguments.
    pub async fn execute(
        &self,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<String, HarnessError> {
        let tool = self
            .get(name)
            .await
            .ok_or_else(|| HarnessError::PluginNotFound(format!("tool#{name}")))?;
        tool.execute(arguments).await
    }

    /// Publishes a `tool.call` event for observability (bus is injected).
    pub(crate) fn event(name: &str, args: &serde_json::Value) -> Event {
        Event::new(
            "tool.call",
            serde_json::json!({ "tool": name, "arguments": args }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Echo;

    #[async_trait]
    impl Tool for Echo {
        fn def(&self) -> ToolDef {
            ToolDef {
                name: "echo".into(),
                description: "echoes".into(),
                parameters: json!({}),
            }
        }
        async fn execute(&self, args: &serde_json::Value) -> Result<String, HarnessError> {
            Ok(args.to_string())
        }
    }

    #[tokio::test]
    async fn register_execute_and_allowlist() {
        let reg = ToolRegistry::new();
        reg.register(Arc::new(Echo)).await.unwrap();
        assert!(reg.register(Arc::new(Echo)).await.is_err());
        assert_eq!(
            reg.execute("echo", &json!({ "x": 1 })).await.unwrap(),
            r#"{"x":1}"#
        );
        assert!(reg.execute("missing", &json!({})).await.is_err());
        assert_eq!(reg.defs_for(&[]).await.len(), 1);
        assert_eq!(reg.defs_for(&["other".to_string()]).await.len(), 0);
        assert_eq!(reg.defs_for(&["echo".to_string()]).await.len(), 1);
    }
}
