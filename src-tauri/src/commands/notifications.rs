use tauri::State;

use crate::db::notifications::{self, Notification};
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub async fn list_notifications(
    state: State<'_, AppState>,
    only_unread: Option<bool>,
) -> AppResult<Vec<Notification>> {
    notifications::list(&state.db, only_unread.unwrap_or(false)).await
}

#[tauri::command]
pub async fn unread_notification_count(state: State<'_, AppState>) -> AppResult<i64> {
    notifications::count_unread(&state.db).await
}

#[tauri::command]
pub async fn mark_notification_read(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<()> {
    notifications::mark_read(&state.db, &id).await
}

#[tauri::command]
pub async fn mark_all_notifications_read(state: State<'_, AppState>) -> AppResult<()> {
    notifications::mark_all_read(&state.db).await
}

#[tauri::command]
pub async fn delete_notification(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<()> {
    notifications::delete_notification(&state.db, &id).await
}

#[tauri::command]
pub async fn delete_all_read_notifications(
    state: State<'_, AppState>,
) -> AppResult<u64> {
    notifications::delete_all_read(&state.db).await
}
