use crate::paths;
use std::path::PathBuf;
use std::process::Command;

pub struct BackupInfo {
    pub archive: PathBuf,
    pub size_bytes: u64,
}

/// Tar up ~/.mimi/ (minus the backups dir itself) into a timestamped archive.
/// Returns the archive path + size on success. Never exits the process — the
/// dashboard handler depends on this so a tar failure doesn't kill the server.
pub fn create_backup() -> Result<BackupInfo, String> {
    let home = paths::home();
    let backups = paths::backups_dir();
    std::fs::create_dir_all(&backups)
        .map_err(|e| format!("failed to create backups dir {}: {e}", backups.display()))?;

    let parent = home
        .parent()
        .ok_or_else(|| format!("mimi home {} has no parent dir", home.display()))?;
    let home_name = home
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("mimi home {} has no file name", home.display()))?;

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let archive = backups.join(format!("mimi_backup_{}.tar.gz", timestamp));
    let archive_str = archive
        .to_str()
        .ok_or_else(|| format!("archive path {} is not valid UTF-8", archive.display()))?;
    let parent_str = parent
        .to_str()
        .ok_or_else(|| format!("parent path {} is not valid UTF-8", parent.display()))?;
    let exclude = format!("{home_name}/backups");

    let status = Command::new("tar")
        .args([
            "czf",
            archive_str,
            "-C",
            parent_str,
            home_name,
            "--exclude",
            &exclude,
        ])
        .status()
        .map_err(|e| format!("failed to spawn tar: {e}"))?;

    if !status.success() {
        // Best-effort cleanup: tar may have left a partial archive behind.
        let _ = std::fs::remove_file(&archive);
        return Err(match status.code() {
            Some(code) => format!("tar exited with status {code}"),
            None => "tar terminated by signal".to_string(),
        });
    }

    let size_bytes = std::fs::metadata(&archive)
        .map(|m| m.len())
        .unwrap_or(0);
    Ok(BackupInfo {
        archive,
        size_bytes,
    })
}

pub fn run() {
    match create_backup() {
        Ok(info) => {
            println!("Backup created: {}", info.archive.display());
            println!("Size: {:.1} KB", info.size_bytes as f64 / 1024.0);
        }
        Err(e) => {
            eprintln!("Backup failed: {e}");
            std::process::exit(1);
        }
    }
}
