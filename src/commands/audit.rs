use std::process::Command;

const CONTROLLER: &str = include_str!("../../scripts/maintain.py");

pub fn run(apply: bool, review: bool, status_only: bool) {
    let mut command = Command::new("python3");
    command.args(["-c", CONTROLLER, "--repo"]);
    command.arg(find_repo_dir());
    if apply {
        command.arg("--apply");
    }
    if review {
        command.arg("--review");
    }
    if status_only {
        command.arg("--status");
    }
    let status = command
        .status()
        .expect("failed to run maintenance controller — is python3 installed?");
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

pub fn find_repo_dir() -> std::path::PathBuf {
    let candidates = [
        std::env::current_dir().ok(),
        dirs::home_dir().map(|home| home.join("mimi-brain-interface")),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.join("src/commands/audit.rs").exists() {
            return candidate;
        }
    }
    eprintln!("Could not find mimi-brain-interface repo. Run from the repo directory.");
    std::process::exit(1);
}
