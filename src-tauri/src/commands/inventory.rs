//! Tauri commands — inventariere + registru-inventar.
//!
//! Thin dispatch layer to `db::inventory`. All arguments validated at the IPC boundary
//! (valid dates, non-empty required strings).

use tauri::State;

use crate::commands::require_valid_date;
use crate::db::inventory::{
    self, CreateSessionInput, InventoryLine, InventorySession, RegistruInventarEntry,
    UpdateLineFapticInput,
};
use crate::error::AppResult;
use crate::state::AppState;

// ─── Session CRUD ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn create_inventory_session(
    state: State<'_, AppState>,
    input: CreateSessionInput,
) -> AppResult<InventorySession> {
    require_valid_date("Data de referință", &input.reference_date)?;
    inventory::create_session(&state.db, input).await
}

#[tauri::command]
pub async fn get_inventory_session(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<InventorySession> {
    inventory::get_session(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn list_inventory_sessions(
    state: State<'_, AppState>,
    company_id: String,
    fiscal_year: Option<i64>,
) -> AppResult<Vec<InventorySession>> {
    inventory::list_sessions(&state.db, &company_id, fiscal_year).await
}

#[tauri::command]
pub async fn delete_inventory_session(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    inventory::delete_session(&state.db, &id, &company_id).await
}

// ─── Lines ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_inventory_lines(
    state: State<'_, AppState>,
    session_id: String,
    company_id: String,
) -> AppResult<Vec<InventoryLine>> {
    inventory::list_lines(&state.db, &session_id, &company_id).await
}

#[tauri::command]
pub async fn update_inventory_line_faptic(
    state: State<'_, AppState>,
    input: UpdateLineFapticInput,
) -> AppResult<InventoryLine> {
    inventory::update_line_faptic(&state.db, input).await
}

// ─── Pre-fill + Finalize + GL posting ────────────────────────────────────────

#[tauri::command]
pub async fn prefill_inventory_session(
    state: State<'_, AppState>,
    session_id: String,
    company_id: String,
) -> AppResult<Vec<InventoryLine>> {
    inventory::prefill_session_lines(&state.db, &session_id, &company_id).await
}

#[tauri::command]
pub async fn finalize_inventory_session(
    state: State<'_, AppState>,
    session_id: String,
    company_id: String,
) -> AppResult<InventorySession> {
    inventory::finalize_session(&state.db, &session_id, &company_id).await
}

/// Post neimputabil inventory diffs to GL (D 607 = C stoc / D stoc = C 607).
/// Imputabil, TVA adjustment, and perisabilități cases are DEFERRED — post those
/// via the manual journal (Contabilitate → Note manuale).
#[tauri::command]
pub async fn post_inventory_diffs(
    state: State<'_, AppState>,
    session_id: String,
    company_id: String,
) -> AppResult<()> {
    inventory::post_inventory_diffs(&state.db, &session_id, &company_id).await
}

// ─── Registru-inventar ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_registru_inventar(
    state: State<'_, AppState>,
    company_id: String,
    fiscal_year: i64,
) -> AppResult<Vec<RegistruInventarEntry>> {
    inventory::list_registru_entries(&state.db, &company_id, fiscal_year).await
}
