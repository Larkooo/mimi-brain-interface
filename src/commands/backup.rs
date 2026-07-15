use crate::paths;
use std::process::Command;

const BACKUP_PREFIX: &str = "mimi_backup_";
const BACKUP_SUFFIX: &str = ".tar.gz";

/// Default number of recent backups to retain. Nightly cron + 14 = ~two weeks
/// of history before the oldest tarball is pruned.
const DEFAULT_KEEP: usize = 14;

pub fn run(keep: Option<usize>) {
    let keep = keep.unwrap_or(DEFAULT_KEEP).max(1);
    let home = paths::home();
    let backups = paths::backups_dir();
    std::fs::create_dir_all(&backups).ok();

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let archive = backups.join(format!("{BACKUP_PREFIX}{timestamp}{BACKUP_SUFFIX}"));

    // `--exclude` is positional in GNU tar — it only applies to file args
    // that come *after* it on the command line. Put it before the `.mimi`
    // positional so the backups dir is actually excluded; otherwise every
    // new tarball includes every old tarball and disk usage explodes.
    let status = Command::new("tar")
        .args([
            "czf",
            archive.to_str().unwrap(),
            "-C",
            home.parent().unwrap().to_str().unwrap(),
            "--exclude=.mimi/backups",
            ".mimi",
        ])
        .status()
        .expect("failed to create backup");

    if !status.success() {
        eprintln!("Backup failed");
        std::process::exit(1);
    }

    let size = std::fs::metadata(&archive).map(|m| m.len()).unwrap_or(0);
    println!("Backup created: {}", archive.display());
    println!("Size: {}", format_bytes(size));

    let pruned = prune_old_backups(&backups, keep);
    if pruned > 0 {
        println!("Pruned {pruned} old backup(s) — keeping {keep} most recent.");
    }
}

/// Delete `mimi_backup_*.tar.gz` files beyond the `keep` newest. Filenames
/// embed a zero-padded `%Y%m%d_%H%M%S` timestamp, so lexical sort is the
/// same as chronological order — no need to stat each file for mtime.
fn prune_old_backups(dir: &std::path::Path, keep: usize) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut archives: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(BACKUP_PREFIX) && n.ends_with(BACKUP_SUFFIX))
        })
        .collect();
    archives.sort();
    if archives.len() <= keep {
        return 0;
    }
    let prune_count = archives.len() - keep;
    let mut removed = 0;
    for path in archives.iter().take(prune_count) {
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
        } else {
            eprintln!("Warning: failed to prune {}", path.display());
        }
    }
    removed
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
