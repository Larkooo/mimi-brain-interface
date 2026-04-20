//! Managed systemd user units.
//!
//! The bridges (`mimi-telegram`, `mimi-discord`) and the `mimi-dashboard`
//! run as systemd --user services in production. This module centralizes
//! the list of units we manage and the `systemctl show` parsing so the
//! CLI (`mimi status`) and the dashboard API can report the same state.

use serde::Serialize;

pub const MANAGED_SERVICES: &[&str] = &["mimi-telegram", "mimi-discord", "mimi-dashboard"];

#[derive(Serialize)]
pub struct ServiceInfo {
    pub name: String,
    pub active_state: String,
    pub sub_state: String,
    pub main_pid: Option<u32>,
    pub enabled: bool,
}

pub fn systemctl_user(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn service_info(name: &str) -> ServiceInfo {
    let show = systemctl_user(&["show", name, "--no-page"]).unwrap_or_default();
    let mut active_state = String::from("unknown");
    let mut sub_state = String::from("unknown");
    let mut main_pid: Option<u32> = None;
    for line in show.lines() {
        if let Some(v) = line.strip_prefix("ActiveState=") {
            active_state = v.into();
        } else if let Some(v) = line.strip_prefix("SubState=") {
            sub_state = v.into();
        } else if let Some(v) = line.strip_prefix("MainPID=") {
            main_pid = v.parse().ok().filter(|p| *p != 0);
        }
    }
    let enabled = systemctl_user(&["is-enabled", name])
        .map(|s| s.trim() == "enabled")
        .unwrap_or(false);
    ServiceInfo {
        name: name.into(),
        active_state,
        sub_state,
        main_pid,
        enabled,
    }
}

pub fn list() -> Vec<ServiceInfo> {
    MANAGED_SERVICES.iter().map(|n| service_info(n)).collect()
}

pub fn is_managed(name: &str) -> bool {
    MANAGED_SERVICES.contains(&name)
}
