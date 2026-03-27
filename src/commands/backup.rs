use crate::paths;
use std::process::Command;

pub fn run() {
    let home = paths::home();
    let backups = paths::backups_dir();
    std::fs::create_dir_all(&backups).ok();

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
        let size = std::fs::metadata(&archive)
            .map(|m| m.len())
            .unwrap_or(0);
        println!("Backup created: {}", archive.display());
        println!("Size: {:.1} KB", size as f64 / 1024.0);
    } else {
        eprintln!("Backup failed");
        std::process::exit(1);
    }
}
