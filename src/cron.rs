//! Recurring prompt scheduler.
//!
//! The dashboard's "schedules" view writes jobs to `~/.mimi/crons.json`. This
//! module is what actually fires them: a tick loop wakes once per wall-clock
//! minute, matches each enabled job's cron expression against the current
//! local time, and runs due prompts as `claude --print` in `~/.mimi`.
//!
//! The scheduler is started by the dashboard server (`mimi dashboard`), which
//! is the always-on process on a Mimi host. `mimi cron start` runs the same
//! loop standalone for setups without the dashboard — run one or the other,
//! not both.
//!
//! Expressions are standard 5-field cron: `minute hour day-of-month month
//! day-of-week`, with `*`, `a`, `a-b`, `*/n`, `a-b/n`, `a/n` and comma lists.
//! Day-of-week is 0-7 (both 0 and 7 mean Sunday). When both day-of-month and
//! day-of-week are restricted a job fires when *either* matches, as Vixie
//! cron does.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, TimeDelta, Timelike};
use serde::{Deserialize, Serialize};

use crate::paths;

/// Where run output goes. Surfaced in the dashboard's logs view.
pub const LOG_PATH: &str = "/tmp/mimi-cron.log";

/// How far ahead `next_run` will look before giving up (a year of minutes).
const NEXT_RUN_HORIZON_MINS: i64 = 366 * 24 * 60;

/// Lines of a run's stdout to keep in the log.
const OUTPUT_TAIL_LINES: usize = 40;

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
    /// RFC3339 local timestamp of the last time the scheduler started this job.
    #[serde(default)]
    pub last_run: Option<String>,
    /// Outcome of the last run: `"ok"`, `"running"`, or `"failed: …"`.
    #[serde(default)]
    pub last_status: Option<String>,
}

fn default_enabled() -> bool {
    true
}

// --- Storage ---
//
// crons.json is rewritten wholesale on every mutation, so all writers take
// this lock. Critical sections are a read + a write of a file with a handful
// of entries — short enough that a blocking mutex is fine on the async side.

fn file_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn path() -> PathBuf {
    paths::home().join("crons.json")
}

pub fn load() -> Vec<CronJob> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(jobs: &[CronJob]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(jobs).map_err(|e| e.to_string())?;
    std::fs::write(path(), json).map_err(|e| e.to_string())
}

/// Read-modify-write a single job under the file lock.
fn update_job(id: &str, f: impl FnOnce(&mut CronJob)) {
    let _guard = file_lock();
    let mut jobs = load();
    let Some(job) = jobs.iter_mut().find(|j| j.id == id) else {
        return;
    };
    f(job);
    if let Err(e) = save(&jobs) {
        eprintln!("cron: failed to persist {id}: {e}");
    }
}

// --- Schedule parsing ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    minute: Vec<bool>,
    hour: Vec<bool>,
    dom: Vec<bool>,
    month: Vec<bool>,
    dow: Vec<bool>,
    dom_restricted: bool,
    dow_restricted: bool,
}

impl Schedule {
    pub fn parse(expr: &str) -> Result<Schedule, String> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!(
                "expected 5 fields (minute hour day-of-month month day-of-week), got {}",
                fields.len()
            ));
        }
        let mut dow = parse_field(fields[4], 0, 7, "day-of-week")?;
        // Both 0 and 7 mean Sunday; fold 7 onto 0 so matching can key off
        // `num_days_from_sunday()`.
        if dow[7] {
            dow[0] = true;
        }
        dow.truncate(7);

        Ok(Schedule {
            minute: parse_field(fields[0], 0, 59, "minute")?,
            hour: parse_field(fields[1], 0, 23, "hour")?,
            dom: parse_field(fields[2], 1, 31, "day-of-month")?,
            month: parse_field(fields[3], 1, 12, "month")?,
            dow,
            dom_restricted: fields[2] != "*",
            dow_restricted: fields[4] != "*",
        })
    }

    pub fn matches(&self, dt: &DateTime<Local>) -> bool {
        if !self.minute[dt.minute() as usize]
            || !self.hour[dt.hour() as usize]
            || !self.month[dt.month() as usize]
        {
            return false;
        }
        let dom = self.dom[dt.day() as usize];
        let dow = self.dow[dt.weekday().num_days_from_sunday() as usize];
        match (self.dom_restricted, self.dow_restricted) {
            (true, true) => dom || dow,
            (true, false) => dom,
            (false, true) => dow,
            (false, false) => true,
        }
    }

    /// First minute strictly after `after` that this schedule fires on.
    /// `None` if nothing matches within a year (e.g. `0 0 30 2 *`).
    pub fn next_run(&self, after: DateTime<Local>) -> Option<DateTime<Local>> {
        let start = after
            .with_second(0)?
            .with_nanosecond(0)?
            .checked_add_signed(TimeDelta::minutes(1))?;
        (0..NEXT_RUN_HORIZON_MINS)
            .filter_map(|i| start.checked_add_signed(TimeDelta::minutes(i)))
            .find(|dt| self.matches(dt))
    }
}

/// Expand one cron field into a `min..=max`-indexed membership table.
/// Index 0 is always present so callers can index by the natural value
/// (month 1-12, day 1-31) without offsetting.
fn parse_field(spec: &str, min: u32, max: u32, name: &str) -> Result<Vec<bool>, String> {
    let mut set = vec![false; max as usize + 1];
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(format!("{name}: empty term in {spec:?}"));
        }
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => {
                let step: u32 = step
                    .parse()
                    .map_err(|_| format!("{name}: bad step {step:?}"))?;
                if step == 0 {
                    return Err(format!("{name}: step must be greater than 0"));
                }
                (range, step)
            }
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((lo, hi)) = range.split_once('-') {
            (
                parse_value(lo, min, max, name)?,
                parse_value(hi, min, max, name)?,
            )
        } else {
            let value = parse_value(range, min, max, name)?;
            // `5/15` is the Vixie shorthand for `5-max/15`; a bare `5` is a
            // single value.
            if step == 1 {
                (value, value)
            } else {
                (value, max)
            }
        };
        if lo > hi {
            return Err(format!("{name}: range {lo}-{hi} is inverted"));
        }
        let mut value = lo;
        while value <= hi {
            set[value as usize] = true;
            value += step;
        }
    }
    Ok(set)
}

fn parse_value(raw: &str, min: u32, max: u32, name: &str) -> Result<u32, String> {
    let value: u32 = raw
        .trim()
        .parse()
        .map_err(|_| format!("{name}: {raw:?} is not a number"))?;
    if value < min || value > max {
        return Err(format!("{name}: {value} out of range {min}-{max}"));
    }
    Ok(value)
}

// --- Scheduler ---

/// Tick once per wall-clock minute, forever.
pub async fn scheduler() {
    log_line(&format!(
        "scheduler started (jobs file: {})",
        path().display()
    ));
    loop {
        sleep_to_next_minute().await;
        tick(Local::now());
    }
}

/// Fire everything due at `now`.
fn tick(now: DateTime<Local>) {
    for job in due_jobs(now) {
        spawn_job(job);
    }
}

/// Fire a job now, ignoring its schedule. Used by the dashboard's "run now".
pub fn run_now(id: &str) -> Result<(), String> {
    let job = load()
        .into_iter()
        .find(|j| j.id == id)
        .ok_or_else(|| format!("no schedule with id {id}"))?;
    spawn_job(job);
    Ok(())
}

/// Mark a job started *before* spawning it, so a run that outlives its minute
/// can't be picked up a second time by the next tick.
fn spawn_job(job: CronJob) {
    update_job(&job.id, |j| {
        j.last_run = Some(Local::now().to_rfc3339());
        j.last_status = Some("running".into());
    });
    tokio::spawn(run_job(job));
}

fn due_jobs(now: DateTime<Local>) -> Vec<CronJob> {
    let this_minute = now.format("%Y-%m-%dT%H:%M").to_string();
    load()
        .into_iter()
        .filter(|job| {
            if !job.enabled {
                return false;
            }
            // Belt and braces against a double-fire if a tick lands twice in
            // the same minute (clock adjustment, drifted sleep).
            if job
                .last_run
                .as_deref()
                .is_some_and(|t| t.starts_with(&this_minute))
            {
                return false;
            }
            match Schedule::parse(&job.schedule) {
                Ok(schedule) => schedule.matches(&now),
                Err(e) => {
                    log_line(&format!(
                        "skipping {} — invalid schedule {:?}: {e}",
                        job.name, job.schedule
                    ));
                    false
                }
            }
        })
        .collect()
}

async fn run_job(job: CronJob) {
    log_line(&format!("▶ {} [{}] ({})", job.name, job.id, job.schedule));
    let started = Local::now();

    let result = tokio::process::Command::new("claude")
        .args(["--print", "--dangerously-skip-permissions", &job.prompt])
        .current_dir(paths::home())
        .stdin(Stdio::null())
        .output()
        .await;

    let elapsed = (Local::now() - started).num_seconds();
    let status = match result {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            log_line(&format!(
                "✓ {} finished in {elapsed}s\n{}",
                job.name,
                tail(&stdout, OUTPUT_TAIL_LINES)
            ));
            "ok".to_string()
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let code = out
                .status
                .code()
                .map_or_else(|| "signal".to_string(), |c| c.to_string());
            log_line(&format!(
                "✗ {} exited {code} after {elapsed}s\n{}",
                job.name,
                tail(&stderr, OUTPUT_TAIL_LINES)
            ));
            format!("failed: exit {code}")
        }
        Err(e) => {
            log_line(&format!("✗ {} could not start claude: {e}", job.name));
            format!("failed: {e}")
        }
    };

    update_job(&job.id, |j| j.last_status = Some(status));
}

async fn sleep_to_next_minute() {
    let now = Local::now();
    let secs = 60 - u64::from(now.second());
    // Land a beat past the boundary so `Local::now()` inside the tick is
    // reliably in the new minute.
    tokio::time::sleep(Duration::from_secs(secs) + Duration::from_millis(250)).await;
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.trim_end().lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

fn log_line(message: &str) {
    let line = format!("[{}] {message}\n", Local::now().to_rfc3339());
    eprint!("{line}");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(LOG_PATH) {
        let _ = f.write_all(line.as_bytes());
    }
}

// --- CLI entry points ---

pub async fn cli_start() {
    println!("Mimi cron scheduler running — logging to {LOG_PATH}");
    scheduler().await;
}

pub fn cli_list() {
    let jobs = load();
    if jobs.is_empty() {
        println!("(no schedules)");
        return;
    }
    let now = Local::now();
    println!(
        "{:<15} {:<16} {:<8} {:<12} {:<12} {:<16} {}",
        "ID", "SCHEDULE", "ENABLED", "LAST RUN", "NEXT RUN", "LAST STATUS", "NAME"
    );
    for job in jobs {
        let (next, note) = match Schedule::parse(&job.schedule) {
            Ok(s) if job.enabled => (
                s.next_run(now)
                    .map_or_else(|| "never".into(), |dt| dt.format("%m-%d %H:%M").to_string()),
                String::new(),
            ),
            Ok(_) => ("paused".to_string(), String::new()),
            Err(e) => ("invalid".to_string(), format!("  ← {e}")),
        };
        let last = job.last_run.as_deref().map_or("-".to_string(), |t| {
            DateTime::parse_from_rfc3339(t)
                .map_or_else(|_| t.to_string(), |dt| dt.format("%m-%d %H:%M").to_string())
        });
        println!(
            "{:<15} {:<16} {:<8} {:<12} {:<12} {:<16} {}{}",
            job.id,
            job.schedule,
            job.enabled,
            last,
            next,
            job.last_status.as_deref().unwrap_or("-"),
            job.name,
            note
        );
    }
}

/// Run a job now, inheriting stdio so the operator sees the output live.
pub fn cli_run(id: &str) {
    let Some(job) = load().into_iter().find(|j| j.id == id || j.name == id) else {
        eprintln!("no schedule with id or name {id:?}");
        std::process::exit(1);
    };
    println!("Running {} [{}]...\n", job.name, job.id);
    update_job(&job.id, |j| {
        j.last_run = Some(Local::now().to_rfc3339());
        j.last_status = Some("running".into());
    });

    let status = std::process::Command::new("claude")
        .args(["--print", "--dangerously-skip-permissions", &job.prompt])
        .current_dir(paths::home())
        .status();

    let outcome = match status {
        Ok(s) if s.success() => "ok".to_string(),
        Ok(s) => format!("failed: exit {}", s.code().unwrap_or(-1)),
        Err(e) => format!("failed: {e}"),
    };
    update_job(&job.id, |j| j.last_status = Some(outcome.clone()));

    if outcome != "ok" {
        eprintln!("\n{} {}", job.name, outcome);
        std::process::exit(1);
    }
    println!("\n{} ok", job.name);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    #[test]
    fn every_five_minutes() {
        let s = Schedule::parse("*/5 * * * *").unwrap();
        assert!(s.matches(&at(2026, 8, 24, 13, 0)));
        assert!(s.matches(&at(2026, 8, 24, 13, 55)));
        assert!(!s.matches(&at(2026, 8, 24, 13, 7)));
    }

    #[test]
    fn daily_at_three() {
        let s = Schedule::parse("0 3 * * *").unwrap();
        assert!(s.matches(&at(2026, 8, 24, 3, 0)));
        assert!(!s.matches(&at(2026, 8, 24, 3, 1)));
        assert!(!s.matches(&at(2026, 8, 24, 4, 0)));
    }

    #[test]
    fn lists_ranges_and_stepped_ranges() {
        let s = Schedule::parse("0,30 9-17 * * *").unwrap();
        assert!(s.matches(&at(2026, 8, 24, 9, 30)));
        assert!(s.matches(&at(2026, 8, 24, 17, 0)));
        assert!(!s.matches(&at(2026, 8, 24, 18, 0)));

        let s = Schedule::parse("0 0-12/6 * * *").unwrap();
        for hour in [0, 6, 12] {
            assert!(s.matches(&at(2026, 8, 24, hour, 0)));
        }
        assert!(!s.matches(&at(2026, 8, 24, 18, 0)));
    }

    #[test]
    fn sunday_is_zero_or_seven() {
        // 2026-08-23 is a Sunday.
        for expr in ["0 9 * * 0", "0 9 * * 7"] {
            let s = Schedule::parse(expr).unwrap();
            assert!(s.matches(&at(2026, 8, 23, 9, 0)), "{expr}");
            assert!(!s.matches(&at(2026, 8, 24, 9, 0)), "{expr}");
        }
    }

    #[test]
    fn restricted_dom_and_dow_are_ored() {
        // Vixie semantics: the 1st OR any Monday. 2026-08-24 is a Monday.
        let s = Schedule::parse("0 9 1 * 1").unwrap();
        assert!(s.matches(&at(2026, 8, 1, 9, 0)));
        assert!(s.matches(&at(2026, 8, 24, 9, 0)));
        assert!(!s.matches(&at(2026, 8, 25, 9, 0)));

        // Only day-of-month restricted: day-of-week must not narrow it.
        let s = Schedule::parse("0 9 1 * *").unwrap();
        assert!(s.matches(&at(2026, 8, 1, 9, 0)));
        assert!(!s.matches(&at(2026, 8, 24, 9, 0)));
    }

    #[test]
    fn rejects_malformed_expressions() {
        for expr in [
            "* * * *",      // too few fields
            "60 * * * *",   // minute out of range
            "* 24 * * *",   // hour out of range
            "0 0 0 * *",    // day-of-month is 1-based
            "*/0 * * * *",  // zero step
            "10-5 * * * *", // inverted range
            "0 0 * * mon",  // names are not supported
            "0,, * * * *",  // empty term
        ] {
            assert!(
                Schedule::parse(expr).is_err(),
                "{expr:?} should be rejected"
            );
        }
    }

    #[test]
    fn next_run_skips_the_current_minute() {
        let s = Schedule::parse("0 3 * * *").unwrap();
        assert_eq!(
            s.next_run(at(2026, 8, 24, 3, 0)),
            Some(at(2026, 8, 25, 3, 0))
        );
        assert_eq!(
            s.next_run(at(2026, 8, 24, 2, 30)),
            Some(at(2026, 8, 24, 3, 0))
        );
    }

    #[test]
    fn next_run_gives_up_on_impossible_dates() {
        assert_eq!(
            Schedule::parse("0 0 30 2 *")
                .unwrap()
                .next_run(at(2026, 8, 24, 0, 0)),
            None
        );
    }
}
