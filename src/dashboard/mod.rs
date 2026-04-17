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
        .route("/api/brain/entities/{id}", delete(api_brain_delete_entity))
        .route("/api/brain/relationships/add", post(api_brain_add_relationship))
        .route("/api/brain/search", get(api_brain_search))
        .route("/api/brain/query", post(api_brain_query))
        .route("/api/brain/graph", get(api_brain_graph))
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
        // Memory
        .route("/api/memory", get(api_memory_list))
        .route("/api/memory/{filename}", get(api_memory_file))
        // Crons
        .route("/api/crons", get(api_crons_list).post(api_crons_create))
        .route("/api/crons/{id}", delete(api_crons_delete))
        .route("/api/crons/{id}/toggle", post(api_crons_toggle))
        // Secrets
        .route("/api/secrets", get(api_secrets_list).post(api_secrets_set))
        .route("/api/secrets/{name}", delete(api_secrets_delete))
        // Logs
        .route("/api/logs", get(api_logs_list))
        .route("/api/logs/{name}", get(api_logs_tail))
        // Services (systemd user units)
        .route("/api/services", get(api_services_list))
        .route("/api/services/{name}/start", post(api_services_start))
        .route("/api/services/{name}/stop", post(api_services_stop))
        .route("/api/services/{name}/restart", post(api_services_restart))
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
    session_name: String,
    model: String,
    dashboard_port: u16,
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
    let model = config.get("model").and_then(|v| v.as_str()).unwrap_or("sonnet").to_string();
    let dashboard_port = config.get("dashboard_port").and_then(|v| v.as_u64()).unwrap_or(3131) as u16;

    Ok(Json(StatusResponse {
        name,
        session_name: session.to_string(),
        model,
        dashboard_port,
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
    let id = brain::add_entity(&db, &body.r#type, &body.name, &body.properties)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "id": id, "name": body.name, "type": body.r#type })))
}

async fn api_brain_delete_entity(
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = brain::open();
    brain::delete_entity(&db, id)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::json!({ "ok": true, "deleted": id })))
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
    let id = brain::add_relationship(&db, body.source_id, &body.r#type, body.target_id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
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
    let rows = brain::raw_query(&db, sql)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(rows))
}

async fn api_brain_graph() -> Result<Json<brain::GraphData>, (StatusCode, String)> {
    let db = brain::open();
    let graph = brain::get_graph(&db)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(graph))
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
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    commands::channel::add(&body.r#type)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    // If a token was provided, configure it immediately
    if let Some(token) = &body.token {
        if !token.is_empty() {
            commands::channel::configure(&body.r#type, token)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        }
    }
    Ok(Json(serde_json::json!({ "ok": true, "channel": body.r#type })))
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
    fs::write(&path, serde_json::to_string_pretty(&config).unwrap())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to write config: {}", e)))?;
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
    commands::channel::configure(&name, &body.token)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
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

async fn api_session_launch() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let config = load_config();
    let session = config.get("session_name").and_then(|v| v.as_str()).unwrap_or("mimi");
    crate::claude::launch_tmux(session)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true, "session": session })))
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

async fn api_mcp_list() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = crate::claude::plugin_list_output()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "output": output })))
}

// --- Backup ---

async fn api_backup() -> Json<serde_json::Value> {
    commands::backup::run();
    Json(serde_json::json!({ "ok": true }))
}

// --- Memory ---

#[derive(Serialize)]
struct MemoryFileInfo {
    name: String,
    description: String,
    r#type: String,
    filename: String,
}

/// Parse simple YAML frontmatter from a markdown file.
/// Extracts `name`, `description`, and `type` fields from the `---` block.
fn parse_frontmatter(content: &str) -> (String, String, String) {
    let mut name = String::new();
    let mut description = String::new();
    let mut mem_type = String::new();

    if content.starts_with("---") {
        if let Some(end) = content[3..].find("---") {
            let block = &content[3..3 + end];
            for line in block.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("name:") {
                    name = val.trim().to_string();
                } else if let Some(val) = line.strip_prefix("description:") {
                    description = val.trim().to_string();
                } else if let Some(val) = line.strip_prefix("type:") {
                    mem_type = val.trim().to_string();
                }
            }
        }
    }
    (name, description, mem_type)
}

async fn api_memory_list() -> Json<Vec<MemoryFileInfo>> {
    let dir = paths::memory_dir();
    let mut files: Vec<MemoryFileInfo> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let path = e.path();
            path.extension().is_some_and(|ext| ext == "md")
                && path.file_name().is_some_and(|n| n != "MEMORY.md")
        })
        .filter_map(|entry| {
            let filename = entry.file_name().to_string_lossy().to_string();
            let content = fs::read_to_string(entry.path()).ok()?;
            let (name, description, mem_type) = parse_frontmatter(&content);
            Some(MemoryFileInfo {
                name: if name.is_empty() {
                    filename.trim_end_matches(".md").to_string()
                } else {
                    name
                },
                description,
                r#type: mem_type,
                filename,
            })
        })
        .collect();
    files.sort_by(|a, b| a.filename.cmp(&b.filename));
    Json(files)
}

#[derive(Serialize)]
struct MemoryContent {
    content: String,
}

async fn api_memory_file(
    axum::extract::Path(filename): axum::extract::Path<String>,
) -> Result<Json<MemoryContent>, StatusCode> {
    // Prevent path traversal
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = paths::memory_dir().join(&filename);
    let content = fs::read_to_string(&path).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(MemoryContent { content }))
}

// --- Crons ---

#[derive(Serialize, Deserialize, Clone)]
struct CronJob {
    id: String,
    name: String,
    schedule: String,
    prompt: String,
    #[serde(default)]
    description: String,
    #[serde(default = "crons_default_enabled")]
    enabled: bool,
}

fn crons_default_enabled() -> bool { true }

fn crons_path() -> std::path::PathBuf {
    paths::home().join("crons.json")
}

fn load_crons() -> Vec<CronJob> {
    fs::read_to_string(crons_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_crons(crons: &[CronJob]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(crons).map_err(|e| e.to_string())?;
    fs::write(crons_path(), json).map_err(|e| e.to_string())
}

async fn api_crons_list() -> Json<Vec<CronJob>> { Json(load_crons()) }

#[derive(Deserialize)]
struct CreateCronBody { name: String, schedule: String, prompt: String, #[serde(default)] description: String }

async fn api_crons_create(Json(body): Json<CreateCronBody>)
    -> Result<Json<CronJob>, (StatusCode, String)>
{
    let mut crons = load_crons();
    let job = CronJob {
        id: format!("{}", chrono::Utc::now().timestamp_millis()),
        name: body.name,
        schedule: body.schedule,
        prompt: body.prompt,
        description: body.description,
        enabled: true,
    };
    crons.push(job.clone());
    save_crons(&crons).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(job))
}

async fn api_crons_delete(axum::extract::Path(id): axum::extract::Path<String>)
    -> Result<Json<serde_json::Value>, (StatusCode, String)>
{
    let mut crons = load_crons();
    let len_before = crons.len();
    crons.retain(|c| c.id != id);
    if crons.len() == len_before {
        return Err((StatusCode::NOT_FOUND, format!("cron {id} not found")));
    }
    save_crons(&crons).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn api_crons_toggle(axum::extract::Path(id): axum::extract::Path<String>)
    -> Result<Json<serde_json::Value>, (StatusCode, String)>
{
    let mut crons = load_crons();
    let job = crons.iter_mut().find(|c| c.id == id)
        .ok_or((StatusCode::NOT_FOUND, format!("cron {id} not found")))?;
    job.enabled = !job.enabled;
    let enabled = job.enabled;
    save_crons(&crons).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "ok": true, "enabled": enabled })))
}

// --- Secrets ---

#[derive(Serialize)]
struct SecretEntry { name: String, created_at: String }

async fn api_secrets_list() -> Json<Vec<SecretEntry>> {
    let entries = commands::secret::list_json()
        .into_iter()
        .map(|(name, created_at)| SecretEntry { name, created_at })
        .collect();
    Json(entries)
}

#[derive(Deserialize)]
struct SetSecretBody { name: String, value: String }

async fn api_secrets_set(Json(body): Json<SetSecretBody>)
    -> Result<Json<serde_json::Value>, (StatusCode, String)>
{
    if body.name.is_empty() || body.value.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name and value required".into()));
    }
    commands::secret::set(&body.name, &body.value);
    Ok(Json(serde_json::json!({ "ok": true, "name": body.name })))
}

async fn api_secrets_delete(axum::extract::Path(name): axum::extract::Path<String>)
    -> Json<serde_json::Value>
{
    commands::secret::delete(&name);
    Json(serde_json::json!({ "ok": true }))
}

// --- Logs ---

const LOG_FILES: &[(&str, &str)] = &[
    ("telegram", "/tmp/mimi-telegram.log"),
    ("discord", "/tmp/mimi-discord.log"),
    ("update", "/tmp/mimi-update.log"),
    ("audit", "/tmp/mimi-audit.log"),
    ("reflect", "/tmp/mimi-reflect.log"),
    ("dashboard", "/tmp/mimi-dashboard.log"),
];

#[derive(Serialize)]
struct LogEntry { name: String, path: String, size: u64, exists: bool }

async fn api_logs_list() -> Json<Vec<LogEntry>> {
    let entries = LOG_FILES.iter()
        .map(|(name, path)| {
            let meta = fs::metadata(path).ok();
            LogEntry {
                name: name.to_string(),
                path: path.to_string(),
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                exists: meta.is_some(),
            }
        })
        .collect();
    Json(entries)
}

async fn api_logs_tail(axum::extract::Path(name): axum::extract::Path<String>)
    -> Result<Json<serde_json::Value>, (StatusCode, String)>
{
    let path = LOG_FILES.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
        .ok_or((StatusCode::NOT_FOUND, format!("unknown log: {name}")))?;
    let contents = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = contents.lines().collect();
    let tail_start = lines.len().saturating_sub(500);
    let text = lines[tail_start..].join("\n");
    Ok(Json(serde_json::json!({ "name": name, "path": path, "text": text, "lines": lines.len() })))
}

// --- Services (systemd user) ---

const MANAGED_SERVICES: &[&str] = &["mimi-telegram", "mimi-discord", "mimi-dashboard"];

#[derive(Serialize)]
struct ServiceInfo {
    name: String,
    active_state: String,
    sub_state: String,
    main_pid: Option<u32>,
    enabled: bool,
}

fn systemctl_user(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("systemctl")
        .arg("--user").args(args).output().ok()?;
    if !out.status.success() { return None; }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

fn service_info(name: &str) -> ServiceInfo {
    let show = systemctl_user(&["show", name, "--no-page"]).unwrap_or_default();
    let mut active_state = String::from("unknown");
    let mut sub_state = String::from("unknown");
    let mut main_pid: Option<u32> = None;
    for line in show.lines() {
        if let Some(v) = line.strip_prefix("ActiveState=") { active_state = v.into(); }
        else if let Some(v) = line.strip_prefix("SubState=") { sub_state = v.into(); }
        else if let Some(v) = line.strip_prefix("MainPID=") { main_pid = v.parse().ok().filter(|p| *p != 0); }
    }
    let enabled = systemctl_user(&["is-enabled", name])
        .map(|s| s.trim() == "enabled")
        .unwrap_or(false);
    ServiceInfo { name: name.into(), active_state, sub_state, main_pid, enabled }
}

async fn api_services_list() -> Json<Vec<ServiceInfo>> {
    Json(MANAGED_SERVICES.iter().map(|n| service_info(n)).collect())
}

fn is_managed(name: &str) -> bool { MANAGED_SERVICES.contains(&name) }

async fn api_services_start(axum::extract::Path(name): axum::extract::Path<String>)
    -> Result<Json<serde_json::Value>, (StatusCode, String)>
{
    if !is_managed(&name) { return Err((StatusCode::FORBIDDEN, "unknown service".into())); }
    systemctl_user(&["start", &name])
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, format!("systemctl start {name} failed")))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn api_services_stop(axum::extract::Path(name): axum::extract::Path<String>)
    -> Result<Json<serde_json::Value>, (StatusCode, String)>
{
    if !is_managed(&name) { return Err((StatusCode::FORBIDDEN, "unknown service".into())); }
    systemctl_user(&["stop", &name])
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, format!("systemctl stop {name} failed")))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn api_services_restart(axum::extract::Path(name): axum::extract::Path<String>)
    -> Result<Json<serde_json::Value>, (StatusCode, String)>
{
    if !is_managed(&name) { return Err((StatusCode::FORBIDDEN, "unknown service".into())); }
    systemctl_user(&["restart", &name])
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, format!("systemctl restart {name} failed")))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- Helpers ---

fn load_config() -> serde_json::Value {
    fs::read_to_string(paths::config_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}))
}
