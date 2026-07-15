use std::path::Path;
use std::process::Command;

const AUDIT_PROMPT: &str = r#"You are Mimi's self-improvement agent. Your job is to audit Mimi's own codebase and propose improvements.

You are in the mimi-brain-interface repository. This is Mimi's brain interface — the Rust CLI and React dashboard that manages an autonomous AI assistant.

The repo is already synced to the latest origin/master before you start, so
HEAD is master. Create your audit branch off the current HEAD.

Your task:
1. Read through the codebase (src/, dashboard/src/, CLAUDE.md.template, Cargo.toml)
2. Identify ONE concrete improvement to make. Prioritize by impact:
   - Bug fixes (highest priority)
   - Missing functionality that's referenced but not implemented
   - UX improvements to the dashboard or CLI
   - Code quality, error handling, robustness
   - New features that would make Mimi more capable
3. Create a new git branch named `mimi/audit-YYYY-MM-DD`
4. Implement the change
5. Commit with a clear message explaining what and why
6. Push the branch and create a PR with:
   - A clear title
   - Description of what changed and why
   - How to test it

Rules:
- Only make ONE focused change per audit. Don't combine multiple improvements.
- The change must be small enough to review quickly.
- Write clean, idiomatic Rust code.
- If the change touches the dashboard, make sure the TypeScript compiles.
- Don't break existing functionality.
- The PR description should explain your reasoning — why this change matters for Mimi.

Start by exploring the codebase, then pick the single highest-impact improvement."#;

pub fn run() {
    // Find the repo directory
    let repo_dir = find_repo_dir();

    // Ensure every audit starts from a clean origin/master. Without this, the
    // nightly cron can end up creating today's audit branch on top of an
    // unmerged previous audit branch — stacking PRs and mixing yesterday's
    // proposal into today's diff.
    println!("Syncing repo to origin/master before audit...");
    if let Err(e) = sync_to_master(&repo_dir) {
        eprintln!("Error: {e}");
        eprintln!("Aborting audit — refusing to run on a stale or dirty base.");
        std::process::exit(1);
    }

    println!("Running self-audit on codebase...\n");

    let status = Command::new("claude")
        .args([
            "--print",
            "--dangerously-skip-permissions",
            AUDIT_PROMPT,
        ])
        .current_dir(&repo_dir)
        .status()
        .expect("failed to run claude — is it installed?");

    if status.success() {
        println!("\nAudit complete. Check GitHub for any new PRs.");
    } else {
        eprintln!("Audit failed.");
        std::process::exit(1);
    }
}

/// Fetch and hard-reset the working tree to `origin/master` so the audit
/// always builds its branch from the latest published mainline.
fn sync_to_master(repo: &Path) -> Result<(), String> {
    git(repo, &["fetch", "origin", "master"])
        .map_err(|e| format!("git fetch origin master failed: {e}"))?;
    git(repo, &["checkout", "master"])
        .map_err(|e| format!("git checkout master failed: {e}"))?;
    git(repo, &["reset", "--hard", "origin/master"])
        .map_err(|e| format!("git reset --hard origin/master failed: {e}"))?;
    Ok(())
}

fn git(repo: &Path, args: &[&str]) -> Result<(), String> {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .map_err(|e| format!("failed to spawn git: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("git {} exited with status {}", args.join(" "), status))
    }
}

pub fn find_repo_dir() -> std::path::PathBuf {
    // Check common locations
    let candidates = [
        dirs::home_dir().map(|h| h.join("mimi-brain-interface")),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.parent().unwrap_or(p).to_path_buf())),
    ];

    for candidate in candidates.into_iter().flatten() {
        if candidate.join("Cargo.toml").exists() {
            return candidate;
        }
    }

    // Fallback: try current directory
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.join("Cargo.toml").exists() {
        return cwd;
    }

    eprintln!("Could not find mimi-brain-interface repo. Run from the repo directory.");
    std::process::exit(1);
}
