//! Tauri commands — Provizioane (class 15x, OMFP 1802/2014 pct. 374).

use tauri::State;

use crate::db::provisions::{self, CreateProvisionInput, Provision};
use crate::error::AppResult;
use crate::state::AppState;

/// Constituie un provizion (după confirmarea celor 3 condiții cumulative) și postează D 6812 / C 15x.
#[tauri::command]
pub async fn create_provision(
    state: State<'_, AppState>,
    input: CreateProvisionInput,
) -> AppResult<Provision> {
    provisions::create(&state.db, input).await
}

/// Listează provizioanele companiei.
#[tauri::command]
pub async fn list_provisions(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<Provision>> {
    provisions::list(&state.db, &company_id).await
}

/// Reluare/utilizare provizion: D 15x / C 7812 în perioada dată (AAAA-LL).
#[tauri::command]
pub async fn reverse_provision(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    period: String,
) -> AppResult<Provision> {
    provisions::reverse(&state.db, &id, &company_id, &period).await
}

/// Șterge un provizion (guard multi-tenant) și notele GL aferente.
#[tauri::command]
pub async fn delete_provision(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    provisions::delete(&state.db, &id, &company_id).await
}
