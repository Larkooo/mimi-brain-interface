use crate::paths;
use std::path::Path;
use std::process::Command;

/// How many backup snapshots to keep on disk. Older ones are pruned after a
/// successful new snapshot. Each snapshot includes brain.db + memory + configs,
/// so unbounded retention would slowly fill the data partition when the
/// dashboard button or a cron drives this regularly.
const MAX_BACKUPS: usize = 10;

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
        prune_old_backups(&backups, MAX_BACKUPS);
    } else {
        eprintln!("Backup failed");
        std::process::exit(1);
    }
}

/// Keep the most recent `keep` `mimi_backup_*.tar.gz` snapshots in `dir`;
/// delete older ones. The filename timestamp format (`%Y%m%d_%H%M%S`) sorts
/// lexicographically in chronological order, so a sorted ascending list puts
/// the oldest first.
fn prune_old_backups(dir: &Path, keep: usize) {
    let mut snapshots: Vec<std::path::PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    n.starts_with("mimi_backup_") && n.ends_with(".tar.gz")
                })
            })
            .collect(),
        Err(_) => return,
    };
    if snapshots.len() <= keep {
        return;
    }
    snapshots.sort();
    let to_drop = snapshots.len() - keep;
    let mut pruned = 0usize;
    for path in snapshots.into_iter().take(to_drop) {
        if std::fs::remove_file(&path).is_ok() {
            pruned += 1;
        }
    }
    if pruned > 0 {
        println!("Pruned {} old backup(s) (keeping last {})", pruned, keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"").unwrap();
    }

    #[test]
    fn prune_keeps_newest_and_ignores_unrelated_files() {
        let tmp = tempdir();
        // Older first — filename timestamps sort lexicographically.
        for ts in [
            "20250101_010101",
            "20250102_010101",
            "20250103_010101",
            "20250104_010101",
            "20250105_010101",
        ] {
            touch(&tmp, &format!("mimi_backup_{ts}.tar.gz"));
        }
        // Should not be touched.
        touch(&tmp, "notes.txt");
        touch(&tmp, "mimi_backup_partial.tmp");

        prune_old_backups(&tmp, 2);

        let mut remaining: Vec<String> = fs::read_dir(&tmp)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        remaining.sort();
        assert_eq!(
            remaining,
            vec![
                "mimi_backup_20250104_010101.tar.gz".to_string(),
                "mimi_backup_20250105_010101.tar.gz".to_string(),
                "mimi_backup_partial.tmp".to_string(),
                "notes.txt".to_string(),
            ]
        );

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn prune_noop_when_under_cap() {
        let tmp = tempdir();
        touch(&tmp, "mimi_backup_20250101_010101.tar.gz");
        touch(&tmp, "mimi_backup_20250102_010101.tar.gz");

        prune_old_backups(&tmp, 10);

        assert_eq!(fs::read_dir(&tmp).unwrap().count(), 2);
        fs::remove_dir_all(&tmp).ok();
    }

    fn tempdir() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("mimi-backup-test-{nanos}"));
        fs::create_dir_all(&p).unwrap();
        p
    }
}
