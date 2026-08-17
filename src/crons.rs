//! Recurring prompt scheduler.
//!
//! The dashboard's "schedules" view writes jobs to `~/.mimi/crons.json`
//! (name + cron expression + prompt). Until now nothing ever fired them —
//! this module is the missing runner.
//!
//! `mimi cron run` is a foreground daemon: it wakes on every wall-clock minute
//! boundary, re-reads `crons.json` (so jobs added from the dashboard are
//! picked up without a restart), and for every enabled job whose expression
//! matches the current local minute it spawns
//! `claude --print --dangerously-skip-permissions <prompt>` in `~/.mimi`.
//!
//! Run outcomes live in a separate file, `~/.mimi/cron_state.json`, keyed by
//! job id. Splitting them keeps the two writers apart: the dashboard owns
//! `crons.json`, the daemon owns `cron_state.json`, so a job created while
//! another one is firing can't be clobbered by a last-write-wins race.
//!
//! Jobs run in their own thread and are guarded against overlap — if a run is
//! still going when the next tick matches, that tick is skipped rather than
//! stacking a second `claude` on top of the first. There is deliberately no
//! catch-up for ticks missed while the daemon was down: a prompt like "summarize
//! my unread messages" is worth firing on schedule, not eight times at boot.

use std::collections::{BTreeMap, HashSet};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    /// Standard 5-field cron expression, or an `@hourly`-style alias.
    pub schedule: String,
    pub prompt: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// Outcome of the most recent run of a job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunState {
    /// RFC3339 timestamp of when the run started.
    #[serde(default)]
    pub last_run: Option<String>,
    /// "ok", "exit 1", "timed out", "spawn failed: ..." — human readable.
    #[serde(default)]
    pub last_status: Option<String>,
    #[serde(default)]
    pub last_duration_ms: Option<u64>,
}

pub type StateMap = BTreeMap<String, RunState>;

// ---------- storage ----------

fn jobs_path() -> std::path::PathBuf {
    paths::home().join("crons.json")
}

fn state_path() -> std::path::PathBuf {
    paths::home().join("cron_state.json")
}

pub fn load_jobs() -> Vec<CronJob> {
    std::fs::read_to_string(jobs_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_jobs(jobs: &[CronJob]) -> Result<(), String> {
    paths::ensure_dirs();
    let json = serde_json::to_string_pretty(jobs).map_err(|e| e.to_string())?;
    std::fs::write(jobs_path(), json).map_err(|e| e.to_string())
}

pub fn load_state() -> StateMap {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn record_state(id: &str, state: RunState) {
    // Read-modify-write. The daemon is the only writer of this file and jobs
    // are fired from separate threads, so serialize through a mutex.
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut map = load_state();
    map.insert(id.to_string(), state);
    match serde_json::to_string_pretty(&map) {
        Ok(json) => {
            if let Err(e) = std::fs::write(state_path(), json) {
                eprintln!("cron: failed to write cron_state.json: {e}");
            }
        }
        Err(e) => eprintln!("cron: failed to serialize cron state: {e}"),
    }
}

/// Drop state entries whose job no longer exists, so the file doesn't grow
/// forever as schedules come and go.
pub fn prune_state(jobs: &[CronJob]) {
    let live: HashSet<&str> = jobs.iter().map(|j| j.id.as_str()).collect();
    let map = load_state();
    let pruned: StateMap = map
        .into_iter()
        .filter(|(id, _)| live.contains(id.as_str()))
        .collect();
    if let Ok(json) = serde_json::to_string_pretty(&pruned) {
        let _ = std::fs::write(state_path(), json);
    }
}

// ---------- cron expression ----------

/// One field of a cron expression: either `*` (matches anything) or an
/// explicit set of allowed values.
#[derive(Debug, Clone, PartialEq)]
struct Field {
    any: bool,
    values: Vec<u32>,
}

impl Field {
    fn parse(spec: &str, min: u32, max: u32, names: &[&str]) -> Result<Field, String> {
        if spec == "*" {
            return Ok(Field { any: true, values: Vec::new() });
        }
        let mut values: Vec<u32> = Vec::new();
        for part in spec.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err(format!("empty term in {spec:?}"));
            }
            // `<range>/<step>` — the range may itself be `*`.
            let (range, step) = match part.split_once('/') {
                Some((r, s)) => {
                    let step: u32 = s
                        .parse()
                        .map_err(|_| format!("invalid step {s:?} in {spec:?}"))?;
                    if step == 0 {
                        return Err(format!("step must be > 0 in {spec:?}"));
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
                // A bare value with a step means "from v to the end", as in
                // vixie cron: `5/10` in the minute field is 5,15,25,...
                if step > 1 { (v, max) } else { (v, v) }
            };
            if lo > hi {
                return Err(format!("range {lo}-{hi} is inverted in {spec:?}"));
            }
            let mut v = lo;
            while v <= hi {
                values.push(v);
                v += step;
            }
        }
        values.sort_unstable();
        values.dedup();
        if values.is_empty() {
            return Err(format!("no values matched by {spec:?}"));
        }
        Ok(Field { any: false, values })
    }

    fn matches(&self, v: u32) -> bool {
        self.any || self.values.binary_search(&v).is_ok()
    }
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
                .ok_or_else(|| format!("unrecognized value {raw:?}"))?;
            min + idx as u32
        }
    };
    // Sunday is both 0 and 7 in the day-of-week field.
    let v = if max == 6 && v == 7 { 0 } else { v };
    if v < min || v > max {
        return Err(format!("value {v} out of range {min}-{max}"));
    }
    Ok(v)
}

const DOW_NAMES: &[&str] = &["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
const MONTH_NAMES: &[&str] = &[
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

#[derive(Debug, Clone)]
pub struct Schedule {
    minute: Field,
    hour: Field,
    dom: Field,
    month: Field,
    dow: Field,
}

impl Schedule {
    pub fn parse(expr: &str) -> Result<Schedule, String> {
        let expr = expand_alias(expr.trim());
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!(
                "expected 5 fields (min hour dom month dow), got {}",
                fields.len()
            ));
        }
        Ok(Schedule {
            minute: Field::parse(fields[0], 0, 59, &[])?,
            hour: Field::parse(fields[1], 0, 23, &[])?,
            dom: Field::parse(fields[2], 1, 31, &[])?,
            month: Field::parse(fields[3], 1, 12, MONTH_NAMES)?,
            dow: Field::parse(fields[4], 0, 6, DOW_NAMES)?,
        })
    }

    pub fn matches<Tz: TimeZone>(&self, t: &DateTime<Tz>) -> bool {
        // Vixie cron semantics: when BOTH day-of-month and day-of-week are
        // restricted the job runs if either matches; otherwise they AND
        // (which a `*` field does for free).
        let dom_hit = self.dom.matches(t.day());
        let dow_hit = self.dow.matches(t.weekday().num_days_from_sunday());
        let day_ok = if self.dom.any || self.dow.any {
            dom_hit && dow_hit
        } else {
            dom_hit || dow_hit
        };
        self.minute.matches(t.minute())
            && self.hour.matches(t.hour())
            && self.month.matches(t.month())
            && day_ok
    }

    /// First matching minute strictly after `from`. Brute-forces forward a
    /// minute at a time; capped at ~400 days so an unsatisfiable expression
    /// (e.g. `0 0 30 2 *`) returns `None` instead of spinning.
    pub fn next_after(&self, from: DateTime<Local>) -> Option<DateTime<Local>> {
        let mut t = (from + chrono::Duration::minutes(1))
            .with_second(0)?
            .with_nanosecond(0)?;
        for _ in 0..(400 * 24 * 60) {
            if self.matches(&t) {
                return Some(t);
            }
            t += chrono::Duration::minutes(1);
        }
        None
    }
}

fn expand_alias(expr: &str) -> &str {
    match expr {
        "@yearly" | "@annually" => "0 0 1 1 *",
        "@monthly" => "0 0 1 * *",
        "@weekly" => "0 0 * * 0",
        "@daily" | "@midnight" => "0 0 * * *",
        "@hourly" => "0 * * * *",
        other => other,
    }
}

// ---------- daemon ----------

/// Ids of jobs with a run currently in flight.
fn in_flight() -> &'static Mutex<HashSet<String>> {
    static INSTANCE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn run_daemon() {
    paths::ensure_dirs();
    let jobs = load_jobs();
    println!(
        "mimi cron: scheduler started ({} job{}, local time UTC{})",
        jobs.len(),
        if jobs.len() == 1 { "" } else { "s" },
        Local::now().format("%:z")
    );
    for job in &jobs {
        match Schedule::parse(&job.schedule) {
            Ok(_) => println!(
                "  {} {} [{}]",
                if job.enabled { "•" } else { "◦" },
                job.name,
                job.schedule
            ),
            Err(e) => eprintln!("  ! {} [{}]: {e}", job.name, job.schedule),
        }
    }
    prune_state(&jobs);

    // Broken expressions are reported once per (job, expression) rather than
    // on every tick — a typo shouldn't produce a line a minute forever.
    let mut warned: HashSet<(String, String)> = HashSet::new();

    loop {
        sleep_until_next_minute();
        let now = Local::now();
        for job in load_jobs().into_iter().filter(|j| j.enabled) {
            match Schedule::parse(&job.schedule) {
                Ok(schedule) => {
                    if schedule.matches(&now) {
                        fire(job);
                    }
                }
                Err(e) => {
                    if warned.insert((job.id.clone(), job.schedule.clone())) {
                        eprintln!(
                            "cron: job {} ({}) has an invalid schedule {:?}: {e} — it will never fire",
                            job.id, job.name, job.schedule
                        );
                    }
                }
            }
        }
    }
}

/// Sleep until the top of the next minute. Sleeping in bounded chunks keeps
/// the daemon honest if the machine suspends or the clock jumps.
fn sleep_until_next_minute() {
    loop {
        let now = Local::now();
        let secs_left = 60 - now.second() as i64;
        let nanos_left = 1_000_000_000 - now.nanosecond().min(999_999_999) as i64;
        let remaining =
            Duration::from_nanos(((secs_left - 1).max(0) * 1_000_000_000 + nanos_left) as u64);
        if remaining <= Duration::from_secs(30) {
            std::thread::sleep(remaining);
            return;
        }
        std::thread::sleep(Duration::from_secs(30));
    }
}

fn fire(job: CronJob) {
    {
        let mut guard = in_flight().lock().unwrap_or_else(|e| e.into_inner());
        if !guard.insert(job.id.clone()) {
            eprintln!(
                "cron: {} is still running from an earlier tick — skipping",
                job.name
            );
            return;
        }
    }
    std::thread::spawn(move || {
        let state = execute(&job);
        record_state(&job.id, state);
        in_flight()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&job.id);
    });
}

/// Run a job's prompt through `claude --print` and report the outcome.
pub fn execute(job: &CronJob) -> RunState {
    let started_at = Local::now();
    let clock = Instant::now();
    println!(
        "[{}] cron: firing {} ({})",
        started_at.format("%Y-%m-%d %H:%M:%S"),
        job.name,
        job.schedule
    );

    let output = Command::new("claude")
        .args(["--print", "--dangerously-skip-permissions", &job.prompt])
        .current_dir(paths::home())
        .output();

    let elapsed = clock.elapsed().as_millis() as u64;
    let status = match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stdout = stdout.trim();
            if !stdout.is_empty() {
                println!("--- {} output ---\n{stdout}\n--- end ---", job.name);
            }
            if out.status.success() {
                println!("cron: {} finished in {}ms", job.name, elapsed);
                "ok".to_string()
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                eprintln!(
                    "cron: {} failed ({}): {}",
                    job.name,
                    out.status,
                    stderr.trim()
                );
                match out.status.code() {
                    Some(code) => format!("exit {code}"),
                    None => "killed by signal".to_string(),
                }
            }
        }
        Err(e) => {
            eprintln!("cron: {} could not start claude: {e}", job.name);
            format!("spawn failed: {e}")
        }
    };

    RunState {
        last_run: Some(started_at.to_rfc3339()),
        last_status: Some(status),
        last_duration_ms: Some(elapsed),
    }
}

// ---------- CLI ----------

pub fn cli_run() {
    run_daemon();
}

pub fn cli_list() {
    let jobs = load_jobs();
    if jobs.is_empty() {
        println!("(no schedules — add one from the dashboard's schedules view)");
        return;
    }
    let state = load_state();
    let now = Local::now();
    println!(
        "{:<16} {:<24} {:<18} {:<20} {}",
        "ID", "NAME", "SCHEDULE", "NEXT RUN", "LAST RUN"
    );
    for job in &jobs {
        let (schedule_col, next) = match Schedule::parse(&job.schedule) {
            Ok(s) => {
                let next = match s.next_after(now) {
                    Some(t) => t.format("%Y-%m-%d %H:%M").to_string(),
                    None => "never".to_string(),
                };
                (job.schedule.clone(), if job.enabled { next } else { "disabled".into() })
            }
            Err(e) => (job.schedule.clone(), format!("invalid: {e}")),
        };
        let last = state
            .get(&job.id)
            .and_then(|s| s.last_run.as_deref())
            .map(|ts| {
                let status = state
                    .get(&job.id)
                    .and_then(|s| s.last_status.as_deref())
                    .unwrap_or("?");
                format!("{ts} ({status})")
            })
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<16} {:<24} {:<18} {:<20} {}",
            job.id,
            truncate(&job.name, 24),
            truncate(&schedule_col, 18),
            next,
            last
        );
    }
}

/// Fire a job immediately, ignoring its schedule. Useful for verifying a
/// prompt without waiting for the next tick.
pub fn cli_trigger(id: &str) {
    let jobs = load_jobs();
    let Some(job) = jobs
        .iter()
        .find(|j| j.id == id || j.name == id)
        .or_else(|| jobs.iter().find(|j| j.name.eq_ignore_ascii_case(id)))
    else {
        eprintln!("no schedule with id or name {id:?}");
        std::process::exit(1);
    };
    let state = execute(job);
    record_state(&job.id, state.clone());
    if state.last_status.as_deref() != Some("ok") {
        std::process::exit(1);
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
    }

    fn matches(expr: &str, t: DateTime<Local>) -> bool {
        Schedule::parse(expr).expect("valid expression").matches(&t)
    }

    #[test]
    fn every_minute() {
        assert!(matches("* * * * *", at(2026, 8, 17, 4, 37)));
    }

    #[test]
    fn step_and_list() {
        // 2026-08-17 is a Monday.
        assert!(matches("*/10 * * * *", at(2026, 8, 17, 4, 30)));
        assert!(!matches("*/10 * * * *", at(2026, 8, 17, 4, 31)));
        assert!(matches("0,30 * * * *", at(2026, 8, 17, 4, 30)));
        assert!(!matches("0,30 * * * *", at(2026, 8, 17, 4, 15)));
        assert!(matches("5/10 * * * *", at(2026, 8, 17, 4, 25)));
    }

    #[test]
    fn ranges_and_names() {
        assert!(matches("0 9-17 * * mon-fri", at(2026, 8, 17, 9, 0)));
        assert!(!matches("0 9-17 * * mon-fri", at(2026, 8, 17, 8, 0)));
        // 2026-08-16 is a Sunday.
        assert!(!matches("0 9-17 * * mon-fri", at(2026, 8, 16, 9, 0)));
        assert!(matches("0 0 1 jan *", at(2027, 1, 1, 0, 0)));
    }

    #[test]
    fn sunday_is_zero_or_seven() {
        assert!(matches("0 3 * * 7", at(2026, 8, 16, 3, 0)));
        assert!(matches("0 3 * * 0", at(2026, 8, 16, 3, 0)));
    }

    #[test]
    fn dom_and_dow_are_ored_when_both_restricted() {
        // 1st of the month (a Tuesday) and every Friday.
        assert!(matches("0 0 1 * fri", at(2026, 9, 1, 0, 0)));
        assert!(matches("0 0 1 * fri", at(2026, 9, 4, 0, 0)));
        assert!(!matches("0 0 1 * fri", at(2026, 9, 2, 0, 0)));
    }

    #[test]
    fn aliases() {
        assert!(matches("@daily", at(2026, 8, 17, 0, 0)));
        assert!(!matches("@daily", at(2026, 8, 17, 0, 1)));
        assert!(matches("@hourly", at(2026, 8, 17, 13, 0)));
    }

    #[test]
    fn rejects_garbage() {
        assert!(Schedule::parse("* * * *").is_err());
        assert!(Schedule::parse("60 * * * *").is_err());
        assert!(Schedule::parse("*/0 * * * *").is_err());
        assert!(Schedule::parse("30-10 * * * *").is_err());
        assert!(Schedule::parse("0 0 * * funday").is_err());
    }

    #[test]
    fn next_after_finds_following_slot() {
        let s = Schedule::parse("0 3 * * *").unwrap();
        let next = s.next_after(at(2026, 8, 17, 4, 0)).unwrap();
        assert_eq!(next, at(2026, 8, 18, 3, 0));
    }

    #[test]
    fn next_after_gives_up_on_impossible_expressions() {
        // February 30th never happens.
        let s = Schedule::parse("0 0 30 2 *").unwrap();
        assert!(s.next_after(at(2026, 8, 17, 4, 0)).is_none());
    }
}
