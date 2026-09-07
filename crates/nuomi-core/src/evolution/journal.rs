//! Harness Journal: the structured, append-only audit log behind
//! Self-Evolution. Every evolution action — reflection trigger, proposal,
//! gate verdict, apply, rollback, drift alert — lands as an immutable entry
//! answering who / when / why / what changed / on what evidence, enabling
//! time-travel rollback and drift alerting (drift containment package).
//!
//! Storage model — a deliberately simplified sibling of
//! `exchange::journal` (which this module must neither depend on nor
//! modify):
//! - **Hot**: every entry lives in an in-memory vec kept seq-ascending.
//! - **Durable**: appends are synchronous — one `write_all + flush` per
//!   entry into per-domain JSONL segment files
//!   (`segments/<domain>/<seg>.jsonl`, rotated every
//!   [`SEGMENT_ROTATE_LINES`] lines). Evolution actions are rare (bounded
//!   by the review gate and cooldown windows), so per-entry durability is
//!   free and keeps behaviour deterministic — no background writer task.
//! - **Mirror**: each entry is also appended to the SQLite `events` table
//!   (canonical aggregate `journal/<domain>` plus a session-scoped mirror
//!   under the synthetic [`JOURNAL_SINK_SESSION_ID`] session) so the
//!   existing `list_events` IPC can serve the Harness Journal view without
//!   a new command (see `store::repos::journal`).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::now_ms;
use crate::domain::PromptVersion;
use crate::store::{repos, Db, StoreError};

use super::review::{ReviewGate, ReviewProposal, ReviewVerdict};
use super::versioning::{ApplySnapshot, PromptVersionManager};
use super::EvolutionError;

/// Event-kind prefix shared by every mirrored journal event; the Harness
/// Journal view filters `list_events` output on this prefix.
pub const JOURNAL_KIND_PREFIX: &str = "journal.";

/// Synthetic session id hosting the `list_events`-readable mirror. Exists
/// as a real row in `sessions` (created lazily by the mirror repo) because
/// `list_events` validates the session before reading its events.
pub const JOURNAL_SINK_SESSION_ID: &str = "journal";

/// Per-domain segment files rotate after this many lines.
const SEGMENT_ROTATE_LINES: usize = 512;

/// Domain used for cross-plugin entries (drift alerts, scheduler ticks).
pub const EVOLUTION_DOMAIN: &str = "evolution";

/// Discriminated journal entry kinds. The serde tag mirrors the event-kind
/// suffix appended after [`JOURNAL_KIND_PREFIX`] when mirrored into
/// `events`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JournalKind {
    /// The reflection loop was triggered (scheduler tick or explicit run).
    ReflectionTriggered,
    /// A candidate prompt was stored in the version state machine.
    ProposalGenerated {
        /// id of the stored `PromptVersion` candidate.
        proposal_ref: String,
    },
    /// The review gate passed a verdict on a proposal.
    GateVerdict {
        verdict: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// A candidate was activated; carries before/after version refs.
    Applied {
        #[serde(skip_serializing_if = "Option::is_none")]
        before_ref: Option<String>,
        after_ref: String,
    },
    /// A rollback completed: `from_seq` is the undone Applied entry, `to_seq`
    /// is this entry itself (the audit loop closes on its own seq).
    RolledBack { from_seq: u64, to_seq: u64 },
    /// Drift detector tripped: `metric` + observed `delta`.
    DriftAlert { metric: String, delta: f64 },
    /// An apply baseline was captured (optimistic-concurrency anchor).
    BaselineCaptured { digest: String },
}

impl JournalKind {
    /// Full event kind used for the `events` mirror, e.g.
    /// `journal.applied`.
    pub fn kind_str(&self) -> String {
        let suffix = match self {
            JournalKind::ReflectionTriggered => "reflection_triggered",
            JournalKind::ProposalGenerated { .. } => "proposal_generated",
            JournalKind::GateVerdict { .. } => "gate_verdict",
            JournalKind::Applied { .. } => "applied",
            JournalKind::RolledBack { .. } => "rolled_back",
            JournalKind::DriftAlert { .. } => "drift_alert",
            JournalKind::BaselineCaptured { .. } => "baseline_captured",
        };
        format!("{JOURNAL_KIND_PREFIX}{suffix}")
    }
}

/// One immutable audit entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Global monotonic journal sequence.
    pub seq: u64,
    /// Unix-ms timestamp.
    pub ts: i64,
    /// Domain the entry belongs to (plugin name or [`EVOLUTION_DOMAIN`]).
    pub domain: String,
    pub kind: JournalKind,
    /// Who performed the action (e.g. `versioning`, `review_gate`).
    pub actor: String,
    /// Human-readable one-liner.
    pub summary: String,
    /// Evidence backing the action: version ids, session ids, topics…
    pub evidence_refs: Vec<String>,
    /// Free-form structured detail (e.g. the full `ApplySnapshot`).
    pub payload: Value,
}

/// Optional filters for [`EvolutionJournal::entries`].
#[derive(Debug, Clone, Default)]
pub struct JournalFilter {
    pub domain: Option<String>,
    pub actor: Option<String>,
    /// Matches the event-kind suffix, e.g. `journal.applied`.
    pub kind_prefix: Option<String>,
}

impl JournalFilter {
    fn matches(&self, entry: &JournalEntry) -> bool {
        if let Some(domain) = &self.domain {
            if &entry.domain != domain {
                return false;
            }
        }
        if let Some(actor) = &self.actor {
            if &entry.actor != actor {
                return false;
            }
        }
        if let Some(prefix) = &self.kind_prefix {
            if !entry.kind.kind_str().starts_with(prefix.as_str()) {
                return false;
            }
        }
        true
    }
}

/// Writer-side per-domain append state; the file is dropped (and re-opened)
/// on segment rotation.
struct SegmentWriter {
    file: Option<std::fs::File>,
    seg_index: u64,
    count: usize,
}

/// The Harness Journal. Clone-free sharing: wrap in `Arc` and hand copies
/// of the `Arc` to the evolution components.
pub struct EvolutionJournal {
    root: PathBuf,
    /// When set, every persisted entry is mirrored into the SQLite events
    /// table via `repos::journal::append_mirror`.
    db_path: Option<Arc<str>>,
    entries: Mutex<Vec<JournalEntry>>,
    next_seq: AtomicU64,
    writers: Mutex<HashMap<String, SegmentWriter>>,
}

fn domain_key(domain: &str) -> String {
    let sanitized: String = domain
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

impl EvolutionJournal {
    /// Creates a journal rooted at `root` (conventionally
    /// `<app_data>/evolution-journal`), recovering any previous entries from
    /// the JSONL segments. With `db_path` set, entries are mirrored into
    /// that SQLite database's `events` table.
    pub fn new(root: PathBuf, db_path: Option<impl Into<Arc<str>>>) -> Self {
        let _ = std::fs::create_dir_all(root.join("segments"));
        let mut journal = Self {
            root,
            db_path: db_path.map(Into::into),
            entries: Mutex::new(Vec::new()),
            next_seq: AtomicU64::new(1),
            writers: Mutex::new(HashMap::new()),
        };
        if let Err(error) = journal.recover() {
            tracing::warn!("journal: recover failed: {error}");
        }
        journal
    }

    /// Rebuilds the hot index from segment files; the last line per seq
    /// wins. Returns the number of unique entries loaded.
    pub fn recover(&mut self) -> Result<usize, EvolutionError> {
        let segments_root = self.root.join("segments");
        let dirs = match std::fs::read_dir(&segments_root) {
            Ok(dirs) => dirs,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error.into()),
        };
        // Last line per seq wins across all domains (seq is global).
        let mut latest: std::collections::BTreeMap<u64, JournalEntry> =
            std::collections::BTreeMap::new();
        for dir in dirs {
            let dir = dir?;
            if !dir.file_type()?.is_dir() {
                continue;
            }
            for path in segment_files(&dir.path())? {
                let content = std::fs::read_to_string(&path)?;
                for line in content.lines() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<JournalEntry>(line) {
                        Ok(entry) => {
                            latest.insert(entry.seq, entry);
                        }
                        Err(error) => {
                            tracing::warn!(
                                "journal: recover skipped a corrupt line in {:?}: {error}",
                                dir.file_name()
                            );
                        }
                    }
                }
            }
        }
        let loaded = latest.len();
        let max_seq = latest.keys().copied().max().unwrap_or(0);
        *self.entries.lock().unwrap_or_else(PoisonError::into_inner) =
            latest.into_values().collect();
        self.next_seq.store(max_seq + 1, Ordering::Relaxed);
        Ok(loaded)
    }

    fn alloc_seq(&self) -> u64 {
        self.next_seq.fetch_add(1, Ordering::Relaxed)
    }

    /// Persists a fully-built entry: JSONL segment (durable), hot index and
    /// best-effort SQLite mirror.
    fn persist(&self, entry: JournalEntry) -> Result<JournalEntry, EvolutionError> {
        let line = serde_json::to_string(&entry)
            .map_err(|e| EvolutionError::JournalCorrupt(format!("serialize entry: {e}")))?;
        self.write_segment(&entry.domain, &line)?;
        if let Some(db_path) = &self.db_path {
            let kind = entry.kind.kind_str();
            let result = mirror_to_sqlite(db_path, &entry.domain, &kind, &entry.ts, &entry.payload);
            if let Err(error) = result {
                // The JSONL segment is the durable source of truth; the
                // events mirror is a read projection and may be re-synced.
                tracing::warn!("journal: events mirror failed for #{}: {error}", entry.seq);
            }
        }
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry.clone());
        Ok(entry)
    }

    fn write_segment(&self, domain: &str, line: &str) -> Result<(), EvolutionError> {
        let key = domain_key(domain);
        let mut writers = self.writers.lock().unwrap_or_else(PoisonError::into_inner);
        let writer = writers.entry(key.clone()).or_insert(SegmentWriter {
            file: None,
            seg_index: 0,
            count: 0,
        });
        if writer.file.is_none() {
            let dir = self.root.join("segments").join(&key);
            std::fs::create_dir_all(&dir)?;
            writer.file = Some(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join(format!("{:06}.jsonl", writer.seg_index)))?,
            );
        }
        if let Some(file) = writer.file.as_mut() {
            let mut bytes = line.as_bytes().to_vec();
            bytes.push(b'\n');
            file.write_all(&bytes)?;
            file.flush()?;
            writer.count += 1;
            if writer.count >= SEGMENT_ROTATE_LINES {
                writer.file = None;
                writer.seg_index += 1;
                writer.count = 0;
            }
        }
        Ok(())
    }

    /// Appends one entry (durable JSONL + hot index + events mirror).
    #[allow(clippy::too_many_arguments)]
    pub fn append(
        &self,
        domain: &str,
        kind: JournalKind,
        actor: &str,
        summary: impl Into<String>,
        evidence_refs: Vec<String>,
        payload: Value,
    ) -> Result<JournalEntry, EvolutionError> {
        let entry = JournalEntry {
            seq: self.alloc_seq(),
            ts: now_ms(),
            domain: domain.to_string(),
            kind,
            actor: actor.to_string(),
            summary: summary.into(),
            evidence_refs,
            payload,
        };
        self.persist(entry)
    }

    /// All entries (seq-ascending), optionally filtered.
    pub fn entries(&self, filter: Option<&JournalFilter>) -> Vec<JournalEntry> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|entry| filter.map(|f| f.matches(entry)).unwrap_or(true))
            .cloned()
            .collect()
    }

    /// Most recent entry of `domain`.
    pub fn latest(&self, domain: &str) -> Option<JournalEntry> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .rev()
            .find(|entry| entry.domain == domain)
            .cloned()
    }

    /// Prefix replay: every entry up to and including `seq` (time travel).
    pub fn replay_to(&self, seq: u64) -> Vec<JournalEntry> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .take_while(|entry| entry.seq <= seq)
            .cloned()
            .collect()
    }

    /// Time-travel rollback: finds the `Applied` entry at `seq`, restores its
    /// `before` snapshot through the versioning manager (append-only: the
    /// rollback lands as a new forward version) and records a `RolledBack`
    /// audit entry whose `to_seq` is its own seq — the audit loop closes.
    pub async fn rollback_to(
        &self,
        seq: u64,
        versions: &PromptVersionManager,
    ) -> Result<PromptVersion, EvolutionError> {
        let applied = self
            .entries(None)
            .into_iter()
            .find(|entry| entry.seq == seq)
            .ok_or(EvolutionError::JournalNotApplied(seq))?;
        if !matches!(applied.kind, JournalKind::Applied { .. }) {
            return Err(EvolutionError::JournalNotApplied(seq));
        }
        let snapshot: ApplySnapshot =
            serde_json::from_value(applied.payload.get("snapshot").cloned().ok_or_else(|| {
                EvolutionError::JournalCorrupt(format!("entry #{seq} carries no snapshot"))
            })?)
            .map_err(|e| EvolutionError::JournalCorrupt(format!("snapshot for #{seq}: {e}")))?;

        let restored = versions.rollback(&snapshot).await?;

        let entry_seq = self.alloc_seq();
        let entry = JournalEntry {
            seq: entry_seq,
            ts: now_ms(),
            domain: snapshot.plugin.clone(),
            kind: JournalKind::RolledBack {
                from_seq: seq,
                to_seq: entry_seq,
            },
            actor: "journal".to_string(),
            summary: format!(
                "rolled back applied entry #{seq} (v{}); restored content as forward version v{}",
                snapshot.applied_version, restored.version
            ),
            evidence_refs: vec![restored.id.clone()],
            payload: serde_json::json!({
                "rolled_back_seq": seq,
                "restored_version": restored,
            }),
        };
        self.persist(entry)?;
        Ok(restored)
    }
}

/// Blocking SQLite mirror: runs inside `persist` (evolution actions are rare,
/// so the brief block is preferable to a fire-and-forget task that would make
/// tests and ordering non-deterministic).
fn mirror_to_sqlite(
    db_path: &Arc<str>,
    domain: &str,
    kind: &str,
    ts: &i64,
    payload: &Value,
) -> Result<(), EvolutionError> {
    fn once(
        db_path: &Arc<str>,
        domain: &str,
        kind: &str,
        ts: i64,
        payload: &Value,
    ) -> Result<(), StoreError> {
        let db = Db::open(db_path)?;
        crate::store::migrations::run(&db.0)?;
        repos::journal::append_mirror(&db.0, domain, kind, payload, ts)
    }
    once(db_path, domain, kind, *ts, payload).map_err(|e| EvolutionError::Store(e.to_string()))
}

fn segment_files(dir: &Path) -> Result<Vec<PathBuf>, EvolutionError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().map(|ext| ext == "jsonl").unwrap_or(false))
        .collect();
    files.sort();
    Ok(files)
}

/// Fire-and-forget audit helper for optional-journal call sites: appends and
/// downgrades any failure to a warning (auditing must never break the action
/// it observes).
pub fn audit(
    journal: &Option<Arc<EvolutionJournal>>,
    domain: &str,
    kind: JournalKind,
    actor: &str,
    summary: String,
    evidence_refs: Vec<String>,
    payload: Value,
) {
    if let Some(journal) = journal {
        if let Err(error) = journal.append(domain, kind, actor, summary, evidence_refs, payload) {
            tracing::warn!("journal: audit append failed: {error}");
        }
    }
}

/// Wraps any [`ReviewGate`] and records every verdict as a `GateVerdict`
/// journal entry — the review checkpoint becomes part of the audit trail
/// without touching `review.rs`.
pub struct JournaledReviewGate<G: ReviewGate> {
    inner: G,
    journal: Arc<EvolutionJournal>,
    domain: String,
}

impl<G: ReviewGate> JournaledReviewGate<G> {
    pub fn new(inner: G, journal: Arc<EvolutionJournal>, domain: impl Into<String>) -> Self {
        Self {
            inner,
            journal,
            domain: domain.into(),
        }
    }
}

#[async_trait]
impl<G: ReviewGate> ReviewGate for JournaledReviewGate<G> {
    async fn review(&self, proposal: &ReviewProposal) -> ReviewVerdict {
        let verdict = self.inner.review(proposal).await;
        let (label, reason) = match &verdict {
            ReviewVerdict::Approve => ("approve", None),
            ReviewVerdict::Reject(reason) => ("reject", Some(reason.clone())),
            ReviewVerdict::Defer => ("defer", None),
        };
        if let Err(error) = self.journal.append(
            &self.domain,
            JournalKind::GateVerdict {
                verdict: label.to_string(),
                reason,
            },
            "review_gate",
            format!("gate {label} proposal for '{}'", proposal.plugin),
            vec![proposal.plugin.clone()],
            serde_json::json!({ "plugin": proposal.plugin }),
        ) {
            tracing::warn!("journal: gate verdict append failed: {error}");
        }
        verdict
    }
}

/// Drift thresholds. Defaults are conservative starting points, not gospel —
/// tune per deployment once real telemetry exists.
#[derive(Debug, Clone)]
pub struct DriftConfig {
    /// Sliding window for all statistics (default 24h).
    pub window_ms: i64,
    /// Minimum applies inside the window before the rollback-rate rule fires
    /// (keeps the rate meaningful: 1 apply + 1 rollback = 100% noise).
    pub min_applies: usize,
    /// Rollback-rate fraction (rolled_back / applied) that signals drift.
    pub max_rollback_rate: f64,
    /// Cumulative prompt diff characters tolerated inside the window.
    pub max_diff_chars: usize,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self {
            window_ms: 24 * 60 * 60 * 1000,
            min_applies: 10,
            max_rollback_rate: 0.3,
            max_diff_chars: 20_000,
        }
    }
}

/// Sliding-window drift detector over the journal. When a rule trips it
/// appends a `DriftAlert` entry and logs a `tracing::warn`. Alerts are
/// suppressed while one is already active inside the window (no spam).
pub struct DriftDetector {
    config: DriftConfig,
}

impl DriftDetector {
    pub fn new(config: DriftConfig) -> Self {
        Self { config }
    }

    pub fn with_default_config() -> Self {
        Self::new(DriftConfig::default())
    }

    /// Evaluates the window; returns the appended alert, if any.
    pub fn evaluate(
        &self,
        journal: &EvolutionJournal,
    ) -> Result<Option<JournalEntry>, EvolutionError> {
        let now = now_ms();
        let window: Vec<JournalEntry> = journal
            .entries(None)
            .into_iter()
            .filter(|entry| entry.ts >= now - self.config.window_ms)
            .collect();
        // One active alert per window.
        if window
            .iter()
            .any(|entry| matches!(entry.kind, JournalKind::DriftAlert { .. }))
        {
            return Ok(None);
        }
        let applied: Vec<&JournalEntry> = window
            .iter()
            .filter(|entry| matches!(entry.kind, JournalKind::Applied { .. }))
            .collect();
        let rolled_back = window
            .iter()
            .filter(|entry| matches!(entry.kind, JournalKind::RolledBack { .. }))
            .count();

        let rollback_rate = if applied.is_empty() {
            0.0
        } else {
            rolled_back as f64 / applied.len() as f64
        };
        // Diff scale: sum of the applied versions' diff texts (the "how much
        // did the prompt move" proxy).
        let diff_chars: usize = applied
            .iter()
            .filter_map(|entry| entry.payload.get("snapshot"))
            .filter_map(|snapshot| snapshot.get("after"))
            .filter_map(|after| after.get("diff_text"))
            .filter_map(|diff| diff.as_str())
            .map(str::len)
            .sum();

        let (metric, delta, summary) = if applied.len() >= self.config.min_applies
            && rollback_rate >= self.config.max_rollback_rate
        {
            (
                "apply_rollback_rate",
                rollback_rate,
                format!(
                    "{rolled_back} of {} applies rolled back within the window (rate {rollback_rate:.2} >= {})",
                    applied.len(),
                    self.config.max_rollback_rate
                ),
            )
        } else if diff_chars > self.config.max_diff_chars {
            (
                "prompt_diff_scale",
                diff_chars as f64,
                format!(
                    "prompt diff scale {diff_chars} chars exceeds {} within the window",
                    self.config.max_diff_chars
                ),
            )
        } else {
            return Ok(None);
        };

        tracing::warn!(metric, delta, "evolution drift detected: {summary}");
        let entry = journal.append(
            EVOLUTION_DOMAIN,
            JournalKind::DriftAlert {
                metric: metric.to_string(),
                delta,
            },
            "drift_detector",
            summary,
            applied.iter().map(|entry| entry.seq.to_string()).collect(),
            serde_json::json!({
                "window_ms": self.config.window_ms,
                "applied": applied.len(),
                "rolled_back": rolled_back,
                "diff_chars": diff_chars,
            }),
        )?;
        Ok(Some(entry))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evolution::reflection::PromptCandidate;
    use crate::evolution::DefaultReviewGate;
    use serde_json::json;

    fn journal(root: PathBuf) -> EvolutionJournal {
        EvolutionJournal::new(root, None::<String>)
    }

    fn journal_with_db(root: PathBuf, db: &Path) -> EvolutionJournal {
        EvolutionJournal::new(root, Some(db.to_string_lossy().to_string()))
    }

    fn applied_payload(after_diff: &str) -> Value {
        json!({
            "snapshot": {
                "plugin": "system_prompt",
                "applied_version": 2,
                "after": { "diff_text": after_diff },
            }
        })
    }

    #[test]
    fn append_entries_latest_replay_and_filter() {
        let dir = tempfile::tempdir().unwrap();
        let j = journal(dir.path().to_path_buf());
        let e1 = j
            .append(
                "system_prompt",
                JournalKind::ReflectionTriggered,
                "reflector",
                "reflect over 2 trajectories",
                vec!["s1".into(), "s2".into()],
                json!({ "trajectory_count": 2 }),
            )
            .unwrap();
        let e2 = j
            .append(
                EVOLUTION_DOMAIN,
                JournalKind::DriftAlert {
                    metric: "apply_rollback_rate".into(),
                    delta: 0.5,
                },
                "drift_detector",
                "rate tripped",
                vec![],
                json!({}),
            )
            .unwrap();
        assert_eq!((e1.seq, e2.seq), (1, 2));
        assert_eq!(e2.kind.kind_str(), "journal.drift_alert");

        assert_eq!(
            j.latest("system_prompt").unwrap().seq,
            1,
            "latest scopes by domain"
        );
        assert_eq!(j.latest("nope"), None);

        let replay = j.replay_to(1);
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].seq, 1);

        let filtered = j.entries(Some(&JournalFilter {
            kind_prefix: Some("journal.drift".into()),
            ..Default::default()
        }));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].seq, 2);
    }

    #[test]
    fn segments_survive_restart_and_seq_continues() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        {
            let j = journal(root.clone());
            j.append(
                "system_prompt",
                JournalKind::ReflectionTriggered,
                "reflector",
                "a",
                vec![],
                json!({}),
            )
            .unwrap();
            j.append(
                "system_prompt",
                JournalKind::ReflectionTriggered,
                "reflector",
                "b",
                vec![],
                json!({}),
            )
            .unwrap();
        }
        let mut j2 = journal(root);
        assert_eq!(j2.recover().unwrap(), 2);
        assert_eq!(j2.entries(None).len(), 2);
        let next = j2
            .append(
                "system_prompt",
                JournalKind::ReflectionTriggered,
                "reflector",
                "c",
                vec![],
                json!({}),
            )
            .unwrap();
        assert_eq!(next.seq, 3, "seq continues from the recovered max");
    }

    #[test]
    fn mirror_lands_in_events_and_synthetic_session() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("evo.db");
        let j = journal_with_db(dir.path().to_path_buf(), &db_path);
        j.append(
            "system_prompt",
            JournalKind::ProposalGenerated {
                proposal_ref: "pv-1".into(),
            },
            "versioning",
            "candidate stored",
            vec!["pv-1".into()],
            json!({}),
        )
        .unwrap();

        let conn = rusqlite::Connection::open(db_path.as_path()).unwrap();
        crate::store::migrations::run(&conn).unwrap();
        // Synthetic sink session exists so list_events("journal") validates.
        assert!(crate::store::repos::sessions::get(&conn, JOURNAL_SINK_SESSION_ID).is_ok());
        let canonical =
            crate::store::repos::events::list_by_aggregate(&conn, "journal", "system_prompt", None)
                .unwrap();
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].kind, "journal.proposal_generated");
        let mirrored = crate::store::repos::events::list_by_aggregate(
            &conn,
            "session",
            JOURNAL_SINK_SESSION_ID,
            None,
        )
        .unwrap();
        assert_eq!(mirrored.len(), 1);
        assert_eq!(mirrored[0].kind, "journal.proposal_generated");
    }

    #[tokio::test]
    async fn journaled_review_gate_records_verdicts() {
        let dir = tempfile::tempdir().unwrap();
        let j = Arc::new(journal(dir.path().to_path_buf()));
        let gate =
            JournaledReviewGate::new(DefaultReviewGate::new(4), Arc::clone(&j), "system_prompt");
        let proposal = ReviewProposal {
            plugin: "system_prompt".into(),
            candidate: PromptCandidate {
                content: "body".into(),
                diff_text: "diff".into(),
                parent_version: None,
            },
        };
        assert_eq!(gate.review(&proposal).await, ReviewVerdict::Approve);
        // Duplicate within the dedup window → reject.
        assert!(matches!(
            gate.review(&proposal).await,
            ReviewVerdict::Reject(_)
        ));
        let all = j.entries(None);
        let verdicts: Vec<&JournalEntry> = all
            .iter()
            .filter(|e| matches!(e.kind, JournalKind::GateVerdict { .. }))
            .collect();
        assert_eq!(verdicts.len(), 2);
        assert_eq!(verdicts[0].actor, "review_gate");
        assert_eq!(verdicts[0].domain, "system_prompt");
    }

    #[tokio::test]
    async fn rollback_to_restores_content_and_closes_audit_loop() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("evo.db");
        let j = Arc::new(journal_with_db(dir.path().to_path_buf(), &db_path));
        let manager =
            PromptVersionManager::new(db_path.to_string_lossy()).with_journal(Arc::clone(&j));

        let v1 = manager
            .propose_candidate("system_prompt", candidate("safe body", "init"))
            .await
            .unwrap();
        manager.activate("system_prompt", v1.version).await.unwrap();
        let baseline = manager.plan_baseline("system_prompt").await.unwrap();
        let v2 = manager
            .propose_candidate("system_prompt", candidate("risky body", "+risky"))
            .await
            .unwrap();
        let snapshot = manager
            .apply_candidate("system_prompt", v2.version, baseline)
            .await
            .unwrap();

        // The applied entry carries the full before/after snapshot.
        let applied = j
            .entries(None)
            .into_iter()
            .find(|e| matches!(e.kind, JournalKind::Applied { .. }))
            .expect("applied entry journaled");
        let JournalKind::Applied {
            ref before_ref,
            ref after_ref,
        } = applied.kind
        else {
            panic!("unreachable");
        };
        assert_eq!(before_ref.as_deref(), Some(v1.id.as_str()));
        assert_eq!(after_ref, &snapshot.after.id);

        let restored = j.rollback_to(applied.seq, &manager).await.unwrap();
        assert_eq!(restored.content, "safe body");

        // Audit loop closed: RolledBack points at the undone Applied seq.
        let rolled = j.entries(None).last().unwrap().clone();
        assert_eq!(
            rolled.kind,
            JournalKind::RolledBack {
                from_seq: applied.seq,
                to_seq: rolled.seq,
            }
        );

        // Time-travel replay up to the applied entry excludes the rollback.
        assert!(j.replay_to(applied.seq).iter().all(|e| e.seq != rolled.seq));

        // Rolling back a non-Applied entry is rejected.
        let first = j.entries(None).first().unwrap().clone();
        assert!(matches!(
            j.rollback_to(first.seq, &manager).await,
            Err(EvolutionError::JournalNotApplied(_))
        ));
    }

    #[tokio::test]
    async fn versioning_journals_baseline_proposal_and_apply() {
        let dir = tempfile::tempdir().unwrap();
        let j = Arc::new(journal(dir.path().to_path_buf()));
        let manager = PromptVersionManager::new(dir.path().join("e.db").to_string_lossy())
            .with_journal(Arc::clone(&j));

        let v1 = manager
            .propose_candidate("system_prompt", candidate("base", "init"))
            .await
            .unwrap();
        let baseline = manager.plan_baseline("system_prompt").await.unwrap();
        let v2 = manager
            .propose_candidate("system_prompt", candidate("next", "+tweaks"))
            .await
            .unwrap();
        manager
            .apply_candidate("system_prompt", v2.version, baseline)
            .await
            .unwrap();

        let kinds: Vec<String> = j.entries(None).iter().map(|e| e.kind.kind_str()).collect();
        assert!(kinds.contains(&"journal.proposal_generated".to_string()));
        assert!(kinds.contains(&"journal.baseline_captured".to_string()));
        assert!(kinds.contains(&"journal.applied".to_string()));
        assert_eq!(v1.version, 1);
    }

    #[test]
    fn drift_detector_fires_then_suppresses_within_window() {
        let dir = tempfile::tempdir().unwrap();
        let j = journal(dir.path().to_path_buf());
        // 2 applies, 1 rollback → rate 0.5; min_applies lowered via config.
        for diff in ["+a", "+b"] {
            j.append(
                "system_prompt",
                JournalKind::Applied {
                    before_ref: None,
                    after_ref: "x".into(),
                },
                "versioning",
                "applied",
                vec![],
                applied_payload(diff),
            )
            .unwrap();
        }
        j.append(
            "system_prompt",
            JournalKind::RolledBack {
                from_seq: 1,
                to_seq: 3,
            },
            "journal",
            "rolled back",
            vec![],
            json!({}),
        )
        .unwrap();

        let detector = DriftDetector::new(DriftConfig {
            min_applies: 2,
            ..DriftConfig::default()
        });
        let alert = detector.evaluate(&j).unwrap().expect("drift alert fires");
        assert!(matches!(
            alert.kind,
            JournalKind::DriftAlert { ref metric, delta } if metric == "apply_rollback_rate" && (delta - 0.5).abs() < 1e-9
        ));

        // Second evaluation inside the same window is suppressed.
        assert!(detector.evaluate(&j).unwrap().is_none());
    }

    #[test]
    fn drift_detector_fires_on_diff_scale_and_stays_quiet_below_thresholds() {
        let dir = tempfile::tempdir().unwrap();
        let j = journal(dir.path().to_path_buf());
        j.append(
            "system_prompt",
            JournalKind::Applied {
                before_ref: None,
                after_ref: "x".into(),
            },
            "versioning",
            "applied",
            vec![],
            applied_payload(&"d".repeat(25_000)),
        )
        .unwrap();

        let detector = DriftDetector::with_default_config();
        let alert = detector
            .evaluate(&j)
            .unwrap()
            .expect("diff-scale rule fires");
        assert!(matches!(
            alert.kind,
            JournalKind::DriftAlert { ref metric, .. } if metric == "prompt_diff_scale"
        ));

        // Small, healthy activity stays below every threshold.
        let quiet = journal(tempfile::tempdir().unwrap().path().to_path_buf());
        quiet
            .append(
                "system_prompt",
                JournalKind::Applied {
                    before_ref: None,
                    after_ref: "x".into(),
                },
                "versioning",
                "applied",
                vec![],
                applied_payload("+tiny"),
            )
            .unwrap();
        assert!(detector.evaluate(&quiet).unwrap().is_none());
    }

    fn candidate(content: &str, diff: &str) -> PromptCandidate {
        PromptCandidate {
            content: content.to_string(),
            diff_text: diff.to_string(),
            parent_version: None,
        }
    }
}
