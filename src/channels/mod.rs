pub mod discord;
pub mod presence;
pub mod telegram;

use chrono::{Datelike, Utc};

/// Per-turn system-reminder that pins the current UTC time so the long-lived
/// Claude session doesn't drift off the date across restarts and midnight.
pub fn time_context_preamble() -> String {
    let now = Utc::now();
    format!(
        "<system-reminder>\nCurrent time (authoritative): {} UTC, {} (ISO week {}). Long-lived session — trust this value over any cached date from earlier context when logging meals, scheduling, or any time-sensitive work. Owner local tz: America/Chicago (CDT UTC-5 ~Mar–Nov, CST UTC-6 otherwise).\n</system-reminder>\n",
        now.format("%Y-%m-%d %H:%M:%S"),
        now.format("%A"),
        now.iso_week().week()
    )
}
