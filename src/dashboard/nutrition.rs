use axum::{
    Json,
    extract::Path,
    http::StatusCode,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::brain;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS nutrition_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user TEXT NOT NULL DEFAULT 'larko',
  meal_date TEXT NOT NULL,
  logged_at TEXT DEFAULT (datetime('now','localtime')),
  food_text TEXT NOT NULL,
  calories REAL NOT NULL,
  protein_g REAL DEFAULT 0,
  carbs_g REAL DEFAULT 0,
  fat_g REAL DEFAULT 0,
  source TEXT DEFAULT 'chat'
);
CREATE INDEX IF NOT EXISTS idx_nutr_date ON nutrition_log(user, meal_date);

CREATE TABLE IF NOT EXISTS nutrition_goals (
  user TEXT PRIMARY KEY,
  tdee INTEGER,
  target_cals INTEGER,
  target_protein_g INTEGER,
  target_carbs_g INTEGER,
  target_fat_g INTEGER,
  weight_kg REAL,
  height_cm INTEGER,
  bodyfat_pct REAL,
  phase TEXT DEFAULT 'lean_bulk',
  updated_at TEXT DEFAULT (datetime('now','localtime'))
);
"#;

pub fn ensure_schema() {
    let db = brain::open();
    db.execute_batch(SCHEMA).ok();
}

#[derive(Serialize)]
pub struct Meal {
    id: i64,
    meal_date: String,
    logged_at: String,
    food_text: String,
    calories: f64,
    protein_g: f64,
    carbs_g: f64,
    fat_g: f64,
    source: String,
}

#[derive(Serialize, Default)]
pub struct Totals {
    calories: f64,
    protein_g: f64,
    carbs_g: f64,
    fat_g: f64,
    meals_count: i64,
}

#[derive(Serialize)]
pub struct DaySummary {
    date: String,
    user: String,
    totals: Totals,
    meals: Vec<Meal>,
}

#[derive(Serialize, Default)]
pub struct Goals {
    user: String,
    tdee: Option<i64>,
    target_cals: Option<i64>,
    target_protein_g: Option<i64>,
    target_carbs_g: Option<i64>,
    target_fat_g: Option<i64>,
    weight_kg: Option<f64>,
    height_cm: Option<i64>,
    bodyfat_pct: Option<f64>,
    phase: Option<String>,
    updated_at: Option<String>,
}

#[derive(Serialize)]
pub struct DayRow {
    date: String,
    cal: f64,
    prot: f64,
    carbs: f64,
    fat: f64,
    meals: i64,
}

#[derive(Serialize)]
pub struct Trend {
    days: Vec<DayRow>,
    avg: Totals,
}

fn user_param(user: Option<String>) -> String {
    user.unwrap_or_else(|| "larko".into())
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn query_day(user: &str, date: &str) -> Result<DaySummary, String> {
    let db = brain::open();
    let mut stmt = db
        .prepare(
            "SELECT id, meal_date, logged_at, food_text, calories, protein_g, carbs_g, fat_g, source
             FROM nutrition_log WHERE user=?1 AND meal_date=?2
             ORDER BY logged_at",
        )
        .map_err(|e| e.to_string())?;
    let meals: Vec<Meal> = stmt
        .query_map(params![user, date], |r| {
            Ok(Meal {
                id: r.get(0)?,
                meal_date: r.get(1)?,
                logged_at: r.get(2)?,
                food_text: r.get(3)?,
                calories: r.get(4)?,
                protein_g: r.get(5)?,
                carbs_g: r.get(6)?,
                fat_g: r.get(7)?,
                source: r.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let mut totals = Totals::default();
    for m in &meals {
        totals.calories += m.calories;
        totals.protein_g += m.protein_g;
        totals.carbs_g += m.carbs_g;
        totals.fat_g += m.fat_g;
    }
    totals.meals_count = meals.len() as i64;

    Ok(DaySummary {
        date: date.to_string(),
        user: user.to_string(),
        totals,
        meals,
    })
}

fn query_trend(user: &str, days: i64) -> Result<Trend, String> {
    let db = brain::open();
    let mut stmt = db
        .prepare(
            "SELECT meal_date,
                    ROUND(SUM(calories),1) AS cal,
                    ROUND(SUM(protein_g),1) AS prot,
                    ROUND(SUM(carbs_g),1) AS carbs,
                    ROUND(SUM(fat_g),1) AS fat,
                    COUNT(*) AS meals
             FROM nutrition_log
             WHERE user=?1 AND meal_date >= date('now','localtime', ?2)
             GROUP BY meal_date
             ORDER BY meal_date",
        )
        .map_err(|e| e.to_string())?;
    let offset = format!("-{} days", days - 1);
    let day_rows: Vec<DayRow> = stmt
        .query_map(params![user, offset], |r| {
            Ok(DayRow {
                date: r.get(0)?,
                cal: r.get(1)?,
                prot: r.get(2)?,
                carbs: r.get(3)?,
                fat: r.get(4)?,
                meals: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let n = day_rows.len().max(1) as f64;
    let avg = Totals {
        calories: (day_rows.iter().map(|d| d.cal).sum::<f64>() / n).round(),
        protein_g: (day_rows.iter().map(|d| d.prot).sum::<f64>() / n).round(),
        carbs_g: (day_rows.iter().map(|d| d.carbs).sum::<f64>() / n).round(),
        fat_g: (day_rows.iter().map(|d| d.fat).sum::<f64>() / n).round(),
        meals_count: day_rows.iter().map(|d| d.meals).sum(),
    };

    Ok(Trend { days: day_rows, avg })
}

fn query_goals(user: &str) -> Result<Goals, String> {
    let db = brain::open();
    let mut stmt = db
        .prepare(
            "SELECT user, tdee, target_cals, target_protein_g, target_carbs_g, target_fat_g,
                    weight_kg, height_cm, bodyfat_pct, phase, updated_at
             FROM nutrition_goals WHERE user=?1",
        )
        .map_err(|e| e.to_string())?;
    stmt.query_row(params![user], |r| {
        Ok(Goals {
            user: r.get(0)?,
            tdee: r.get(1)?,
            target_cals: r.get(2)?,
            target_protein_g: r.get(3)?,
            target_carbs_g: r.get(4)?,
            target_fat_g: r.get(5)?,
            weight_kg: r.get(6)?,
            height_cm: r.get(7)?,
            bodyfat_pct: r.get(8)?,
            phase: r.get(9)?,
            updated_at: r.get(10)?,
        })
    })
    .map_err(|e| e.to_string())
}

// --- Handlers ---

#[derive(Deserialize)]
pub struct UserQ {
    user: Option<String>,
}

pub async fn api_today(
    axum::extract::Query(q): axum::extract::Query<UserQ>,
) -> Result<Json<DaySummary>, (StatusCode, String)> {
    ensure_schema();
    query_day(&user_param(q.user), &today())
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub async fn api_day(
    Path(date): Path<String>,
    axum::extract::Query(q): axum::extract::Query<UserQ>,
) -> Result<Json<DaySummary>, (StatusCode, String)> {
    ensure_schema();
    query_day(&user_param(q.user), &date)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub async fn api_week(
    axum::extract::Query(q): axum::extract::Query<UserQ>,
) -> Result<Json<Trend>, (StatusCode, String)> {
    ensure_schema();
    query_trend(&user_param(q.user), 7)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub async fn api_month(
    axum::extract::Query(q): axum::extract::Query<UserQ>,
) -> Result<Json<Trend>, (StatusCode, String)> {
    ensure_schema();
    query_trend(&user_param(q.user), 30)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub async fn api_goals(
    axum::extract::Query(q): axum::extract::Query<UserQ>,
) -> Result<Json<Goals>, (StatusCode, String)> {
    ensure_schema();
    query_goals(&user_param(q.user))
        .map(Json)
        .map_err(|e| (StatusCode::NOT_FOUND, e))
}

#[derive(Deserialize)]
pub struct SetGoalsBody {
    #[serde(default)]
    user: Option<String>,
    tdee: Option<i64>,
    target_cals: Option<i64>,
    target_protein_g: Option<i64>,
    target_carbs_g: Option<i64>,
    target_fat_g: Option<i64>,
    weight_kg: Option<f64>,
    height_cm: Option<i64>,
    bodyfat_pct: Option<f64>,
    phase: Option<String>,
}

pub async fn api_set_goals(
    Json(body): Json<SetGoalsBody>,
) -> Result<Json<Goals>, (StatusCode, String)> {
    ensure_schema();
    let user = user_param(body.user);
    let db = brain::open();
    db.execute(
        "INSERT INTO nutrition_goals
           (user, tdee, target_cals, target_protein_g, target_carbs_g, target_fat_g,
            weight_kg, height_cm, bodyfat_pct, phase, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10, datetime('now','localtime'))
         ON CONFLICT(user) DO UPDATE SET
           tdee=COALESCE(excluded.tdee,tdee),
           target_cals=COALESCE(excluded.target_cals,target_cals),
           target_protein_g=COALESCE(excluded.target_protein_g,target_protein_g),
           target_carbs_g=COALESCE(excluded.target_carbs_g,target_carbs_g),
           target_fat_g=COALESCE(excluded.target_fat_g,target_fat_g),
           weight_kg=COALESCE(excluded.weight_kg,weight_kg),
           height_cm=COALESCE(excluded.height_cm,height_cm),
           bodyfat_pct=COALESCE(excluded.bodyfat_pct,bodyfat_pct),
           phase=COALESCE(excluded.phase,phase),
           updated_at=datetime('now','localtime')",
        params![
            user,
            body.tdee,
            body.target_cals,
            body.target_protein_g,
            body.target_carbs_g,
            body.target_fat_g,
            body.weight_kg,
            body.height_cm,
            body.bodyfat_pct,
            body.phase,
        ],
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    query_goals(&user)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[derive(Deserialize)]
pub struct LogBody {
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    meal_date: Option<String>,
    food_text: String,
    calories: f64,
    #[serde(default)]
    protein_g: f64,
    #[serde(default)]
    carbs_g: f64,
    #[serde(default)]
    fat_g: f64,
    #[serde(default)]
    source: Option<String>,
}

pub async fn api_log(
    Json(body): Json<LogBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    ensure_schema();
    let user = user_param(body.user);
    let date = body.meal_date.unwrap_or_else(today);
    let source = body.source.unwrap_or_else(|| "web".into());
    let db = brain::open();
    db.execute(
        "INSERT INTO nutrition_log
           (user, meal_date, food_text, calories, protein_g, carbs_g, fat_g, source)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![user, date, body.food_text, body.calories, body.protein_g, body.carbs_g, body.fat_g, source],
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let id = db.last_insert_rowid();
    Ok(Json(serde_json::json!({"id": id, "date": date})))
}

pub async fn api_delete_log(
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    ensure_schema();
    let db = brain::open();
    let n = db
        .execute("DELETE FROM nutrition_log WHERE id=?1", params![id])
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(serde_json::json!({"deleted": n})))
}
