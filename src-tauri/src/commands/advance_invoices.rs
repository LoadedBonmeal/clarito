//! Tauri commands — FACTURĂ DE AVANS (art. 282 Cod Fiscal).
//!
//! Avansurile emise postează D 4111 = C 419 + C 4427 (nu 707).
//! Avansurile primite postează D 4091 + D 4426 = C 401 (nu 607).
//! La regularizare (factură finală), settlement-ul stornează avansul la rata
//! PROPRIE a avansului (nu rata livrării) — critic pentru traversarea perioadelor.

use tauri::State;

use crate::db::advance_invoices::{
    self, AdvanceInvoiceSettlement, AdvanceReceivedSettlement,
    CreateAdvanceReceivedSettlementInput, CreateAdvanceSettlementInput,
};
use crate::error::AppResult;
use crate::state::AppState;

// ─── Avansuri emise ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn create_advance_settlement(
    state: State<'_, AppState>,
    input: CreateAdvanceSettlementInput,
) -> AppResult<AdvanceInvoiceSettlement> {
    advance_invoices::create_advance_settlement(&state.db, input).await
}

#[tauri::command]
pub async fn list_advance_settlements(
    state: State<'_, AppState>,
    company_id: String,
    final_invoice_id: String,
) -> AppResult<Vec<AdvanceInvoiceSettlement>> {
    advance_invoices::list_advance_settlements_for_final(&state.db, &final_invoice_id, &company_id)
        .await
}

#[tauri::command]
pub async fn get_advance_settlement(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<AdvanceInvoiceSettlement> {
    advance_invoices::get_advance_settlement(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn delete_advance_settlement(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    advance_invoices::delete_advance_settlement(&state.db, &id, &company_id).await
}

// ─── Avansuri primite ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn create_advance_received_settlement(
    state: State<'_, AppState>,
    input: CreateAdvanceReceivedSettlementInput,
) -> AppResult<AdvanceReceivedSettlement> {
    advance_invoices::create_advance_received_settlement(&state.db, input).await
}

#[tauri::command]
pub async fn list_advance_received_settlements(
    state: State<'_, AppState>,
    company_id: String,
    final_received_id: String,
) -> AppResult<Vec<AdvanceReceivedSettlement>> {
    advance_invoices::list_advance_received_settlements_for_final(
        &state.db,
        &final_received_id,
        &company_id,
    )
    .await
}

#[tauri::command]
pub async fn get_advance_received_settlement(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<AdvanceReceivedSettlement> {
    advance_invoices::get_advance_received_settlement(&state.db, &id, &company_id).await
}
