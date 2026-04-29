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

    println!("\nStart a channel bot with: mimi channel start <type>");
}

pub fn add(channel_type: &str) -> Result<(), String> {
    let dir = paths::channels_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create channels directory {}: {}", dir.display(), e))?;

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
        // presence is not a Claude Code plugin — it's a long-running gateway
        // bridge that keeps a user account showing as online. Setup is via
        // the secret vault + a systemd unit, not the channel configure flow.
        "presence" => serde_json::json!({
            "type": "presence",
            "enabled": true,
            "notes": "1. Store your Discord user token: mimi secret set discord_user_token <token>\n2. (Optional) Edit ~/.mimi/channels/presence/schedule.json to set online windows\n3. Run under systemd (recommended):\n     ExecStart=/usr/local/bin/mimi secret run discord_user_token DISCORD_USER_TOKEN /usr/local/bin/mimi channel start presence\n   or one-shot: mimi secret run discord_user_token DISCORD_USER_TOKEN mimi channel start presence"
        }),
        _ => serde_json::json!({
            "type": channel_type,
            "enabled": true,
        }),
    };

    let path = dir.join(format!("{}.json", channel_type));
    fs::write(&path, serde_json::to_string_pretty(&config).unwrap())
        .map_err(|e| format!("Failed to write channel config {}: {}", path.display(), e))?;
    println!("\nChannel added: {}", channel_type);
    println!("Config: {}", path.display());

    if let Some(notes) = config.get("notes").and_then(|v| v.as_str()) {
        println!("\nNext steps:");
        for line in notes.lines() {
            println!("  {}", line);
        }
    }
    Ok(())
}

/// Configure a channel with a bot token
/// Writes the token to ~/.claude/channels/<type>/.env
pub fn configure(channel_type: &str, token: &str) -> Result<(), String> {
    let env_var = match channel_type {
        "telegram" => "TELEGRAM_BOT_TOKEN",
        "discord" => "DISCORD_BOT_TOKEN",
        _ => {
            return Err(format!("Unknown channel type for token configuration: {}", channel_type));
        }
    };

    // Write to ~/.claude/channels/<type>/.env (where the Claude Code plugin expects it)
    let claude_channel_dir = dirs::home_dir()
        .expect("no home dir")
        .join(".claude")
        .join("channels")
        .join(channel_type);
    fs::create_dir_all(&claude_channel_dir)
        .map_err(|e| format!("Failed to create channel directory {}: {}", claude_channel_dir.display(), e))?;

    let env_path = claude_channel_dir.join(".env");
    fs::write(&env_path, format!("{}={}\n", env_var, token))
        .map_err(|e| format!("Failed to write bot token to {}: {}", env_path.display(), e))?;

    // Also mark the channel as configured in our config
    let config_path = paths::channels_dir().join(format!("{}.json", channel_type));
    if let Ok(content) = fs::read_to_string(&config_path) {
        if let Ok(mut config) = serde_json::from_str::<serde_json::Value>(&content) {
            config["configured"] = serde_json::json!(true);
            fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap())
                .map_err(|e| format!("Failed to update channel config {}: {}", config_path.display(), e))?;
        }
    }

    println!("Configured {} with bot token", channel_type);
    println!("Token written to: {}", env_path.display());
    Ok(())
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

