//! Default executor: CubeSandbox-class runtime client (ADR-0022 task 7.4).
//!
//! Talks to an independent executor process/service (a CubeSandbox-class
//! runtime) over HTTP. The service owns the isolation boundary — container
//! or strongly isolated process, multi-language runtimes, resource caps
//! (CPU / memory / wall clock / network switch), timeout aborts and the
//! file-system boundary that rejects host paths outside the authorized
//! directories. This client stays crash-isolated from the host: every
//! call carries a request timeout and transport failures map to
//! [`SandboxError::Unavailable`] — a dead or hung service can never hang
//! or panic the application.
//!
//! ## Wire protocol (the executor-side contract)
//!
//! | Operation | Request | Success | Failure |
//! |---|---|---|---|
//! | submit | `POST {base}/v1/sandbox/tasks` `{task_id, task_type, runtime, source, authorized_dirs, limits}` | `2xx` | `{code, message}` |
//! | poll | `GET {base}/v1/sandbox/tasks/{id}` | `{status, stdout_total, stderr_total, stdout_truncated?, stderr_truncated?, usage?, exit_info?, duration_ms?, artifact_available?}` | `{code, message}` |
//! | cancel | `POST {base}/v1/sandbox/tasks/{id}/cancel` | `2xx` | `{code, message}` |
//! | fetch_output | `GET {base}/v1/sandbox/tasks/{id}/output?stdout_from&stderr_from` | `{stdout: {start, text, total, truncated?}, stderr: {...}}` | `{code, message}` |
//! | fetch_artifact | `GET {base}/v1/sandbox/tasks/{id}/artifact` | `{name, size_bytes?, local_path?, bytes_base64?}` | `{code, message}` |
//!
//! Error `code` values map onto the sandbox error taxonomy:
//! `unavailable` / `dir_not_authorized` / `runtime_unknown` /
//! `limit_invalid` / `task_not_found`; anything else is `Internal`.
//!
//! The declared runtime list is configuration (the plugin shell surfaces
//! it as `sandbox_list_runtimes`); the service validates the runtime id
//! at submit time. Real-runtime integration tests run only when
//! `NUOMI_SANDBOX_CUBE_URL` is set (CI has no sandbox environment).

use async_trait::async_trait;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::providers::pool::shared_client;
use crate::sandbox::executor::{
    SandboxArtifact, SandboxExecutor, SandboxOutputChunk, SandboxTaskSnapshot,
};
use crate::sandbox::types::{
    SandboxError, SandboxLimits, SandboxResourceUsage, SandboxRuntimeInfo, SandboxTaskSpec,
    SandboxTaskStatus,
};

/// Control-call budget (submit / poll / cancel / output fetch).
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Artifact download budget (payloads can be large).
const ARTIFACT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// HTTP client for a CubeSandbox-class runtime service.
pub struct CubeSandboxExecutor {
    /// Base URL of the service, e.g. `http://127.0.0.1:8819`.
    base_url: String,
    runtimes: Vec<SandboxRuntimeInfo>,
    http: reqwest::Client,
}

impl CubeSandboxExecutor {
    /// Client against `base_url` with the default runtime declarations
    /// (Python 3 / Node.js).
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            runtimes: default_runtimes(),
            http: shared_client(),
        }
    }

    /// Replaces the declared runtime list (plugin configuration).
    pub fn with_runtimes(mut self, runtimes: Vec<SandboxRuntimeInfo>) -> Self {
        self.runtimes = runtimes;
        self
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/v1/sandbox/{suffix}", self.base_url)
    }
}

fn default_runtimes() -> Vec<SandboxRuntimeInfo> {
    vec![
        SandboxRuntimeInfo {
            id: "python".to_string(),
            name: "Python 3".to_string(),
            isolation: "CubeSandbox container-isolated runtime".to_string(),
        },
        SandboxRuntimeInfo {
            id: "node".to_string(),
            name: "Node.js".to_string(),
            isolation: "CubeSandbox container-isolated runtime".to_string(),
        },
        SandboxRuntimeInfo {
            id: "rust".to_string(),
            name: "Rust".to_string(),
            isolation: "CubeSandbox container-isolated runtime".to_string(),
        },
    ]
}

// ---- wire DTOs -----------------------------------------------------------

#[derive(Serialize)]
struct SubmitBody<'a> {
    task_id: &'a str,
    task_type: &'a str,
    runtime: &'a str,
    source: &'a str,
    authorized_dirs: &'a [String],
    limits: &'a SandboxLimits,
}

#[derive(Deserialize)]
struct SnapshotBody {
    status: String,
    #[serde(default)]
    stdout_total: u64,
    #[serde(default)]
    stderr_total: u64,
    #[serde(default)]
    stdout_truncated: bool,
    #[serde(default)]
    stderr_truncated: bool,
    #[serde(default)]
    usage: Option<SandboxResourceUsage>,
    #[serde(default)]
    exit_info: Option<String>,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    artifact_available: bool,
}

#[derive(Serialize, Deserialize)]
struct ChunkBody {
    start: u64,
    text: String,
    total: u64,
    #[serde(default)]
    truncated: bool,
}

#[derive(Deserialize)]
struct OutputBody {
    stdout: ChunkBody,
    stderr: ChunkBody,
}

#[derive(Deserialize)]
struct ArtifactBody {
    name: String,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    local_path: Option<String>,
    #[serde(default)]
    bytes_base64: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    code: String,
    #[serde(default)]
    message: String,
}

// ---- error mapping -------------------------------------------------------

/// Transport failures mean the service is unreachable or hung — the host
/// stays unaffected (`Unavailable`), never a panic.
fn transport_err(base_url: &str, err: reqwest::Error) -> SandboxError {
    SandboxError::Unavailable(format!(
        "cannot reach the sandbox runtime service at {base_url}: {err}"
    ))
}

/// Maps a non-2xx response body `{"code","message"}` onto the taxonomy.
async fn map_error_response(
    base_url: &str,
    status: reqwest::StatusCode,
    body: bytes::Bytes,
) -> SandboxError {
    let parsed: Option<ErrorBody> = serde_json::from_slice(&body).ok();
    match parsed {
        Some(error) => match error.code.as_str() {
            "unavailable" => SandboxError::Unavailable(error.message),
            "dir_not_authorized" => SandboxError::DirNotAuthorized(error.message),
            "runtime_unknown" => SandboxError::RuntimeUnknown(error.message),
            "limit_invalid" => SandboxError::LimitInvalid(error.message),
            "task_not_found" => SandboxError::TaskNotFound(error.message),
            _ => {
                SandboxError::Internal(format!("sandbox service error {status}: {}", error.message))
            }
        },
        None => SandboxError::Unavailable(format!(
            "sandbox runtime service at {base_url} replied {status} with an unparseable body"
        )),
    }
}

async fn check_response(
    base_url: &str,
    resp: reqwest::Response,
) -> Result<reqwest::Response, SandboxError> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let body = resp.bytes().await.map_err(|e| transport_err(base_url, e))?;
    Err(map_error_response(base_url, status, body).await)
}

fn parse_status(raw: &str) -> Result<SandboxTaskStatus, SandboxError> {
    SandboxTaskStatus::parse(raw).ok_or_else(|| {
        SandboxError::Internal(format!(
            "sandbox service reported unknown task status: {raw}"
        ))
    })
}

fn chunk_from_body(body: ChunkBody) -> SandboxOutputChunk {
    SandboxOutputChunk {
        start: body.start,
        text: body.text,
        total: body.total,
        truncated: body.truncated,
    }
}

#[async_trait]
impl SandboxExecutor for CubeSandboxExecutor {
    fn id(&self) -> &str {
        "cube"
    }

    fn runtimes(&self) -> Vec<SandboxRuntimeInfo> {
        self.runtimes.clone()
    }

    async fn submit(&self, task_id: &str, spec: &SandboxTaskSpec) -> Result<(), SandboxError> {
        let body = SubmitBody {
            task_id,
            task_type: spec.task_type.as_str(),
            runtime: &spec.runtime,
            source: &spec.source,
            authorized_dirs: &spec.authorized_dirs,
            limits: &spec.limits,
        };
        let resp = self
            .http
            .post(self.url("tasks"))
            .json(&body)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|e| transport_err(&self.base_url, e))?;
        check_response(&self.base_url, resp).await.map(|_| ())
    }

    async fn poll(&self, task_id: &str) -> Result<SandboxTaskSnapshot, SandboxError> {
        let resp = self
            .http
            .get(self.url(&format!("tasks/{task_id}")))
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|e| transport_err(&self.base_url, e))?;
        let resp = check_response(&self.base_url, resp).await?;
        let body: SnapshotBody = resp
            .json()
            .await
            .map_err(|e| transport_err(&self.base_url, e))?;
        Ok(SandboxTaskSnapshot {
            status: parse_status(&body.status)?,
            stdout_total: body.stdout_total,
            stderr_total: body.stderr_total,
            stdout_truncated: body.stdout_truncated,
            stderr_truncated: body.stderr_truncated,
            usage: body.usage,
            exit_info: body.exit_info,
            duration_ms: body.duration_ms,
            artifact_available: body.artifact_available,
        })
    }

    async fn cancel(&self, task_id: &str) -> Result<(), SandboxError> {
        let resp = self
            .http
            .post(self.url(&format!("tasks/{task_id}/cancel")))
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|e| transport_err(&self.base_url, e))?;
        check_response(&self.base_url, resp).await.map(|_| ())
    }

    async fn fetch_output(
        &self,
        task_id: &str,
        stdout_from: u64,
        stderr_from: u64,
    ) -> Result<(SandboxOutputChunk, SandboxOutputChunk), SandboxError> {
        let resp = self
            .http
            .get(self.url(&format!("tasks/{task_id}/output")))
            .query(&[("stdout_from", stdout_from), ("stderr_from", stderr_from)])
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|e| transport_err(&self.base_url, e))?;
        let resp = check_response(&self.base_url, resp).await?;
        let body: OutputBody = resp
            .json()
            .await
            .map_err(|e| transport_err(&self.base_url, e))?;
        Ok((chunk_from_body(body.stdout), chunk_from_body(body.stderr)))
    }

    async fn fetch_artifact(&self, task_id: &str) -> Result<SandboxArtifact, SandboxError> {
        let resp = self
            .http
            .get(self.url(&format!("tasks/{task_id}/artifact")))
            .timeout(ARTIFACT_TIMEOUT)
            .send()
            .await
            .map_err(|e| transport_err(&self.base_url, e))?;
        let resp = check_response(&self.base_url, resp).await?;
        let body: ArtifactBody = resp
            .json()
            .await
            .map_err(|e| transport_err(&self.base_url, e))?;
        let bytes = body
            .bytes_base64
            .map(|encoded| {
                base64::engine::general_purpose::STANDARD
                    .decode(encoded.as_bytes())
                    .map_err(|e| {
                        SandboxError::Internal(format!("artifact base64 decode failed: {e}"))
                    })
            })
            .transpose()?;
        Ok(SandboxArtifact {
            name: body.name,
            size_bytes: body.size_bytes,
            local_path: body.local_path,
            bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockBuilder, MockServer, ResponseTemplate};

    type Captured = Arc<Mutex<Vec<(String, serde_json::Value)>>>;

    /// Mounts a catch-all capture mock for one matcher group.
    async fn capturing_server(
        builder: MockBuilder,
        status: u16,
        reply_body: serde_json::Value,
    ) -> (MockServer, Captured) {
        let server = MockServer::start().await;
        let captured: Captured = Arc::new(Mutex::new(Vec::new()));
        let sink = captured.clone();
        builder
            .respond_with(move |req: &wiremock::Request| {
                let body: serde_json::Value =
                    serde_json::from_slice(&req.body).unwrap_or(serde_json::Value::Null);
                let url = match req.url.query() {
                    Some(q) => format!("{}?{q}", req.url.path()),
                    None => req.url.path().to_string(),
                };
                sink.lock().unwrap().push((url, body));
                ResponseTemplate::new(status).set_body_json(reply_body.clone())
            })
            .mount(&server)
            .await;
        (server, captured)
    }

    fn spec() -> SandboxTaskSpec {
        SandboxTaskSpec {
            task_type: crate::sandbox::types::SandboxTaskType::RunCode,
            runtime: "python".to_string(),
            source: "print('hi')".to_string(),
            authorized_dirs: vec!["C:\\proj\\alpha".to_string()],
            app_temp_dirs: vec![],
            limits: SandboxLimits {
                timeout_secs: 90,
                cpu_percent: Some(50),
                memory_mb: Some(1024),
                network_enabled: false,
            },
        }
    }

    fn snapshot_json(status: &str) -> serde_json::Value {
        serde_json::json!({
            "status": status,
            "stdout_total": 6,
            "stderr_total": 2,
            "stdout_truncated": false,
            "stderr_truncated": true,
            "usage": { "cpu_time_ms": 120, "peak_memory_mb": 64 },
            "exit_info": null,
            "duration_ms": null,
            "artifact_available": true,
        })
    }

    fn executor(server: &MockServer) -> CubeSandboxExecutor {
        CubeSandboxExecutor::new(server.uri())
    }

    #[test]
    fn declares_default_runtimes_and_configurable_override() {
        let defaults = CubeSandboxExecutor::new("http://127.0.0.1:8819");
        assert_eq!(defaults.id(), "cube");
        assert!(defaults
            .runtimes()
            .iter()
            .any(|r| r.id == "python" && r.isolation.contains("CubeSandbox")));
        assert!(defaults
            .runtimes()
            .iter()
            .any(|r| r.id == "node" && r.isolation.contains("CubeSandbox")));
        assert!(defaults
            .runtimes()
            .iter()
            .any(|r| r.id == "rust" && r.name == "Rust" && r.isolation.contains("CubeSandbox")));

        let custom =
            CubeSandboxExecutor::new("http://x/").with_runtimes(vec![SandboxRuntimeInfo {
                id: "go".to_string(),
                name: "Go".to_string(),
                isolation: "custom".to_string(),
            }]);
        assert_eq!(custom.runtimes().len(), 1);
        assert_eq!(custom.runtimes()[0].id, "go");
        // 尾斜杠归一
        assert!(custom.url("tasks").starts_with("http://x/v1/sandbox/"));
    }

    #[tokio::test]
    async fn submit_posts_full_spec_with_limits_and_dirs() {
        let (server, captured) = capturing_server(
            Mock::given(method("POST")).and(path("/v1/sandbox/tasks")),
            202,
            serde_json::json!({}),
        )
        .await;
        let exec = executor(&server);

        exec.submit("t-1", &spec()).await.unwrap();

        let (url, body) = captured.lock().unwrap()[0].clone();
        assert!(url.starts_with("/v1/sandbox/tasks"));
        assert_eq!(body["task_id"], "t-1");
        assert_eq!(body["task_type"], "run_code");
        assert_eq!(body["runtime"], "python");
        assert_eq!(body["source"], "print('hi')");
        assert_eq!(
            body["authorized_dirs"],
            serde_json::json!(["C:\\proj\\alpha"])
        );
        assert_eq!(body["limits"]["timeout_secs"], 90);
        assert_eq!(body["limits"]["cpu_percent"], 50);
        assert_eq!(body["limits"]["memory_mb"], 1024);
        assert_eq!(body["limits"]["network_enabled"], false);
    }

    #[tokio::test]
    async fn poll_maps_snapshot_fields_onto_domain_snapshot() {
        let (server, _) = capturing_server(
            Mock::given(method("GET")).and(path("/v1/sandbox/tasks/t-1")),
            200,
            snapshot_json("running"),
        )
        .await;
        let exec = executor(&server);

        let snap = exec.poll("t-1").await.unwrap();
        assert_eq!(snap.status, SandboxTaskStatus::Running);
        assert_eq!(snap.stdout_total, 6);
        assert_eq!(snap.stderr_total, 2);
        assert!(!snap.stdout_truncated);
        assert!(snap.stderr_truncated);
        assert_eq!(
            snap.usage,
            Some(SandboxResourceUsage {
                cpu_time_ms: Some(120),
                peak_memory_mb: Some(64),
            })
        );
        assert!(snap.artifact_available);

        // 终态 + 缺省字段
        let (server2, _) = capturing_server(
            Mock::given(method("GET")).and(path("/v1/sandbox/tasks/t-1")),
            200,
            serde_json::json!({ "status": "timed_out", "exit_info": "sigkill", "duration_ms": 9000 }),
        )
        .await;
        let exec2 = executor(&server2);
        let snap2 = exec2.poll("t-1").await.unwrap();
        assert_eq!(snap2.status, SandboxTaskStatus::TimedOut);
        assert_eq!(snap2.exit_info.as_deref(), Some("sigkill"));
        assert_eq!(snap2.duration_ms, Some(9_000));
        assert_eq!(snap2.stdout_total, 0, "缺省字段回退默认值");
        assert!(snap2.usage.is_none());
    }

    #[tokio::test]
    async fn poll_rejects_unknown_status_strings() {
        let (server, _) = capturing_server(
            Mock::given(method("GET")).and(path("/v1/sandbox/tasks/t-1")),
            200,
            serde_json::json!({ "status": "hibernating" }),
        )
        .await;
        let exec = executor(&server);
        assert!(matches!(
            exec.poll("t-1").await,
            Err(SandboxError::Internal(_))
        ));
    }

    #[tokio::test]
    async fn cancel_posts_to_cancel_endpoint() {
        let (server, captured) = capturing_server(
            Mock::given(method("POST")).and(path("/v1/sandbox/tasks/t-9/cancel")),
            204,
            serde_json::json!({}),
        )
        .await;
        let exec = executor(&server);

        exec.cancel("t-9").await.unwrap();
        assert_eq!(captured.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn fetch_output_sends_cursors_and_maps_chunks() {
        let (server, captured) = capturing_server(
            Mock::given(method("GET"))
                .and(path("/v1/sandbox/tasks/t-1/output"))
                .and(query_param("stdout_from", "6"))
                .and(query_param("stderr_from", "2")),
            200,
            serde_json::json!({
                "stdout": { "start": 6, "text": "world", "total": 11, "truncated": false },
                "stderr": { "start": 2, "text": "!", "total": 3, "truncated": true },
            }),
        )
        .await;
        let exec = executor(&server);

        let (stdout, stderr) = exec.fetch_output("t-1", 6, 2).await.unwrap();
        assert_eq!(stdout.start, 6);
        assert_eq!(stdout.text, "world");
        assert_eq!(stdout.total, 11);
        assert!(!stdout.truncated);
        assert_eq!(stderr.text, "!");
        assert!(stderr.truncated);
        let url = captured.lock().unwrap()[0].0.clone();
        assert!(url.contains("stdout_from=6"), "游标必须随查询发送: {url}");
        assert!(url.contains("stderr_from=2"), "游标必须随查询发送: {url}");
    }

    #[tokio::test]
    async fn artifact_download_decodes_base64_and_keeps_local_path() {
        let (server, _) = capturing_server(
            Mock::given(method("GET")).and(path("/v1/sandbox/tasks/t-1/artifact")),
            200,
            serde_json::json!({
                "name": "app.exe",
                "size_bytes": 4,
                "local_path": "C:\\nuomi\\.nuomi\\sandbox\\t-1\\app.exe",
                "bytes_base64": base64::engine::general_purpose::STANDARD.encode(b"DATA"),
            }),
        )
        .await;
        let exec = executor(&server);

        let artifact = exec.fetch_artifact("t-1").await.unwrap();
        assert_eq!(artifact.name, "app.exe");
        assert_eq!(artifact.size_bytes, Some(4));
        assert_eq!(
            artifact.local_path.as_deref(),
            Some("C:\\nuomi\\.nuomi\\sandbox\\t-1\\app.exe")
        );
        assert_eq!(artifact.bytes.as_deref(), Some(b"DATA".as_slice()));

        // 纯元数据（无内联字节）也合法
        let (server2, _) = capturing_server(
            Mock::given(method("GET")).and(path("/v1/sandbox/tasks/t-1/artifact")),
            200,
            serde_json::json!({ "name": "out.zip" }),
        )
        .await;
        let exec2 = executor(&server2);
        let meta_only = exec2.fetch_artifact("t-1").await.unwrap();
        assert!(meta_only.bytes.is_none());
        assert_eq!(meta_only.name, "out.zip");
    }

    #[tokio::test]
    async fn service_error_codes_map_one_to_one() {
        for (code, status) in [
            ("dir_not_authorized", 403),
            ("runtime_unknown", 400),
            ("limit_invalid", 400),
            ("task_not_found", 404),
            ("unavailable", 503),
        ] {
            let (server, _) = capturing_server(
                Mock::given(method("GET")).and(path("/v1/sandbox/tasks/t-1")),
                status,
                serde_json::json!({ "code": code, "message": "boom" }),
            )
            .await;
            let exec = executor(&server);
            let err = exec.poll("t-1").await.unwrap_err();
            assert_eq!(err.code(), format!("sandbox.{code}"), "code {code}");
            assert_eq!(err.message(), "boom");
        }

        // 未知 code → Internal
        let (server, _) = capturing_server(
            Mock::given(method("GET")).and(path("/v1/sandbox/tasks/t-1")),
            500,
            serde_json::json!({ "code": "mystery", "message": "???" }),
        )
        .await;
        let exec = executor(&server);
        assert!(matches!(
            exec.poll("t-1").await,
            Err(SandboxError::Internal(_))
        ));
    }

    #[tokio::test]
    async fn unparseable_error_body_maps_to_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(502).set_body_string("bad gateway html"))
            .mount(&server)
            .await;
        let exec = executor(&server);
        assert!(matches!(
            exec.poll("t-1").await,
            Err(SandboxError::Unavailable(_))
        ));
    }

    /// 宿主崩溃隔离：服务不可达（连接拒绝）映射 Unavailable，绝不 panic
    /// 或挂起主应用。Port 1 immediately refuses connections.
    #[tokio::test]
    async fn dead_service_maps_to_unavailable_without_panicking() {
        let exec = CubeSandboxExecutor::new("http://127.0.0.1:1");
        let results: Vec<Result<(), SandboxError>> = vec![
            exec.submit("t-1", &spec()).await,
            exec.poll("t-1").await.map(|_| ()),
            exec.cancel("t-1").await,
            exec.fetch_output("t-1", 0, 0).await.map(|_| ()),
            exec.fetch_artifact("t-1").await.map(|_| ()),
        ];
        for result in results {
            match result {
                Err(SandboxError::Unavailable(msg)) => {
                    assert!(msg.contains("cannot reach"), "{msg}");
                }
                other => panic!("expected Unavailable, got {other:?}"),
            }
        }
    }

    // ---- real-runtime integration (explicit switch, skips in CI) ---------

    fn real_runtime_url() -> Option<String> {
        std::env::var("NUOMI_SANDBOX_CUBE_URL")
            .ok()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
    }

    #[tokio::test]
    async fn real_runtime_submit_poll_roundtrip() {
        let Some(url) = real_runtime_url() else {
            eprintln!("skipping: NUOMI_SANDBOX_CUBE_URL 未设置（CI 无沙箱环境）");
            return;
        };
        let exec = CubeSandboxExecutor::new(url);
        let task_id = format!("it-{}", crate::domain::now_ms());
        exec.submit(&task_id, &spec()).await.unwrap();
        let mut snapshot = exec.poll(&task_id).await.unwrap();
        for _ in 0..60 {
            if snapshot.status.is_terminal() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            snapshot = exec.poll(&task_id).await.unwrap();
        }
        assert!(
            snapshot.status.is_terminal(),
            "任务未在 30s 内到达终态: {:?}",
            snapshot.status
        );
        let (stdout, _) = exec.fetch_output(&task_id, 0, 0).await.unwrap();
        assert!(!stdout.text.is_empty(), "run_code 应有 stdout 输出");
    }
}
