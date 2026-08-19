//! Scheduled prompts — the "schedules" surface in the dashboard.
//!
//! Jobs are stored as a JSON array in `~/.mimi/crons.json`:
//!
//!   { id, name, schedule, prompt, description, enabled, last_run, last_status }
//!
//! `schedule` is a standard 5-field cron expression (minute hour dom month
//! dow) interpreted in the host's local timezone, matching the semantics of
//! the system crontab Mimi already runs `reflect`/`audit`/`update` from.
//!
//! `run_scheduler()` is spawned by the dashboard server and wakes once a
//! minute to fire whatever is due. A due job runs `claude --print` in
//! `~/.mimi` — the same one-shot invocation `mimi reflect` and `mimi audit`
//! use — with its transcript appended to `~/.mimi/cron_logs/<id>.log`.
//!
//! Missed minutes are NOT replayed. If the dashboard is down for an hour, the
//! jobs that fell in that hour are skipped rather than fired in a burst on
//! restart — same as cron(8).

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::paths;

/// Hard ceiling on a single job. A prompt that wedges shouldn't hold its slot
/// forever and block every later firing of the same schedule.
const JOB_TIMEOUT_SECS: u64 = 900;

/// crons.json is a read-modify-write replace shared by the dashboard's CRUD
/// handlers and the scheduler's last_run bookkeeping. Serialize every access.
static FILE_LOCK: Mutex<()> = Mutex::new(());

/// Ids currently executing, so a job that outruns its own interval doesn't
/// stack up concurrent copies of itself.
fn in_flight() -> &'static Mutex<HashSet<String>> {
    static INSTANCE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(HashSet::new()))
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub prompt: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// When the scheduler last fired this job (set at launch, not completion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<DateTime<Utc>>,
    /// Outcome of that run: "ok", "failed (exit 1)", "timed out", …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
}

fn default_enabled() -> bool {
    true
}

// --- Storage ---

fn crons_path() -> PathBuf {
    paths::home().join("crons.json")
}

fn log_path(id: &str) -> PathBuf {
    paths::cron_logs_dir().join(format!("{id}.log"))
}

pub fn load() -> Vec<CronJob> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    load_unlocked()
}

fn load_unlocked() -> Vec<CronJob> {
    std::fs::read_to_string(crons_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_unlocked(crons: &[CronJob]) -> Result<(), String> {
    paths::ensure_dirs();
    let json = serde_json::to_string_pretty(crons).map_err(|e| e.to_string())?;
    std::fs::write(crons_path(), json).map_err(|e| e.to_string())
}

pub fn create(
    name: String,
    schedule: String,
    prompt: String,
    description: String,
) -> Result<CronJob, String> {
    // Reject a malformed expression up front rather than storing a job that
    // can never match — that failure mode is exactly what this module fixes.
    Schedule::parse(&schedule)?;
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut crons = load_unlocked();
    let job = CronJob {
        id: format!("{}", Utc::now().timestamp_millis()),
        name,
        schedule,
        prompt,
        description,
        enabled: true,
        last_run: None,
        last_status: None,
    };
    crons.push(job.clone());
    save_unlocked(&crons)?;
    Ok(job)
}

pub fn delete(id: &str) -> Result<(), String> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut crons = load_unlocked();
    let before = crons.len();
    crons.retain(|c| c.id != id);
    if crons.len() == before {
        return Err(format!("cron {id} not found"));
    }
    save_unlocked(&crons)
}

pub fn toggle(id: &str) -> Result<bool, String> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut crons = load_unlocked();
    let job = crons
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("cron {id} not found"))?;
    job.enabled = !job.enabled;
    let enabled = job.enabled;
    save_unlocked(&crons)?;
    Ok(enabled)
}

/// Re-read, mutate one job, write back — under the lock, so a concurrent
/// dashboard edit can't be clobbered by the scheduler's bookkeeping.
fn patch(id: &str, f: impl FnOnce(&mut CronJob)) {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut crons = load_unlocked();
    let Some(job) = crons.iter_mut().find(|c| c.id == id) else {
        return;
    };
    f(job);
    if let Err(e) = save_unlocked(&crons) {
        eprintln!("crons: failed to persist {id}: {e}");
    }
}

// --- Cron expression parsing ---

/// One field of a cron expression, expanded into the concrete values it
/// accepts. `wildcard` is tracked separately because day-of-month and
/// day-of-week combine with OR when both are restricted (POSIX behaviour).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Field {
    values: Vec<u32>,
    wildcard: bool,
}

impl Field {
    fn parse(spec: &str, min: u32, max: u32, dow: bool) -> Result<Field, String> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err("empty field".into());
        }
        let mut values = Vec::new();
        for part in spec.split(',') {
            let part = part.trim();
            let (range, step) = match part.split_once('/') {
                Some((r, s)) => {
                    let step: u32 = s
                        .parse()
                        .map_err(|_| format!("bad step in '{part}'"))?;
                    if step == 0 {
                        return Err(format!("step must be > 0 in '{part}'"));
                    }
                    (r, step)
                }
                None => (part, 1),
            };
            // `*` and `a-b` are ranges; a bare `a` is a single value, except
            // with a step (`5/10`), where cron reads it as `5-max/10`.
            let (lo, hi) = if range == "*" {
                (min, max)
            } else if let Some((a, b)) = range.split_once('-') {
                (parse_value(a, dow)?, parse_value(b, dow)?)
            } else {
                let v = parse_value(range, dow)?;
                if step > 1 { (v, max) } else { (v, v) }
            };
            if lo > hi {
                return Err(format!("descending range '{range}'"));
            }
            if lo < min || hi > max {
                return Err(format!("'{range}' out of range {min}-{max}"));
            }
            values.extend((lo..=hi).step_by(step as usize));
        }
        // Sunday is both 0 and 7 in crontab(5); normalize so matching is a
        // plain lookup against chrono's 0=Sunday numbering.
        if dow {
            for v in &mut values {
                if *v == 7 {
                    *v = 0;
                }
            }
        }
        values.sort_unstable();
        values.dedup();
        Ok(Field {
            values,
            wildcard: spec == "*",
        })
    }

    fn contains(&self, v: u32) -> bool {
        self.values.binary_search(&v).is_ok()
    }
}

fn parse_value(s: &str, dow: bool) -> Result<u32, String> {
    let s = s.trim();
    // Day-of-week accepts 0-7 where 7 aliases Sunday; the caller folds it.
    let max = if dow { 7 } else { u32::MAX };
    let v: u32 = s.parse().map_err(|_| format!("'{s}' is not a number"))?;
    if v > max {
        return Err(format!("'{s}' out of range"));
    }
    Ok(v)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    minute: Field,
    hour: Field,
    dom: Field,
    month: Field,
    dow: Field,
}

impl Schedule {
    pub fn parse(expr: &str) -> Result<Schedule, String> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!(
                "expected 5 cron fields (minute hour day month weekday), got {}",
                fields.len()
            ));
        }
        Ok(Schedule {
            minute: Field::parse(fields[0], 0, 59, false).map_err(|e| format!("minute: {e}"))?,
            hour: Field::parse(fields[1], 0, 23, false).map_err(|e| format!("hour: {e}"))?,
            dom: Field::parse(fields[2], 1, 31, false).map_err(|e| format!("day: {e}"))?,
            month: Field::parse(fields[3], 1, 12, false).map_err(|e| format!("month: {e}"))?,
            dow: Field::parse(fields[4], 0, 7, true).map_err(|e| format!("weekday: {e}"))?,
        })
    }

    /// True if `dt` falls in a minute this schedule selects.
    pub fn matches<Tz: TimeZone>(&self, dt: &DateTime<Tz>) -> bool {
        if !self.minute.contains(dt.minute())
            || !self.hour.contains(dt.hour())
            || !self.month.contains(dt.month())
        {
            return false;
        }
        let dom_ok = self.dom.contains(dt.day());
        let dow_ok = self.dow.contains(dt.weekday().num_days_from_sunday());
        // crontab(5): when both day fields are restricted the job runs if
        // EITHER matches; otherwise the restricted one decides.
        match (self.dom.wildcard, self.dow.wildcard) {
            (true, true) => true,
            (true, false) => dow_ok,
            (false, true) => dom_ok,
            (false, false) => dom_ok || dow_ok,
        }
    }
}

// --- Execution ---

/// Fire every enabled job whose schedule selects `now` and that hasn't
/// already run in this minute. Returns the ids launched.
pub fn tick(now: DateTime<Local>) -> Vec<String> {
    let mut fired = Vec::new();
    for job in load() {
        if !job.enabled {
            continue;
        }
        let schedule = match Schedule::parse(&job.schedule) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("crons: job '{}' has an invalid schedule: {e}", job.name);
                continue;
            }
        };
        if !schedule.matches(&now) {
            continue;
        }
        // A restart mid-minute must not re-fire something already launched.
        if let Some(last) = job.last_run {
            if same_minute(&last.with_timezone(&Local), &now) {
                continue;
            }
        }
        match launch(job.clone()) {
            Ok(()) => fired.push(job.id),
            Err(e) => eprintln!("crons: job '{}': {e}", job.name),
        }
    }
    fired
}

/// Why a job could not be launched. Callers map this onto their own surface
/// — the dashboard turns it into 404 vs 409, the scheduler just logs it.
#[derive(Debug, PartialEq, Eq)]
pub enum LaunchError {
    NotFound,
    AlreadyRunning,
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::NotFound => write!(f, "no such schedule"),
            LaunchError::AlreadyRunning => {
                write!(f, "still running from a previous firing — skipping")
            }
        }
    }
}

/// Start a job on a background task. Records the launch immediately so a
/// crash mid-job costs one firing rather than re-firing every minute after
/// restart, and refuses to stack a second copy on top of a slow one.
fn launch(job: CronJob) -> Result<(), LaunchError> {
    if !claim(&job.id) {
        return Err(LaunchError::AlreadyRunning);
    }
    patch(&job.id, |j| {
        j.last_run = Some(Utc::now());
        j.last_status = Some("running".into());
    });
    tokio::spawn(async move {
        let status = execute(&job).await;
        patch(&job.id, |j| j.last_status = Some(status));
        release(&job.id);
    });
    Ok(())
}

/// Fire a job now regardless of its schedule or enabled flag. Used by the
/// dashboard's "run now" control; returns as soon as the job is launched.
pub fn trigger(id: &str) -> Result<(), LaunchError> {
    launch(find(id).ok_or(LaunchError::NotFound)?)
}

/// Look a job up by id, falling back to name so the CLI stays typeable.
fn find(id: &str) -> Option<CronJob> {
    load().into_iter().find(|c| c.id == id || c.name == id)
}

/// Read a job's accumulated run log.
pub fn read_log(id: &str) -> Result<String, String> {
    let job = find(id).ok_or_else(|| format!("no schedule with id or name '{id}'"))?;
    std::fs::read_to_string(log_path(&job.id))
        .map_err(|e| format!("no log for '{}' yet: {e}", job.name))
}

fn same_minute(a: &DateTime<Local>, b: &DateTime<Local>) -> bool {
    a.date_naive() == b.date_naive() && a.hour() == b.hour() && a.minute() == b.minute()
}

fn claim(id: &str) -> bool {
    in_flight()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.to_string())
}

fn release(id: &str) {
    in_flight()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(id);
}

/// Run one job's prompt through a one-shot `claude` session, streaming the
/// transcript straight into the job's log. Returns the status to persist.
async fn execute(job: &CronJob) -> String {
    paths::ensure_dirs();
    let log = log_path(&job.id);
    append_log(
        &log,
        &format!(
            "\n===== {} · {} =====\n",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            job.name
        ),
    );

    // Hand the log file to the child directly rather than piping: output
    // lands in the log as it is produced, and a chatty 15-minute job doesn't
    // sit buffered in the dashboard's memory until it exits.
    let out_file = match std::fs::OpenOptions::new().create(true).append(true).open(&log) {
        Ok(f) => f,
        Err(e) => return format!("cannot open log: {e}"),
    };
    let err_file = match out_file.try_clone() {
        Ok(f) => f,
        Err(e) => return format!("cannot open log: {e}"),
    };

    let child = tokio::process::Command::new("claude")
        .args(["--print", "--dangerously-skip-permissions", &job.prompt])
        .current_dir(paths::home())
        .stdin(Stdio::null())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file))
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("spawn failed: {e}");
            append_log(&log, &format!("{msg}\n"));
            return msg;
        }
    };

    let timeout = std::time::Duration::from_secs(JOB_TIMEOUT_SECS);
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            let msg = format!("failed: {e}");
            append_log(&log, &format!("{msg}\n"));
            return msg;
        }
        Err(_) => {
            let _ = child.kill().await;
            let msg = format!("timed out after {JOB_TIMEOUT_SECS}s");
            append_log(&log, &format!("\n[{msg} — killed]\n"));
            return msg;
        }
    };

    if status.success() {
        "ok".into()
    } else {
        format!("failed (exit {})", status.code().unwrap_or(-1))
    }
}

fn append_log(path: &PathBuf, text: &str) {
    use std::io::Write;
    let f = std::fs::OpenOptions::new().create(true).append(true).open(path);
    if let Ok(mut f) = f {
        let _ = f.write_all(text.as_bytes());
    }
}

/// Wake once a minute and fire whatever is due. Spawned by the dashboard
/// server, which runs as the always-on `mimi-dashboard` systemd unit.
pub async fn run_scheduler() {
    println!("Scheduler started — {} job(s) configured", load().len());
    loop {
        sleep_to_next_minute().await;
        let now = Local::now();
        for id in tick(now) {
            println!("[{}] fired schedule {id}", now.format("%H:%M"));
        }
    }
}

/// Sleep until a couple of seconds past the next minute boundary, so a tick
/// always lands inside the minute it is evaluating.
async fn sleep_to_next_minute() {
    let now = Local::now();
    let secs_into_minute = now.second() as u64;
    let nanos = now.timestamp_subsec_nanos() as u64;
    let wait = std::time::Duration::from_secs(60 - secs_into_minute + 2)
        - std::time::Duration::from_nanos(nanos);
    tokio::time::sleep(wait).await;
}

// --- CLI entry points ---

pub fn cli_list() {
    let crons = load();
    if crons.is_empty() {
        println!("(no schedules — add one from the dashboard)");
        return;
    }
    println!(
        "{:<16} {:<16} {:<9} {:<18} {}",
        "ID", "SCHEDULE", "ENABLED", "LAST RUN", "NAME"
    );
    for c in crons {
        let last = c
            .last_run
            .map(|t| {
                t.with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|| "never".into());
        println!(
            "{:<16} {:<16} {:<9} {:<18} {} {}",
            c.id,
            c.schedule,
            if c.enabled { "yes" } else { "no" },
            last,
            c.name,
            c.last_status
                .map(|s| format!("({s})"))
                .unwrap_or_default(),
        );
    }
}

/// Evaluate all schedules once against the current minute. Intended for a
/// `* * * * *` system-crontab entry if you'd rather not rely on the dashboard.
pub async fn cli_tick() {
    let fired = tick(Local::now());
    if fired.is_empty() {
        println!("nothing due");
        return;
    }
    println!("fired {} job(s): {}", fired.len(), fired.join(", "));
    // Jobs run on spawned tasks; wait for them so the process doesn't exit
    // out from under a job that was just launched.
    while !in_flight().lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// Run a single job now, in the foreground, ignoring its schedule.
pub async fn cli_run(id: &str) {
    let Some(job) = find(id) else {
        eprintln!("no schedule with id or name '{id}'");
        std::process::exit(1);
    };
    println!("Running '{}'...", job.name);
    let started = Utc::now();
    let status = execute(&job).await;
    patch(&job.id, |j| {
        j.last_run = Some(started);
        j.last_status = Some(status.clone());
    });
    println!("{status} — log: {}", log_path(&job.id).display());
    if status != "ok" {
        std::process::exit(1);
    }
}

/// Print a job's accumulated run log.
pub fn cli_logs(id: &str) {
    match read_log(id) {
        Ok(s) => print!("{s}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
            .and_utc()
    }

    #[test]
    fn every_minute() {
        let s = Schedule::parse("* * * * *").unwrap();
        assert!(s.matches(&at(2026, 8, 19, 3, 7)));
    }

    #[test]
    fn step_and_list() {
        let s = Schedule::parse("*/10 * * * *").unwrap();
        assert!(s.matches(&at(2026, 8, 19, 3, 0)));
        assert!(s.matches(&at(2026, 8, 19, 3, 30)));
        assert!(!s.matches(&at(2026, 8, 19, 3, 31)));

        let s = Schedule::parse("0,15,45 * * * *").unwrap();
        assert!(s.matches(&at(2026, 8, 19, 3, 45)));
        assert!(!s.matches(&at(2026, 8, 19, 3, 30)));
    }

    #[test]
    fn daily_at_hour() {
        let s = Schedule::parse("30 3 * * *").unwrap();
        assert!(s.matches(&at(2026, 8, 19, 3, 30)));
        assert!(!s.matches(&at(2026, 8, 19, 4, 30)));
    }

    #[test]
    fn ranges_and_offset_steps() {
        let s = Schedule::parse("0 9-17 * * *").unwrap();
        assert!(s.matches(&at(2026, 8, 19, 9, 0)));
        assert!(s.matches(&at(2026, 8, 19, 17, 0)));
        assert!(!s.matches(&at(2026, 8, 19, 18, 0)));

        // `5/10` means 5,15,25,... — the same reading crontab(5) gives it.
        let s = Schedule::parse("5/10 * * * *").unwrap();
        assert!(s.matches(&at(2026, 8, 19, 0, 5)));
        assert!(s.matches(&at(2026, 8, 19, 0, 55)));
        assert!(!s.matches(&at(2026, 8, 19, 0, 10)));
    }

    #[test]
    fn weekday_names_are_numeric_and_sunday_is_both_0_and_7() {
        // 2026-08-19 is a Wednesday, 2026-08-23 a Sunday.
        let s = Schedule::parse("0 1 * * 2-6").unwrap();
        assert!(s.matches(&at(2026, 8, 19, 1, 0)));
        assert!(!s.matches(&at(2026, 8, 23, 1, 0)));

        for expr in ["0 1 * * 0", "0 1 * * 7"] {
            let s = Schedule::parse(expr).unwrap();
            assert!(s.matches(&at(2026, 8, 23, 1, 0)), "{expr}");
            assert!(!s.matches(&at(2026, 8, 19, 1, 0)), "{expr}");
        }
    }

    #[test]
    fn dom_and_dow_are_ored_when_both_restricted() {
        // 1st of the month OR any Sunday.
        let s = Schedule::parse("0 0 1 * 0").unwrap();
        assert!(s.matches(&at(2026, 8, 1, 0, 0))); // Saturday the 1st
        assert!(s.matches(&at(2026, 8, 23, 0, 0))); // a Sunday
        assert!(!s.matches(&at(2026, 8, 19, 0, 0))); // neither
    }

    #[test]
    fn rejects_malformed_expressions() {
        for bad in [
            "* * * *",          // too few fields
            "* * * * * *",      // too many
            "60 * * * *",       // minute out of range
            "* 24 * * *",       // hour out of range
            "* * 0 * *",        // day-of-month is 1-based
            "*/0 * * * *",      // zero step
            "10-5 * * * *",     // descending range
            "abc * * * *",      // not a number
            "",                 // empty
        ] {
            assert!(Schedule::parse(bad).is_err(), "should reject: {bad:?}");
        }
    }
}
