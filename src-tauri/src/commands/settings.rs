use std::collections::HashMap;
use tauri::State;

use crate::db::settings;
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub async fn get_setting(state: State<'_, AppState>, key: String) -> AppResult<Option<String>> {
    settings::get(&state.db, &key).await
}

#[tauri::command]
pub async fn set_setting(state: State<'_, AppState>, key: String, value: String) -> AppResult<()> {
    settings::set(&state.db, &key, &value).await
}

#[tauri::command]
pub async fn get_all_settings(state: State<'_, AppState>) -> AppResult<HashMap<String, String>> {
    let rows = settings::get_all(&state.db).await?;
    Ok(rows.into_iter().collect())
}
