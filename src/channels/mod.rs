pub mod discord;
pub mod presence;
pub mod telegram;
pub mod voice;

use chrono::{Datelike, Utc};
use chrono_tz::Tz;

use crate::paths;

const DEFAULT_OWNER_TZ: &str = "America/Chicago";

/// Resolve the owner's IANA timezone from `~/.mimi/config.json`'s `owner_tz`
/// field, falling back to `America/Chicago` if missing or unparseable.
fn owner_tz() -> Tz {
    let raw = std::fs::read_to_string(paths::config_file())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("owner_tz").and_then(|x| x.as_str()).map(str::to_string))
        .unwrap_or_else(|| DEFAULT_OWNER_TZ.to_string());
    raw.parse::<Tz>().unwrap_or_else(|_| {
        DEFAULT_OWNER_TZ.parse().expect("America/Chicago is a valid tz")
    })
}

/// Per-turn system-reminder that pins the current time so the long-lived
/// Claude session doesn't drift off the date across restarts and midnight.
/// Surfaces both owner-local time (for user-facing reasoning — "today",
/// meal logging, scheduling) and UTC (for cross-referencing with logs and
/// timestamps stored in UTC).
pub fn time_context_preamble() -> String {
    let utc_now = Utc::now();
    let tz = owner_tz();
    let local = utc_now.with_timezone(&tz);
    format!(
        "<system-reminder>\nCurrent time (authoritative):\n  Owner local: {} {} ({}), {}\n  UTC:         {} (ISO week {})\nLong-lived session — trust these values over any cached date from earlier context. Use owner-local for anything user-facing (\"today\", meal logging, scheduling, schedule-aware decisions); UTC is for matching log timestamps or talking to APIs that explicitly want UTC.\n</system-reminder>\n",
        local.format("%Y-%m-%d %H:%M:%S"),
        tz.name(),
        local.format("%Z"),
        local.format("%A"),
        utc_now.format("%Y-%m-%d %H:%M:%S"),
        utc_now.iso_week().week(),
    )
}
