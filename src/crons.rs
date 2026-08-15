//! Scheduled prompts ("schedules" in the dashboard).
//!
//! The dashboard writes recurring jobs to `~/.mimi/crons.json`; this module is
//! what actually fires them. A job is a cron expression plus a prompt — when
//! the expression matches, we run the prompt through a headless
//! `claude --print` session rooted at `~/.mimi`, so it boots with Mimi's
//! CLAUDE.md, brain, and `~/.mimi/bin/*` wrappers available. That mirrors how
//! `mimi reflect` and `mimi audit` invoke claude.
//!
//! Two entry points:
//!
//!   `mimi cron tick`  — fire everything due right now, then exit. Meant for a
//!                       `* * * * *` line in the system crontab.
//!   `mimi cron start` — same thing on a loop, aligned to minute boundaries.
//!                       Meant for a systemd user unit alongside the other
//!                       mimi-* services.
//!
//! Ticks are serialized by an flock on `~/.mimi/crons.lock`: a job that runs
//! longer than a minute won't be started twice by the following tick. Because
//! that means ticks can be skipped, dueness is evaluated as a catch-up over the
//! window since the job's `last_run` (capped at MAX_CATCHUP_MINUTES) rather
//! than an exact match on the current minute — a job scheduled for 03:00 still
//! fires if the tick lands at 03:02.

use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::paths;

/// How far back a tick will look when catching up a job it missed. Bounds the
/// work done per tick and stops a machine that was asleep for a week from
/// replaying a month of hourly jobs on wake.
const MAX_CATCHUP_MINUTES: i64 = 60;

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
    /// When this job last fired (UTC). `None` until the first run.
    #[serde(default)]
    pub last_run: Option<DateTime<Utc>>,
    /// Outcome of the last run: "ok" or "failed: …".
    #[serde(default)]
    pub last_status: Option<String>,
}

fn default_enabled() -> bool {
    true
}

pub fn path() -> PathBuf {
    paths::home().join("crons.json")
}

fn lock_path() -> PathBuf {
    paths::home().join("crons.lock")
}

fn log_path(job: &CronJob) -> PathBuf {
    paths::logs_dir().join(format!("cron-{}.log", job.id))
}

pub fn load() -> Vec<CronJob> {
    fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(crons: &[CronJob]) -> Result<(), String> {
    paths::ensure_dirs();
    let json = serde_json::to_string_pretty(crons).map_err(|e| e.to_string())?;
    fs::write(path(), json).map_err(|e| e.to_string())
}

// ---------- cron expression ----------

/// A parsed 5-field cron expression (minute hour day-of-month month day-of-week).
///
/// Supports `*`, `a`, `a-b`, `*/n`, `a-b/n`, `a/n`, comma-separated lists, and
/// the `@hourly`/`@daily`/`@weekly`/`@monthly`/`@yearly` macros. Day-of-week
/// accepts 0-7 with both 0 and 7 meaning Sunday.
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
        let expr = match expr.trim() {
            "@hourly" => "0 * * * *",
            "@daily" | "@midnight" => "0 0 * * *",
            "@weekly" => "0 0 * * 0",
            "@monthly" => "0 0 1 * *",
            "@yearly" | "@annually" => "0 0 1 1 *",
            other => other,
        };
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!(
                "expected 5 fields (minute hour day month weekday), got {}",
                fields.len()
            ));
        }
        let minute = parse_field(fields[0], 0, 59).map_err(|e| format!("minute: {e}"))?;
        let hour = parse_field(fields[1], 0, 23).map_err(|e| format!("hour: {e}"))?;
        let dom = parse_field(fields[2], 1, 31).map_err(|e| format!("day of month: {e}"))?;
        let month = parse_field(fields[3], 1, 12).map_err(|e| format!("month: {e}"))?;
        // 7 is an alias for Sunday; fold it onto 0 after parsing.
        let mut dow = parse_field(fields[4], 0, 7).map_err(|e| format!("weekday: {e}"))?;
        if dow[7] {
            dow[0] = true;
        }
        dow.truncate(7);

        Ok(Schedule {
            minute,
            hour,
            dom,
            month,
            dow,
            dom_restricted: fields[2] != "*",
            dow_restricted: fields[4] != "*",
        })
    }

    /// Does this schedule fire during the minute containing `dt`?
    pub fn matches(&self, dt: &DateTime<Local>) -> bool {
        if !self.minute[dt.minute() as usize]
            || !self.hour[dt.hour() as usize]
            || !self.month[dt.month() as usize]
        {
            return false;
        }
        let dom_ok = self.dom[dt.day() as usize];
        let dow_ok = self.dow[dt.weekday().num_days_from_sunday() as usize];
        // Standard cron: when both day fields are restricted they OR together,
        // otherwise only the restricted one applies.
        match (self.dom_restricted, self.dow_restricted) {
            (true, true) => dom_ok || dow_ok,
            (true, false) => dom_ok,
            (false, true) => dow_ok,
            (false, false) => true,
        }
    }
}

/// Parse one cron field into a `min..=max` membership table. The returned
/// vector is indexed by the raw value, so it has `max + 1` slots and slots
/// below `min` are always false.
fn parse_field(spec: &str, min: u32, max: u32) -> Result<Vec<bool>, String> {
    let mut set = vec![false; max as usize + 1];
    if spec.is_empty() {
        return Err("empty field".into());
    }
    for part in spec.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => {
                let step: u32 = s.parse().map_err(|_| format!("bad step in '{part}'"))?;
                if step == 0 {
                    return Err(format!("step must be > 0 in '{part}'"));
                }
                (r, step)
            }
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (
                a.parse().map_err(|_| format!("bad value '{a}'"))?,
                b.parse().map_err(|_| format!("bad value '{b}'"))?,
            )
        } else {
            let v: u32 = range.parse().map_err(|_| format!("bad value '{range}'"))?;
            // `a/n` means "from a to the end of the range, every n" — a bare
            // `a` with no step is just the single value.
            if step > 1 { (v, max) } else { (v, v) }
        };
        if lo < min || hi > max || lo > hi {
            return Err(format!("'{part}' out of range {min}-{max}"));
        }
        let mut v = lo;
        while v <= hi {
            set[v as usize] = true;
            v += step;
        }
    }
    Ok(set)
}

// ---------- dueness ----------

/// Is `job` due as of `now`? True when the schedule matched any minute in
/// `(last_run, now]`, so a tick that lands late still fires a missed job. A job
/// that has never run only fires on an exact match for the current minute — we
/// don't want creating a job at 15:00 to instantly replay this morning's 09:00.
fn is_due(job: &CronJob, now: DateTime<Local>) -> Result<bool, String> {
    let schedule = Schedule::parse(&job.schedule)?;
    let now = now
        .with_second(0)
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(now);

    let earliest = now - chrono::Duration::minutes(MAX_CATCHUP_MINUTES);
    let mut cursor = match job.last_run {
        Some(last) => {
            let last = last.with_timezone(&Local) + chrono::Duration::minutes(1);
            if last > earliest { last } else { earliest }
        }
        None => now,
    };
    cursor = cursor
        .with_second(0)
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(cursor);

    while cursor <= now {
        if schedule.matches(&cursor) {
            return Ok(true);
        }
        cursor += chrono::Duration::minutes(1);
    }
    Ok(false)
}

// ---------- running ----------

/// Run a job's prompt through a headless claude session rooted at `~/.mimi`.
/// stdout/stderr are appended to `~/.mimi/logs/cron-<id>.log`.
fn run_job(job: &CronJob) -> Result<(), String> {
    paths::ensure_dirs();
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path(job))
        .map_err(|e| format!("open log: {e}"))?;
    let log_err = log.try_clone().map_err(|e| format!("clone log fd: {e}"))?;

    let prompt = format!(
        "[scheduled job: {}]\nThis is an automated run on the schedule `{}` — there is no human waiting on this turn, so do the work and report through a channel if the task calls for it.\n\n{}",
        job.name, job.schedule, job.prompt
    );

    let status = Command::new("claude")
        .args(["--print", "--dangerously-skip-permissions", &prompt])
        .current_dir(paths::home())
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .status()
        .map_err(|e| format!("failed to run claude: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("claude exited with {}", status))
    }
}

/// Take an exclusive non-blocking flock so overlapping ticks are dropped rather
/// than double-firing a long job. The lock is held for the returned `File`'s
/// lifetime and released when the process exits.
fn acquire_lock() -> Result<Option<File>, String> {
    paths::ensure_dirs();
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock_path())
        .map_err(|e| format!("open lockfile: {e}"))?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Ok(None);
        }
        return Err(format!("flock: {err}"));
    }
    Ok(Some(file))
}

/// Fire every job due as of now. Returns the number of jobs run, or `None` if
/// the tick was skipped because another one still holds the lock.
pub fn tick() -> Option<usize> {
    let _lock = match acquire_lock() {
        Ok(Some(f)) => f,
        Ok(None) => {
            eprintln!("cron: previous tick still running — skipping");
            return None;
        }
        Err(e) => {
            eprintln!("cron: {e}");
            return None;
        }
    };

    let now = Local::now();
    let mut crons = load();
    let mut ran = 0;

    for i in 0..crons.len() {
        if !crons[i].enabled {
            continue;
        }
        match is_due(&crons[i], now) {
            Ok(false) => continue,
            Ok(true) => {}
            Err(e) => {
                eprintln!("cron: job '{}' has an invalid schedule: {e}", crons[i].name);
                continue;
            }
        }

        // Stamp last_run *before* running: a job that crashes the tick or runs
        // long shouldn't be re-fired from the top on the next pass.
        crons[i].last_run = Some(Utc::now());
        if let Err(e) = save(&crons) {
            eprintln!("cron: failed to persist last_run: {e}");
        }

        println!("cron: running '{}' ({})", crons[i].name, crons[i].schedule);
        let result = run_job(&crons[i]);

        // Reload so concurrent dashboard edits (add/delete/toggle) aren't
        // clobbered by the copy we've been holding across a long run.
        let mut latest = load();
        if let Some(job) = latest.iter_mut().find(|c| c.id == crons[i].id) {
            job.last_run = crons[i].last_run;
            job.last_status = Some(match &result {
                Ok(()) => "ok".to_string(),
                Err(e) => format!("failed: {e}"),
            });
        }
        if let Err(e) = save(&latest) {
            eprintln!("cron: failed to persist status: {e}");
        }
        crons = latest;

        match result {
            Ok(()) => println!("cron: '{}' ok", crons[i].name),
            Err(e) => eprintln!("cron: '{}' failed: {e}", crons[i].name),
        }
        ran += 1;
    }
    Some(ran)
}

/// Tick forever, waking on each minute boundary.
pub fn run_forever() -> ! {
    println!("cron: scheduler started ({} jobs loaded)", load().len());
    loop {
        tick();
        let now = Local::now();
        let secs_into_minute = now.second() as u64;
        let nanos = now.nanosecond() as u64;
        let sleep_ms = 60_000u64.saturating_sub(secs_into_minute * 1000 + nanos / 1_000_000);
        std::thread::sleep(Duration::from_millis(sleep_ms.max(1_000)));
    }
}

// ---------- CLI ----------

pub fn cli_list() {
    let crons = load();
    if crons.is_empty() {
        println!("(no schedules — add one from the dashboard or `mimi cron add`)");
        return;
    }
    println!("{:<16} {:<24} {:<16} {:<22} {}", "ID", "NAME", "SCHEDULE", "LAST RUN", "STATUS");
    for c in crons {
        let last = c
            .last_run
            .map(|t| t.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "-".into());
        let status = match (c.enabled, c.last_status.as_deref()) {
            (false, _) => "disabled".to_string(),
            (true, Some(s)) => s.to_string(),
            (true, None) => "-".to_string(),
        };
        println!("{:<16} {:<24} {:<16} {:<22} {}", c.id, c.name, c.schedule, last, status);
    }
}

pub fn cli_tick() {
    if let Some(0) = tick() {
        println!("cron: nothing due");
    }
}

pub fn cli_start() -> ! {
    run_forever()
}

/// Run one job immediately, regardless of its schedule or enabled flag.
pub fn cli_run(id_or_name: &str) {
    let crons = load();
    let Some(job) = crons
        .iter()
        .find(|c| c.id == id_or_name || c.name == id_or_name)
    else {
        eprintln!("no schedule with id or name '{id_or_name}'");
        std::process::exit(1);
    };
    println!("cron: running '{}' now", job.name);
    match run_job(job) {
        Ok(()) => println!("cron: '{}' ok (output in {})", job.name, log_path(job).display()),
        Err(e) => {
            eprintln!("cron: '{}' failed: {e}", job.name);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn parses_dashboard_presets() {
        for expr in ["*/5 * * * *", "*/10 * * * *", "*/30 * * * *", "0 * * * *", "0 3 * * *"] {
            assert!(Schedule::parse(expr).is_ok(), "failed to parse {expr}");
        }
    }

    #[test]
    fn rejects_malformed_expressions() {
        for expr in ["", "* * * *", "* * * * * *", "60 * * * *", "* 24 * * *", "*/0 * * * *", "abc * * * *"] {
            assert!(Schedule::parse(expr).is_err(), "should have rejected {expr}");
        }
    }

    #[test]
    fn step_and_list_and_range() {
        let s = Schedule::parse("*/15 9-17 * * 1-5").unwrap();
        // Tue 2026-08-18, 09:15 — weekday, in-hours, on a 15-minute boundary.
        assert!(s.matches(&at(2026, 8, 18, 9, 15)));
        assert!(!s.matches(&at(2026, 8, 18, 9, 16)));
        assert!(!s.matches(&at(2026, 8, 18, 8, 15)));
        // Sat 2026-08-15 is outside 1-5.
        assert!(!s.matches(&at(2026, 8, 15, 9, 15)));

        let s = Schedule::parse("0,30 * * * *").unwrap();
        assert!(s.matches(&at(2026, 8, 15, 4, 0)));
        assert!(s.matches(&at(2026, 8, 15, 4, 30)));
        assert!(!s.matches(&at(2026, 8, 15, 4, 15)));
    }

    #[test]
    fn sunday_is_both_zero_and_seven() {
        let zero = Schedule::parse("0 0 * * 0").unwrap();
        let seven = Schedule::parse("0 0 * * 7").unwrap();
        // 2026-08-16 is a Sunday.
        assert!(zero.matches(&at(2026, 8, 16, 0, 0)));
        assert!(seven.matches(&at(2026, 8, 16, 0, 0)));
        assert!(!zero.matches(&at(2026, 8, 17, 0, 0)));
    }

    #[test]
    fn dom_and_dow_or_together_when_both_restricted() {
        let s = Schedule::parse("0 0 1 * 0").unwrap();
        // 1st of the month (a Tuesday) matches on day-of-month alone...
        assert!(s.matches(&at(2026, 9, 1, 0, 0)));
        // ...and any Sunday matches on day-of-week alone.
        assert!(s.matches(&at(2026, 8, 16, 0, 0)));
        assert!(!s.matches(&at(2026, 8, 18, 0, 0)));
    }

    #[test]
    fn macros_expand() {
        assert_eq!(Schedule::parse("@hourly").unwrap(), Schedule::parse("0 * * * *").unwrap());
        assert_eq!(Schedule::parse("@daily").unwrap(), Schedule::parse("0 0 * * *").unwrap());
    }

    fn job(schedule: &str, last_run: Option<DateTime<Local>>) -> CronJob {
        CronJob {
            id: "t".into(),
            name: "test".into(),
            schedule: schedule.into(),
            prompt: "noop".into(),
            description: String::new(),
            enabled: true,
            last_run: last_run.map(|t| t.with_timezone(&Utc)),
            last_status: None,
        }
    }

    #[test]
    fn never_run_job_fires_only_on_exact_minute() {
        let j = job("0 3 * * *", None);
        assert!(is_due(&j, at(2026, 8, 15, 3, 0)).unwrap());
        // No backfill for a job that has never run.
        assert!(!is_due(&j, at(2026, 8, 15, 3, 5)).unwrap());
    }

    #[test]
    fn catches_up_a_missed_minute() {
        let j = job("0 3 * * *", Some(at(2026, 8, 14, 3, 0)));
        // Tick landed two minutes late — still fires.
        assert!(is_due(&j, at(2026, 8, 15, 3, 2)).unwrap());
    }

    #[test]
    fn does_not_refire_within_the_same_minute() {
        let j = job("*/5 * * * *", Some(at(2026, 8, 15, 4, 5)));
        assert!(!is_due(&j, at(2026, 8, 15, 4, 5)).unwrap());
        assert!(!is_due(&j, at(2026, 8, 15, 4, 9)).unwrap());
        assert!(is_due(&j, at(2026, 8, 15, 4, 10)).unwrap());
    }

    #[test]
    fn catchup_window_is_bounded() {
        // Machine was off for a day; an hourly job fires once, not 24 times.
        let j = job("0 * * * *", Some(at(2026, 8, 14, 4, 0)));
        assert!(is_due(&j, at(2026, 8, 15, 4, 30)).unwrap());
        // Nothing scheduled in the last MAX_CATCHUP_MINUTES → not due.
        let j = job("0 3 * * *", Some(at(2026, 8, 10, 3, 0)));
        assert!(!is_due(&j, at(2026, 8, 15, 12, 30)).unwrap());
    }

    #[test]
    fn invalid_schedule_surfaces_as_error_not_a_fire() {
        assert!(is_due(&job("not a cron", None), at(2026, 8, 15, 3, 0)).is_err());
    }
}
