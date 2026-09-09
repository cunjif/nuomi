//! Process-wide shared HTTP connection pool and startup warming.
//!
//! Every HTTP outlet in the crate (provider clients, MCP HTTP transport,
//! webhook integrations) draws its `reqwest::Client` from [`shared_client`],
//! so DNS lookups, TLS handshakes and idle connections are reused across all
//! of them instead of each component paying cold-start costs on its own pool.
//!
//! [`warm`] pre-resolves DNS and pre-establishes TLS against every configured
//! provider base URL by issuing one cheap request; failures are soft and only
//! logged, so warming never blocks or breaks startup.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use futures::future::join_all;

use super::ProviderError;

/// Idle keep-alive connections kept per host in the shared pool.
const POOL_MAX_IDLE_PER_HOST: usize = 32;

/// Idle connections older than this are closed (mirrors common harness
/// practice; servers usually close at ~60-120s).
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// Per-request budget for one warm probe.
const WARM_TIMEOUT: Duration = Duration::from_secs(5);

static SHARED_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Returns the process-wide shared HTTP client (cheap `Arc` clone; all
/// handles share one connection pool).
///
/// Configuration: `pool_max_idle_per_host(32)`, `pool_idle_timeout(90s)`,
/// `tcp_nodelay(true)`. No client-level timeout is set — call sites apply
/// their own per-request timeouts, preserving existing semantics. HTTP/2 is
/// enabled by default (negotiated via ALPN on TLS connections).
pub fn shared_client() -> reqwest::Client {
    SHARED_CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .pool_max_idle_per_host(POOL_MAX_IDLE_PER_HOST)
                .pool_idle_timeout(POOL_IDLE_TIMEOUT)
                .tcp_nodelay(true)
                .build()
                // TLS backend init failure is pathological; fall back to a
                // default client rather than panicking at startup.
                .unwrap_or_else(|_| reqwest::Client::new())
        })
        .clone()
}

/// Client for one provider endpoint: the shared direct pool when `proxy` is
/// `None`/empty, otherwise a dedicated pool routed through the proxy URL
/// (`http://host:port`; socks requires the reqwest `socks` feature).
/// Proxied and direct traffic deliberately never share connections.
///
/// Errors (unparseable proxy URL, TLS backend failure) surface to the caller
/// — the settings UI shows them instead of silently falling back to a direct
/// connection, which would leak traffic the user wanted proxied.
pub fn client_for_endpoint(proxy: Option<&str>) -> Result<reqwest::Client, ProviderError> {
    let Some(proxy) = proxy.map(str::trim).filter(|p| !p.is_empty()) else {
        return Ok(shared_client());
    };
    Ok(reqwest::Client::builder()
        .pool_max_idle_per_host(POOL_MAX_IDLE_PER_HOST)
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        .tcp_nodelay(true)
        .proxy(reqwest::Proxy::all(proxy)?)
        .build()?)
}

/// Outcome of one warm probe against a provider base URL.
#[derive(Debug, Clone)]
pub struct WarmResult {
    /// The probed base URL (trailing slash trimmed).
    pub base_url: String,
    /// `true` when the probe completed with a 2xx status.
    pub ok: bool,
    /// Human-readable outcome: `"<status>"` on HTTP replies, the transport
    /// error message otherwise.
    pub detail: String,
    /// Round-trip latency of the probe.
    pub latency_ms: u64,
}

/// Warms the shared connection pool against each base URL concurrently.
///
/// Each probe is a `GET {base}/models` with a 5s timeout; the goal is to
/// pre-resolve DNS and pre-establish the TLS/TCP connection so the first
/// real conversation has zero cold start. Failures are recorded in the
/// returned [`WarmResult`] and logged — they never propagate.
pub async fn warm(base_urls: Vec<String>) -> Vec<WarmResult> {
    let probes = base_urls
        .into_iter()
        .map(|url| url.trim().trim_end_matches('/').to_string())
        .filter(|url| !url.is_empty())
        .map(|url| tokio::spawn(async move { probe(&url).await }));

    let results: Vec<WarmResult> = join_all(probes)
        .await
        .into_iter()
        .filter_map(|joined| match joined {
            Ok(result) => Some(result),
            Err(e) => {
                tracing::warn!(error = %e, "warm probe task panicked");
                None
            }
        })
        .collect();

    for r in &results {
        if r.ok {
            tracing::info!(base_url = %r.base_url, latency_ms = r.latency_ms, "warm probe ok");
        } else {
            tracing::warn!(base_url = %r.base_url, detail = %r.detail, "warm probe failed");
        }
    }
    results
}

async fn probe(base_url: &str) -> WarmResult {
    let url = format!("{base_url}/models");
    let started = Instant::now();
    let outcome = shared_client().get(&url).timeout(WARM_TIMEOUT).send().await;
    let latency_ms = started.elapsed().as_millis() as u64;
    match outcome {
        Ok(resp) => {
            let status = resp.status();
            WarmResult {
                base_url: base_url.to_string(),
                ok: status.is_success(),
                detail: status.to_string(),
                latency_ms,
            }
        }
        Err(e) => WarmResult {
            base_url: base_url.to_string(),
            ok: false,
            detail: e.to_string(),
            latency_ms,
        },
    }
}

/// Reads enabled provider base URLs from SQLite (read-only) and warms them.
///
/// Opens the database on the blocking pool, collects the deduplicated
/// `base_url` list of providers whose `settings.enabled` flag is on, then
/// delegates to [`warm`]. No rows are written; the caller (facade) decides
/// when to invoke this — it is not called on any internal startup path.
/// Database failures degrade to an empty result with a warning.
pub async fn warm_from_store(db_path: Arc<str>) -> Vec<WarmResult> {
    let urls = match load_enabled_base_urls(db_path).await {
        Ok(urls) => urls,
        Err(e) => {
            tracing::warn!(error = %e, "warm_from_store: failed to read provider base urls");
            return Vec::new();
        }
    };
    warm(urls).await
}

async fn load_enabled_base_urls(
    db_path: Arc<str>,
) -> Result<Vec<String>, crate::store::StoreError> {
    tokio::task::spawn_blocking(move || -> Result<Vec<String>, crate::store::StoreError> {
        let conn = rusqlite::Connection::open_with_flags(
            &*db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let providers = crate::store::repos::providers::list_providers(&conn)?;
        let mut seen = std::collections::HashSet::new();
        let mut urls = Vec::new();
        for p in providers {
            if !crate::domain::entities::ProviderSettings::from_params(&p.params).enabled {
                continue;
            }
            let url = p.base_url.trim().trim_end_matches('/').to_string();
            if url.is_empty() || !seen.insert(url.clone()) {
                continue;
            }
            urls.push(url);
        }
        Ok(urls)
    })
    .await
    .map_err(|e| {
        crate::store::StoreError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Minimal keep-alive HTTP/1.1 server on an ephemeral port that counts
    /// accepted TCP connections. Used to prove that handles from
    /// [`shared_client`] share one connection pool.
    fn spawn_counting_server() -> (String, Arc<AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let conns = Arc::new(AtomicUsize::new(0));
        let server_conns = conns.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                server_conns.fetch_add(1, Ordering::SeqCst);
                // Serve each connection on its own thread, honoring
                // keep-alive: answer every request until the client goes
                // away or the read fails.
                std::thread::spawn(move || {
                    let mut stream = stream;
                    loop {
                        let mut buf = Vec::new();
                        let mut chunk = [0u8; 512];
                        // Read until the end of request headers.
                        loop {
                            match stream.read(&mut chunk) {
                                Ok(0) | Err(_) => return,
                                Ok(n) => {
                                    buf.extend_from_slice(&chunk[..n]);
                                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                        break;
                                    }
                                }
                            }
                        }
                        if stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                            .is_err()
                        {
                            return;
                        }
                        let _ = stream.flush();
                    }
                });
            }
        });
        (format!("http://{addr}"), conns)
    }

    async fn get_and_drain(client: &reqwest::Client, url: &str) {
        let resp = client.get(url).send().await.unwrap();
        // Drain the body so the connection is returned to the pool.
        let _ = resp.bytes().await.unwrap();
    }

    #[tokio::test]
    async fn shared_client_handles_share_one_pool() {
        let (base, conns) = spawn_counting_server();
        let url = format!("{base}/models");

        let first = shared_client();
        get_and_drain(&first, &url).await;
        // A second handle (as any other component would obtain) must reuse
        // the same pooled connection rather than opening a new one.
        let second = shared_client();
        get_and_drain(&second, &url).await;

        assert_eq!(
            conns.load(Ordering::SeqCst),
            1,
            "both handles must share a single TCP connection"
        );
    }

    #[tokio::test]
    async fn warm_reports_unreachable_base_url_without_failing() {
        // Port 1 refuses connections immediately: fail-soft, no panic/Err.
        let results = warm(vec!["http://127.0.0.1:1".into()]).await;
        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert!(!results[0].detail.is_empty());
        assert_eq!(results[0].base_url, "http://127.0.0.1:1");
    }

    #[test]
    fn client_for_endpoint_falls_back_to_shared_and_rejects_bad_proxies() {
        // None/empty/blank → the shared direct client.
        assert!(super::client_for_endpoint(None).is_ok());
        assert!(super::client_for_endpoint(Some("")).is_ok());
        assert!(super::client_for_endpoint(Some("   ")).is_ok());
        // Unparseable proxy URL surfaces as an error, never a silent direct
        // fallback (that would leak traffic the user wanted proxied).
        assert!(super::client_for_endpoint(Some("::not-a-proxy")).is_err());
    }

    #[tokio::test]
    async fn warm_records_success_and_skips_empty_urls() {
        let (base, _conns) = spawn_counting_server();
        let mut results = warm(vec![base.clone(), String::new(), "   ".into()]).await;
        assert_eq!(results.len(), 1, "empty base urls are skipped");
        let r = results.swap_remove(0);
        assert!(r.ok);
        assert_eq!(r.base_url, base);
    }

    #[tokio::test]
    async fn warm_from_store_reads_enabled_provider_urls() {
        use crate::domain::{ProviderConfig, ProviderProtocol};
        use crate::store::{migrations, repos};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            migrations::run(&conn).unwrap();
            let base = ProviderConfig {
                id: "p1".into(),
                name: "enabled-a".into(),
                protocol: ProviderProtocol::OpenAiCompatible,
                base_url: "http://127.0.0.1:1".into(),
                keyring_ref: None,
                capabilities: vec![],
                is_master: true,
                fallback_order: None,
                params: serde_json::json!({ "settings": { "enabled": true } }),
                created_at: 1,
                updated_at: 1,
            };
            let dup = {
                let mut d = base.clone();
                d.id = "p2".into();
                d.name = "enabled-dup".into();
                d
            };
            let mut disabled = base.clone();
            disabled.id = "p3".into();
            disabled.name = "disabled".into();
            disabled.base_url = "http://127.0.0.1:2".into();
            disabled.params = serde_json::json!({ "settings": { "enabled": false } });
            repos::providers::insert_provider(&conn, &base).unwrap();
            repos::providers::insert_provider(&conn, &dup).unwrap();
            repos::providers::insert_provider(&conn, &disabled).unwrap();
        }

        let db_path: Arc<str> = Arc::from(path.to_string_lossy().into_owned().as_str());
        let results = warm_from_store(db_path).await;
        let urls: Vec<&str> = results.iter().map(|r| r.base_url.as_str()).collect();
        assert_eq!(
            urls,
            vec!["http://127.0.0.1:1"],
            "enabled urls only, deduplicated"
        );
    }
}
