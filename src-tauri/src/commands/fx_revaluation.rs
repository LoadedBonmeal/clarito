//! Reevaluare valutară lunară — comenzi Tauri.
//!
//! Expune două comenzi:
//! - `compute_fx_revaluation` — calculează + postează reevaluarea pentru o perioadă.
//! - `list_fx_revaluations`   — listează rândurile de reevaluare pentru o perioadă.

use tauri::State;

use crate::db::fx_revaluation::{
    compute_fx_revaluation as db_compute, list_fx_revaluations as db_list, FxRevaluationResult,
    FxRevaluationRow,
};
use crate::error::AppResult;
use crate::state::AppState;

/// Calculează și postează reevaluarea valutară pentru luna `period` ("YYYY-MM").
///
/// Idempotentă: re-rularea înlocuiește nota GL + rândurile existente pentru perioadă.
/// Nu trebuie confirmată a doua oară dacă utilizatorul re-rulează.
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

/// Listează rândurile de reevaluare pentru perioada `period` ("YYYY-MM").
#[tauri::command]
pub async fn list_fx_revaluations(
    state: State<'_, AppState>,
    company_id: String,
    period: String,
) -> AppResult<Vec<FxRevaluationRow>> {
    db_list(&state.db, &company_id, &period).await
}
