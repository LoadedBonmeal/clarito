//! Tauri commands — condică de prezență (pontaj lunar per angajat).

use tauri::State;

use crate::db::pontaj::{self, CreatePontajInput, Pontaj, UpdatePontajInput};
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub async fn list_pontaje(
    state: State<'_, AppState>,
    company_id: String,
    period: String,
) -> AppResult<Vec<Pontaj>> {
    pontaj::list(&state.db, &company_id, &period).await
}

#[tauri::command]
pub async fn create_pontaj(
    state: State<'_, AppState>,
    input: CreatePontajInput,
) -> AppResult<Pontaj> {
    pontaj::create(&state.db, input).await
}

#[tauri::command]
pub async fn update_pontaj(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: UpdatePontajInput,
) -> AppResult<Pontaj> {
    pontaj::update(&state.db, &id, &company_id, input).await
}

#[tauri::command]
pub async fn delete_pontaj(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    pontaj::delete(&state.db, &id, &company_id).await
}
