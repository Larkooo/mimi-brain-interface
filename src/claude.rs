use std::process::Command;

fn run_claude(args: &[&str]) {
    let status = Command::new("claude")
        .args(args)
        .status()
        .expect("failed to run claude — is it installed?");
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn try_run_claude_output(args: &[&str]) -> Result<String, String> {
    let output = Command::new("claude")
        .args(args)
        .output()
        .map_err(|e| format!("failed to run claude: {e}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn mcp(args: &[&str]) {
    let mut cmd_args = vec!["mcp"];
    cmd_args.extend_from_slice(args);
    run_claude(&cmd_args);
}

pub fn plugin(args: &[&str]) {
    let mut cmd_args = vec!["plugin"];
    cmd_args.extend_from_slice(args);
    run_claude(&cmd_args);
}

/// Install a Claude Code plugin
pub fn plugin_install(plugin_name: &str) {
    run_claude(&["plugin", "install", plugin_name]);
}

/// List installed plugins (fallible, safe for server use)
pub fn plugin_list_output() -> Result<String, String> {
    try_run_claude_output(&["plugin", "list"])
}

/// Launch an interactive claude in a tmux session.
/// Channels run out-of-process — use `mimi channel start <name>` for those.
pub fn launch_tmux(session_name: &str) -> Result<(), String> {
    let mimi_home = crate::paths::home();

    Command::new("tmux")
        .args(["kill-session", "-t", session_name])
        .output()
        .ok();

    let claude_cmd = "claude --resume --dangerously-skip-permissions";

    let status = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            session_name,
            "-c",
            mimi_home.to_str().unwrap(),
            claude_cmd,
        ])
        .status()
        .map_err(|e| format!("failed to start tmux: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("tmux session creation failed".to_string())
    }
}

/// Get claude version (returns "unknown" if claude is not available)
pub fn version() -> String {
    try_run_claude_output(&["--version"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}
