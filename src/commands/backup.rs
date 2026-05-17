use crate::paths;
use std::path::PathBuf;
use std::process::Command;

pub struct BackupReport {
    pub archive: PathBuf,
    pub size_bytes: u64,
}

/// Tar up `~/.mimi/` (excluding the backups dir itself) into a timestamped
/// `.tar.gz` under `~/.mimi/backups/`. Returns the archive path + size on
/// success, or a human-readable error. Never panics or calls `exit()` — the
/// dashboard `/api/backup` handler shares this code path and used to take the
/// whole server down on tar failure.
pub fn run() -> Result<BackupReport, String> {
    let home = paths::home();
    let parent = home
        .parent()
        .ok_or_else(|| format!("mimi home has no parent: {}", home.display()))?;
    let parent_str = parent
        .to_str()
        .ok_or_else(|| format!("mimi home parent is not valid UTF-8: {}", parent.display()))?;

    let backups = paths::backups_dir();
    std::fs::create_dir_all(&backups)
        .map_err(|e| format!("create {}: {e}", backups.display()))?;

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let archive = backups.join(format!("mimi_backup_{}.tar.gz", timestamp));
    let archive_str = archive
        .to_str()
        .ok_or_else(|| format!("archive path is not valid UTF-8: {}", archive.display()))?;

    let status = Command::new("tar")
        .args([
            "czf",
            archive_str,
            "-C",
            parent_str,
            ".mimi",
            "--exclude",
            ".mimi/backups",
        ])
        .status()
        .map_err(|e| format!("exec tar: {e}"))?;

    if !status.success() {
        return Err(format!(
            "tar exited with status {}",
            status.code().map(|c| c.to_string()).unwrap_or_else(|| "(signal)".into())
        ));
    }

    let size_bytes = std::fs::metadata(&archive)
        .map(|m| m.len())
        .unwrap_or(0);
    Ok(BackupReport { archive, size_bytes })
}

/// CLI entrypoint. Prints the success banner or the error, then exits 1 on
/// failure. The pure `run()` above is shared with the dashboard handler.
pub fn cli_run() {
    match run() {
        Ok(report) => {
            println!("Backup created: {}", report.archive.display());
            println!("Size: {:.1} KB", report.size_bytes as f64 / 1024.0);
        }
        Err(e) => {
            eprintln!("Backup failed: {e}");
            std::process::exit(1);
        }
    }
}
