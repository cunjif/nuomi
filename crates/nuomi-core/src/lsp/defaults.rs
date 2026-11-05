//! Default Language Server command mapping per language (ADR-0027 §5).
//!
//! These are the common LS commands for each supported language. Actual
//! availability depends on the user's PATH; the manager uses a three-tier
//! discovery strategy: user config → SDK config inference → PATH detection.

use super::types::{LspLanguageId, LspServerConfig};

/// Returns the default LspServerConfig for a given language, if known.
pub fn default_config(language: &LspLanguageId) -> Option<LspServerConfig> {
    match language.as_str() {
        "typescript" => Some(
            LspServerConfig::new("typescript", "typescript-language-server")
                .with_args(vec!["--stdio".into()]),
        ),
        "javascript" => Some(
            LspServerConfig::new("javascript", "typescript-language-server")
                .with_args(vec!["--stdio".into()]),
        ),
        "python" => Some(
            LspServerConfig::new("python", "pyright-langserver").with_args(vec!["--stdio".into()]),
        ),
        "java" => Some(LspServerConfig::new("java", "jdtls")),
        "go" => Some(LspServerConfig::new("go", "gopls").with_args(vec!["serve".into()])),
        "rust" => Some(LspServerConfig::new("rust", "rust-analyzer")),
        "c" | "cpp" => Some(LspServerConfig::new(language.as_str(), "clangd")),
        _ => None,
    }
}

/// Returns all default LspServerConfigs.
pub fn all_defaults() -> Vec<LspServerConfig> {
    [
        "typescript",
        "javascript",
        "python",
        "java",
        "go",
        "rust",
        "c",
        "cpp",
    ]
    .iter()
    .filter_map(|lang| default_config(&LspLanguageId::new(*lang)))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_typescript() {
        let cfg = default_config(&LspLanguageId::new("typescript")).unwrap();
        assert_eq!(cfg.command, "typescript-language-server");
        assert_eq!(cfg.args, vec!["--stdio"]);
    }

    #[test]
    fn default_config_python() {
        let cfg = default_config(&LspLanguageId::new("python")).unwrap();
        assert_eq!(cfg.command, "pyright-langserver");
        assert_eq!(cfg.args, vec!["--stdio"]);
    }

    #[test]
    fn default_config_go() {
        let cfg = default_config(&LspLanguageId::new("go")).unwrap();
        assert_eq!(cfg.command, "gopls");
        assert_eq!(cfg.args, vec!["serve"]);
    }

    #[test]
    fn default_config_rust() {
        let cfg = default_config(&LspLanguageId::new("rust")).unwrap();
        assert_eq!(cfg.command, "rust-analyzer");
        assert!(cfg.args.is_empty());
    }

    #[test]
    fn default_config_cpp() {
        let cfg = default_config(&LspLanguageId::new("cpp")).unwrap();
        assert_eq!(cfg.command, "clangd");
        assert_eq!(cfg.language.as_str(), "cpp");
    }

    #[test]
    fn default_config_unknown_language() {
        assert!(default_config(&LspLanguageId::new("brainfuck")).is_none());
    }

    #[test]
    fn all_defaults_covers_eight_languages() {
        let configs = all_defaults();
        assert_eq!(configs.len(), 8);
    }
}
