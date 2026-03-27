use crate::paths;
use std::fs;

pub fn list() {
    let dir = paths::channels_dir();
    if !dir.exists() {
        println!("No channels configured.");
        return;
    }

    let entries: Vec<_> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();

    if entries.is_empty() {
        println!("No channels configured.");
        return;
    }

    println!("=== Channels ===\n");
    for entry in entries {
        let name = entry.path().file_stem().unwrap().to_string_lossy().to_string();
        if let Ok(content) = fs::read_to_string(entry.path()) {
            if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                let channel_type = config.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");
                let enabled = config.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                let status = if enabled { "enabled" } else { "disabled" };
                println!("  {:15} {:12} {}", name, channel_type, status);
                continue;
            }
        }
        println!("  {}", name);
    }
}

pub fn add(channel_type: &str) {
    let dir = paths::channels_dir();
    fs::create_dir_all(&dir).ok();

    let config = match channel_type {
        "telegram" => serde_json::json!({
            "type": "telegram",
            "enabled": true,
            "bot_token": "",
            "notes": "Set bot_token to your Telegram bot token from @BotFather"
        }),
        "imessage" => serde_json::json!({
            "type": "imessage",
            "enabled": true,
            "notes": "Requires macOS with Messages.app configured"
        }),
        _ => serde_json::json!({
            "type": channel_type,
            "enabled": true,
        }),
    };

    let path = dir.join(format!("{}.json", channel_type));
    fs::write(&path, serde_json::to_string_pretty(&config).unwrap()).ok();
    println!("Added channel: {}", channel_type);
    println!("Config: {}", path.display());
    println!("\nEdit the config file to complete setup.");
}

pub fn remove(name: &str) {
    let path = paths::channels_dir().join(format!("{}.json", name));
    if path.exists() {
        fs::remove_file(&path).ok();
        println!("Removed channel: {}", name);
    } else {
        eprintln!("Channel '{}' not found.", name);
    }
}
