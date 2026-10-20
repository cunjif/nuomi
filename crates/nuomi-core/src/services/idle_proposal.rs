//! Idle team-proposal trigger (ADR 0023, P2-F 组 4).
//!
//! The frontend owns idle *detection*: DOM interaction resets a timer and
//! expiry fires one `idle_check` IPC call. This module owns idle
//! *judgement*: read the four `app_settings` keys, count the focused
//! workspace's queued backlog, and apply the anti-nag gates (at most one
//! pending idle proposal; cooldown since the last idle proposal of any
//! status; user presence is implied by the frontend timer itself). A pass
//! means the caller plans one backlog-cleanup team and records a pending,
//! advisory proposal — never an automatic run.

use rusqlite::Connection;

use crate::domain::Task;
use crate::store::repos::{proposals, settings, tasks_runs};
use crate::store::StoreError;

/// `app_settings` key: master switch (default true — the "自发" spirit).
pub const IDLE_ENABLED_KEY: &str = "idle_proposal.enabled";
/// `app_settings` key: idle timeout in minutes (default 15, Settings UI).
pub const IDLE_TIMEOUT_KEY: &str = "idle_proposal.timeout_minutes";
/// `app_settings` key: queued-backlog threshold (default 3, key documented
/// only — no UI in the base version).
pub const IDLE_BACKLOG_KEY: &str = "idle_proposal.backlog_threshold";
/// `app_settings` key: cooldown hours since the last idle proposal
/// (default 6, key documented only).
pub const IDLE_COOLDOWN_KEY: &str = "idle_proposal.cooldown_hours";

/// Proposal `source` value for this trigger.
pub const SOURCE: &str = "idle";

/// Backlog tasks folded into one proposal's planning input (ADR 0023 §2).
pub const MAX_BACKLOG_TASKS: usize = 5;

/// Upper bound of the backlog scan — counting beyond this adds nothing.
const BACKLOG_SCAN_LIMIT: u32 = 200;

const DEFAULT_ENABLED: bool = true;
/// Default idle timeout (minutes) — also the fallback shown when the
/// stored value exceeds the Settings-input domain.
pub const DEFAULT_TIMEOUT_MINUTES: u64 = 15;
const DEFAULT_BACKLOG_THRESHOLD: usize = 3;
const DEFAULT_COOLDOWN_HOURS: u64 = 6;

const MS_PER_HOUR: i64 = 3_600_000;

/// The four tunables, loaded best-effort from `app_settings` (missing or
/// invalid values fall back to defaults — the `load_max_bytes` pattern).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleProposalConfig {
    pub enabled: bool,
    pub timeout_minutes: u64,
    pub backlog_threshold: usize,
    pub cooldown_hours: u64,
}

impl Default for IdleProposalConfig {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_ENABLED,
            timeout_minutes: DEFAULT_TIMEOUT_MINUTES,
            backlog_threshold: DEFAULT_BACKLOG_THRESHOLD,
            cooldown_hours: DEFAULT_COOLDOWN_HOURS,
        }
    }
}

impl IdleProposalConfig {
    pub fn load(conn: &Connection) -> Self {
        let read = |key: &str| settings::get(conn, key).ok().flatten();
        Self {
            enabled: read(IDLE_ENABLED_KEY)
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(DEFAULT_ENABLED),
            timeout_minutes: read(IDLE_TIMEOUT_KEY)
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(DEFAULT_TIMEOUT_MINUTES),
            backlog_threshold: read(IDLE_BACKLOG_KEY)
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(DEFAULT_BACKLOG_THRESHOLD),
            cooldown_hours: read(IDLE_COOLDOWN_KEY)
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(DEFAULT_COOLDOWN_HOURS),
        }
    }
}

/// Verdict of an idle check: either the advisory proposal should be
/// planned now, or a specific gate said no.
#[derive(Debug, Clone)]
pub enum IdleCheckOutcome {
    SkipDisabled,
    SkipNoBacklog { queued: usize },
    SkipPendingProposal,
    SkipCooldown { remaining_ms: i64 },
    Trigger { tasks: Vec<Task> },
}

/// Runs the full decision table. `workspace_id` scopes the backlog count
/// (the focused workspace; `None` = whole library). `now` is the caller's
/// clock so tests can time-travel through the cooldown gate.
pub fn evaluate(
    conn: &Connection,
    now: i64,
    workspace_id: Option<&str>,
) -> Result<IdleCheckOutcome, StoreError> {
    let config = IdleProposalConfig::load(conn);
    if !config.enabled {
        return Ok(IdleCheckOutcome::SkipDisabled);
    }
    let backlog = tasks_runs::list_queued_tasks(conn, workspace_id, BACKLOG_SCAN_LIMIT)?;
    if backlog.len() < config.backlog_threshold {
        return Ok(IdleCheckOutcome::SkipNoBacklog {
            queued: backlog.len(),
        });
    }
    if proposals::has_pending_by_source(conn, SOURCE)? {
        return Ok(IdleCheckOutcome::SkipPendingProposal);
    }
    if let Some(latest) = proposals::latest_by_source(conn, SOURCE)? {
        let cooldown_ms = i64::try_from(config.cooldown_hours)
            .unwrap_or(i64::MAX)
            .saturating_mul(MS_PER_HOUR);
        let elapsed = now - latest.created_at;
        if elapsed < cooldown_ms {
            return Ok(IdleCheckOutcome::SkipCooldown {
                remaining_ms: cooldown_ms - elapsed,
            });
        }
    }
    Ok(IdleCheckOutcome::Trigger {
        tasks: backlog.into_iter().take(MAX_BACKLOG_TASKS).collect(),
    })
}

/// Builds the planning input for a backlog batch: each task contributes
/// title plus description when present (the schedule_dispatcher prompt
/// composition semantics — a title-only prompt would plan a hollow team).
pub fn backlog_prompt(tasks: &[Task]) -> String {
    let mut out = String::from("清理看板积压任务（按创建时间升序）：");
    for task in tasks {
        out.push_str("\n- ");
        out.push_str(&task.title);
        let description = task.description.trim();
        if !description.is_empty() {
            out.push('：');
            out.push_str(description);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Proposal, ProposalStatus, TaskStatus};
    use crate::store::migrations;
    use crate::store::repos::settings;
    use rusqlite::Connection;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn task(id: &str, workspace_id: &str, created_at: i64) -> Task {
        Task {
            id: id.into(),
            session_id: None,
            title: format!("task-{id}"),
            description: if id == "t1" {
                "first backlog item".into()
            } else {
                String::new()
            },
            status: TaskStatus::Queued,
            created_at,
            updated_at: created_at,
            workspace_id: workspace_id.into(),
        }
    }

    fn idle_proposal(id: &str, status: ProposalStatus, created_at: i64) -> Proposal {
        Proposal {
            id: id.into(),
            kind: "team_plan".into(),
            source: SOURCE.into(),
            plan_json: "{}".into(),
            status,
            workspace_id: "__migrated__".into(),
            resolved_at: None,
            created_at,
        }
    }

    fn seed_backlog(conn: &Connection, workspace_id: &str, count: usize) {
        for i in 0..count {
            let t = task(&format!("t{i}"), workspace_id, (i + 1) as i64);
            tasks_runs::insert_task(conn, &t).unwrap();
        }
    }

    fn seed_backlog_prefixed(conn: &Connection, prefix: &str, workspace_id: &str, count: usize) {
        for i in 0..count {
            let t = task(&format!("{prefix}{i}"), workspace_id, (i + 1) as i64);
            tasks_runs::insert_task(conn, &t).unwrap();
        }
    }

    #[test]
    fn config_loads_defaults_then_overrides() {
        let conn = db();
        assert_eq!(
            IdleProposalConfig::load(&conn),
            IdleProposalConfig::default()
        );

        settings::set(&conn, IDLE_ENABLED_KEY, "false").unwrap();
        settings::set(&conn, IDLE_TIMEOUT_KEY, "30").unwrap();
        settings::set(&conn, IDLE_BACKLOG_KEY, "5").unwrap();
        settings::set(&conn, IDLE_COOLDOWN_KEY, "12").unwrap();
        assert_eq!(
            IdleProposalConfig::load(&conn),
            IdleProposalConfig {
                enabled: false,
                timeout_minutes: 30,
                backlog_threshold: 5,
                cooldown_hours: 12,
            }
        );

        // Invalid values fall back to defaults, not zero.
        settings::set(&conn, IDLE_TIMEOUT_KEY, "abc").unwrap();
        settings::set(&conn, IDLE_BACKLOG_KEY, "0").unwrap();
        let config = IdleProposalConfig::load(&conn);
        assert_eq!(config.timeout_minutes, DEFAULT_TIMEOUT_MINUTES);
        assert_eq!(config.backlog_threshold, DEFAULT_BACKLOG_THRESHOLD);
    }

    #[test]
    fn evaluate_skips_when_disabled() {
        let conn = db();
        settings::set(&conn, IDLE_ENABLED_KEY, "false").unwrap();
        seed_backlog(&conn, "__migrated__", 5);
        assert!(matches!(
            evaluate(&conn, 100, Some("__migrated__")).unwrap(),
            IdleCheckOutcome::SkipDisabled
        ));
    }

    #[test]
    fn evaluate_skips_when_backlog_below_threshold() {
        let conn = db();
        seed_backlog(&conn, "__migrated__", 2);
        assert!(matches!(
            evaluate(&conn, 100, Some("__migrated__")).unwrap(),
            IdleCheckOutcome::SkipNoBacklog { queued: 2 }
        ));
    }

    #[test]
    fn evaluate_scopes_backlog_to_the_focused_workspace() {
        let conn = db();
        seed_backlog_prefixed(&conn, "a", "ws-a", 1);
        seed_backlog_prefixed(&conn, "b", "ws-b", 4);
        // ws-a alone stays below the threshold; ws-b and the whole library pass.
        assert!(matches!(
            evaluate(&conn, 100, Some("ws-a")).unwrap(),
            IdleCheckOutcome::SkipNoBacklog { queued: 1 }
        ));
        assert!(matches!(
            evaluate(&conn, 100, Some("ws-b")).unwrap(),
            IdleCheckOutcome::Trigger { .. }
        ));
        assert!(matches!(
            evaluate(&conn, 100, None).unwrap(),
            IdleCheckOutcome::Trigger { .. }
        ));
    }

    #[test]
    fn evaluate_skips_when_pending_idle_proposal_exists() {
        let conn = db();
        seed_backlog(&conn, "__migrated__", 3);
        proposals::insert(&conn, &idle_proposal("p1", ProposalStatus::Pending, 1)).unwrap();
        assert!(matches!(
            evaluate(&conn, 100, None).unwrap(),
            IdleCheckOutcome::SkipPendingProposal
        ));
    }

    #[test]
    fn evaluate_skips_during_cooldown_even_for_resolved_proposals() {
        let conn = db();
        seed_backlog(&conn, "__migrated__", 3);
        // Denied two hours ago — still inside the six-hour cooldown.
        proposals::insert(&conn, &idle_proposal("p1", ProposalStatus::Denied, 1)).unwrap();
        match evaluate(&conn, 1 + 2 * MS_PER_HOUR, None).unwrap() {
            IdleCheckOutcome::SkipCooldown { remaining_ms } => {
                assert_eq!(remaining_ms, 4 * MS_PER_HOUR);
            }
            other => panic!("expected SkipCooldown, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_triggers_after_cooldown_expires() {
        let conn = db();
        seed_backlog(&conn, "__migrated__", 3);
        proposals::insert(&conn, &idle_proposal("p1", ProposalStatus::Approved, 1)).unwrap();
        // Exactly at the cooldown edge (>= cooldown) passes.
        assert!(matches!(
            evaluate(&conn, 1 + 6 * MS_PER_HOUR, None).unwrap(),
            IdleCheckOutcome::Trigger { .. }
        ));
    }

    #[test]
    fn evaluate_triggers_with_the_oldest_five_tasks() {
        let conn = db();
        seed_backlog(&conn, "__migrated__", 7);
        let outcome = evaluate(&conn, 100, None).unwrap();
        match outcome {
            IdleCheckOutcome::Trigger { tasks } => {
                assert_eq!(tasks.len(), MAX_BACKLOG_TASKS);
                assert_eq!(
                    tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
                    vec!["t0", "t1", "t2", "t3", "t4"]
                );
            }
            other => panic!("expected Trigger, got {other:?}"),
        }
    }

    #[test]
    fn backlog_prompt_composes_titles_and_descriptions() {
        let t1 = task("t1", "ws", 1);
        let t2 = task("t2", "ws", 2);
        let prompt = backlog_prompt(&[t1, t2]);
        assert!(prompt.contains("清理看板积压任务"));
        assert!(prompt.contains("- task-t1：first backlog item"));
        // t2 has no description — title only, no dangling separator.
        assert!(prompt.contains("- task-t2\n") || prompt.ends_with("- task-t2"));
    }
}
