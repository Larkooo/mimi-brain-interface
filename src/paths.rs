use std::fs;
use std::path::PathBuf;

pub fn home() -> PathBuf {
    dirs::home_dir().expect("no home directory").join(".mimi")
}

pub fn brain_db() -> PathBuf {
    home().join("brain.db")
}

pub fn memory_dir() -> PathBuf {
    home().join("memory")
}

pub fn memory_index() -> PathBuf {
    memory_dir().join("MEMORY.md")
}

pub fn accounts_dir() -> PathBuf {
    home().join("accounts")
}

pub fn channels_dir() -> PathBuf {
    home().join("channels")
}

pub fn config_file() -> PathBuf {
    home().join("config.json")
}

pub fn backups_dir() -> PathBuf {
    home().join("backups")
}

pub fn claude_md() -> PathBuf {
    home().join("CLAUDE.md")
}

pub fn ensure_dirs() {
    for dir in [home(), memory_dir(), accounts_dir(), channels_dir(), backups_dir()] {
        fs::create_dir_all(&dir).ok();
    }
}
