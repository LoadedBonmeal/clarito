//! Tauri commands pentru catalogul editabil de cote TVA.
//!
//! Toate comenzile operează pe tabelul GLOBAL `vat_rates` (fără company_id).
//! Cotele TVA sunt reglementate la nivel național în România și se aplică
//! uniform tuturor companiilor — nu există scoping pe company_id.

use tauri::State;

use crate::db::vat_rates::{self, UpdateVatRateInput, VatRate, VatRateInput};
use crate::error::AppResult;
use crate::state::AppState;

/// R15 Wave 2: List VAT rates from the global catalog.
/// Pass `active_only = true` to get only rates available for use in line items.
#[tauri::command]
pub async fn list_vat_rates(
    state: State<'_, AppState>,
    active_only: Option<bool>,
) -> AppResult<Vec<VatRate>> {
    vat_rates::list(&state.db, active_only.unwrap_or(false)).await
}

/// R15 Wave 2: Get a single VAT rate by id.
#[tauri::command]
pub async fn get_vat_rate(state: State<'_, AppState>, id: String) -> AppResult<VatRate> {
    vat_rates::get(&state.db, &id).await
}

/// R15 Wave 2: Create a new VAT rate entry.
#[tauri::command]
pub async fn create_vat_rate(
    state: State<'_, AppState>,
    input: VatRateInput,
) -> AppResult<VatRate> {
    vat_rates::create(&state.db, input).await
}

/// R15 Wave 2: Update an existing VAT rate entry.
#[tauri::command]
pub async fn update_vat_rate(
    state: State<'_, AppState>,
    id: String,
    input: UpdateVatRateInput,
) -> AppResult<VatRate> {
    vat_rates::update(&state.db, &id, input).await
}

/// R15 Wave 2: Delete a VAT rate entry by id.
#[tauri::command]
pub async fn delete_vat_rate(state: State<'_, AppState>, id: String) -> AppResult<()> {
    vat_rates::delete(&state.db, &id).await
}

/// R15 Wave 2: Activate or deactivate a VAT rate entry.
#[tauri::command]
pub async fn set_vat_rate_active(
    state: State<'_, AppState>,
    id: String,
    active: bool,
) -> AppResult<VatRate> {
    vat_rates::set_active(&state.db, &id, active).await
}
