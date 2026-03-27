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

fn run_claude_output(args: &[&str]) -> String {
    let output = Command::new("claude")
        .args(args)
        .output()
        .expect("failed to run claude — is it installed?");
    String::from_utf8_lossy(&output.stdout).to_string()
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

/// List installed plugins
pub fn plugin_list_output() -> String {
    run_claude_output(&["plugin", "list"])
}

/// Launch claude in a tmux session with optional channels
pub fn launch_tmux(session_name: &str, channels: &[String]) {
    let mimi_home = crate::paths::home();

    // Kill existing session
    Command::new("tmux")
        .args(["kill-session", "-t", session_name])
        .output()
        .ok();

    // Build the claude command string
    // Mimi runs with full permissions — she manages herself
    let mut claude_cmd = "claude --resume --dangerously-skip-permissions".to_string();
    for channel in channels {
        claude_cmd.push_str(&format!(" --channels {}", channel));
    }

    let status = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            session_name,
            "-c",
            mimi_home.to_str().unwrap(),
            &claude_cmd,
        ])
        .status()
        .expect("failed to start tmux — is it installed?");

    if status.success() {
        println!("Mimi is alive in tmux session '{session_name}'");
        if !channels.is_empty() {
            println!("Channels: {}", channels.join(", "));
        }
        println!("Attach with: tmux attach -t {session_name}");
    } else {
        eprintln!("Failed to start tmux session");
        std::process::exit(1);
    }
}

/// Get claude version
pub fn version() -> String {
    run_claude_output(&["--version"]).trim().to_string()
}
