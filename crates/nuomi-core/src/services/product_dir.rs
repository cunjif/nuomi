//! Unified product directory service: the single file-system root where all
//! temporary artifacts from the six modules (sandbox, workbench, terminal,
//! chat, steward, evolution) are placed, indexed, and discovered.
//!
//! `<workspace>/.nuomi/products/<kind>/<producer>/<key>` is the canonical
//! path layout. Every product is registered in the `product_registry` SQLite
//! table so other modules can find and consume it without knowing the
//! producer's internal path conventions.
//!
//! This service is the spine of cross-module product sharing (ADR-0025 §4.2
//! extends the four-layer file hierarchy with a unified product layer).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The sub-directory name under `.nuomi/` for unified products.
pub const PRODUCTS_DIR_NAME: &str = "products";

/// Kinds of products that flow through the unified product directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductKind {
    ToolResult,
    SubagentResult,
    SearchResult,
    SandboxOutput,
    SandboxArtifact,
    TerminalLog,
    EvolutionSnapshot,
}

impl ProductKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProductKind::ToolResult => "tool_result",
            ProductKind::SubagentResult => "subagent_result",
            ProductKind::SearchResult => "search_result",
            ProductKind::SandboxOutput => "sandbox_output",
            ProductKind::SandboxArtifact => "sandbox_artifact",
            ProductKind::TerminalLog => "terminal_log",
            ProductKind::EvolutionSnapshot => "evolution_snapshot",
        }
    }

    pub fn parse(text: &str) -> Option<ProductKind> {
        match text {
            "tool_result" => Some(ProductKind::ToolResult),
            "subagent_result" => Some(ProductKind::SubagentResult),
            "search_result" => Some(ProductKind::SearchResult),
            "sandbox_output" => Some(ProductKind::SandboxOutput),
            "sandbox_artifact" => Some(ProductKind::SandboxArtifact),
            "terminal_log" => Some(ProductKind::TerminalLog),
            "evolution_snapshot" => Some(ProductKind::EvolutionSnapshot),
            _ => None,
        }
    }

    pub fn dir_name(&self) -> &'static str {
        match self {
            ProductKind::ToolResult => "tool_result",
            ProductKind::SubagentResult => "subagent_result",
            ProductKind::SearchResult => "search_result",
            ProductKind::SandboxOutput => "sandbox_output",
            ProductKind::SandboxArtifact => "sandbox_artifact",
            ProductKind::TerminalLog => "terminal_log",
            ProductKind::EvolutionSnapshot => "evolution_snapshot",
        }
    }
}

/// Identifies the module that produced a product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Producer {
    pub module: ProducerModule,
    pub id: String,
}

impl Producer {
    pub fn new(module: ProducerModule, id: impl Into<String>) -> Self {
        Self {
            module,
            id: id.into(),
        }
    }

    pub fn as_string(&self) -> String {
        format!("{}:{}", self.module.as_str(), self.id)
    }

    pub fn parse(text: &str) -> Option<Producer> {
        let (module_str, id) = text.split_once(':')?;
        let module = ProducerModule::parse(module_str)?;
        Some(Producer {
            module,
            id: id.to_string(),
        })
    }
}

/// The six modules that produce artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerModule {
    Loop,
    Steward,
    Sandbox,
    Terminal,
    Evolution,
    CrossWorkspace,
}

impl ProducerModule {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProducerModule::Loop => "loop",
            ProducerModule::Steward => "steward",
            ProducerModule::Sandbox => "sandbox",
            ProducerModule::Terminal => "terminal",
            ProducerModule::Evolution => "evolution",
            ProducerModule::CrossWorkspace => "cross_workspace",
        }
    }

    pub fn parse(text: &str) -> Option<ProducerModule> {
        match text {
            "loop" => Some(ProducerModule::Loop),
            "steward" => Some(ProducerModule::Steward),
            "sandbox" => Some(ProducerModule::Sandbox),
            "terminal" => Some(ProducerModule::Terminal),
            "evolution" => Some(ProducerModule::Evolution),
            "cross_workspace" => Some(ProducerModule::CrossWorkspace),
            _ => None,
        }
    }

    pub fn dir_name(&self) -> &'static str {
        self.as_str()
    }
}

/// A handle returned by `place` — the caller uses this to write the product
/// file and later register it in the database.
#[derive(Debug, Clone)]
pub struct ProductHandle {
    pub kind: ProductKind,
    pub producer: Producer,
    pub key: String,
    pub path: PathBuf,
}

/// Default TTL per kind (unix-ms from creation). `None` means no expiry.
pub fn default_ttl_ms(kind: ProductKind) -> Option<i64> {
    match kind {
        ProductKind::ToolResult => Some(7 * 24 * 3600 * 1000),       // 7 days
        ProductKind::SubagentResult => Some(7 * 24 * 3600 * 1000),   // 7 days
        ProductKind::SearchResult => Some(3600 * 1000),              // 1 hour
        ProductKind::SandboxOutput => Some(7 * 24 * 3600 * 1000),    // 7 days
        ProductKind::SandboxArtifact => Some(7 * 24 * 3600 * 1000),  // 7 days
        ProductKind::TerminalLog => Some(24 * 3600 * 1000),          // 24 hours
        ProductKind::EvolutionSnapshot => None,                       // no expiry
    }
}

/// Computes the canonical product path:
/// `<root>/.nuomi/products/<kind>/<producer_module>/<key>`
///
/// Does NOT create the directory — callers must `create_dir_all` on the
/// parent before writing.
pub fn product_path(
    root: &Path,
    kind: ProductKind,
    producer: &Producer,
    key: &str,
) -> PathBuf {
    root.join(crate::services::nuomi_dir::NUOMI_DIR_NAME)
        .join(PRODUCTS_DIR_NAME)
        .join(kind.dir_name())
        .join(producer.module.dir_name())
        .join(key)
}

/// Ensures the parent directory of a product path exists.
pub fn ensure_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Computes the sha256 of a file's contents.
pub fn file_sha256(path: &Path) -> std::io::Result<String> {
    use sha2::Digest;
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Returns the file size in bytes.
pub fn file_size(path: &Path) -> std::io::Result<u64> {
    Ok(std::fs::metadata(path)?.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_kind_roundtrip() {
        for kind in [
            ProductKind::ToolResult,
            ProductKind::SubagentResult,
            ProductKind::SearchResult,
            ProductKind::SandboxOutput,
            ProductKind::SandboxArtifact,
            ProductKind::TerminalLog,
            ProductKind::EvolutionSnapshot,
        ] {
            assert_eq!(ProductKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ProductKind::parse("unknown"), None);
    }

    #[test]
    fn producer_roundtrip() {
        let p = Producer::new(ProducerModule::Loop, "run-42");
        let s = p.as_string();
        assert_eq!(s, "loop:run-42");
        assert_eq!(Producer::parse(&s), Some(p));
    }

    #[test]
    fn product_path_layout() {
        let root = Path::new("C:\\ws");
        let producer = Producer::new(ProducerModule::Sandbox, "task-1");
        let path = product_path(root, ProductKind::SandboxOutput, &producer, "stdout.log");
        assert_eq!(
            path,
            Path::new("C:\\ws\\.nuomi\\products\\sandbox_output\\sandbox\\stdout.log")
        );
    }

    #[test]
    fn default_ttl_values() {
        assert!(default_ttl_ms(ProductKind::ToolResult).is_some());
        assert!(default_ttl_ms(ProductKind::EvolutionSnapshot).is_none());
        assert_eq!(
            default_ttl_ms(ProductKind::SearchResult),
            Some(3600 * 1000)
        );
    }

    #[test]
    fn file_sha256_works() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, b"hello world").unwrap();
        let hash = file_sha256(&path).unwrap();
        assert_eq!(hash.len(), 64); // hex-encoded 32 bytes
    }
}
