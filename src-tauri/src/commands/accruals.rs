//! Tauri commands — Accruals (cheltuieli/venituri înregistrate în avans, 471/472,
//! OMFP 1802/2014 pct. 351).

use tauri::State;

use crate::db::accruals::{self, Accrual, CreateAccrualInput};
use crate::db::gl::RegisterPostResult;
use crate::error::AppResult;
use crate::state::AppState;

/// Creează un accrual și postează constituirea (D 471 / C 6xx pentru cheltuieli în avans,
/// D 7xx / C 472 pentru venituri în avans).
#[tauri::command]
pub async fn create_accrual(
    state: State<'_, AppState>,
    input: CreateAccrualInput,
) -> AppResult<Accrual> {
    accruals::create(&state.db, input).await
}

/// Listează accrual-urile companiei (descrescător după perioada de start).
#[tauri::command]
pub async fn list_accruals(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<Accrual>> {
    accruals::list(&state.db, &company_id).await
}

/// Șterge un accrual (guard multi-tenant) și nota de constituire aferentă.
#[tauri::command]
pub async fn delete_accrual(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    accruals::delete(&state.db, &id, &company_id).await
}

/// Recunoaște tranșa lunară a tuturor accrual-urilor active în perioada dată (AAAA-LL),
/// într-o singură notă contabilă echilibrată (idempotent per perioadă).
#[tauri::command]
pub async fn run_accruals(
    state: State<'_, AppState>,
    company_id: String,
    period: String,
) -> AppResult<RegisterPostResult> {
    accruals::run_accruals(&state.db, &company_id, &period).await
}
