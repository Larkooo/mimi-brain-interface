use crate::paths;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// How many backup archives to retain by default. Override per-instance with
/// a `backup_retention` integer in `~/.mimi/config.json`. Set to 0 to disable
/// pruning. A nightly cron snapshots ~/.mimi, so 14 ≈ two weeks of history.
const DEFAULT_RETENTION: usize = 14;

pub fn run() {
    let home = paths::home();
    let backups = paths::backups_dir();
    fs::create_dir_all(&backups).ok();

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let archive = backups.join(format!("mimi_backup_{}.tar.gz", timestamp));

    let status = Command::new("tar")
        .args([
            "czf",
            archive.to_str().unwrap(),
            "-C",
            home.parent().unwrap().to_str().unwrap(),
            ".mimi",
            "--exclude",
            ".mimi/backups",
        ])
        .status()
        .expect("failed to create backup");

    if status.success() {
        let size = fs::metadata(&archive).map(|m| m.len()).unwrap_or(0);
        println!("Backup created: {}", archive.display());
        println!("Size: {:.1} KB", size as f64 / 1024.0);

        let retention = retention_from_config();
        if retention > 0 {
            match prune(&backups, retention) {
                Ok(removed) if !removed.is_empty() => {
                    println!(
                        "Pruned {} old backup(s) (keeping {} most recent):",
                        removed.len(),
                        retention
                    );
                    for p in removed {
                        println!("  removed {}", p.display());
                    }
                }
                Ok(_) => {}
                Err(e) => eprintln!("Warning: failed to prune old backups: {e}"),
            }
        }
    } else {
        eprintln!("Backup failed");
        std::process::exit(1);
    }
}

fn retention_from_config() -> usize {
    fs::read_to_string(paths::config_file())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("backup_retention").and_then(|n| n.as_u64()))
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_RETENTION)
}

/// Delete `mimi_backup_*.tar.gz` files in `dir` beyond the `keep` most recent
/// (sorted by modified time, newest first). Returns the paths that were
/// removed so the caller can report them.
fn prune(dir: &Path, keep: usize) -> std::io::Result<Vec<PathBuf>> {
    let mut archives: Vec<(PathBuf, std::time::SystemTime)> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_string_lossy().to_string();
            if !name.starts_with("mimi_backup_") || !name.ends_with(".tar.gz") {
                return None;
            }
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((path, mtime))
        })
        .collect();

    // Newest first.
    archives.sort_by(|a, b| b.1.cmp(&a.1));

    let mut removed = Vec::new();
    for (path, _) in archives.into_iter().skip(keep) {
        match fs::remove_file(&path) {
            Ok(()) => removed.push(path),
            Err(e) => eprintln!("Warning: could not remove {}: {e}", path.display()),
        }
    }
    Ok(removed)
}
