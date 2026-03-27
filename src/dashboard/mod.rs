use axum::{
    Router,
    extract::Json,
    http::StatusCode,
    response::Html,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use crate::brain;
use crate::paths;

pub async fn serve(port: u16) {
    let app = Router::new()
        .route("/", get(index))
        .route("/api/status", get(api_status))
        .route("/api/brain/stats", get(api_brain_stats))
        .route("/api/brain/entities", get(api_brain_entities))
        .route("/api/brain/search", get(api_brain_search))
        .route("/api/brain/query", post(api_brain_query));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .expect("failed to bind");

    axum::serve(listener, app).await.expect("server failed");
}

async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

#[derive(Serialize)]
struct StatusResponse {
    name: String,
    session_running: bool,
    claude_version: String,
    brain_stats: brain::Stats,
    memory_files: usize,
}

async fn api_status() -> Json<StatusResponse> {
    let config: serde_json::Value = std::fs::read_to_string(paths::config_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));

    let name = config.get("name").and_then(|v| v.as_str()).unwrap_or("Mimi").to_string();
    let session = config.get("session_name").and_then(|v| v.as_str()).unwrap_or("mimi");

    let session_running = std::process::Command::new("tmux")
        .args(["has-session", "-t", session])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let claude_version = crate::claude::version();

    let db = brain::open();
    let brain_stats = brain::get_stats(&db);

    let memory_files = std::fs::read_dir(paths::memory_dir())
        .map(|d| d.filter(|e| {
            e.as_ref().map(|e| e.path().extension().is_some_and(|ext| ext == "md")).unwrap_or(false)
        }).count())
        .unwrap_or(0);

    Json(StatusResponse {
        name,
        session_running,
        claude_version,
        brain_stats,
        memory_files,
    })
}

async fn api_brain_stats() -> Json<brain::Stats> {
    let db = brain::open();
    Json(brain::get_stats(&db))
}

#[derive(Deserialize)]
struct SearchParams {
    q: Option<String>,
    r#type: Option<String>,
}

async fn api_brain_entities(
    axum::extract::Query(params): axum::extract::Query<SearchParams>,
) -> Json<Vec<brain::Entity>> {
    let db = brain::open();
    Json(brain::find_entities(&db, params.r#type.as_deref()))
}

async fn api_brain_search(
    axum::extract::Query(params): axum::extract::Query<SearchParams>,
) -> Result<Json<Vec<brain::Entity>>, StatusCode> {
    let q = params.q.ok_or(StatusCode::BAD_REQUEST)?;
    let db = brain::open();
    Ok(Json(brain::search_entities(&db, &q)))
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
