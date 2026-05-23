use sqlx::Row;
use tauri::State;

use crate::db::license::{self, License};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Trial standard: 30 zile.
const TRIAL_DAYS: i64 = 30;

#[tauri::command]
pub async fn get_license(state: State<'_, AppState>) -> AppResult<Option<License>> {
    license::get(&state.db).await
}

#[tauri::command]
pub async fn check_license_validity(
    state: State<'_, AppState>,
) -> AppResult<bool> {
    let row = sqlx::query("SELECT expires_at FROM license WHERE id = 1")
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::Database)?;
    let now = chrono::Utc::now().timestamp();
    Ok(row.map(|r| {
        r.try_get::<i64, _>("expires_at")
            .map(|exp| exp > now)
            .unwrap_or(false)
    }).unwrap_or(false))
}

#[tauri::command]
pub async fn start_trial(state: State<'_, AppState>, email: String) -> AppResult<License> {
    let machine_id = current_machine_id();
    license::start_trial(&state.db, &email, &machine_id, TRIAL_DAYS).await
}

#[tauri::command]
pub async fn activate_license(
    state: State<'_, AppState>,
    key: String,
    email: String,
) -> AppResult<License> {
    // TODO: validare cloud reală. Pentru moment: tier=SOLO, 1 an.
    let machine_id = current_machine_id();
    let one_year = chrono::Utc::now().timestamp() + 365 * 86_400;
    license::activate(&state.db, &key, "SOLO", one_year, &email, &machine_id).await
}

/// Identificator stabil al mașinii (host + OS). Înlocuit ulterior cu o
/// soluție mai robustă (de ex. machine-uid crate).
fn current_machine_id() -> String {
    let host = hostname();
    format!("{}-{}", host, std::env::consts::OS)
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".into())
}
