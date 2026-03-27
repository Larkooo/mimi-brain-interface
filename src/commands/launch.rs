use crate::paths;
use crate::commands::channel;

pub fn run() {
    if !paths::brain_db().exists() {
        eprintln!("Mimi is not set up yet. Run `mimi setup` first.");
        std::process::exit(1);
    }

    let config = load_config();
    let session_name = config
        .get("session_name")
        .and_then(|v| v.as_str())
        .unwrap_or("mimi");

    let channels = channel::enabled_channel_flags();
    crate::claude::launch_tmux(session_name, &channels);
}

fn load_config() -> serde_json::Value {
    std::fs::read_to_string(paths::config_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}))
}
