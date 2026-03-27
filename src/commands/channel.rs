use crate::paths;
use std::fs;

/// Known channel plugins and their Claude Code plugin identifiers
const CHANNEL_PLUGINS: &[(&str, &str)] = &[
    ("telegram", "telegram@claude-plugins-official"),
    ("discord", "discord@claude-plugins-official"),
    ("imessage", "imessage@claude-plugins-official"),
];

fn plugin_for_channel(channel_type: &str) -> Option<&'static str> {
    CHANNEL_PLUGINS
        .iter()
        .find(|(name, _)| *name == channel_type)
        .map(|(_, plugin)| *plugin)
}

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
        let name = entry
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        if let Ok(content) = fs::read_to_string(entry.path()) {
            if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                let channel_type =
                    config.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");
                let enabled = config.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                let plugin = config
                    .get("plugin")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");
                let status = if enabled { "enabled" } else { "disabled" };
                println!("  {:12} {:12} plugin:{}  [{}]", name, channel_type, plugin, status);
                continue;
            }
        }
        println!("  {}", name);
    }

    println!("\nEnabled channels will be passed as --channels to Claude Code on launch.");
}

pub fn add(channel_type: &str) {
    let dir = paths::channels_dir();
    fs::create_dir_all(&dir).ok();

    let plugin = plugin_for_channel(channel_type);

    // Install the Claude Code plugin if known
    if let Some(plugin_id) = plugin {
        println!("Installing Claude Code plugin: {}", plugin_id);
        crate::claude::plugin_install(plugin_id);
    }

    let config = match channel_type {
        "telegram" => serde_json::json!({
            "type": "telegram",
            "plugin": plugin.unwrap_or(""),
            "enabled": true,
            "notes": "1. Get a bot token from @BotFather on Telegram\n2. Run: mimi channel configure telegram <bot_token>\n3. Relaunch mimi to connect"
        }),
        "discord" => serde_json::json!({
            "type": "discord",
            "plugin": plugin.unwrap_or(""),
            "enabled": true,
            "notes": "1. Create a Discord app at https://discord.com/developers\n2. Enable Message Content Intent under Bot settings\n3. Generate and copy the bot token\n4. Run: mimi channel configure discord <bot_token>\n5. Invite bot to server with OAuth2 URL Generator (bot scope)\n6. Relaunch mimi to connect"
        }),
        "imessage" => serde_json::json!({
            "type": "imessage",
            "plugin": plugin.unwrap_or(""),
            "enabled": true,
            "notes": "Requires macOS with Messages.app configured. Run /imessage:configure in Claude Code."
        }),
        _ => serde_json::json!({
            "type": channel_type,
            "enabled": true,
        }),
    };

    let path = dir.join(format!("{}.json", channel_type));
    fs::write(&path, serde_json::to_string_pretty(&config).unwrap()).ok();
    println!("\nChannel added: {}", channel_type);
    println!("Config: {}", path.display());

    if plugin.is_some() {
        println!("\nNext steps:");
        if let Some(notes) = config.get("notes").and_then(|v| v.as_str()) {
            for line in notes.lines() {
                println!("  {}", line);
            }
        }
    }
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

/// Get the list of --channels flags for enabled channels
pub fn enabled_channel_flags() -> Vec<String> {
    let dir = paths::channels_dir();
    if !dir.exists() {
        return vec![];
    }

    let mut flags = vec![];
    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                    let enabled = config
                        .get("enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if enabled {
                        if let Some(plugin) = config.get("plugin").and_then(|v| v.as_str()) {
                            if !plugin.is_empty() {
                                flags.push(format!("plugin:{}", plugin));
                            }
                        }
                    }
                }
            }
        }
    }
    flags
}
