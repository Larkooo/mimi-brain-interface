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

/// Launch claude in the ~/.mimi directory
pub fn launch() {
    let mimi_home = crate::paths::home();
    let status = Command::new("claude")
        .arg("--resume")
        .current_dir(&mimi_home)
        .status()
        .expect("failed to run claude — is it installed?");
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

/// Launch claude in a tmux session
pub fn launch_tmux(session_name: &str) {
    let mimi_home = crate::paths::home();

    // Kill existing session
    Command::new("tmux")
        .args(["kill-session", "-t", session_name])
        .output()
        .ok();

    let status = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            session_name,
            "-c",
            mimi_home.to_str().unwrap(),
            "claude --resume",
        ])
        .status()
        .expect("failed to start tmux — is it installed?");

    if status.success() {
        println!("Mimi is alive in tmux session '{session_name}'");
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
