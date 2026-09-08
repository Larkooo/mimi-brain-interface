use crate::paths;
use std::process::Command;

pub fn run() {
    let home = paths::home();
    let backups = paths::backups_dir();
    std::fs::create_dir_all(&backups).ok();

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let archive = backups.join(format!("mimi_backup_{}.tar.gz", timestamp));

    // NOTE: `--exclude` is positional in GNU tar — it only applies to operands
    // that follow it. Passing it after `.mimi` makes tar warn "has no effect"
    // and exit 2, which killed every backup. Keep it ahead of the member name.
    let status = Command::new("tar")
        .args([
            "czf",
            archive.to_str().unwrap(),
            "--exclude=.mimi/backups",
            "-C",
            home.parent().unwrap().to_str().unwrap(),
            ".mimi",
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
        // tar writes a partial archive before bailing out. Leaving it behind is
        // worse than no backup at all — it looks like a restore point.
        let _ = std::fs::remove_file(&archive);
        eprintln!("Backup failed: tar exited with {}", status);
        std::process::exit(1);
    }
}
