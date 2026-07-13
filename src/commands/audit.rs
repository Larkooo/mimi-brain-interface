use std::path::Path;
use std::process::Command;

const AUDIT_PROMPT: &str = r#"You are Mimi's self-improvement agent. Your job is to audit Mimi's own codebase and propose improvements.

You are in the mimi-brain-interface repository. This is Mimi's brain interface — the Rust CLI and React dashboard that manages an autonomous AI assistant.

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

/// How many prior audit branches to surface in the prompt. ~20 covers roughly
/// three weeks of nightly runs — long enough to dodge clear duplicates,
/// short enough that the prompt stays readable.
const RECENT_AUDIT_LIMIT: usize = 20;

pub fn run() {
    // Find the repo directory
    let repo_dir = find_repo_dir();

    // Refresh remote refs so the recent-audit list isn't stale on machines
    // that haven't fetched in a while. Restricted to the audit-branch
    // refspec to keep this cheap and avoid touching anything else.
    let _ = Command::new("git")
        .args([
            "fetch",
            "--quiet",
            "origin",
            "+refs/heads/mimi/audit-*:refs/remotes/origin/mimi/audit-*",
        ])
        .current_dir(&repo_dir)
        .output();

    let recent = recent_audit_titles(&repo_dir, RECENT_AUDIT_LIMIT);
    let prompt = build_prompt(&recent);

    println!("Running self-audit on codebase...\n");
    if !recent.is_empty() {
        println!(
            "Surfacing {} prior audit titles in the prompt to avoid duplicate work.\n",
            recent.len()
        );
    }

    let status = Command::new("claude")
        .args([
            "--print",
            "--dangerously-skip-permissions",
            &prompt,
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

/// Build the prompt text passed to claude. Appends a `## Recent audit branches`
/// section when prior audits exist so the agent picks something orthogonal
/// instead of re-proposing variants of the same fix.
fn build_prompt(recent: &[(String, String)]) -> String {
    if recent.is_empty() {
        return AUDIT_PROMPT.to_string();
    }
    let mut s = String::with_capacity(AUDIT_PROMPT.len() + 1024);
    s.push_str(AUDIT_PROMPT);
    s.push_str("\n\n## Recent audit branches (do NOT repropose these — pick something orthogonal):\n");
    for (date, title) in recent {
        s.push_str(&format!("- {date}: {title}\n"));
    }
    s
}

/// Collect tip-commit subjects of the most recent `origin/mimi/audit-*`
/// branches, sorted newest first. Returns `(date, subject)` pairs where
/// `date` is the suffix on the branch name (e.g. `2026-06-18`).
fn recent_audit_titles(repo: &Path, limit: usize) -> Vec<(String, String)> {
    let out = Command::new("git")
        .args([
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)|%(subject)",
            "refs/remotes/origin/mimi/audit-*",
        ])
        .current_dir(repo)
        .output();
    let stdout = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };
    stdout
        .lines()
        .take(limit)
        .filter_map(|line| {
            let (refname, subject) = line.split_once('|')?;
            let date = refname
                .rsplit('/')
                .next()?
                .trim_start_matches("audit-")
                .to_string();
            Some((date, subject.to_string()))
        })
        .collect()
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
