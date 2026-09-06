//! The exchange journal: hot index + group-commit writer + spill blobs.
//!
//! Tier model (ADR 0007):
//! - **Hot**: every record lives in a per-run in-memory map. Reads of fresh
//!   records are pure memory operations (zero disk IO).
//! - **Durable**: a background writer group-commits queued records into
//!   per-run JSONL segment files — one `write_all` + `flush` per batch,
//!   never a per-record fsync. A process crash loses at most one flush
//!   window of "executed but not yet journaled" records; `recover` replays
//!   segment files to rebuild the hot index (last line per seq wins).
//! - **Spill**: payloads larger than `spill_threshold_bytes` are stored in
//!   content-addressed blob files (`blobs/<sha256>`); the index keeps only
//!   an envelope. Identical payloads deduplicate to a single file.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{oneshot, Notify, RwLock};
use tokio::task::JoinHandle;

use super::gate::{DefaultGate, Gate, GateVerdict};
use super::ulid::new_ulid;
use super::{
    CallHandle, CallRequest, Coordinate, Envelope, ExchangeConfig, ExchangeError, GateOutcome,
    ListFilter, Record, RecordStatus, ResultRecord, RetainPolicy,
};

/// Compaction is offered once a run accumulates at least this many segment
/// files (write-amplification compensation: the rewrite cost is amortized
/// over a majority of full segments).
const COMPACT_SEGMENT_THRESHOLD: usize = 4;

/// Per-run hot state. `seq` and `records` are mutated only while holding
/// `io_lock`, so submit/complete sequences are serialized per run.
struct RunState {
    /// Monotonic per-run sequence, allocated under `io_lock`.
    seq: AtomicU64,
    records: RwLock<HashMap<u64, Arc<Record>>>,
    io_lock: tokio::sync::Mutex<()>,
}

/// A serialized record line waiting to be group-committed.
struct QueuedLine {
    run_key: String,
    payload: String,
}

/// Writer-side per-run append state. `file` is dropped (and re-opened) on
/// segment rotation so each segment file holds at most `flush_batch_min`
/// record lines.
struct WriterRun {
    file: Option<std::fs::File>,
    seg_index: u64,
    count: usize,
}

struct Shared {
    root: PathBuf,
    config: ExchangeConfig,
    gate: RwLock<Box<dyn Gate>>,
    runs: RwLock<HashMap<String, Arc<RunState>>>,
    queue: Mutex<VecDeque<QueuedLine>>,
    files: Mutex<HashMap<String, WriterRun>>,
    ack: Mutex<Option<oneshot::Sender<()>>>,
    notify: Notify,
    closed: AtomicBool,
}

/// The exchange journal. Clone-free sharing: wrap in `Arc` and hand copies
/// of the `Arc` to consumers (tool registry, reader tools).
pub struct ExchangeJournal {
    shared: Arc<Shared>,
    writer: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for ExchangeJournal {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Relaxed);
        // Wake the writer so it observes `closed` and drains one last time.
        self.shared.notify.notify_one();
        if let Some(handle) = self
            .writer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            handle.abort();
        }
    }
}

fn run_key(run_id: &str) -> String {
    let sanitized: String = run_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "_".to_string()
    } else {
        sanitized
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// First `max` bytes of `text`, cut at a char boundary.
fn preview_of(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Applies a JSON merge patch (RFC 7386).
fn merge_patch(base: Value, patch: &Value) -> Value {
    match patch {
        Value::Object(patch_map) => {
            let mut base = if base.is_object() {
                base
            } else {
                Value::Object(serde_json::Map::new())
            };
            if let Some(base_map) = base.as_object_mut() {
                for (key, value) in patch_map {
                    if value.is_null() {
                        base_map.remove(key);
                    } else {
                        let current = base_map.entry(key.clone()).or_insert(Value::Null);
                        *current = merge_patch(std::mem::take(current), value);
                    }
                }
            }
            base
        }
        other => other.clone(),
    }
}

impl ExchangeJournal {
    /// Creates a journal rooted at `root` (conventionally
    /// `<app_data>/exchange`) with `segments/` and `blobs/` subdirectories,
    /// and spawns the background group-commit writer.
    pub fn new(root: PathBuf, config: ExchangeConfig) -> Self {
        let _ = std::fs::create_dir_all(root.join("segments"));
        let _ = std::fs::create_dir_all(root.join("blobs"));
        let shared = Arc::new(Shared {
            root,
            config,
            gate: RwLock::new(Box::new(DefaultGate::new())),
            runs: RwLock::new(HashMap::new()),
            queue: Mutex::new(VecDeque::new()),
            files: Mutex::new(HashMap::new()),
            ack: Mutex::new(None),
            notify: Notify::new(),
            closed: AtomicBool::new(false),
        });
        let handle = tokio::spawn(writer_loop(Arc::clone(&shared)));
        Self {
            shared,
            writer: Mutex::new(Some(handle)),
        }
    }

    /// Replaces the gate (used to plug policy/custom gates in tests and
    /// integrations). Intended before any submit.
    pub async fn with_gate(self, gate: Box<dyn Gate>) -> Self {
        *self.shared.gate.write().await = gate;
        self
    }

    pub fn config(&self) -> &ExchangeConfig {
        &self.shared.config
    }

    async fn run_state(&self, run_id: &str) -> Option<Arc<RunState>> {
        let key = run_key(run_id);
        self.shared.runs.read().await.get(&key).cloned()
    }

    async fn get_or_create_run(&self, run_id: &str) -> Arc<RunState> {
        let key = run_key(run_id);
        if let Some(state) = self.shared.runs.read().await.get(&key) {
            return Arc::clone(state);
        }
        let mut runs = self.shared.runs.write().await;
        runs.entry(key)
            .or_insert_with(|| {
                Arc::new(RunState {
                    seq: AtomicU64::new(0),
                    records: RwLock::new(HashMap::new()),
                    io_lock: tokio::sync::Mutex::new(()),
                })
            })
            .clone()
    }

    async fn enqueue(&self, record: &Record) {
        if self.shared.closed.load(Ordering::Relaxed) {
            return;
        }
        let payload = match serde_json::to_string(record) {
            Ok(text) => text,
            // Records are plain serde types; serialization cannot fail in
            // practice. A placeholder line keeps the journal shape intact.
            Err(error) => {
                tracing::error!("exchange: record serialization failed: {error}");
                return;
            }
        };
        {
            let mut queue = self
                .shared
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            queue.push_back(QueuedLine {
                run_key: run_key(&record.coordinate.run_id),
                payload,
            });
        }
        self.shared.notify.notify_one();
    }

    /// Submits a call through the input gate. The tool must only execute
    /// when the outcome is `Allowed`/`Fixed` (with the patched params).
    pub async fn submit(&self, run_id: &str, handle: CallHandle, params: Value) -> GateOutcome {
        let state = self.get_or_create_run(run_id).await;
        let _io = state.io_lock.lock().await;
        let seq = state.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let coordinate = Coordinate {
            run_id: run_id.to_string(),
            seq,
            ulid: new_ulid(),
        };
        let request = CallRequest {
            coordinate: coordinate.clone(),
            handle: handle.clone(),
            params: params.clone(),
        };
        let verdict = self.shared.gate.read().await.inspect_input(&request).await;
        let (mut record, outcome) = match verdict {
            GateVerdict::Allow => (
                Record {
                    coordinate: coordinate.clone(),
                    handle,
                    params,
                    patch: None,
                    output_patch: None,
                    status: RecordStatus::Pending,
                    output: None,
                    output_spilled: false,
                    output_len: None,
                    output_hash: None,
                    preview: None,
                    reason: None,
                    created_at_ms: now_ms(),
                    completed_at_ms: None,
                },
                GateOutcome::Allowed(coordinate.clone()),
            ),
            GateVerdict::Fix(patch) => {
                let patched = merge_patch(params.clone(), &patch);
                (
                    Record {
                        coordinate: coordinate.clone(),
                        handle,
                        params: patched,
                        patch: Some(patch.clone()),
                        output_patch: None,
                        status: RecordStatus::Pending,
                        output: None,
                        output_spilled: false,
                        output_len: None,
                        output_hash: None,
                        preview: None,
                        reason: None,
                        created_at_ms: now_ms(),
                        completed_at_ms: None,
                    },
                    GateOutcome::Fixed {
                        coordinate: coordinate.clone(),
                        patch,
                    },
                )
            }
            GateVerdict::Reject(reason) => (
                Record {
                    coordinate: coordinate.clone(),
                    handle,
                    params,
                    patch: None,
                    output_patch: None,
                    status: RecordStatus::Rejected,
                    output: None,
                    output_spilled: false,
                    output_len: None,
                    output_hash: None,
                    preview: None,
                    reason: Some(reason.clone()),
                    created_at_ms: now_ms(),
                    completed_at_ms: None,
                },
                GateOutcome::Rejected {
                    coordinate: coordinate.clone(),
                    reason,
                },
            ),
            GateVerdict::Intercept(policy) => (
                Record {
                    coordinate: coordinate.clone(),
                    handle,
                    params,
                    patch: None,
                    output_patch: None,
                    status: RecordStatus::Intercepted,
                    output: None,
                    output_spilled: false,
                    output_len: None,
                    output_hash: None,
                    preview: None,
                    reason: Some(policy.clone()),
                    created_at_ms: now_ms(),
                    completed_at_ms: None,
                },
                GateOutcome::Intercepted {
                    coordinate: coordinate.clone(),
                    policy,
                },
            ),
        };
        record.created_at_ms = now_ms();
        let record = Arc::new(record);
        state
            .records
            .write()
            .await
            .insert(coordinate.seq, Arc::clone(&record));
        self.evict_overflow(&state).await;
        self.enqueue(&record).await;
        outcome
    }

    /// Completes a pending record with its output. The output passes the
    /// same gate hooks (validate / review / fix / reject) before being
    /// journaled; oversized payloads spill to a hash-addressed blob and the
    /// record keeps only an envelope. Returns the updated record.
    pub async fn complete(
        &self,
        coordinate: &Coordinate,
        output: Value,
    ) -> Result<Record, ExchangeError> {
        let state = self
            .run_state(&coordinate.run_id)
            .await
            .ok_or_else(|| ExchangeError::CoordinateNotFound(coordinate.to_string()))?;
        let _io = state.io_lock.lock().await;
        let existing = state
            .records
            .read()
            .await
            .get(&coordinate.seq)
            .cloned()
            .ok_or_else(|| ExchangeError::CoordinateNotFound(coordinate.to_string()))?;
        if existing.status != RecordStatus::Pending {
            return Err(ExchangeError::NotPending(coordinate.to_string()));
        }
        let result = ResultRecord {
            coordinate: coordinate.clone(),
            handle: existing.handle.clone(),
            output: output.clone(),
        };
        let verdict = self.shared.gate.read().await.inspect_output(&result).await;
        let mut updated = (*existing).clone();
        updated.completed_at_ms = Some(now_ms());
        match verdict {
            GateVerdict::Allow => {
                self.store_output(&mut updated, &output)?;
                updated.status = RecordStatus::Succeeded;
            }
            GateVerdict::Fix(patch) => {
                let patched = merge_patch(output, &patch);
                self.store_output(&mut updated, &patched)?;
                updated.output_patch = Some(patch);
                updated.status = RecordStatus::Succeeded;
            }
            GateVerdict::Reject(reason) => {
                updated.output = None;
                updated.output_spilled = false;
                updated.status = RecordStatus::Failed;
                updated.reason = Some(format!("output gate rejected: {reason}"));
            }
            GateVerdict::Intercept(policy) => {
                updated.output = None;
                updated.output_spilled = false;
                updated.status = RecordStatus::Failed;
                updated.reason = Some(format!("output intercepted by policy '{policy}'"));
            }
        }
        let updated = Arc::new(updated);
        state
            .records
            .write()
            .await
            .insert(coordinate.seq, Arc::clone(&updated));
        self.evict_overflow(&state).await;
        self.enqueue(&updated).await;
        Ok((*updated).clone())
    }

    /// Records a terminal failure for a pending coordinate (tool errored).
    pub async fn fail(&self, coordinate: &Coordinate, error: String) -> Result<(), ExchangeError> {
        let state = self
            .run_state(&coordinate.run_id)
            .await
            .ok_or_else(|| ExchangeError::CoordinateNotFound(coordinate.to_string()))?;
        let _io = state.io_lock.lock().await;
        let existing = state
            .records
            .read()
            .await
            .get(&coordinate.seq)
            .cloned()
            .ok_or_else(|| ExchangeError::CoordinateNotFound(coordinate.to_string()))?;
        if existing.status != RecordStatus::Pending {
            return Err(ExchangeError::NotPending(coordinate.to_string()));
        }
        let mut updated = (*existing).clone();
        updated.status = RecordStatus::Failed;
        updated.reason = Some(error);
        updated.completed_at_ms = Some(now_ms());
        let updated = Arc::new(updated);
        state
            .records
            .write()
            .await
            .insert(coordinate.seq, Arc::clone(&updated));
        self.enqueue(&updated).await;
        Ok(())
    }

    /// Stores `output` on the record, spilling to a content-addressed blob
    /// when the serialized payload exceeds the spill threshold. Identical
    /// payloads (same hash) reuse the existing blob file.
    fn store_output(&self, record: &mut Record, output: &Value) -> Result<(), ExchangeError> {
        let serialized = output.to_string();
        record.output_len = Some(serialized.len());
        record.preview = Some(preview_of(&serialized, self.shared.config.preview_bytes));
        if serialized.len() > self.shared.config.spill_threshold_bytes {
            let hash = sha256_hex(serialized.as_bytes());
            let blob_path = self.shared.root.join("blobs").join(&hash);
            if !blob_path.exists() {
                std::fs::write(&blob_path, serialized.as_bytes())?;
            } else {
                tracing::debug!("exchange: blob {hash} deduplicated (reused existing file)");
            }
            record.output_spilled = true;
            record.output = None;
            record.output_hash = Some(hash);
        } else {
            record.output_spilled = false;
            record.output = Some(output.clone());
            record.output_hash = None;
        }
        Ok(())
    }

    /// Demotes the oldest terminal inline payloads to envelope-only entries
    /// once the per-run hot payload cap is exceeded. Payloads remain on
    /// disk (segments/blobs); only the in-memory body is released.
    async fn evict_overflow(&self, state: &RunState) {
        let cap = self.shared.config.max_hot_payloads_per_run;
        let mut records = state.records.write().await;
        let mut holders: Vec<(u64, u64)> = records
            .iter()
            .filter(|(_, record)| record.output.is_some() && record.status != RecordStatus::Pending)
            .map(|(seq, record)| (*seq, record.completed_at_ms.unwrap_or(record.created_at_ms)))
            .collect();
        if holders.len() <= cap {
            return;
        }
        // Oldest first; seq breaks timestamp ties deterministically.
        holders.sort_by_key(|(seq, at)| (*at, *seq));
        let excess = holders.len() - cap;
        for (seq, _) in &holders[..excess] {
            if let Some(record) = records.get(seq) {
                let mut stripped = (**record).clone();
                stripped.output = None;
                records.insert(*seq, Arc::new(stripped));
            }
        }
    }

    /// Reads a record. Hot-tier hits are pure memory (zero disk IO);
    /// spilled records load their blob on demand. After a DropPayloads GC
    /// the blob is gone and the record is returned envelope-only.
    pub async fn read(&self, coordinate: &Coordinate) -> Result<Record, ExchangeError> {
        let state = self
            .run_state(&coordinate.run_id)
            .await
            .ok_or_else(|| ExchangeError::CoordinateNotFound(coordinate.to_string()))?;
        let existing = state
            .records
            .read()
            .await
            .get(&coordinate.seq)
            .cloned()
            .ok_or_else(|| ExchangeError::CoordinateNotFound(coordinate.to_string()))?;
        let mut record = (*existing).clone();
        if record.output_spilled && record.output.is_none() {
            if let Some(hash) = record.output_hash.clone() {
                let blob_path = self.shared.root.join("blobs").join(&hash);
                match std::fs::read_to_string(&blob_path) {
                    Ok(text) => match serde_json::from_str::<Value>(&text) {
                        Ok(value) => record.output = Some(value),
                        Err(error) => {
                            tracing::warn!("exchange: blob {hash} is not valid JSON: {error}");
                        }
                    },
                    Err(error) => {
                        tracing::warn!("exchange: blob {hash} unavailable: {error}");
                    }
                }
            }
        }
        Ok(record)
    }

    /// Lists envelopes for a run (sorted by seq), optionally filtered.
    pub async fn list(&self, run_id: &str, filter: &ListFilter) -> Vec<Envelope> {
        let Some(state) = self.run_state(run_id).await else {
            return Vec::new();
        };
        let records = state.records.read().await;
        let mut envelopes: Vec<Envelope> = records
            .values()
            .filter(|record| filter.matches(record))
            .map(|record| Envelope::from_record(record))
            .collect();
        envelopes.sort_by_key(|envelope| envelope.coordinate.seq);
        envelopes
    }

    /// Last `n` envelopes of a run (most recent calls), seq-ascending.
    pub async fn tail(&self, run_id: &str, n: usize) -> Vec<Envelope> {
        let mut envelopes = self.list(run_id, &ListFilter::default()).await;
        if envelopes.len() > n {
            envelopes.drain(..envelopes.len() - n);
        }
        envelopes
    }

    /// Forces a group commit of everything currently queued and returns
    /// once the bytes have been written + flushed to segment files.
    pub async fn flush_now(&self) -> Result<(), ExchangeError> {
        if self.shared.closed.load(Ordering::Relaxed) {
            return Err(ExchangeError::Closed);
        }
        let (tx, rx) = oneshot::channel();
        {
            let mut ack = self
                .shared
                .ack
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            // Replace any stale waiter; only the newest caller is acked,
            // which is sufficient — the queue is drained for everyone.
            let _ = ack.replace(tx);
        }
        self.shared.notify.notify_one();
        rx.await.map_err(|_| ExchangeError::Closed)
    }

    /// Rebuilds the hot index from segment files (startup path). The last
    /// line per seq wins; spilled records load their blobs lazily via
    /// `read`. Returns the number of unique records loaded.
    pub async fn recover(&self) -> Result<usize, ExchangeError> {
        let segments_root = self.shared.root.join("segments");
        let entries = match std::fs::read_dir(&segments_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(ExchangeError::Io(error)),
        };
        let mut loaded = 0usize;
        let mut rebuilt: Vec<(String, u64, HashMap<u64, Arc<Record>>)> = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let dir_key = entry.file_name().to_string_lossy().to_string();
            let mut latest: HashMap<u64, Record> = HashMap::new();
            for path in segment_files(&entry.path())? {
                let content = std::fs::read_to_string(&path)?;
                for line in content.lines() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Record>(line) {
                        Ok(record) => {
                            latest.insert(record.coordinate.seq, record);
                        }
                        Err(error) => {
                            tracing::warn!(
                                "exchange: recover skipped a corrupt line in {dir_key}: {error}"
                            );
                        }
                    }
                }
            }
            if latest.is_empty() {
                continue;
            }
            let seq = latest.keys().copied().max().unwrap_or(0);
            let records: HashMap<u64, Arc<Record>> = latest
                .into_iter()
                .map(|(key, record)| (key, Arc::new(record)))
                .collect();
            loaded += records.len();
            rebuilt.push((dir_key, seq, records));
        }
        let mut runs = self.shared.runs.write().await;
        for (key, seq, incoming) in rebuilt {
            // Merge into any existing hot state (startup path: normally
            // empty). Last writer wins per seq; seq continues from max.
            match runs.get_mut(&key) {
                Some(existing) => {
                    let existing_state = Arc::clone(existing);
                    let mut records = existing_state.records.write().await;
                    for (seq, record) in incoming {
                        records.insert(seq, record);
                    }
                    existing_state.seq.fetch_max(seq, Ordering::Relaxed);
                }
                None => {
                    runs.insert(
                        key,
                        Arc::new(RunState {
                            seq: AtomicU64::new(seq),
                            records: RwLock::new(incoming),
                            io_lock: tokio::sync::Mutex::new(()),
                        }),
                    );
                }
            }
        }
        Ok(loaded)
    }

    /// Number of segment files for a run.
    pub async fn segment_count(&self, run_id: &str) -> usize {
        let key = run_key(run_id);
        segment_files(&self.shared.root.join("segments").join(&key))
            .map(|files| files.len())
            .unwrap_or(0)
    }

    /// True when the run has accumulated enough segments for a rewrite to
    /// pay off (write-amplification compensation threshold).
    pub async fn needs_compact(&self, run_id: &str) -> bool {
        self.segment_count(run_id).await >= COMPACT_SEGMENT_THRESHOLD
    }

    /// Rewrites all segments of a run into a single snapshot file, keeping
    /// only the last line per coordinate (dropping superseded Pending
    /// lines superseded by later Fixed/Rejected/Completed states). Intended
    /// at run end or while the run is quiesced.
    pub async fn compact(&self, run_id: &str) -> Result<(), ExchangeError> {
        let key = run_key(run_id);
        if let Some(state) = self.run_state(run_id).await {
            let _io = state.io_lock.lock().await;
        }
        self.rewrite_segments(&key, false)
    }

    /// Applies a retention policy (ADR 0007 capacity governance).
    pub async fn gc(&self, policy: RetainPolicy) -> Result<(), ExchangeError> {
        match policy {
            RetainPolicy::KeepAll => Ok(()),
            RetainPolicy::DropPayloads => {
                // 1. Delete every blob file (content-addressed payloads).
                let blobs_dir = self.shared.root.join("blobs");
                if let Ok(entries) = std::fs::read_dir(&blobs_dir) {
                    for entry in entries.flatten() {
                        if entry.file_type()?.is_file() {
                            std::fs::remove_file(entry.path())?;
                        }
                    }
                }
                // 2. Drop inline payloads from the hot index.
                let run_keys: Vec<(String, Arc<RunState>)> = {
                    let runs = self.shared.runs.read().await;
                    runs.iter()
                        .map(|(k, v)| (k.clone(), Arc::clone(v)))
                        .collect()
                };
                for (_, state) in &run_keys {
                    let _io = state.io_lock.lock().await;
                    let mut records = state.records.write().await;
                    let seqs: Vec<u64> = records
                        .iter()
                        .filter(|(_, record)| record.output.is_some())
                        .map(|(seq, _)| *seq)
                        .collect();
                    for seq in seqs {
                        if let Some(record) = records.get(&seq) {
                            let mut stripped = (**record).clone();
                            stripped.output = None;
                            records.insert(seq, Arc::new(stripped));
                        }
                    }
                }
                // 3. Rewrite segments envelope-only (payloads never linger
                //     in snapshots after the GC pass).
                for (key, state) in &run_keys {
                    let _io = state.io_lock.lock().await;
                    self.rewrite_segments(key, true)?;
                }
                Ok(())
            }
            RetainPolicy::KeepLastNRuns(n) => {
                let mut ranked: Vec<(String, Arc<RunState>, u64)> = Vec::new();
                {
                    let runs = self.shared.runs.read().await;
                    for (key, state) in runs.iter() {
                        let latest = state
                            .records
                            .read()
                            .await
                            .values()
                            .map(|record| record.completed_at_ms.unwrap_or(record.created_at_ms))
                            .max()
                            .unwrap_or(0);
                        ranked.push((key.clone(), Arc::clone(state), latest));
                    }
                }
                ranked.sort_by_key(|(_, _, activity)| std::cmp::Reverse(*activity));
                for (key, state, _) in ranked.iter().skip(n) {
                    let _io = state.io_lock.lock().await;
                    let dir = self.shared.root.join("segments").join(key);
                    if let Err(error) = std::fs::remove_dir_all(&dir) {
                        if error.kind() != std::io::ErrorKind::NotFound {
                            return Err(ExchangeError::Io(error));
                        }
                    }
                    self.shared.runs.write().await.remove(key);
                }
                // Blobs are content-addressed across runs; reclaiming them
                // belongs to a DropPayloads pass.
                Ok(())
            }
        }
    }

    /// Rewrites all segment files of `run_key` into `000000.jsonl`, keeping
    /// the last line per seq; optionally strips payloads (GC envelope-only
    /// snapshot). Resets the writer to append to a fresh segment afterwards.
    fn rewrite_segments(&self, run_key: &str, strip_payloads: bool) -> Result<(), ExchangeError> {
        let dir = self.shared.root.join("segments").join(run_key);
        let files = segment_files(&dir)?;
        let mut latest: BTreeMap<u64, Record> = BTreeMap::new();
        for path in &files {
            let content = std::fs::read_to_string(path)?;
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Record>(line) {
                    Ok(record) => {
                        latest.insert(record.coordinate.seq, record);
                    }
                    Err(error) => {
                        tracing::warn!("exchange: compact skipped a corrupt line: {error}");
                    }
                }
            }
        }
        let mut buffer = String::new();
        for record in latest.values_mut() {
            if strip_payloads {
                record.output = None;
            }
        }
        for record in latest.values() {
            buffer.push_str(&serde_json::to_string(record)?);
            buffer.push('\n');
        }
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("000000.jsonl"), buffer.as_bytes())?;
        for path in &files {
            if path
                .file_name()
                .map(|name| name != "000000.jsonl")
                .unwrap_or(false)
            {
                std::fs::remove_file(path)?;
            }
        }
        // Future appends start a fresh segment after the snapshot so the
        // writer never re-opens the compacted file mid-stream.
        let mut files_map = self
            .shared
            .files
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = files_map.get_mut(run_key) {
            entry.file = None;
            entry.count = 0;
            entry.seg_index = entry.seg_index.max(1);
        }
        Ok(())
    }
}

fn segment_files(dir: &Path) -> Result<Vec<PathBuf>, ExchangeError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ExchangeError::Io(error)),
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().map(|ext| ext == "jsonl").unwrap_or(false))
        .collect();
    files.sort();
    Ok(files)
}

fn open_segment_file(
    root: &Path,
    run_key: &str,
    seg_index: u64,
) -> Result<std::fs::File, std::io::Error> {
    let dir = root.join("segments").join(run_key);
    std::fs::create_dir_all(&dir)?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{seg_index:06}.jsonl")))
}

/// Group-commit writer: wakes on notify (backlog >= batch min or an explicit
/// `flush_now`) and on the periodic tick, drains the whole queue, groups
/// lines per run, and appends each run's batch with a single
/// `write_all` + `flush` into its current segment file.
async fn writer_loop(shared: Arc<Shared>) {
    let period = Duration::from_millis(shared.config.flush_interval_ms.max(1));
    let mut ticker = tokio::time::interval(period);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let woken_by_notify;
        tokio::select! {
            _ = shared.notify.notified() => { woken_by_notify = true; }
            _ = ticker.tick() => { woken_by_notify = false; }
        }
        let should_flush = !woken_by_notify || {
            let ack_pending = shared
                .ack
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some();
            let backlog = shared
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .len();
            ack_pending || backlog >= shared.config.flush_batch_min
        };
        if should_flush {
            drain_queue(&shared);
        }
        let ack = shared
            .ack
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(sender) = ack {
            let _ = sender.send(());
        }
        if shared.closed.load(Ordering::Relaxed) {
            drain_queue(&shared);
            return;
        }
    }
}

fn drain_queue(shared: &Shared) {
    let lines: Vec<QueuedLine> = {
        let mut queue = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
        queue.drain(..).collect()
    };
    if lines.is_empty() {
        return;
    }
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in lines {
        grouped.entry(line.run_key).or_default().push(line.payload);
    }
    let mut files = shared.files.lock().unwrap_or_else(PoisonError::into_inner);
    let segment_limit = shared.config.flush_batch_min.max(1);
    for (run_key, payloads) in &grouped {
        let entry = files.entry(run_key.clone()).or_insert_with(|| WriterRun {
            file: None,
            seg_index: 0,
            count: 0,
        });
        for payload in payloads {
            if entry.file.is_none() {
                match open_segment_file(&shared.root, run_key, entry.seg_index) {
                    Ok(file) => entry.file = Some(file),
                    Err(error) => {
                        // Records stay in the hot index; only durability of
                        // this batch is lost. Log loudly and move on.
                        tracing::error!("exchange: cannot open segment for '{run_key}': {error}");
                        break;
                    }
                }
            }
            if let Some(file) = entry.file.as_mut() {
                let mut bytes = payload.as_bytes().to_vec();
                bytes.push(b'\n');
                if let Err(error) = file.write_all(&bytes).and_then(|()| file.flush()) {
                    tracing::error!("exchange: segment write failed for '{run_key}': {error}");
                    continue;
                }
                entry.count += 1;
                if entry.count >= segment_limit {
                    // Rotate: next append opens a new segment file.
                    entry.file = None;
                    entry.seg_index += 1;
                    entry.count = 0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::time::{Duration, Instant};

    use super::super::HandleKind;

    fn test_config() -> ExchangeConfig {
        ExchangeConfig {
            // No periodic flush inside tests: durability only happens on
            // flush_now or when the backlog reaches flush_batch_min —
            // deterministic for assertions.
            flush_interval_ms: 3_600_000,
            flush_batch_min: 32,
            ..Default::default()
        }
    }

    fn make_journal(root: PathBuf) -> Arc<ExchangeJournal> {
        Arc::new(ExchangeJournal::new(root, test_config()))
    }

    fn tool_handle(id: &str) -> CallHandle {
        CallHandle {
            kind: HandleKind::Tool,
            id: id.to_string(),
        }
    }

    fn coord_of(outcome: &GateOutcome) -> Coordinate {
        outcome.coordinate().clone()
    }

    fn snapshot(root: &Path) -> Vec<(String, u64, SystemTime)> {
        fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, u64, SystemTime)>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                if metadata.is_dir() {
                    walk(&entry.path(), &format!("{name}/"), out);
                } else {
                    let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
                    out.push((name, metadata.len(), modified));
                }
            }
        }
        let mut out = Vec::new();
        walk(root, "", &mut out);
        out.sort();
        out
    }

    struct FixedVerdictGate {
        input: GateVerdict,
        output: GateVerdict,
    }

    #[async_trait::async_trait]
    impl Gate for FixedVerdictGate {
        async fn inspect_input(&self, _request: &CallRequest) -> GateVerdict {
            self.input.clone()
        }
        async fn inspect_output(&self, _record: &ResultRecord) -> GateVerdict {
            self.output.clone()
        }
    }

    async fn journal_with(
        root: PathBuf,
        input: GateVerdict,
        output: GateVerdict,
    ) -> Arc<ExchangeJournal> {
        let journal = ExchangeJournal::new(root, test_config());
        let journal = journal
            .with_gate(Box::new(FixedVerdictGate { input, output }))
            .await;
        Arc::new(journal)
    }

    #[tokio::test]
    async fn gate_allow_fix_reject_intercept_four_paths() {
        // Allow
        let root = tempfile::tempdir().unwrap();
        let journal = journal_with(
            root.path().to_path_buf(),
            GateVerdict::Allow,
            GateVerdict::Allow,
        )
        .await;
        let outcome = journal
            .submit("r", tool_handle("t"), json!({ "a": 1 }))
            .await;
        let coordinate = coord_of(&outcome);
        assert!(matches!(outcome, GateOutcome::Allowed(_)));
        let record = journal.read(&coordinate).await.unwrap();
        assert_eq!(record.status, RecordStatus::Pending);
        assert!(record.patch.is_none());

        // Fix — params merged, patch recorded, still pending.
        let journal = journal_with(
            root.path().join("fix"),
            GateVerdict::Fix(json!({ "b": 2 })),
            GateVerdict::Allow,
        )
        .await;
        let outcome = journal
            .submit("r", tool_handle("t"), json!({ "a": 1 }))
            .await;
        let coordinate = match &outcome {
            GateOutcome::Fixed { patch, .. } => {
                assert_eq!(patch, &json!({ "b": 2 }));
                coord_of(&outcome)
            }
            other => panic!("expected Fixed, got {other:?}"),
        };
        let record = journal.read(&coordinate).await.unwrap();
        assert_eq!(record.params, json!({ "a": 1, "b": 2 }));
        assert!(record.patch.is_some());

        // Reject
        let journal = journal_with(
            root.path().join("reject"),
            GateVerdict::Reject("bad schema".to_string()),
            GateVerdict::Allow,
        )
        .await;
        let outcome = journal.submit("r", tool_handle("t"), json!({})).await;
        assert!(matches!(&outcome, GateOutcome::Rejected { reason, .. } if reason == "bad schema"));
        let record = journal.read(&coord_of(&outcome)).await.unwrap();
        assert_eq!(record.status, RecordStatus::Rejected);

        // Intercept
        let journal = journal_with(
            root.path().join("intercept"),
            GateVerdict::Intercept("policy-7".to_string()),
            GateVerdict::Allow,
        )
        .await;
        let outcome = journal.submit("r", tool_handle("t"), json!({})).await;
        assert!(
            matches!(&outcome, GateOutcome::Intercepted { policy, .. } if policy == "policy-7")
        );
        let record = journal.read(&coord_of(&outcome)).await.unwrap();
        assert_eq!(record.status, RecordStatus::Intercepted);
    }

    #[tokio::test]
    async fn coordinates_are_monotonic_per_run() {
        let root = tempfile::tempdir().unwrap();
        let journal = make_journal(root.path().to_path_buf());
        let mut seqs = Vec::new();
        for _ in 0..5 {
            let outcome = journal.submit("run", tool_handle("t"), json!({})).await;
            seqs.push(coord_of(&outcome).seq);
        }
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
        // Distinct runs count independently.
        let outcome = journal.submit("other", tool_handle("t"), json!({})).await;
        assert_eq!(coord_of(&outcome).seq, 1);
        // Every coordinate carries a non-empty ULID.
        assert!(!coord_of(&outcome).ulid.is_empty());
    }

    #[tokio::test]
    async fn hot_read_is_zero_disk_io_then_flush_creates_segments() {
        let root = tempfile::tempdir().unwrap();
        let journal = make_journal(root.path().to_path_buf());
        let outcome = journal
            .submit("run", tool_handle("echo"), json!({ "x": 1 }))
            .await;
        let coordinate = coord_of(&outcome);
        let record = journal.complete(&coordinate, json!("done")).await.unwrap();
        assert_eq!(record.status, RecordStatus::Succeeded);

        journal.flush_now().await.unwrap();
        let before = snapshot(root.path());
        assert!(!before.is_empty(), "flush must produce segment files");

        // Repeated hot reads / list / tail must not touch the disk.
        for _ in 0..3 {
            let fetched = journal.read(&coordinate).await.unwrap();
            assert_eq!(fetched.output, Some(json!("done")));
        }
        let _ = journal.list("run", &ListFilter::default()).await;
        let _ = journal.tail("run", 10).await;
        let after = snapshot(root.path());
        assert_eq!(before, after, "reads must not create or modify files");
    }

    #[tokio::test]
    async fn group_commit_writes_segments_and_recover_replays_them() {
        let root = tempfile::tempdir().unwrap();
        let journal = make_journal(root.path().to_path_buf());
        let mut coordinates = Vec::new();
        for index in 0..3 {
            let outcome = journal
                .submit("run-a", tool_handle("echo"), json!({ "i": index }))
                .await;
            let coordinate = coord_of(&outcome);
            journal
                .complete(&coordinate, json!({ "out": index }))
                .await
                .unwrap();
            coordinates.push(coordinate);
        }
        // Backlog (6 lines) is below flush_batch_min (32) and the interval
        // is 1h — nothing on disk yet.
        assert!(snapshot(root.path()).is_empty());
        journal.flush_now().await.unwrap();
        let segments = fs::read_dir(root.path().join("segments").join("run-a")).unwrap();
        let mut line_count = 0;
        for file in segments.flatten() {
            let content = fs::read_to_string(file.path()).unwrap();
            line_count += content.lines().count();
        }
        assert_eq!(line_count, 6, "submit + complete lines for 3 records");

        // A fresh journal over the same root replays the segments.
        drop(journal);
        let journal2 = make_journal(root.path().to_path_buf());
        let loaded = journal2.recover().await.unwrap();
        assert_eq!(loaded, 3);
        for (index, coordinate) in coordinates.iter().enumerate() {
            let record = journal2.read(coordinate).await.unwrap();
            assert_eq!(record.status, RecordStatus::Succeeded);
            assert_eq!(record.output, Some(json!({ "out": index })));
        }

        // Seq continues from the recovered max.
        let outcome = journal2
            .submit("run-a", tool_handle("echo"), json!({}))
            .await;
        assert_eq!(coord_of(&outcome).seq, 4);
    }

    #[tokio::test]
    async fn oversized_output_spills_to_blob_with_envelope() {
        let root = tempfile::tempdir().unwrap();
        let journal = make_journal(root.path().to_path_buf());
        let outcome = journal.submit("run", tool_handle("big"), json!({})).await;
        let coordinate = coord_of(&outcome);
        let payload = "x".repeat(20 * 1024);
        let record = journal
            .complete(&coordinate, Value::String(payload.clone()))
            .await
            .unwrap();
        assert!(record.output_spilled);
        assert!(record.output.is_none());
        assert!(record.output_hash.is_some());
        assert_eq!(record.output_len, Some(payload.len() + 2)); // + quotes
        assert!(record
            .preview
            .as_ref()
            .map(|p| p.len() <= 256)
            .unwrap_or(false));

        // Blob exists on disk; read loads it back on demand.
        let blobs: Vec<_> = fs::read_dir(root.path().join("blobs"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(blobs.len(), 1);
        let loaded = journal.read(&coordinate).await.unwrap();
        assert_eq!(loaded.output, Some(Value::String(payload)));
    }

    #[tokio::test]
    async fn identical_outputs_deduplicate_to_one_blob() {
        let root = tempfile::tempdir().unwrap();
        let journal = make_journal(root.path().to_path_buf());
        let payload = Value::String("y".repeat(16 * 1024));
        for _ in 0..2 {
            let outcome = journal.submit("run", tool_handle("big"), json!({})).await;
            journal
                .complete(&coord_of(&outcome), payload.clone())
                .await
                .unwrap();
        }
        let blobs: Vec<_> = fs::read_dir(root.path().join("blobs"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(blobs.len(), 1, "same content hash must reuse the blob file");
    }

    #[tokio::test]
    async fn gc_drop_payloads_deletes_blobs_but_keeps_envelopes() {
        let root = tempfile::tempdir().unwrap();
        let journal = make_journal(root.path().to_path_buf());
        let outcome = journal.submit("run", tool_handle("big"), json!({})).await;
        let coordinate = coord_of(&outcome);
        journal
            .complete(&coordinate, Value::String("z".repeat(16 * 1024)))
            .await
            .unwrap();
        journal.gc(RetainPolicy::DropPayloads).await.unwrap();

        let blobs: Vec<_> = fs::read_dir(root.path().join("blobs"))
            .unwrap()
            .flatten()
            .collect();
        assert!(blobs.is_empty(), "payload blobs must be deleted");

        // Envelope survives in the index.
        let envelopes = journal.list("run", &ListFilter::default()).await;
        assert_eq!(envelopes.len(), 1);
        assert!(envelopes[0].output_hash.is_some());
        assert!(envelopes[0].output_len.unwrap() > 0);

        // Reading no longer yields a payload (blob gone), but no error.
        let record = journal.read(&coordinate).await.unwrap();
        assert_eq!(record.output, None);
        assert!(record.output_spilled);
    }

    #[tokio::test]
    async fn gc_keep_last_n_runs_drops_older_runs() {
        let root = tempfile::tempdir().unwrap();
        let journal = make_journal(root.path().to_path_buf());
        let older = journal.submit("run-old", tool_handle("t"), json!({})).await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        let newer = journal.submit("run-new", tool_handle("t"), json!({})).await;
        journal.flush_now().await.unwrap();

        journal.gc(RetainPolicy::KeepLastNRuns(1)).await.unwrap();
        assert!(journal
            .list("run-old", &ListFilter::default())
            .await
            .is_empty());
        assert_eq!(
            journal.list("run-new", &ListFilter::default()).await.len(),
            1
        );
        assert!(journal.read(&coord_of(&older)).await.is_err());
        assert!(journal.read(&coord_of(&newer)).await.is_ok());
        assert!(!root.path().join("segments").join("run-old").exists());
    }

    #[tokio::test]
    async fn compact_keeps_last_line_per_coordinate() {
        let root = tempfile::tempdir().unwrap();
        let journal = make_journal(root.path().to_path_buf());
        let outcome = journal.submit("run", tool_handle("t"), json!({})).await;
        let coordinate = coord_of(&outcome);
        journal.complete(&coordinate, json!("ok")).await.unwrap();
        journal.flush_now().await.unwrap();
        // Pending line + completed line for one coordinate.
        assert!(!journal.needs_compact("run").await);
        journal.compact("run").await.unwrap();
        let content = fs::read_to_string(
            root.path()
                .join("segments")
                .join("run")
                .join("000000.jsonl"),
        )
        .unwrap();
        assert_eq!(
            content.lines().count(),
            1,
            "superseded pending line is dropped"
        );

        // A fresh journal still recovers the single record.
        drop(journal);
        let journal2 = make_journal(root.path().to_path_buf());
        assert_eq!(journal2.recover().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn complete_rejects_unknown_and_non_pending_coordinates() {
        let root = tempfile::tempdir().unwrap();
        let journal = make_journal(root.path().to_path_buf());
        let unknown = Coordinate {
            run_id: "run".to_string(),
            seq: 99,
            ulid: "X".to_string(),
        };
        assert!(matches!(
            journal.complete(&unknown, json!("out")).await,
            Err(ExchangeError::CoordinateNotFound(_))
        ));
        let outcome = journal.submit("run", tool_handle("t"), json!({})).await;
        let coordinate = coord_of(&outcome);
        journal.complete(&coordinate, json!("out")).await.unwrap();
        assert!(matches!(
            journal.complete(&coordinate, json!("again")).await,
            Err(ExchangeError::NotPending(_))
        ));
    }

    #[tokio::test]
    async fn fail_records_terminal_error() {
        let root = tempfile::tempdir().unwrap();
        let journal = make_journal(root.path().to_path_buf());
        let outcome = journal.submit("run", tool_handle("t"), json!({})).await;
        let coordinate = coord_of(&outcome);
        journal.fail(&coordinate, "boom".to_string()).await.unwrap();
        let record = journal.read(&coordinate).await.unwrap();
        assert_eq!(record.status, RecordStatus::Failed);
        assert_eq!(record.reason.as_deref(), Some("boom"));
        assert!(Instant::now().elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn hot_payload_eviction_caps_inline_bodies() {
        let root = tempfile::tempdir().unwrap();
        let config = ExchangeConfig {
            max_hot_payloads_per_run: 2,
            ..test_config()
        };
        let journal = Arc::new(ExchangeJournal::new(root.path().to_path_buf(), config));
        let mut coordinates = Vec::new();
        for index in 0..4 {
            let outcome = journal.submit("run", tool_handle("t"), json!({})).await;
            let coordinate = coord_of(&outcome);
            journal
                .complete(&coordinate, json!({ "i": index }))
                .await
                .unwrap();
            coordinates.push(coordinate);
        }
        // Oldest two payloads were demoted to envelope-only; reads fall back
        // to nothing in memory but the records themselves remain listed.
        for (index, coordinate) in coordinates.iter().enumerate() {
            let record = journal.read(coordinate).await.unwrap();
            if index < 2 {
                assert!(record.output.is_none(), "record {index} should be evicted");
            } else {
                assert!(record.output.is_some());
            }
        }
    }
}
