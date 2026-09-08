//! Scheduled prompts ("schedules" in the dashboard).
//!
//! The dashboard has always let you create a job — a name, a 5-field cron
//! expression, and a prompt — and stored it in `~/.mimi/crons.json`. Nothing
//! ever read that file back, so every job sat there showing a green "enabled"
//! light and never fired. This module is the missing half: a parser for the
//! expressions the UI produces, and a scheduler loop that runs due jobs
//! through `claude --print`.
//!
//! The loop lives in the `mimi dashboard` process (already an always-on
//! systemd unit) so schedules work with no extra setup; `mimi cron daemon`
//! runs the same loop standalone if you'd rather give it its own unit. Run one
//! or the other — two schedulers over the same crons.json will race to fire
//! the same job.
//!
//! Evaluation happens in the local system timezone by default — the same
//! clock the system crontab uses — or in `timezone` from `~/.mimi/config.json`
//! (an IANA name like `America/Chicago`) when set.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::paths;

/// Where the scheduler tees each run's output. Also listed in the dashboard's
/// Logs view so a run leaves visible evidence.
pub const CRON_LOG: &str = "/tmp/mimi-crons.log";

/// Serializes read-modify-write cycles on crons.json. The dashboard API
/// handlers and the scheduler both mutate the file from the same process.
static FILE_LOCK: Mutex<()> = Mutex::new(());

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
    /// When the scheduler last started this job. Written back to disk so a
    /// restart mid-minute doesn't re-fire a job that already ran.
    #[serde(default)]
    pub last_run: Option<DateTime<Utc>>,
    /// `ok`, `exit N`, `spawn failed: …`, or `running`.
    #[serde(default)]
    pub last_status: Option<String>,
}

fn default_enabled() -> bool { true }

pub fn path() -> PathBuf {
    paths::home().join("crons.json")
}

pub fn load() -> Vec<CronJob> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    load_unlocked()
}

fn load_unlocked() -> Vec<CronJob> {
    fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_unlocked(jobs: &[CronJob]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(jobs).map_err(|e| e.to_string())?;
    fs::write(path(), json).map_err(|e| e.to_string())
}

/// Load, mutate, and save the job list as one atomic-enough unit. Every
/// mutation goes through here so a scheduler status write can't clobber a job
/// the dashboard created a moment earlier.
pub fn with_jobs<T>(f: impl FnOnce(&mut Vec<CronJob>) -> T) -> Result<T, String> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut jobs = load_unlocked();
    let out = f(&mut jobs);
    save_unlocked(&jobs)?;
    Ok(out)
}

// ---------- cron expression ----------

/// A parsed 5-field cron expression: minute hour day-of-month month day-of-week.
///
/// Supports `*`, `a`, `a-b`, `*/n`, `a-b/n`, comma lists, and three-letter
/// month/weekday names. Sunday is both `0` and `7`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    minute: Vec<bool>,
    hour: Vec<bool>,
    dom: Vec<bool>,
    month: Vec<bool>,
    dow: Vec<bool>,
    /// Standard cron quirk: when both day-of-month and day-of-week are
    /// restricted, a match on *either* fires the job.
    dom_restricted: bool,
    dow_restricted: bool,
}

const MONTH_NAMES: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const DOW_NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

impl Schedule {
    pub fn parse(expr: &str) -> Result<Schedule, String> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!(
                "expected 5 fields (minute hour day-of-month month day-of-week), got {}",
                fields.len()
            ));
        }
        Ok(Schedule {
            minute: parse_field(fields[0], 0, 59, &[])?,
            hour: parse_field(fields[1], 0, 23, &[])?,
            dom: parse_field(fields[2], 1, 31, &[])?,
            month: parse_field(fields[3], 1, 12, &MONTH_NAMES)?,
            dow: parse_field(fields[4], 0, 7, &DOW_NAMES)?,
            dom_restricted: fields[2] != "*",
            dow_restricted: fields[4] != "*",
        })
    }

    /// Does this expression fire during `when`'s minute?
    pub fn matches<Tz2: TimeZone>(&self, when: &DateTime<Tz2>) -> bool {
        if !self.minute[when.minute() as usize] || !self.hour[when.hour() as usize] {
            return false;
        }
        if !self.month[when.month() as usize - 1] {
            return false;
        }
        let dom_hit = self.dom[when.day() as usize - 1];
        // chrono weekdays start at Monday; cron starts at Sunday.
        let dow_hit = self.dow[when.weekday().num_days_from_sunday() as usize];
        match (self.dom_restricted, self.dow_restricted) {
            (true, true) => dom_hit || dow_hit,
            _ => dom_hit && dow_hit,
        }
    }
}

/// Expand one cron field into a `min..=max` occupancy table (indexed from
/// `min`). `names` maps three-letter aliases onto `min + index`.
fn parse_field(field: &str, min: u32, max: u32, names: &[&str]) -> Result<Vec<bool>, String> {
    let mut set = vec![false; (max - min + 1) as usize];
    for part in field.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(format!("empty term in field {field:?}"));
        }
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => {
                let step: u32 = s
                    .parse()
                    .map_err(|_| format!("bad step {s:?} in field {field:?}"))?;
                if step == 0 {
                    return Err(format!("zero step in field {field:?}"));
                }
                (r, step)
            }
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (parse_value(a, min, max, names)?, parse_value(b, min, max, names)?)
        } else {
            let v = parse_value(range, min, max, names)?;
            // A bare value with a step (`5/15`) means "from 5 to the end".
            if step > 1 { (v, max) } else { (v, v) }
        };
        if lo > hi {
            return Err(format!("inverted range {range:?} in field {field:?}"));
        }
        let mut v = lo;
        while v <= hi {
            set[(v - min) as usize] = true;
            v += step;
        }
    }
    // Sunday-as-7 folds onto index 0 so `0-7` and `7` both mean Sunday.
    if min == 0 && max == 7 && set.len() == 8 {
        if set[7] {
            set[0] = true;
        }
        set.truncate(7);
    }
    if !set.iter().any(|b| *b) {
        return Err(format!("field {field:?} matches nothing"));
    }
    Ok(set)
}

fn parse_value(raw: &str, min: u32, max: u32, names: &[&str]) -> Result<u32, String> {
    let raw = raw.trim();
    let v = match raw.parse::<u32>() {
        Ok(v) => v,
        Err(_) => {
            let lower = raw.to_ascii_lowercase();
            let idx = names
                .iter()
                .position(|n| *n == lower)
                .ok_or_else(|| format!("bad value {raw:?}"))?;
            min + idx as u32
        }
    };
    if v < min || v > max {
        return Err(format!("value {v} out of range {min}-{max}"));
    }
    Ok(v)
}

// ---------- execution ----------

/// Timezone the schedules are evaluated in: `timezone` from config.json when
/// it names a valid IANA zone, otherwise the system local zone.
fn configured_tz() -> Option<Tz> {
    let raw = fs::read_to_string(paths::config_file()).ok()?;
    let config: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let name = config.get("timezone")?.as_str()?;
    match name.parse::<Tz>() {
        Ok(tz) => Some(tz),
        Err(_) => {
            eprintln!("crons: unknown timezone {name:?} in config.json — using system local");
            None
        }
    }
}

/// Format `now` in the scheduling timezone for display/logging.
fn now_local_string() -> String {
    match configured_tz() {
        Some(tz) => Utc::now().with_timezone(&tz).format("%Y-%m-%d %H:%M:%S %Z").to_string(),
        None => Local::now().format("%Y-%m-%d %H:%M:%S %Z").to_string(),
    }
}

fn is_due(schedule: &Schedule, now: DateTime<Utc>) -> bool {
    match configured_tz() {
        Some(tz) => schedule.matches(&now.with_timezone(&tz)),
        None => schedule.matches(&now.with_timezone(&Local)),
    }
}

fn log_line(msg: &str) {
    use std::io::Write;
    let line = format!("[{}] {msg}\n", now_local_string());
    print!("{line}");
    let _ = std::io::stdout().flush();
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(CRON_LOG) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Run one job's prompt through a one-shot `claude --print`, appending its
/// output to `CRON_LOG`. Blocking — callers run it off the reactor.
fn execute(job: &CronJob) -> String {
    use std::io::Write;

    log_line(&format!("▶ {} ({}) — {}", job.name, job.id, job.schedule));
    let output = std::process::Command::new("claude")
        .args(["--print", "--dangerously-skip-permissions", &job.prompt])
        .current_dir(paths::home())
        .output();

    match output {
        Ok(out) => {
            if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(CRON_LOG) {
                let _ = f.write_all(&out.stdout);
                let _ = f.write_all(&out.stderr);
                let _ = f.write_all(b"\n");
            }
            let status = if out.status.success() {
                "ok".to_string()
            } else {
                format!("exit {}", out.status.code().unwrap_or(-1))
            };
            log_line(&format!("■ {} — {status}", job.name));
            status
        }
        Err(e) => {
            let status = format!("spawn failed: {e}");
            log_line(&format!("■ {} — {status}", job.name));
            status
        }
    }
}

fn set_status(id: &str, last_run: Option<DateTime<Utc>>, status: &str) {
    let res = with_jobs(|jobs| {
        if let Some(job) = jobs.iter_mut().find(|j| j.id == id) {
            if let Some(ts) = last_run {
                job.last_run = Some(ts);
            }
            job.last_status = Some(status.to_string());
        }
    });
    if let Err(e) = res {
        eprintln!("crons: failed to record status for {id}: {e}");
    }
}

/// Run a job by id right now, regardless of its schedule or enabled flag.
pub fn run_now(id: &str) -> Result<(), String> {
    let job = load()
        .into_iter()
        .find(|j| j.id == id || j.name == id)
        .ok_or_else(|| format!("no schedule with id or name {id:?}"))?;
    let started = Utc::now();
    set_status(&job.id, Some(started), "running");
    let status = execute(&job);
    set_status(&job.id, None, &status);
    if status == "ok" { Ok(()) } else { Err(status) }
}

/// The scheduler loop: wake on each minute boundary, fire everything due.
///
/// Never returns. A job already running is skipped rather than stacked, so a
/// slow prompt on a `*/5` schedule can't pile up copies of itself.
pub async fn scheduler() {
    let running: std::sync::Arc<Mutex<HashSet<String>>> =
        std::sync::Arc::new(Mutex::new(HashSet::new()));
    log_line("cron scheduler started");

    loop {
        sleep_to_next_minute().await;
        let now = Utc::now();

        for job in load() {
            if !job.enabled {
                continue;
            }
            let schedule = match Schedule::parse(&job.schedule) {
                Ok(s) => s,
                Err(e) => {
                    // Disable rather than warn every single minute forever.
                    log_line(&format!(
                        "✗ {} — invalid schedule {:?}: {e} (disabling)",
                        job.name, job.schedule
                    ));
                    let id = job.id.clone();
                    let _ = with_jobs(|jobs| {
                        if let Some(j) = jobs.iter_mut().find(|j| j.id == id) {
                            j.enabled = false;
                            j.last_status = Some(format!("invalid schedule: {e}"));
                        }
                    });
                    continue;
                }
            };
            if !is_due(&schedule, now) {
                continue;
            }
            // Guard against firing twice inside one minute — a tick that
            // wakes slightly early, or a restart mid-minute.
            if job.last_run.is_some_and(|prev| same_minute(prev, now)) {
                continue;
            }
            {
                let mut guard = running.lock().unwrap_or_else(|e| e.into_inner());
                if !guard.insert(job.id.clone()) {
                    log_line(&format!("↷ {} — previous run still going, skipping", job.name));
                    continue;
                }
            }

            set_status(&job.id, Some(now), "running");
            let running = running.clone();
            tokio::task::spawn_blocking(move || {
                let status = execute(&job);
                set_status(&job.id, None, &status);
                running
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&job.id);
            });
        }
    }
}

fn same_minute(a: DateTime<Utc>, b: DateTime<Utc>) -> bool {
    a.timestamp() / 60 == b.timestamp() / 60
}

/// Sleep until just past the next wall-clock minute boundary, so ticks stay
/// aligned instead of drifting by however long the previous pass took.
async fn sleep_to_next_minute() {
    let now = Utc::now();
    let secs = 60 - (now.timestamp().rem_euclid(60)) as u64;
    tokio::time::sleep(std::time::Duration::from_secs(secs.max(1))).await;
}

// ---------- CLI ----------

pub fn cli_list() {
    let jobs = load();
    if jobs.is_empty() {
        println!("(no schedules)");
        return;
    }
    println!("Evaluating in {}", now_local_string());
    println!("{:<16} {:<10} {:<16} {:<20} {}", "ID", "ENABLED", "SCHEDULE", "LAST RUN", "NAME");
    for j in jobs {
        let last = j
            .last_run
            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "never".into());
        let status = j.last_status.as_deref().unwrap_or("");
        println!(
            "{:<16} {:<10} {:<16} {:<20} {} {}",
            j.id,
            if j.enabled { "yes" } else { "no" },
            j.schedule,
            last,
            j.name,
            if status.is_empty() { String::new() } else { format!("({status})") },
        );
    }
}

pub fn cli_run(id: &str) {
    if let Err(e) = run_now(id) {
        eprintln!("schedule run failed: {e}");
        std::process::exit(1);
    }
}

pub async fn cli_daemon() {
    scheduler().await;
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
        assert!(s.matches(&at(2026, 8, 25, 3, 17)));
    }

    #[test]
    fn step_minutes() {
        let s = Schedule::parse("*/5 * * * *").unwrap();
        assert!(s.matches(&at(2026, 8, 25, 3, 0)));
        assert!(s.matches(&at(2026, 8, 25, 3, 55)));
        assert!(!s.matches(&at(2026, 8, 25, 3, 17)));
    }

    #[test]
    fn daily_at_hour() {
        let s = Schedule::parse("0 3 * * *").unwrap();
        assert!(s.matches(&at(2026, 8, 25, 3, 0)));
        assert!(!s.matches(&at(2026, 8, 25, 3, 1)));
        assert!(!s.matches(&at(2026, 8, 25, 4, 0)));
    }

    #[test]
    fn weekday_range_and_names() {
        // 2026-08-25 is a Tuesday, 2026-08-23 a Sunday.
        let s = Schedule::parse("30 1 * * 2-6").unwrap();
        assert!(s.matches(&at(2026, 8, 25, 1, 30)));
        assert!(!s.matches(&at(2026, 8, 23, 1, 30)));

        let named = Schedule::parse("30 1 * * tue").unwrap();
        assert!(named.matches(&at(2026, 8, 25, 1, 30)));
        assert!(!named.matches(&at(2026, 8, 26, 1, 30)));
    }

    #[test]
    fn sunday_is_zero_and_seven() {
        for expr in ["0 0 * * 0", "0 0 * * 7"] {
            let s = Schedule::parse(expr).unwrap();
            assert!(s.matches(&at(2026, 8, 23, 0, 0)), "{expr} should match Sunday");
            assert!(!s.matches(&at(2026, 8, 24, 0, 0)), "{expr} should not match Monday");
        }
    }

    #[test]
    fn lists_and_month_names() {
        let s = Schedule::parse("0,30 9,17 1 jan,jul *").unwrap();
        assert!(s.matches(&at(2026, 7, 1, 17, 30)));
        assert!(s.matches(&at(2026, 1, 1, 9, 0)));
        assert!(!s.matches(&at(2026, 7, 1, 17, 15)));
        assert!(!s.matches(&at(2026, 6, 1, 9, 0)));
    }

    #[test]
    fn dom_or_dow_when_both_restricted() {
        // Standard cron: restricted dom AND dow means "either matches".
        let s = Schedule::parse("0 0 1 * mon").unwrap();
        assert!(s.matches(&at(2026, 8, 1, 0, 0)), "1st of the month (a Saturday)");
        assert!(s.matches(&at(2026, 8, 24, 0, 0)), "a Monday");
        assert!(!s.matches(&at(2026, 8, 25, 0, 0)), "neither");
    }

    #[test]
    fn rejects_bad_expressions() {
        for expr in ["", "* * * *", "* * * * * *", "60 * * * *", "* 24 * * *", "*/0 * * * *", "5-1 * * * *", "0 0 * * xyz"] {
            assert!(Schedule::parse(expr).is_err(), "{expr:?} should not parse");
        }
    }

    #[test]
    fn accepts_every_ui_preset() {
        for expr in ["*/5 * * * *", "*/10 * * * *", "*/30 * * * *", "0 * * * *", "0 3 * * *"] {
            Schedule::parse(expr).unwrap_or_else(|e| panic!("{expr:?}: {e}"));
        }
    }
}
