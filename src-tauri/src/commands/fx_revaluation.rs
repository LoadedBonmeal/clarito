//! Reevaluare valutară lunară — comenzi Tauri.
//!
//! Expune comenzile:
//! - `compute_fx_revaluation` — calculează + postează reevaluarea pentru o perioadă
//!   (creanțe/datorii 4111/401 + trezorerie 5124/5314).
//! - `list_fx_revaluations` — listează rândurile per factură pentru o perioadă.
//! - `list_fx_treasury_revaluations` — listează rândurile per cont de trezorerie.

use tauri::State;

use crate::db::fx_revaluation::{
    compute_fx_revaluation as db_compute, list_fx_revaluations as db_list,
    list_fx_treasury_revaluations as db_list_treasury, FxRevaluationResult, FxRevaluationRow,
    FxTreasuryRevaluationRow,
};
use crate::error::AppResult;
use crate::state::AppState;

/// Calculează și postează reevaluarea valutară pentru luna `period` ("YYYY-MM").
///
/// Acoperă atât creanțele/datoriile (4111/401) cât și trezoreria (5124/5314).
/// Idempotentă: re-rularea înlocuiește nota GL + rândurile existente pentru perioadă.
#[tauri::command]
pub async fn compute_fx_revaluation(
    state: State<'_, AppState>,
    company_id: String,
    period: String,
) -> AppResult<FxRevaluationResult> {
    // Validăm formatul "YYYY-MM"
    if period.len() != 7 || !period.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return Err(crate::error::AppError::Validation(format!(
            "Perioadă invalidă «{period}» — formatul așteptat este YYYY-MM."
        )));
    }
    db_compute(&state.db, &company_id, &period, None).await
}

/// Listează rândurile de reevaluare per factură pentru perioada `period` ("YYYY-MM").
#[tauri::command]
pub async fn list_fx_revaluations(
    state: State<'_, AppState>,
    company_id: String,
    period: String,
) -> AppResult<Vec<FxRevaluationRow>> {
    db_list(&state.db, &company_id, &period).await
}

/// Listează rândurile de reevaluare per cont de trezorerie (5124/5314) pentru `period`.
#[tauri::command]
pub async fn list_fx_treasury_revaluations(
    state: State<'_, AppState>,
    company_id: String,
    period: String,
) -> AppResult<Vec<FxTreasuryRevaluationRow>> {
    db_list_treasury(&state.db, &company_id, &period).await
}
