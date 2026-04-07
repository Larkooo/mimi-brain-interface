use axum::{
    Router,
    extract::Json,
    http::StatusCode,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use crate::brain;
use crate::commands;
use crate::paths;
use std::fs;

pub async fn serve(port: u16) {
    // Try to find the React build directory
    // Check: cwd/dashboard/dist, exe/../../../dashboard/dist, ~/.mimi/dashboard/dist
    let candidates = [
        std::env::current_dir().ok().map(|d| d.join("dashboard/dist")),
        std::env::current_exe().ok().and_then(|p| p.parent()?.parent()?.parent().map(|p| p.join("dashboard/dist"))),
        dirs::home_dir().map(|d| d.join(".mimi/dashboard/dist")),
    ];
    let dashboard_dist = candidates.into_iter().flatten().find(|p| p.exists());

    let mut app = Router::new()
        // Status
        .route("/api/status", get(api_status))
        // Brain
        .route("/api/brain/stats", get(api_brain_stats))
        .route("/api/brain/entities", get(api_brain_entities))
        .route("/api/brain/entities/add", post(api_brain_add_entity))
        .route("/api/brain/relationships/add", post(api_brain_add_relationship))
        .route("/api/brain/search", get(api_brain_search))
        .route("/api/brain/query", post(api_brain_query))
        // Channels
        .route("/api/channels", get(api_channels_list))
        .route("/api/channels/add", post(api_channels_add))
        .route("/api/channels/{name}", delete(api_channels_remove))
        .route("/api/channels/{name}/toggle", post(api_channels_toggle))
        .route("/api/channels/{name}/configure", post(api_channels_configure))
        // Config
        .route("/api/config", get(api_config_get))
        .route("/api/config", post(api_config_set))
        // Session
        .route("/api/session/launch", post(api_session_launch))
        .route("/api/session/stop", post(api_session_stop))
        // MCP
        .route("/api/mcp/list", get(api_mcp_list))
        // Backup
        .route("/api/backup", post(api_backup));

    // Serve React build
    if let Some(ref dist) = dashboard_dist {
        let index_path = dist.join("index.html");
        app = app.fallback_service(
            tower_http::services::ServeDir::new(dist)
                .fallback(tower_http::services::ServeFile::new(index_path)),
        );
    } else {
        eprintln!("Warning: dashboard/dist not found. Run 'cd dashboard && bun run build' first.");
        eprintln!("Searched: cwd/dashboard/dist, exe dir, ~/.mimi/dashboard/dist");
    }

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .expect("failed to bind");

    axum::serve(listener, app).await.expect("server failed");
}

// --- Status ---

#[derive(Serialize)]
struct StatusResponse {
    name: String,
    session_running: bool,
    claude_version: String,
    brain_stats: brain::Stats,
    memory_files: usize,
    channels: Vec<ChannelInfo>,
}

#[derive(Serialize)]
struct ChannelInfo {
    name: String,
    r#type: String,
    plugin: String,
    enabled: bool,
}

async fn api_status() -> Result<Json<StatusResponse>, (StatusCode, String)> {
    let config = load_config();
    let name = config.get("name").and_then(|v| v.as_str()).unwrap_or("Mimi").to_string();
    let session = config.get("session_name").and_then(|v| v.as_str()).unwrap_or("mimi");

    let session_running = std::process::Command::new("tmux")
        .args(["has-session", "-t", session])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let claude_version = crate::claude::version();
    let db = brain::open();
    let brain_stats = brain::get_stats(&db)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let memory_files = fs::read_dir(paths::memory_dir())
        .map(|d| d.filter(|e| {
            e.as_ref().map(|e| e.path().extension().is_some_and(|ext| ext == "md")).unwrap_or(false)
        }).count())
        .unwrap_or(0);

    let channels = list_channels();

    Ok(Json(StatusResponse {
        name,
        session_running,
        claude_version,
        brain_stats,
        memory_files,
        channels,
    }))
}

// --- Brain ---

async fn api_brain_stats() -> Result<Json<brain::Stats>, (StatusCode, String)> {
    let db = brain::open();
    let stats = brain::get_stats(&db)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(stats))
}

#[derive(Deserialize)]
struct SearchParams {
    q: Option<String>,
    r#type: Option<String>,
}

async fn api_brain_entities(
    axum::extract::Query(params): axum::extract::Query<SearchParams>,
) -> Result<Json<Vec<brain::Entity>>, (StatusCode, String)> {
    let db = brain::open();
    let entities = brain::find_entities(&db, params.r#type.as_deref())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(entities))
}

#[derive(Deserialize)]
struct AddEntityBody {
    r#type: String,
    name: String,
    #[serde(default = "default_props")]
    properties: String,
}

fn default_props() -> String { "{}".to_string() }

async fn api_brain_add_entity(
    Json(body): Json<AddEntityBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = brain::open();
    let id = brain::add_entity(&db, &body.r#type, &body.name, &body.properties);
    Ok(Json(serde_json::json!({ "id": id, "name": body.name, "type": body.r#type })))
}

#[derive(Deserialize)]
struct AddRelBody {
    source_id: i64,
    target_id: i64,
    r#type: String,
}

async fn api_brain_add_relationship(
    Json(body): Json<AddRelBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = brain::open();
    let id = brain::add_relationship(&db, body.source_id, &body.r#type, body.target_id);
    Ok(Json(serde_json::json!({ "id": id })))
}

async fn api_brain_search(
    axum::extract::Query(params): axum::extract::Query<SearchParams>,
) -> Result<Json<Vec<brain::Entity>>, (StatusCode, String)> {
    let q = params.q.ok_or((StatusCode::BAD_REQUEST, "missing 'q' parameter".to_string()))?;
    let db = brain::open();
    let results = brain::search_entities(&db, &q)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(results))
}

#[derive(Deserialize)]
struct QueryBody {
    sql: String,
}

async fn api_brain_query(
    Json(body): Json<QueryBody>,
) -> Result<Json<Vec<Vec<(String, String)>>>, (StatusCode, String)> {
    let sql = body.sql.trim();
    if !(sql.to_uppercase().starts_with("SELECT") || sql.to_uppercase().starts_with("WITH")) {
        return Err((StatusCode::BAD_REQUEST, "Only SELECT/WITH queries allowed via API".to_string()));
    }
    let db = brain::open();
    Ok(Json(brain::raw_query(&db, sql)))
}

// --- Channels ---

fn list_channels() -> Vec<ChannelInfo> {
    let dir = paths::channels_dir();
    if !dir.exists() {
        return vec![];
    }
    fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| {
            let name = entry.path().file_stem()?.to_string_lossy().to_string();
            let content = fs::read_to_string(entry.path()).ok()?;
            let config: serde_json::Value = serde_json::from_str(&content).ok()?;
            Some(ChannelInfo {
                name,
                r#type: config.get("type").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
                plugin: config.get("plugin").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                enabled: config.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
            })
        })
        .collect()
}

async fn api_channels_list() -> Json<Vec<ChannelInfo>> {
    Json(list_channels())
}

#[derive(Deserialize)]
struct AddChannelBody {
    r#type: String,
    token: Option<String>,
}

async fn api_channels_add(
    Json(body): Json<AddChannelBody>,
) -> Json<serde_json::Value> {
    commands::channel::add(&body.r#type);
    // If a token was provided, configure it immediately
    if let Some(token) = &body.token {
        if !token.is_empty() {
            commands::channel::configure(&body.r#type, token);
        }
    }
    Json(serde_json::json!({ "ok": true, "channel": body.r#type }))
}

async fn api_channels_remove(
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    commands::channel::remove(&name);
    Json(serde_json::json!({ "ok": true }))
}

async fn api_channels_toggle(
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let path = paths::channels_dir().join(format!("{}.json", name));
    let content = fs::read_to_string(&path)
        .map_err(|_| (StatusCode::NOT_FOUND, "Channel not found".to_string()))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Bad config".to_string()))?;
    let enabled = config.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    config["enabled"] = serde_json::json!(!enabled);
    fs::write(&path, serde_json::to_string_pretty(&config).unwrap()).ok();
    Ok(Json(serde_json::json!({ "ok": true, "enabled": !enabled })))
}

#[derive(Deserialize)]
struct ConfigureChannelBody {
    token: String,
}

async fn api_channels_configure(
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(body): Json<ConfigureChannelBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if body.token.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Token is required".to_string()));
    }
    commands::channel::configure(&name, &body.token);
    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- Config ---

async fn api_config_get() -> Result<Json<serde_json::Value>, StatusCode> {
    let content = fs::read_to_string(paths::config_file()).map_err(|_| StatusCode::NOT_FOUND)?;
    let config: serde_json::Value =
        serde_json::from_str(&content).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(config))
}

async fn api_config_set(
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    fs::write(
        paths::config_file(),
        serde_json::to_string_pretty(&body).unwrap(),
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- Session ---

async fn api_session_launch() -> Json<serde_json::Value> {
    let config = load_config();
    let session = config.get("session_name").and_then(|v| v.as_str()).unwrap_or("mimi");
    let channels = commands::channel::enabled_channel_flags();
    crate::claude::launch_tmux(session, &channels);
    Json(serde_json::json!({ "ok": true, "session": session }))
}

async fn api_session_stop() -> Json<serde_json::Value> {
    let config = load_config();
    let session = config.get("session_name").and_then(|v| v.as_str()).unwrap_or("mimi");
    let stopped = std::process::Command::new("tmux")
        .args(["kill-session", "-t", session])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    Json(serde_json::json!({ "ok": stopped }))
}

// --- MCP ---

async fn api_mcp_list() -> Json<serde_json::Value> {
    let output = crate::claude::plugin_list_output();
    Json(serde_json::json!({ "output": output }))
}

// --- Backup ---

async fn api_backup() -> Json<serde_json::Value> {
    commands::backup::run();
    Json(serde_json::json!({ "ok": true }))
}

// --- Helpers ---

fn load_config() -> serde_json::Value {
    fs::read_to_string(paths::config_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}))
}
