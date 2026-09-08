use crate::paths;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

#[derive(Serialize)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub size: u64,
}

pub fn create_backup() -> Result<BackupInfo, String> {
    let home = paths::home();
    let backups = paths::backups_dir();
    std::fs::create_dir_all(&backups)
        .map_err(|e| format!("create backups dir {}: {e}", backups.display()))?;

    let parent = home
        .parent()
        .ok_or_else(|| format!("home {} has no parent", home.display()))?;
    let parent_str = parent
        .to_str()
        .ok_or_else(|| format!("home parent {} is not valid utf-8", parent.display()))?;

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let archive = backups.join(format!("mimi_backup_{}.tar.gz", timestamp));
    let archive_str = archive
        .to_str()
        .ok_or_else(|| format!("archive path {} is not valid utf-8", archive.display()))?;

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
        .map_err(|e| format!("failed to exec tar: {e}"))?;

    if !status.success() {
        return Err(format!("tar exited with status {status}"));
    }

    let size = std::fs::metadata(&archive)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(BackupInfo { path: archive, size })
}

pub fn run() {
    match create_backup() {
        Ok(info) => {
            println!("Backup created: {}", info.path.display());
            println!("Size: {:.1} KB", info.size as f64 / 1024.0);
        }
        Err(e) => {
            eprintln!("Backup failed: {e}");
            std::process::exit(1);
        }
    }
}
