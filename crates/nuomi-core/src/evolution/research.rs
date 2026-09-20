//! Allowlisted online research (AC13): fetches reference material from a
//! fixed domain allowlist, gated behind a persistent authorization switch.
//! Output is report entries only — never a direct prompt rewrite.

use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::EvolutionSettings;
use crate::plugins::MemoryService;
use crate::store::StoreError;

use super::EvolutionError;

/// Fetch excerpts are truncated to this many characters.
pub const MAX_EXCERPT_CHARS: usize = 8000;

const AUTH_TAG: &str = "setting";
const AUTH_MARKER: &str = "evolution_online_authorized";

/// The fixed research allowlist. Production sources only — tests inject
/// local mock servers via `ResearchFetcher` URL overrides.
pub struct ResearchAllowlist;

impl ResearchAllowlist {
    pub const DOMAINS: [&'static str; 9] = [
        "github.com",
        "raw.githubusercontent.com",
        "deepseek.com",
        "shikigami.dev",
        "t3.codes",
        "1code.dev",
        "aoagents.dev",
        "parallelcode.app",
        "agor.live",
    ];

    /// True when `url_or_domain` points at an allowlisted domain (or a
    /// subdomain of one). Uses the built-in `DOMAINS` constant.
    pub fn is_allowed(&self, url_or_domain: &str) -> bool {
        Self::is_allowed_with(&Self::DOMAINS.iter().map(|s| s.to_string()).collect::<Vec<_>>(), url_or_domain)
    }

    /// Parameterized allowlist check — accepts a custom `allowlist` and
    /// checks whether `url_or_domain` matches (exact or subdomain).
    /// Falls back to `DOMAINS` when `allowlist` is empty.
    pub fn is_allowed_with(allowlist: &[String], url_or_domain: &str) -> bool {
        let host = normalize_host(url_or_domain);
        let domains: Vec<&str> = if allowlist.is_empty() {
            Self::DOMAINS.to_vec()
        } else {
            allowlist.iter().map(|s| s.as_str()).collect()
        };
        domains
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    }
}

fn normalize_host(raw: &str) -> String {
    let s = raw.trim().to_ascii_lowercase();
    let after_scheme = match s.find("://") {
        Some(idx) => &s[idx + 3..],
        None => s.as_str(),
    };
    let host = after_scheme.split('/').next().unwrap_or(after_scheme);
    host.split('?').next().unwrap_or(host).to_string()
}

/// One fetched research artifact (report entry, not a prompt mutation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchReportEntry {
    /// Logical allowlisted source the entry came from.
    pub source: String,
    /// Actual URL fetched (differs from production only via test overrides).
    pub source_url: String,
    pub excerpt: String,
}

/// HTTP fetcher bound to the [`ResearchAllowlist`]. `overrides` maps a
/// logical source domain to a concrete URL; production leaves it empty,
/// tests map domains onto a local mock server.
pub struct ResearchFetcher {
    http: reqwest::Client,
    overrides: HashMap<String, String>,
}

impl ResearchFetcher {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            overrides: HashMap::new(),
        }
    }

    pub fn with_overrides(http: reqwest::Client, overrides: HashMap<String, String>) -> Self {
        Self { http, overrides }
    }

    pub async fn fetch(&self, source: &str) -> Result<ResearchReportEntry, EvolutionError> {
        self.fetch_with(&ResearchAllowlist::DOMAINS.iter().map(|s| s.to_string()).collect::<Vec<_>>(), source).await
    }

    /// Parameterized fetch — checks against a custom `allowlist` before
    /// issuing the HTTP request.
    pub async fn fetch_with(
        &self,
        allowlist: &[String],
        source: &str,
    ) -> Result<ResearchReportEntry, EvolutionError> {
        if !ResearchAllowlist::is_allowed_with(allowlist, source) {
            return Err(EvolutionError::SourceNotAllowlisted(source.to_string()));
        }
        let url = self
            .overrides
            .get(source)
            .cloned()
            .unwrap_or_else(|| format!("https://{source}"));
        let response =
            self.http
                .get(&url)
                .send()
                .await
                .map_err(|e| EvolutionError::FetchFailed {
                    url: url.clone(),
                    message: e.to_string(),
                })?;
        let text = response
            .text()
            .await
            .map_err(|e| EvolutionError::FetchFailed {
                url: url.clone(),
                message: e.to_string(),
            })?;
        let excerpt: String = text.chars().take(MAX_EXCERPT_CHARS).collect();
        Ok(ResearchReportEntry {
            source: source.to_string(),
            source_url: url,
            excerpt,
        })
    }
}

/// Runs one research pass over a configured subset of allowlisted sources.
/// Without authorization it never issues a single network request.
pub struct ResearchScheduler {
    fetcher: Arc<ResearchFetcher>,
    sources: Vec<String>,
}

impl ResearchScheduler {
    pub fn new(
        fetcher: Arc<ResearchFetcher>,
        sources: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            fetcher,
            sources: sources.into_iter().map(Into::into).collect(),
        }
    }

    pub async fn run_once(
        &self,
        topic: &str,
        authorized: bool,
    ) -> Result<Vec<ResearchReportEntry>, EvolutionError> {
        if !authorized {
            // Hard gate: no HTTP call may happen below this line.
            return Err(EvolutionError::NotAuthorized);
        }
        let mut entries = Vec::new();
        for source in &self.sources {
            match self.fetcher.fetch(source).await {
                Ok(mut entry) => {
                    entry.excerpt = format!("# research: {topic}\n{}", entry.excerpt);
                    entries.push(entry);
                }
                Err(err) => {
                    tracing::warn!(source = %source, error = %err, "research source skipped");
                }
            }
        }
        Ok(entries)
    }

    /// Settings-driven research pass: authorization, allowlist, and source
    /// filtering all come from `EvolutionSettings`. Replaces the fixed
    /// `authorized: bool` gate with a full configuration object.
    pub async fn run_once_with_settings(
        &self,
        topic: &str,
        settings: &EvolutionSettings,
    ) -> Result<Vec<ResearchReportEntry>, EvolutionError> {
        if !settings.online_learning.authorized {
            return Err(EvolutionError::NotAuthorized);
        }
        let allowlist = &settings.online_learning.allowlist;
        let mut entries = Vec::new();
        for source in &self.sources {
            match self.fetcher.fetch_with(allowlist, source).await {
                Ok(mut entry) => {
                    entry.excerpt = format!("# research: {topic}\n{}", entry.excerpt);
                    entries.push(entry);
                }
                Err(err) => {
                    tracing::warn!(source = %source, error = %err, "research source skipped");
                }
            }
        }
        Ok(entries)
    }
}

/// Persists the permanent online-learning authorization switch in the
/// `memory_entries` table (`kind="setting"`, marker-prefixed content).
/// Newest matching entry wins.
pub async fn set_online_authorized(
    mem: &MemoryService,
    authorized: bool,
) -> Result<(), StoreError> {
    mem.remember(
        format!("{AUTH_MARKER}={authorized}"),
        None,
        vec![AUTH_TAG.to_string()],
        "setting",
        false,
    )
    .await
    .map(|_| ())
}

/// Reads the persisted authorization switch (defaults to `false`).
pub async fn online_authorized(mem: &MemoryService) -> bool {
    match mem.recall(None, Some(AUTH_TAG.to_string()), 10).await {
        Ok(entries) => entries
            .into_iter()
            .find_map(|m| {
                m.content
                    .strip_prefix(&format!("{AUTH_MARKER}="))
                    .map(|v| v == "true")
            })
            .unwrap_or(false),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::*;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn scheduler(server_uri: String, sources: &[&str]) -> ResearchScheduler {
        let mut overrides = HashMap::new();
        for s in sources {
            overrides.insert((*s).to_string(), server_uri.clone());
        }
        let fetcher = Arc::new(ResearchFetcher::with_overrides(
            reqwest::Client::new(),
            overrides,
        ));
        ResearchScheduler::new(fetcher, sources.iter().copied())
    }

    #[test]
    fn allowlist_accepts_domains_and_subdomains_only() {
        let al = ResearchAllowlist;
        assert!(al.is_allowed("github.com"));
        assert!(al.is_allowed("https://raw.githubusercontent.com/org/repo/main/README.md"));
        assert!(al.is_allowed("api.deepseek.com"));
        assert!(!al.is_allowed("evil.example"));
        assert!(!al.is_allowed("notgithub.com"));
        assert!(!al.is_allowed("github.com.evil.example"));
    }

    #[test]
    fn parameterized_allowlist_accepts_custom_domains() {
        let custom = vec!["custom.dev".to_string(), "api.example.org".to_string()];
        assert!(ResearchAllowlist::is_allowed_with(&custom, "custom.dev"));
        assert!(ResearchAllowlist::is_allowed_with(&custom, "sub.custom.dev"));
        assert!(ResearchAllowlist::is_allowed_with(&custom, "https://api.example.org/path"));
        assert!(!ResearchAllowlist::is_allowed_with(&custom, "github.com"));
    }

    #[test]
    fn parameterized_allowlist_falls_back_to_domains_when_empty() {
        let empty: Vec<String> = vec![];
        assert!(ResearchAllowlist::is_allowed_with(&empty, "github.com"));
        assert!(!ResearchAllowlist::is_allowed_with(&empty, "evil.example"));
    }

    #[tokio::test]
    async fn unauthorized_run_never_issues_a_request() {
        let server = MockServer::start().await;
        let hit = Mock::given(any())
            .respond_with(ResponseTemplate::new(200))
            .expect(0);
        server.register(hit).await;

        let sched = scheduler(server.uri(), &["github.com"]);
        let err = sched.run_once("rust patterns", false).await.unwrap_err();
        assert!(matches!(err, EvolutionError::NotAuthorized));
        // Zero requests reached the mock server.
        server.verify().await;
    }

    #[tokio::test]
    async fn authorized_run_fetches_via_override_and_truncates() {
        let body = "x".repeat(MAX_EXCERPT_CHARS + 500);
        let server = MockServer::start().await;
        let hit = Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .expect(1);
        server.register(hit).await;

        let sched = scheduler(server.uri(), &["github.com"]);
        let entries = sched.run_once("gepa", true).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, "github.com");
        assert_eq!(entries[0].source_url, server.uri());
        let expected_len = format!("# research: {topic}\n", topic = "gepa")
            .chars()
            .count()
            + MAX_EXCERPT_CHARS;
        assert_eq!(entries[0].excerpt.chars().count(), expected_len);
        assert!(entries[0].excerpt.starts_with("# research: gepa\n"));
        server.verify().await;
    }

    #[tokio::test]
    async fn non_allowlisted_source_is_rejected_before_any_http() {
        let fetcher = ResearchFetcher::new(reqwest::Client::new());
        let err = fetcher
            .fetch("totally-not-allowlisted.dev")
            .await
            .unwrap_err();
        assert!(
            matches!(err, EvolutionError::SourceNotAllowlisted(ref s) if s == "totally-not-allowlisted.dev")
        );
    }

    #[tokio::test]
    async fn authorization_switch_persists_across_service_instances() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.db");
        let mem = MemoryService::new(path.to_string_lossy().to_string());

        assert!(!online_authorized(&mem).await);
        set_online_authorized(&mem, true).await.unwrap();
        // A fresh instance over the same db sees the persisted switch.
        let mem2 = MemoryService::new(path.to_string_lossy().to_string());
        assert!(online_authorized(&mem2).await);

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        set_online_authorized(&mem2, false).await.unwrap();
        assert!(!online_authorized(&mem2).await);
    }
}
