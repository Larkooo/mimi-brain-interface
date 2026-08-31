use std::path::Path;
use std::process::Command;

use crate::commands::audit::find_repo_dir;
use crate::paths;

const BIN_PATH: &str = "/usr/local/bin/mimi";
const BIN_BACKUP: &str = "/usr/local/bin/mimi.prev";

pub fn run() {
    let repo = find_repo_dir();
    println!("Updating from {}", repo.display());

    // `mimi audit` leaves this checkout parked on the `mimi/audit-*` branch it
    // just pushed, so `mimi update` rarely runs from master. The `git reset
    // --hard origin/master` below would silently rewrite that branch ref and
    // discard any uncommitted work, and — worse — `before` would be the audit
    // branch tip instead of the deployed master commit, so the rust/dashboard
    // change detection that decides what to rebuild compares the wrong pair of
    // revisions. Refuse to touch a dirty tree, and always land on master first.
    if let Some(dirty) = working_tree_dirty(&repo) {
        eprintln!(
            "Error: {} has uncommitted changes — refusing to reset:\n{}",
            repo.display(),
            dirty
        );
        eprintln!("Commit, stash, or discard them, then re-run `mimi update`.");
        std::process::exit(1);
    }

    if !git(&repo, &["fetch", "origin", "master"]) {
        eprintln!("Error: git fetch failed");
        std::process::exit(1);
    }

    let branch = current_branch(&repo);
    if branch != "master" {
        println!("On branch '{branch}' — switching to master before updating.");
        if !git(&repo, &["checkout", "master"])
            && !git(&repo, &["checkout", "-B", "master", "origin/master"])
        {
            eprintln!("Error: could not switch {} to master", repo.display());
            std::process::exit(1);
        }
    }

    let before = git_rev(&repo, "HEAD");
    let after = git_rev(&repo, "origin/master");
    if before == after {
        println!("Already up to date ({}).", &after[..7.min(after.len())]);
        return;
    }

    println!("Pulling {}..{}", &before[..7.min(before.len())], &after[..7.min(after.len())]);

    let commits = git_log(&repo, &format!("{}..{}", before, after));
    println!("\nCommits:\n{}", commits);

    if !git(&repo, &["reset", "--hard", "origin/master"]) {
        eprintln!("Error: git reset failed");
        std::process::exit(1);
    }

    let dashboard_changed = git_files_changed(&repo, &before, &after)
        .iter()
        .any(|f| f.starts_with("dashboard/"));
    let rust_changed = git_files_changed(&repo, &before, &after)
        .iter()
        .any(|f| f.starts_with("src/") || f == "Cargo.toml" || f == "Cargo.lock");

    if rust_changed {
        println!("\nBuilding Rust binary...");
        let status = Command::new("cargo")
            .args(["build", "--release"])
            .current_dir(&repo)
            .status();
        match status {
            Ok(s) if s.success() => {}
            _ => {
                eprintln!("Error: cargo build failed — leaving existing binary in place");
                std::process::exit(1);
            }
        }

        let built = repo.join("target/release/mimi");
        if !built.exists() {
            eprintln!("Error: built binary not found at {}", built.display());
            std::process::exit(1);
        }

        if Path::new(BIN_PATH).exists() {
            let _ = Command::new("sudo")
                .args(["cp", BIN_PATH, BIN_BACKUP])
                .status();
        }

        let install = Command::new("sudo")
            .args(["install", "-m", "755", built.to_str().unwrap(), BIN_PATH])
            .status();
        match install {
            Ok(s) if s.success() => println!("Installed {}", BIN_PATH),
            _ => {
                eprintln!("Error: failed to install binary to {}", BIN_PATH);
                std::process::exit(1);
            }
        }
    } else {
        println!("No Rust changes — skipping cargo build.");
    }

    if dashboard_changed {
        println!("\nBuilding dashboard...");
        let dashboard_dir = repo.join("dashboard");
        let status = Command::new("npm")
            .args(["ci"])
            .current_dir(&dashboard_dir)
            .status();
        if !matches!(status, Ok(s) if s.success()) {
            eprintln!("Error: npm ci failed");
            std::process::exit(1);
        }
        let status = Command::new("npm")
            .args(["run", "build"])
            .current_dir(&dashboard_dir)
            .status();
        if !matches!(status, Ok(s) if s.success()) {
            eprintln!("Error: npm run build failed");
            std::process::exit(1);
        }

        let dist = dashboard_dir.join("dist");
        let dest = paths::home().join("dashboard").join("dist");
        let _ = std::fs::remove_dir_all(&dest);
        copy_dir(&dist, &dest).unwrap_or_else(|e| {
            eprintln!("Error: failed to copy dashboard to {}: {}", dest.display(), e);
            std::process::exit(1);
        });
        println!("Copied dashboard to {}", dest.display());
    } else {
        println!("No dashboard changes — skipping npm build.");
    }

    println!("\nUpdate complete: now at {}", &after[..7.min(after.len())]);
}

fn git(repo: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn git_rev(repo: &Path, rev: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(repo)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

/// `git rev-parse --abbrev-ref HEAD` — the checked-out branch name, or "HEAD"
/// when detached. Empty string if git fails.
fn current_branch(repo: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

/// `Some(porcelain output)` when tracked files have staged or unstaged changes
/// that `git reset --hard` would throw away; `None` when the tree is clean.
/// Untracked files are ignored — `reset --hard` leaves those alone.
fn working_tree_dirty(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let status = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if status.is_empty() { None } else { Some(status) }
}

fn git_log(repo: &Path, range: &str) -> String {
    let out = Command::new("git")
        .args(["log", "--oneline", range])
        .current_dir(repo)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => String::new(),
    }
}

fn git_files_changed(repo: &Path, from: &str, to: &str) -> Vec<String> {
    let out = Command::new("git")
        .args(["diff", "--name-only", from, to])
        .current_dir(repo)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
