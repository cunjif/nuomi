//! Source-evidence collector (chat_rich_bind spec 5.2, design §2.1.3.2).
//!
//! Collects file paths and web URLs touched by a RoleAgent's HOST-EXECUTED
//! read/search tool calls during one loop run. Entries ride the assistant
//! `message` event payload (`sources` array, D1) so replay/resume stay
//! consistent with zero extra tables. Agent identity (agentId/agentName) is
//! filled by the persistence phase (facade), which knows the turn's role.

use serde::Serialize;

/// A collected source entry. `agent_id`/`agent_name` are empty at collection
/// time (the loop engine has no display names) and completed at persist time.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SourceEntry {
    /// `workspace_file` | `external_file` | `web_url` | `cli_agent`.
    pub kind: String,
    /// Absolute path (files) or URL (web).
    pub target: String,
    /// Display label (file name / host).
    pub label: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub agent_name: String,
}

impl SourceEntry {
    /// A `cli_agent` identity entry (spec 5.2.1-4 degraded shape).
    pub fn cli_agent(agent_id: &str, agent_name: &str) -> Self {
        Self {
            kind: "cli_agent".to_string(),
            target: agent_id.to_string(),
            label: agent_name.to_string(),
            agent_id: agent_id.to_string(),
            agent_name: agent_name.to_string(),
        }
    }
}

/// Read/search-ish tool names whose calls yield source evidence. Host file
/// tools are registered per deployment; matching is by exact tool name and
/// stays independent from the sensitive-tools policy (design §2.1.3.2).
pub const SOURCE_TOOL_NAMES: &[&str] = &[
    "read_file",
    "search_files",
    "list_dir",
    "glob",
    "grep",
    "fs.read",
    "fs.search",
];

/// Extracts an absolute-looking path from tool arguments: the first string
/// field named `path`/`file`/`abs_path` that looks like a filesystem path.
fn extract_path_argument(arguments: &serde_json::Value) -> Option<String> {
    const KEYS: [&str; 3] = ["path", "file", "abs_path"];
    let obj = arguments.as_object()?;
    for key in KEYS {
        if let Some(value) = obj.get(key) {
            if let Some(s) = value.as_str() {
                if looks_like_path(s) {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// Heuristic: contains a path separator and no URL scheme.
fn looks_like_path(s: &str) -> bool {
    !s.is_empty() && (s.contains('/') || s.contains('\\')) && !s.contains("://")
}

/// Extracts http/https URLs from free text (tool result bodies).
fn extract_urls(text: &str) -> Vec<String> {
    const PREFIXES: [&str; 2] = ["http://", "https://"];
    let mut urls = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let earliest = PREFIXES
            .iter()
            .filter_map(|p| rest.find(p).map(|at| (at, *p)))
            .min_by_key(|(at, _)| *at);
        let Some((at, prefix)) = earliest else { break };
        let after = &rest[at + prefix.len()..];
        let end = after
            .find(|c: char| {
                c.is_whitespace() || matches!(c, ')' | ']' | '}' | '"' | '\'' | '<' | '|' | ',')
            })
            .unwrap_or(after.len());
        if end > 0 {
            urls.push(format!("{prefix}{}", &after[..end]));
        }
        let skip = at + prefix.len() + end;
        rest = rest.get(skip..).unwrap_or("");
    }
    urls
}

/// Short display label for a file path: the final path segment.
fn file_label(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// Short display label for a URL: the host (+ first path segment).
fn url_label(url: &str) -> String {
    let no_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    no_scheme
        .split_once('/')
        .map(|(host, _)| host.to_string())
        .unwrap_or_else(|| no_scheme.to_string())
}

/// Collects evidence from one host tool call into `out` (deduped by
/// `(kind, target)`). Only whitelisted read/search tools contribute.
pub fn collect_from_tool_call(
    tool: &str,
    arguments: &serde_json::Value,
    result: &str,
    out: &mut Vec<SourceEntry>,
) {
    if !SOURCE_TOOL_NAMES.contains(&tool) {
        return;
    }
    let mut candidates: Vec<SourceEntry> = Vec::new();
    if let Some(path) = extract_path_argument(arguments) {
        candidates.push(SourceEntry {
            kind: "workspace_file".to_string(),
            target: path.clone(),
            label: file_label(&path),
            agent_id: String::new(),
            agent_name: String::new(),
        });
    }
    for url in extract_urls(result) {
        candidates.push(SourceEntry {
            kind: "web_url".to_string(),
            target: url.clone(),
            label: url_label(&url),
            agent_id: String::new(),
            agent_name: String::new(),
        });
    }
    for entry in candidates {
        if !out
            .iter()
            .any(|existing| existing.kind == entry.kind && existing.target == entry.target)
        {
            out.push(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn whitelisted_tool_collects_path_and_urls() {
        let mut out = Vec::new();
        collect_from_tool_call(
            "read_file",
            &json!({ "path": "C:\\proj\\src\\main.rs" }),
            "see https://example.com/docs and http://example.org/x",
            &mut out,
        );
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].kind, "workspace_file");
        assert_eq!(out[0].target, "C:\\proj\\src\\main.rs");
        assert_eq!(out[0].label, "main.rs");
        assert_eq!(out[1].kind, "web_url");
        assert_eq!(out[1].target, "https://example.com/docs");
        assert_eq!(out[1].label, "example.com");
        assert_eq!(out[2].target, "http://example.org/x");
    }

    #[test]
    fn non_whitelisted_tool_collects_nothing() {
        let mut out = Vec::new();
        collect_from_tool_call(
            "fs.write",
            &json!({ "path": "C:\\x" }),
            "https://a.com",
            &mut out,
        );
        assert!(out.is_empty());
        collect_from_tool_call("exchange_read", &json!({ "path": "C:\\x" }), "", &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn same_target_deduped_across_calls() {
        let mut out = Vec::new();
        collect_from_tool_call("read_file", &json!({ "path": "/a/b/c.txt" }), "", &mut out);
        collect_from_tool_call("read_file", &json!({ "path": "/a/b/c.txt" }), "", &mut out);
        collect_from_file_read_alias(&mut out);
        assert_eq!(out.len(), 1);
    }

    fn collect_from_file_read_alias(out: &mut Vec<SourceEntry>) {
        collect_from_tool_call("fs.read", &json!({ "path": "/a/b/c.txt" }), "", out);
    }

    #[test]
    fn url_extraction_stops_at_delimiters() {
        let urls = extract_urls("go to https://example.com/page, then http://x.io). done");
        assert_eq!(
            urls,
            vec![
                "https://example.com/page".to_string(),
                "http://x.io".to_string()
            ]
        );
    }

    #[test]
    fn path_argument_rejects_urls_and_relative_junk() {
        let mut out = Vec::new();
        collect_from_tool_call(
            "read_file",
            &json!({ "path": "https://not-a-path" }),
            "",
            &mut out,
        );
        collect_from_tool_call("read_file", &json!({ "path": "plainname" }), "", &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn cli_agent_entry_shape() {
        let entry = SourceEntry::cli_agent("ap-1", "Codex CLI");
        assert_eq!(entry.kind, "cli_agent");
        assert_eq!(entry.label, "Codex CLI");
        assert_eq!(entry.agent_id, "ap-1");
    }
}
