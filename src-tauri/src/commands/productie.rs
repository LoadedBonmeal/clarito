//! Tauri commands pentru Producție / BOM (P2 Wave 5).

use tauri::State;

use crate::db::productie::{self, Bom, BomInput, BomWithLines, ProduceInput, ProductieOrder};
use crate::error::AppResult;
use crate::state::AppState;

// ─── BOM commands ──────────────────────────────────────────────────────────────

/// Creează un BOM (cap + linii). Validează produs_finit + componente aparțin companiei.
#[tauri::command]
pub async fn create_bom(
    state: State<'_, AppState>,
    company_id: String,
    input: BomInput,
) -> AppResult<BomWithLines> {
    productie::create_bom(&state.db, &company_id, input).await
}

/// Listează toate rețetele de producție pentru o companie.
#[tauri::command]
pub async fn list_bom(state: State<'_, AppState>, company_id: String) -> AppResult<Vec<Bom>> {
    productie::list_bom(&state.db, &company_id).await
}

/// Returnează un BOM cu liniile sale (guard multi-tenant).
#[tauri::command]
pub async fn get_bom(
    state: State<'_, AppState>,
    company_id: String,
    bom_id: String,
) -> AppResult<BomWithLines> {
    productie::get_bom(&state.db, &company_id, &bom_id).await
}

/// Șterge un BOM. Respinge dacă există ordine de producție asociate.
#[tauri::command]
pub async fn delete_bom(
    state: State<'_, AppState>,
    company_id: String,
    bom_id: String,
) -> AppResult<()> {
    productie::delete_bom(&state.db, &company_id, &bom_id).await
}

/// Actualizează un BOM (delete linii vechi + reinserează).
#[tauri::command]
pub async fn update_bom(
    state: State<'_, AppState>,
    company_id: String,
    bom_id: String,
    input: BomInput,
) -> AppResult<BomWithLines> {
    productie::update_bom(&state.db, &company_id, &bom_id, input).await
}

// ─── Production order commands ─────────────────────────────────────────────────

/// Lansează un ordin de producție (all-or-nothing).
///
/// Consumă componentele (D601=C301 sau D602=C302) și produce produsul finit
/// (D345=C711) la costul materialelor. Respinge dacă stocul oricărei componente
/// este insuficient (verificare pre-consume, nu parțial).
#[tauri::command]
pub async fn produce(
    state: State<'_, AppState>,
    company_id: String,
    input: ProduceInput,
) -> AppResult<ProductieOrder> {
    productie::produce(&state.db, &company_id, input).await
}

/// Listează ordinele de producție (descrescător după dată).
#[tauri::command]
pub async fn list_productie(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<ProductieOrder>> {
    productie::list_productie(&state.db, &company_id).await
}

/// Returnează un ordin de producție (guard multi-tenant).
#[tauri::command]
pub async fn get_productie(
    state: State<'_, AppState>,
    company_id: String,
    order_id: String,
) -> AppResult<ProductieOrder> {
    productie::get_productie(&state.db, &company_id, &order_id).await
}
