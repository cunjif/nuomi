//! Data cleansing: redaction + aggregation + batching.
//! (K-Steward-5, T5-1 ~ T5-5)
//!
//! Reads raw events/memory → redacts sensitive fields → aggregates into
//! structured summaries → writes to `evolution_data_pools`.

use std::sync::Arc;

use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::domain::{now_ms, EvolutionDataPool};
use crate::store::repos::steward;
use crate::store::Db;

use super::StewardError;

/// Default batch size for cleansing.
const DEFAULT_BATCH_SIZE: usize = 5000;

/// Errors produced by the cleansing pipeline.
#[derive(Debug, thiserror::Error)]
pub enum CleanseError {
    #[error("store error: {0}")]
    Store(String),
    #[error("invalid scope: {0}")]
    InvalidScope(String),
    #[error("regex error: {0}")]
    Regex(String),
}

impl From<crate::store::StoreError> for CleanseError {
    fn from(e: crate::store::StoreError) -> Self {
        CleanseError::Store(e.to_string())
    }
}

impl From<rusqlite::Error> for CleanseError {
    fn from(e: rusqlite::Error) -> Self {
        CleanseError::Store(e.to_string())
    }
}

impl From<StewardError> for CleanseError {
    fn from(e: StewardError) -> Self {
        CleanseError::Store(e.to_string())
    }
}

/// The scope of data to cleanse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanseScope {
    pub time_range: (i64, i64),
    #[serde(default)]
    pub include_kinds: Vec<String>,
    #[serde(default)]
    pub include_memory_kinds: Vec<String>,
}

/// Rules governing redaction and aggregation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanseRules {
    pub redact_patterns: Vec<RedactPattern>,
    pub aggregation: AggregationConfig,
}

/// A regex-based redaction pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactPattern {
    pub pattern: String,
    pub replacement: String,
}

/// Aggregation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationConfig {
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_true")]
    pub by_session: bool,
    #[serde(default = "default_true")]
    pub by_behavior: bool,
    #[serde(default = "default_true")]
    pub by_time_window: bool,
}

fn default_batch_size() -> usize {
    DEFAULT_BATCH_SIZE
}

fn default_true() -> bool {
    true
}

impl Default for CleanseRules {
    fn default() -> Self {
        CleanseRules {
            redact_patterns: vec![RedactPattern {
                pattern: r"(?i)(api[_-]?key|secret|token|password)".into(),
                replacement: "[REDACTED]".into(),
            }],
            aggregation: AggregationConfig::default(),
        }
    }
}

impl Default for AggregationConfig {
    fn default() -> Self {
        AggregationConfig {
            batch_size: DEFAULT_BATCH_SIZE,
            by_session: true,
            by_behavior: true,
            by_time_window: true,
        }
    }
}

/// The result of a cleanse operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanseReport {
    pub pool_id: String,
    pub input_count: usize,
    pub output_count: usize,
    pub duration_ms: i64,
    pub uncovered_kinds: Vec<String>,
}

/// The trait for data cleansing implementations.
#[async_trait::async_trait]
pub trait DataCleanser: Send + Sync {
    async fn cleanse(
        &self,
        db_path: Arc<str>,
        scope: &CleanseScope,
        rules: &CleanseRules,
    ) -> Result<CleanseReport, CleanseError>;
}

// ============================================================ redaction engine

/// Redacts sensitive fields in a JSON value based on patterns.
pub fn redact_value(
    value: &serde_json::Value,
    patterns: &[Regex],
    _uncovered_kinds: &mut Vec<String>,
) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (k, v) in map {
                let redacted = redact_value(v, patterns, _uncovered_kinds);
                if is_sensitive_key(k) || patterns.iter().any(|p| p.is_match(k)) {
                    result.insert(k.clone(), serde_json::Value::String("[REDACTED]".into()));
                } else {
                    result.insert(k.clone(), redacted);
                }
            }
            serde_json::Value::Object(result)
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(
            arr.iter()
                .map(|v| redact_value(v, patterns, _uncovered_kinds))
                .collect(),
        ),
        serde_json::Value::String(s) => {
            let mut result = s.clone();
            for p in patterns {
                result = p.replace_all(&result, "[REDACTED]").to_string();
            }
            serde_json::Value::String(result)
        }
        other => other.clone(),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    lower.contains("key")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("token")
        || lower.contains("credential")
        || lower.contains("api_key")
}

/// Compiles redact patterns into regexes.
pub fn compile_patterns(rules: &CleanseRules) -> Result<Vec<Regex>, CleanseError> {
    rules
        .redact_patterns
        .iter()
        .map(|p| Regex::new(&p.pattern).map_err(|e| CleanseError::Regex(e.to_string())))
        .collect()
}

// ============================================================ aggregation

/// Aggregates redacted events into a structured summary.
pub fn aggregate(events: &[serde_json::Value], config: &AggregationConfig) -> serde_json::Value {
    let mut summary = serde_json::Map::new();

    if config.by_session {
        let mut session_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for e in events {
            if let Some(sid) = e.get("aggregate_id").and_then(|v| v.as_str()) {
                *session_counts.entry(sid.to_string()).or_insert(0) += 1;
            }
        }
        summary.insert(
            "session_distribution".into(),
            serde_json::to_value(&session_counts).unwrap_or_default(),
        );
    }

    if config.by_behavior {
        let mut kind_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for e in events {
            if let Some(kind) = e.get("kind").and_then(|v| v.as_str()) {
                *kind_counts.entry(kind.to_string()).or_insert(0) += 1;
            }
        }
        summary.insert(
            "behavior_patterns".into(),
            serde_json::to_value(&kind_counts).unwrap_or_default(),
        );
    }

    if config.by_time_window {
        summary.insert(
            "total_events".into(),
            serde_json::Value::Number(events.len().into()),
        );
        if !events.is_empty() {
            let times: Vec<i64> = events
                .iter()
                .filter_map(|e| e.get("created_at").and_then(|v| v.as_i64()))
                .collect();
            if let (Some(&min), Some(&max)) = (times.iter().min(), times.iter().max()) {
                summary.insert("time_range".into(), serde_json::json!([min, max]));
            }
        }
    }

    serde_json::Value::Object(summary)
}

// ============================================================ batching

/// Splits a vector into batches of `batch_size`.
pub fn batch<T: Clone>(items: &[T], batch_size: usize) -> Vec<Vec<T>> {
    if batch_size == 0 {
        return vec![items.to_vec()];
    }
    items
        .chunks(batch_size)
        .map(|chunk| chunk.to_vec())
        .collect()
}

// ============================================================ DefaultCleanser

/// Default cleansing implementation: read → redact → aggregate → write.
pub struct DefaultCleanser;

#[async_trait::async_trait]
impl DataCleanser for DefaultCleanser {
    async fn cleanse(
        &self,
        db_path: Arc<str>,
        scope: &CleanseScope,
        rules: &CleanseRules,
    ) -> Result<CleanseReport, CleanseError> {
        let start = std::time::Instant::now();
        let scope = scope.clone();
        let rules = rules.clone();
        let patterns = compile_patterns(&rules)?;

        spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            let conn = &db.0;

            let mut events = Vec::new();
            let mut stmt = conn.prepare(
                "SELECT id, aggregate_type, aggregate_id, kind, payload, created_at
                 FROM events
                 WHERE created_at >= ?1 AND created_at < ?2
                 ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map(
                rusqlite::params![scope.time_range.0, scope.time_range.1],
                |row| {
                    let payload: String = row.get(4)?;
                    let payload_val: serde_json::Value =
                        serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null);
                    Ok(serde_json::json!({
                        "id": row.get::<_, i64>(0)?,
                        "aggregate_type": row.get::<_, String>(1)?,
                        "aggregate_id": row.get::<_, String>(2)?,
                        "kind": row.get::<_, String>(3)?,
                        "payload": payload_val,
                        "created_at": row.get::<_, i64>(5)?,
                    }))
                },
            )?;
            for row in rows {
                events.push(row?);
            }

            let input_count = events.len();
            let mut uncovered_kinds = Vec::new();

            let batches = batch(&events, rules.aggregation.batch_size);
            let mut aggregated = serde_json::Map::new();
            let mut output_count = 0;

            for (i, batch_events) in batches.iter().enumerate() {
                let redacted: Vec<serde_json::Value> = batch_events
                    .iter()
                    .map(|e| redact_value(e, &patterns, &mut uncovered_kinds))
                    .collect();
                let batch_summary = aggregate(&redacted, &rules.aggregation);
                aggregated.insert(format!("batch_{i}"), batch_summary);
                output_count += redacted.len();
            }

            let pool_id = crate::domain::new_id();
            let pool = EvolutionDataPool {
                id: pool_id.clone(),
                scope: serde_json::to_value(&scope).unwrap_or_default(),
                rules_id: "default".into(),
                product: serde_json::Value::Object(aggregated),
                created_at: now_ms(),
            };
            steward::insert_data_pool(conn, &pool)?;

            let duration_ms = start.elapsed().as_millis() as i64;
            Ok(CleanseReport {
                pool_id,
                input_count,
                output_count,
                duration_ms,
                uncovered_kinds,
            })
        })
        .await
        .map_err(|e| CleanseError::Store(format!("join error: {e}")))?
    }
}

/// T8-7: wraps `DataCleanser::cleanse` with `steward.cleanse_completed` event.
pub async fn cleanse_with_event(
    cleanser: &dyn DataCleanser,
    db_path: Arc<str>,
    scope: &CleanseScope,
    rules: &CleanseRules,
) -> Result<CleanseReport, CleanseError> {
    let report = cleanser.cleanse(db_path.clone(), scope, rules).await?;
    super::events::publish_event(
        db_path,
        &report.pool_id,
        super::events::StewardEventKind::CleanseCompleted,
        &serde_json::json!({
            "pool_id": report.pool_id,
            "input_count": report.input_count,
            "output_count": report.output_count,
        }),
    )
    .await;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn db_path() -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_string_lossy().to_string();
        let db = Db::open(&path_str).unwrap();
        migrations::run(&db.0).unwrap();
        drop(db);
        std::mem::forget(dir);
        path_str
    }

    #[test]
    fn redact_replaces_sensitive_keys() {
        let value = serde_json::json!({
            "api_key": "sk-123456",
            "name": "test",
            "nested": {"secret": "hidden", "ok": "visible"}
        });
        let patterns = vec![];
        let mut uncovered = Vec::new();
        let result = redact_value(&value, &patterns, &mut uncovered);
        assert_eq!(result["api_key"], "[REDACTED]");
        assert_eq!(result["name"], "test");
        assert_eq!(result["nested"]["secret"], "[REDACTED]");
        assert_eq!(result["nested"]["ok"], "visible");
    }

    #[test]
    fn redact_applies_regex_patterns() {
        let value = serde_json::json!({"text": "my key is sk-abc123"});
        let patterns = vec![Regex::new(r"sk-\w+").unwrap()];
        let mut uncovered = Vec::new();
        let result = redact_value(&value, &patterns, &mut uncovered);
        assert_eq!(result["text"], "my key is [REDACTED]");
    }

    #[test]
    fn batch_splits_correctly() {
        let items: Vec<i32> = (0..10).collect();
        let batches = batch(&items, 3);
        assert_eq!(batches.len(), 4);
        assert_eq!(batches[0].len(), 3);
        assert_eq!(batches[3].len(), 1);
    }

    #[test]
    fn aggregate_produces_summary() {
        let events = vec![
            serde_json::json!({"aggregate_id": "s1", "kind": "message", "created_at": 100}),
            serde_json::json!({"aggregate_id": "s1", "kind": "tool_call", "created_at": 200}),
            serde_json::json!({"aggregate_id": "s2", "kind": "message", "created_at": 300}),
        ];
        let config = AggregationConfig {
            batch_size: 5000,
            by_session: true,
            by_behavior: true,
            by_time_window: true,
        };
        let summary = aggregate(&events, &config);
        assert!(summary.get("session_distribution").is_some());
        assert!(summary.get("behavior_patterns").is_some());
        assert_eq!(summary["total_events"], 3);
    }

    #[tokio::test]
    async fn default_cleanser_writes_data_pool() {
        let path = db_path();
        let dbp: Arc<str> = Arc::from(path);
        let scope = CleanseScope {
            time_range: (0, i64::MAX),
            include_kinds: vec![],
            include_memory_kinds: vec![],
        };
        let rules = CleanseRules {
            redact_patterns: vec![RedactPattern {
                pattern: r"sk-\w+".into(),
                replacement: "[REDACTED]".into(),
            }],
            aggregation: AggregationConfig {
                batch_size: 5000,
                by_session: true,
                by_behavior: true,
                by_time_window: true,
            },
        };
        let cleanser = DefaultCleanser;
        let report = cleanser.cleanse(dbp, &scope, &rules).await.unwrap();
        assert!(!report.pool_id.is_empty());
        assert_eq!(report.input_count, 0);
    }

    #[tokio::test]
    #[ignore = "performance test: 10000 events ≤ 60s (run with --ignored)"]
    async fn perf_cleanse_10000_events_under_60s() {
        let path = db_path();
        let dbp: Arc<str> = Arc::from(path.clone());

        // Seed 10000 events into the events table
        {
            let db = Db::open(&dbp).unwrap();
            let conn = &db.0;
            for i in 0..10000i64 {
                let payload = serde_json::json!({
                    "api_key": "sk-secret123",
                    "message": format!("event {i}"),
                    "session": format!("s{}", i % 100),
                });
                let payload_json = serde_json::to_string(&payload).unwrap();
                conn.execute(
                    "INSERT INTO events (aggregate_type, aggregate_id, kind, payload, seq, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        "chat",
                        format!("sess_{}", i % 100),
                        "message",
                        payload_json,
                        i,
                        i,
                    ],
                )
                .unwrap();
            }
        }

        let scope = CleanseScope {
            time_range: (0, i64::MAX),
            include_kinds: vec![],
            include_memory_kinds: vec![],
        };
        let rules = CleanseRules {
            redact_patterns: vec![RedactPattern {
                pattern: r"sk-\w+".into(),
                replacement: "[REDACTED]".into(),
            }],
            aggregation: AggregationConfig {
                batch_size: 5000,
                by_session: true,
                by_behavior: true,
                by_time_window: true,
            },
        };

        let start = std::time::Instant::now();
        let cleanser = DefaultCleanser;
        let report = cleanser.cleanse(dbp, &scope, &rules).await.unwrap();
        let elapsed = start.elapsed();

        assert_eq!(report.input_count, 10000);
        assert_eq!(report.output_count, 10000);
        assert!(
            elapsed.as_secs() <= 60,
            "cleansing 10000 events took {:?}, expected ≤ 60s",
            elapsed
        );
    }
}
