//! Scheduler (SPEC ui-m1 D9 / AC3): a minimal cron subset plus an
//! `@every <seconds>` interval mode, and a cancellable runner that turns due
//! schedules into queued Tasks (+ `schedule_triggered` events).
//!
//! No third-party cron crate: the supported grammar is exactly
//! `min hour dom mon dow` with `*`, numbers, `,`, `-`, `*/n` per field —
//! computed on the `time` crate in UTC throughout.

use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use thiserror::Error;
use time::{OffsetDateTime, Time};
use tokio_util::sync::CancellationToken;

use crate::domain::{
    new_id, now_ms, AgentRefKind, ConversationKind, Schedule, ScheduleSessionMode,
    ScheduleTargetKind, Task, TaskStatus,
};
use crate::harness::bus::{Event, EventBus};
use crate::services::conversation_service;
use crate::store::{migrations, repos, Db, StoreError};

/// Upper bound for the minute-scan forward search (366 days).
const MAX_SCAN_MINUTES: usize = 366 * 24 * 60;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("bad schedule expression: {0}")]
    BadExpression(String),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("time error: {0}")]
    Time(#[from] time::error::ComponentRange),
}

/// One cron field: the set of accepted values plus whether it was an
/// unrestricted `*` (needed for the day-of-month/day-of-week OR rule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSet {
    values: BTreeSet<u32>,
    star: bool,
}

impl FieldSet {
    fn parse(spec: &str, min: u32, max: u32) -> Result<FieldSet, SchedulerError> {
        let mut values = BTreeSet::new();
        let mut star = false;
        for item in spec.split(',') {
            let item = item.trim();
            if item.is_empty() {
                return Err(SchedulerError::BadExpression(format!(
                    "empty field item in '{spec}'"
                )));
            }
            let (range_part, step) = match item.split_once('/') {
                Some((r, s)) => {
                    let n: u32 = s.parse().map_err(|_| {
                        SchedulerError::BadExpression(format!("bad step '{s}' in '{item}'"))
                    })?;
                    if n == 0 {
                        return Err(SchedulerError::BadExpression(format!(
                            "zero step in '{item}'"
                        )));
                    }
                    (r, n)
                }
                None => (item, 1),
            };
            let (lo, hi) = if range_part == "*" {
                star = true;
                (min, max)
            } else if let Some((a, b)) = range_part.split_once('-') {
                let lo: u32 = a.trim().parse().map_err(|_| {
                    SchedulerError::BadExpression(format!("bad number '{a}' in '{item}'"))
                })?;
                let hi: u32 = b.trim().parse().map_err(|_| {
                    SchedulerError::BadExpression(format!("bad number '{b}' in '{item}'"))
                })?;
                (lo, hi)
            } else {
                let v: u32 = range_part.parse().map_err(|_| {
                    SchedulerError::BadExpression(format!("bad number '{range_part}' in '{item}'"))
                })?;
                (v, v)
            };
            if lo < min || hi > max || lo > hi {
                return Err(SchedulerError::BadExpression(format!(
                    "value(s) of '{item}' outside [{min},{max}]"
                )));
            }
            let mut v = lo;
            while v <= hi {
                values.insert(v);
                v += step;
                if v > max {
                    break;
                }
            }
        }
        Ok(FieldSet { values, star })
    }

    fn contains(&self, v: u32) -> bool {
        self.values.contains(&v)
    }

    /// True when the field restricts the value (i.e. was not written `*`).
    fn restricted(&self) -> bool {
        !self.star
    }
}

/// Parsed 5-field cron expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronExpr {
    minute: FieldSet,
    hour: FieldSet,
    dom: FieldSet,
    mon: FieldSet,
    dow: FieldSet,
}

/// Either a cron expression or a fixed-interval trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleSpec {
    Cron(CronExpr),
    Every { seconds: i64 },
}

/// Parses `@every <seconds>` or a 5-field cron subset (UTC semantics).
pub fn parse_schedule(expr: &str) -> Result<ScheduleSpec, SchedulerError> {
    let trimmed = expr.trim();
    if let Some(rest) = trimmed.strip_prefix("@every") {
        let secs: i64 = rest
            .trim()
            .parse()
            .map_err(|_| SchedulerError::BadExpression(format!("bad '@every' value: {expr}")))?;
        if secs <= 0 {
            return Err(SchedulerError::BadExpression(format!(
                "'@every' must be positive: {expr}"
            )));
        }
        return Ok(ScheduleSpec::Every { seconds: secs });
    }
    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(SchedulerError::BadExpression(format!(
            "expected 5 fields, got {}: {expr}",
            fields.len()
        )));
    }
    Ok(ScheduleSpec::Cron(CronExpr {
        minute: FieldSet::parse(fields[0], 0, 59)?,
        hour: FieldSet::parse(fields[1], 0, 23)?,
        dom: FieldSet::parse(fields[2], 1, 31)?,
        mon: FieldSet::parse(fields[3], 1, 12)?,
        dow: FieldSet::parse(fields[4], 0, 6)?,
    }))
}

impl CronExpr {
    /// Whether `t` (UTC) fires under this expression. When both day-of-month
    /// and day-of-week are restricted, either may match (Vixie convention).
    pub fn matches(&self, t: OffsetDateTime) -> bool {
        if !self.minute.contains(t.minute() as u32)
            || !self.hour.contains(t.hour() as u32)
            || !self.mon.contains(u8::from(t.month()) as u32)
        {
            return false;
        }
        let dom_ok = self.dom.contains(u32::from(t.day()));
        // cron: 0 = Sunday … 6 = Saturday; time crate: Monday = 0 … Sunday = 6
        let dow_cron = (t.weekday().number_days_from_monday() + 1) % 7;
        let dow_ok = self.dow.contains(u32::from(dow_cron));
        match (self.dom.restricted(), self.dow.restricted()) {
            (true, true) => dom_ok || dow_ok,
            _ => dom_ok && dow_ok,
        }
    }
}

/// First fire strictly after `after`, in UTC.
pub fn next_after(spec: &ScheduleSpec, after: OffsetDateTime) -> Option<OffsetDateTime> {
    match spec {
        ScheduleSpec::Every { seconds } => after.checked_add(time::Duration::seconds(*seconds)),
        ScheduleSpec::Cron(expr) => {
            let mut t = after
                .replace_nanosecond(0)
                .and_then(|t| t.replace_second(0))
                .ok()?;
            for _ in 0..MAX_SCAN_MINUTES {
                t = t.checked_add(time::Duration::minutes(1))?;
                if expr.matches(t) {
                    return Some(t);
                }
            }
            None
        }
    }
}

/// Convenience: [`next_after`] over a raw expression string + unix-ms instant.
pub fn next_after_ms(expr: &str, after_unix_ms: i64) -> Result<Option<i64>, SchedulerError> {
    let spec = parse_schedule(expr)?;
    let after = unix_ms_to_dt(after_unix_ms)?;
    Ok(next_after(&spec, after).map(|dt| dt.unix_timestamp() * 1000))
}

/// Unix-ms → UTC datetime truncated to the minute (cron granularity;
/// `@every` intervals therefore carry sub-second jitter of < 1s).
fn unix_ms_to_dt(ms: i64) -> Result<OffsetDateTime, SchedulerError> {
    let secs = ms.div_euclid(1000);
    let day_secs = secs.rem_euclid(86_400);
    Ok(
        OffsetDateTime::from_unix_timestamp(secs)?.replace_time(Time::from_hms(
            (day_secs / 3600) as u8,
            ((day_secs % 3600) / 60) as u8,
            0,
        )?),
    )
}

/// Cancellable background loop scanning due schedules.
pub struct SchedulerRunner {
    db_path: Arc<str>,
    check_interval: Duration,
    bus: Option<EventBus>,
}

impl SchedulerRunner {
    pub fn new(db_path: impl Into<Arc<str>>, check_interval: Duration) -> Self {
        Self {
            db_path: db_path.into(),
            check_interval,
            bus: None,
        }
    }

    /// Attaches a bus so [`tick`] can publish `schedule.triggered` events.
    pub fn with_bus(mut self, bus: EventBus) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Spawns the scan loop; drop/cancel the handle to stop it.
    pub fn spawn(self) -> SchedulerHandle {
        let token = CancellationToken::new();
        let join =
            tokio::spawn(run_loop(self.db_path, self.check_interval, self.bus, token.clone()));
        SchedulerHandle { token, join }
    }
}

/// Handle over one spawned scheduler loop.
pub struct SchedulerHandle {
    token: CancellationToken,
    join: tokio::task::JoinHandle<u64>,
}

impl SchedulerHandle {
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    /// Cancels and awaits the loop, returning how many triggers it fired.
    pub async fn cancel(self) -> u64 {
        self.token.cancel();
        self.join.await.unwrap_or(0)
    }
}

async fn run_loop(
    db_path: Arc<str>,
    interval: Duration,
    bus: Option<EventBus>,
    token: CancellationToken,
) -> u64 {
    let mut fired_total = 0u64;
    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            _ = tokio::time::sleep(interval) => {}
        }
        match tick(&db_path, bus.as_ref()).await {
            Ok(n) => fired_total += n,
            Err(e) => tracing::warn!(error = %e, "scheduler tick failed"),
        }
    }
    fired_total
}

/// One scan pass: fire every enabled schedule whose `next_trigger_at <= now`.
/// Each fire creates a queued Task (with a session for chat/group targets),
/// appends a `schedule_triggered` event, publishes `schedule.triggered` on the
/// bus (if attached), and advances last/next trigger bookkeeping.
pub async fn tick(db_path: &Arc<str>, bus: Option<&EventBus>) -> Result<u64, SchedulerError> {
    let path = db_path.clone();
    let bus_owned = bus.cloned();
    let fired = tokio::task::spawn_blocking(move || -> Result<u64, SchedulerError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let conn = &db.0;
        let now = now_ms();
        let due = repos::tasks_runs::due_schedules(conn, now)?;
        let mut fired = 0u64;
        for s in due {
            let next_at = match next_after_ms(&s.cron_expr, now) {
                Ok(next) => next,
                Err(e) => {
                    tracing::warn!(schedule = %s.name, error = %e, "unparsable cron expr — disabling");
                    repos::tasks_runs::set_schedule_enabled(conn, &s.id, false, now)?;
                    continue;
                }
            };

            // Session creation: chat/group targets get a Scheduled conversation;
            // task targets keep the legacy session_id = None behaviour.
            let session_id = if s.target_kind != ScheduleTargetKind::Task {
                match (s.session_mode, &s.session_id) {
                    (ScheduleSessionMode::Reuse, Some(existing_sid)) => {
                        Some(existing_sid.clone())
                    }
                    _ => {
                        // ADR 0013: Schedule.agent 保留，但透传为 participants
                        // 写入 conversation_participants 表（而非 session.agent）。
                        let participants: Vec<(AgentRefKind, &str)> = s
                            .agent
                            .as_ref()
                            .map(|(k, id)| vec![(*k, id.as_str())])
                            .unwrap_or_default();
                        let session = conversation_service::create_conversation(
                            conn,
                            ConversationKind::Scheduled,
                            &s.task_title,
                            &participants,
                            s.team_id.as_deref(),
                            // Anchor the conversation to its schedule so a
                            // scheduled session can be traced back later.
                            Some(s.id.as_str()),
                        )?;
                        let new_sid = session.id.clone();
                        let active_ws = repos::workspaces::find_active(conn)?
                            .map(|e| e.id)
                            .unwrap_or_default();
                        if !active_ws.is_empty() {
                            repos::sessions::set_workspace_id(conn, &session.id, &active_ws)?;
                        }
                        if s.session_mode == ScheduleSessionMode::Reuse {
                            let mut updated = s.clone();
                            updated.session_id = Some(new_sid.clone());
                            updated.updated_at = now;
                            repos::tasks_runs::update_schedule(conn, &updated)?;
                        }
                        Some(new_sid)
                    }
                }
            } else {
                None
            };

            let task = Task {
                id: new_id(),
                session_id: session_id.clone(),
                title: s.task_title.clone(),
                description: s.task_description.clone(),
                status: TaskStatus::Queued,
                created_at: now,
                updated_at: now,
            };
            repos::tasks_runs::insert_task(conn, &task)?;
            repos::events::append(
                conn,
                "schedule",
                &s.id,
                "schedule_triggered",
                &json!({
                    "task_id": task.id,
                    "session_id": session_id,
                    "cron_expr": s.cron_expr,
                    "target_kind": s.target_kind.as_str(),
                    "auto_dispatch": s.auto_dispatch,
                }),
                now,
            )?;

            if let Some(bus) = bus_owned.as_ref() {
                bus.publish(Event::new(
                    "schedule.triggered",
                    json!({
                        "schedule_id": s.id,
                        "task_id": task.id,
                        "session_id": session_id,
                        "target_kind": s.target_kind.as_str(),
                        "agent_kind": s.agent.as_ref().map(|(k, _)| k.as_str()),
                        "agent_ref_id": s.agent.as_ref().map(|(_, id)| id.as_str()),
                        "team_id": s.team_id,
                        "auto_dispatch": s.auto_dispatch,
                        "task_title": s.task_title,
                        "task_description": s.task_description,
                    }),
                ));
            }

            repos::tasks_runs::mark_schedule_triggered(conn, &s.id, now, next_at)?;
            fired += 1;
        }
        Ok(fired)
    })
    .await
    .map_err(|e| SchedulerError::Store(join_err(e)))??;
    Ok(fired)
}

fn join_err(e: tokio::task::JoinError) -> StoreError {
    StoreError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

/// Unused today; kept so `Schedule` stays importable for callers building rows.
pub fn new_schedule_row(
    name: &str,
    cron_expr: &str,
    task_title: &str,
    task_description: &str,
    next_trigger_at: Option<i64>,
) -> Schedule {
    let now = now_ms();
    Schedule {
        id: new_id(),
        name: name.to_string(),
        cron_expr: cron_expr.to_string(),
        task_title: task_title.to_string(),
        task_description: task_description.to_string(),
        enabled: true,
        last_triggered_at: None,
        next_trigger_at,
        created_at: now,
        updated_at: now,
        target_kind: ScheduleTargetKind::Task,
        agent: None,
        team_id: None,
        session_mode: ScheduleSessionMode::PerTrigger,
        session_id: None,
        auto_dispatch: true,
    }
}

/// `FromStr` sugar so expressions can be validated inline.
impl FromStr for ScheduleSpec {
    type Err = SchedulerError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_schedule(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{Date, Month, PrimitiveDateTime};

    /// Test helper: builds a UTC datetime without the `macros` feature.
    fn dt((y, m, d): (i32, u8, u8), (hh, mm, ss): (u8, u8, u8)) -> OffsetDateTime {
        let date = Date::from_calendar_date(y, Month::try_from(m).expect("month 1-12"), d)
            .expect("valid date");
        let time = Time::from_hms(hh, mm, ss).expect("valid time");
        PrimitiveDateTime::new(date, time).assume_utc()
    }

    // ---------------- expression parsing (table-driven)

    #[test]
    fn table_parse_accepts_supported_grammar() {
        let cases = [
            ("* * * * *", 60usize),
            ("*/15 * * * *", 4),
            ("0,30 * * * *", 2),
            ("5-7 * * * *", 3),
            ("1-9/3 * * * *", 3), // 1,4,7
            ("0 9 * * 1-5", 1),   // minute field is a single value
            ("59 23 31 12 *", 1),
        ];
        for (expr, minute_values) in cases {
            match parse_schedule(expr).unwrap() {
                ScheduleSpec::Cron(c) => {
                    assert_eq!(c.minute.values.len(), minute_values, "{expr} minute set")
                }
                other => panic!("{expr} parsed as {other:?}"),
            }
        }
        // @every form
        assert_eq!(
            parse_schedule("@every 90").unwrap(),
            ScheduleSpec::Every { seconds: 90 }
        );
    }

    #[test]
    fn table_parse_rejects_malformed_expressions() {
        let bad = [
            "* * * *",     // 4 fields
            "* * * * * *", // 6 fields
            "61 * * * *",  // minute out of range
            "* 24 * * *",  // hour out of range
            "0 0 32 * *",  // dom out of range
            "0 0 * 13 *",  // month out of range
            "0 0 * * 7",   // dow out of range
            "*/0 * * * *", // zero step
            "a * * * *",   // non-number
            "@every 0",    // non-positive interval
            "@every -5",
            "@every later",
            "",
        ];
        for expr in bad {
            assert!(
                parse_schedule(expr).is_err(),
                "expected '{expr}' to be rejected"
            );
        }
    }

    // ---------------- matching + next_after computation

    #[test]
    fn wildcard_matches_any_minute() {
        let c = cron("* * * * *");
        assert!(c.matches(dt((2026, 8, 24), (7, 53, 0))));
        assert!(c.matches(dt((2026, 1, 1), (0, 0, 0))));
    }

    #[test]
    fn step_list_and_range_fields_match_selectively() {
        let step = cron("*/15 * * * *");
        assert!(step.matches(dt((2026, 8, 24), (10, 0, 0))));
        assert!(step.matches(dt((2026, 8, 24), (10, 45, 0))));
        assert!(!step.matches(dt((2026, 8, 24), (10, 16, 0))));

        let list = cron("5,35 * * * *");
        assert!(list.matches(dt((2026, 8, 24), (3, 5, 0))));
        assert!(!list.matches(dt((2026, 8, 24), (3, 6, 0))));

        let range = cron("20-25 * * * *");
        assert!(range.matches(dt((2026, 8, 24), (11, 22, 0))));
        assert!(!range.matches(dt((2026, 8, 24), (11, 26, 0))));
    }

    #[test]
    fn daily_at_hour_boundary_with_carry() {
        let nine = cron("0 9 * * *");
        assert!(nine.matches(dt((2026, 8, 24), (9, 0, 0))));
        assert!(!nine.matches(dt((2026, 8, 24), (9, 1, 0))));

        // next_after carries into the next day past the fire time
        let next = next_after(
            &parse_schedule("0 9 * * *").unwrap(),
            dt((2026, 8, 24), (10, 0, 0)),
        )
        .unwrap();
        assert_eq!(next, dt((2026, 8, 25), (9, 0, 0)));
        // same-day when still before the hour
        let next2 = next_after(
            &parse_schedule("0 9 * * *").unwrap(),
            dt((2026, 8, 24), (8, 59, 0)),
        )
        .unwrap();
        assert_eq!(next2, dt((2026, 8, 24), (9, 0, 0)));
    }

    #[test]
    fn month_rollover_and_short_month_boundaries() {
        let first = cron("0 0 1 * *");
        let next = next_after(
            &parse_schedule("0 0 1 * *").unwrap(),
            dt((2026, 1, 31), (12, 0, 0)),
        )
        .unwrap();
        assert_eq!(next, dt((2026, 2, 1), (0, 0, 0)));
        assert!(first.matches(next));

        // Mar 31 → Apr 1 (Feb handled above)
        let next2 = next_after(
            &parse_schedule("0 0 1 * *").unwrap(),
            dt((2026, 3, 31), (23, 59, 0)),
        )
        .unwrap();
        assert_eq!(next2, dt((2026, 4, 1), (0, 0, 0)));
    }

    #[test]
    fn day_of_week_mapping_sunday_is_zero() {
        // 2026-08-24 is a Monday. "0 0 * * 0" = Sundays.
        let sunday = cron("0 0 * * 0");
        assert!(!sunday.matches(dt((2026, 8, 24), (0, 0, 0))));
        assert!(sunday.matches(dt((2026, 8, 30), (0, 0, 0)))); // following Sunday
        let next = next_after(
            &parse_schedule("0 0 * * 0").unwrap(),
            dt((2026, 8, 24), (6, 0, 0)),
        )
        .unwrap();
        assert_eq!(next, dt((2026, 8, 30), (0, 0, 0)));
    }

    #[test]
    fn dom_and_dow_both_restricted_use_the_or_rule() {
        // Vixie convention: when both are restricted, either may match.
        let expr = parse_schedule("0 0 13 * 5").unwrap();
        let next = next_after(&expr, dt((2026, 8, 10), (0, 0, 0))).unwrap();
        // Aug 13 2026 is a Thursday → dom rule fires on the 13th itself.
        assert_eq!(next, dt((2026, 8, 13), (0, 0, 0)));
    }

    #[test]
    fn every_mode_adds_exact_seconds() {
        let spec = parse_schedule("@every 90").unwrap();
        let base = dt((2026, 8, 24), (10, 0, 30));
        assert_eq!(next_after(&spec, base), Some(dt((2026, 8, 24), (10, 2, 0))));
    }

    #[test]
    fn next_after_ms_roundtrips_through_unix_ms() {
        let now = now_ms();
        let next = next_after_ms("@every 60", now).unwrap().unwrap();
        // unix-ms conversion truncates to the minute, then @every 60 adds
        // exactly one interval.
        let minute_start = now.div_euclid(60_000) * 60_000;
        assert_eq!(next - minute_start, 60_000);
    }

    fn cron(expr: &str) -> CronExpr {
        match parse_schedule(expr).unwrap() {
            ScheduleSpec::Cron(c) => c,
            other => panic!("{expr} parsed as {other:?}"),
        }
    }

    // ---------------- runner integration

    async fn count_queued_tasks(path: &std::path::Path) -> usize {
        let conn = rusqlite::Connection::open(path).unwrap();
        migrations::run(&conn).unwrap();
        repos::tasks_runs::list_tasks_by_status(&conn, TaskStatus::Queued, 1000)
            .unwrap()
            .len()
    }

    #[tokio::test]
    async fn every_one_second_schedule_fires_repeatedly_then_cancel_stops_it() {
        let dir = tempfile::tempdir().unwrap();
        let db_file = dir.path().join("sched.db");
        {
            let conn = rusqlite::Connection::open(&db_file).unwrap();
            migrations::run(&conn).unwrap();
            let mut row = new_schedule_row("ticker", "@every 1", "tick", "", Some(now_ms()));
            row.enabled = true;
            repos::tasks_runs::insert_schedule(&conn, &row).unwrap();
        }

        let db_path: Arc<str> = Arc::from(db_file.to_string_lossy().to_string());
        let handle = SchedulerRunner::new(db_path.clone(), Duration::from_millis(150)).spawn();

        // ≥2 queued tasks appear well inside 2.5s.
        let deadline = std::time::Instant::now() + Duration::from_millis(2500);
        loop {
            let n = count_queued_tasks(dir.path().join("sched.db").as_path()).await;
            if n >= 2 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "scheduler produced only {n} tasks in time"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Cancel: counts freeze afterwards.
        handle.cancel().await;
        let frozen = count_queued_tasks(dir.path().join("sched.db").as_path()).await;
        tokio::time::sleep(Duration::from_millis(1600)).await;
        let after = count_queued_tasks(dir.path().join("sched.db").as_path()).await;
        assert_eq!(frozen, after, "cancelled scheduler must not fire again");

        // Events were appended per fire.
        let conn = rusqlite::Connection::open(&db_file).unwrap();
        let events = {
            let sched_id = repos::tasks_runs::list_schedules(&conn, 10).unwrap()[0]
                .id
                .clone();
            repos::events::list_by_aggregate(&conn, "schedule", &sched_id, None).unwrap()
        };
        assert!(events.len() >= 2);
        assert!(events.iter().all(|e| e.kind == "schedule_triggered"));
    }

    #[tokio::test]
    async fn disabled_schedule_never_fires() {
        let dir = tempfile::tempdir().unwrap();
        let db_file = dir.path().join("sched.db");
        {
            let conn = rusqlite::Connection::open(&db_file).unwrap();
            migrations::run(&conn).unwrap();
            let mut row = new_schedule_row("off", "@every 1", "tick", "", Some(now_ms()));
            row.enabled = false;
            repos::tasks_runs::insert_schedule(&conn, &row).unwrap();
        }
        let db_path: Arc<str> = Arc::from(db_file.to_string_lossy().to_string());
        // drive several ticks manually instead of spawning
        for _ in 0..3 {
            tick(&db_path, None).await.unwrap();
        }
        assert_eq!(
            count_queued_tasks(dir.path().join("sched.db").as_path()).await,
            0
        );
    }

    #[tokio::test]
    async fn manual_tick_creates_task_event_and_advances_trigger() {
        let dir = tempfile::tempdir().unwrap();
        let db_file = dir.path().join("sched.db");
        {
            let conn = rusqlite::Connection::open(&db_file).unwrap();
            migrations::run(&conn).unwrap();
            let row = new_schedule_row(
                "daily",
                "0 9 * * *",
                "morning report",
                "auto",
                Some(now_ms()),
            );
            repos::tasks_runs::insert_schedule(&conn, &row).unwrap();
        }
        let db_path: Arc<str> = Arc::from(db_file.to_string_lossy().to_string());
        let fired = tick(&db_path, None).await.unwrap();
        assert_eq!(fired, 1);

        let conn = rusqlite::Connection::open(&db_file).unwrap();
        let tasks = repos::tasks_runs::list_tasks_by_status(&conn, TaskStatus::Queued, 10).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "morning report");

        let sched = repos::tasks_runs::get_schedule_by_name(&conn, "daily").unwrap();
        assert!(sched.last_triggered_at.is_some());
        let next = sched.next_trigger_at.unwrap();
        assert!(next > now_ms() - 5_000);

        // second tick does nothing (trigger moved to tomorrow 09:00)
        let db_path2: Arc<str> = Arc::from(db_file.to_string_lossy().to_string());
        assert_eq!(tick(&db_path2, None).await.unwrap(), 0);
    }
}
