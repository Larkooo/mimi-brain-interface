use crate::paths;
use std::path::PathBuf;
use std::process::Command;

pub struct Backup {
    pub path: PathBuf,
    pub size_bytes: u64,
}

/// Snapshot ~/.mimi to a timestamped tar.gz under ~/.mimi/backups/.
/// Errors are returned (not panicked) so callers in long-lived processes
/// like the dashboard can surface them without taking the server down.
pub fn create() -> Result<Backup, String> {
    let home = paths::home();
    let backups = paths::backups_dir();
    std::fs::create_dir_all(&backups)
        .map_err(|e| format!("failed to create backups dir {}: {e}", backups.display()))?;

    let parent = home
        .parent()
        .ok_or_else(|| format!("can't resolve parent of {}", home.display()))?;
    let dir_name = home
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("home dir has no file name: {}", home.display()))?;

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let archive = backups.join(format!("mimi_backup_{}.tar.gz", timestamp));

    let archive_str = archive
        .to_str()
        .ok_or("archive path is not valid UTF-8")?;
    let parent_str = parent
        .to_str()
        .ok_or("home parent path is not valid UTF-8")?;
    let exclude = format!("{dir_name}/backups");

    // --exclude must come BEFORE the positional path on GNU tar — otherwise
    // tar prints "has no effect" and exits with status 2.
    let status = Command::new("tar")
        .args([
            "--exclude", &exclude, "-czf", archive_str, "-C", parent_str, dir_name,
        ])
        .status()
        .map_err(|e| format!("failed to invoke tar: {e}"))?;

    if !status.success() {
        return Err(format!("tar exited with status {status}"));
    }

    let size_bytes = std::fs::metadata(&archive)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(Backup { path: archive, size_bytes })
}

pub fn run() {
    match create() {
        Ok(b) => {
            println!("Backup created: {}", b.path.display());
            println!("Size: {:.1} KB", b.size_bytes as f64 / 1024.0);
        }
        Err(e) => {
            eprintln!("Backup failed: {e}");
            std::process::exit(1);
        }
    }
}
