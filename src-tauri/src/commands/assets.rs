//! Tauri commands for fixed assets (Assets SAF-T MasterFiles section).
//! MVP: manual entry capture. UI is out of scope for P6.

use tauri::State;

use crate::db::assets::{
    self, AssetDepreciationRow, AssetRevaluation, DepreciationRun, FiscalScheduleRow, FixedAsset,
    FixedAssetInput, RevaluationResult,
};
use crate::error::AppResult;
use crate::state::AppState;

/// Create a new fixed asset.
#[tauri::command]
pub async fn create_fixed_asset(
    state: State<'_, AppState>,
    company_id: String,
    input: FixedAssetInput,
) -> AppResult<FixedAsset> {
    assets::create(&state.db, &company_id, input).await
}

/// List all fixed assets for a company.
#[tauri::command]
pub async fn list_fixed_assets(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<FixedAsset>> {
    assets::list(&state.db, &company_id).await
}

/// Delete a fixed asset (cascades to asset_transactions).
#[tauri::command]
pub async fn delete_fixed_asset(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    assets::delete(&state.db, &id, &company_id).await
}

/// Update a fixed asset (partial).
#[tauri::command]
pub async fn update_fixed_asset(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: FixedAssetInput,
) -> AppResult<FixedAsset> {
    assets::update(&state.db, &id, &company_id, input).await
}

/// Run the monthly straight-line depreciation + post 6811/281x to the GL. Idempotent per month.
#[tauri::command]
pub async fn run_depreciation(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<DepreciationRun> {
    assets::run_depreciation(&state.db, &company_id, &period_from, &period_to).await
}

/// Dispose of an asset (de-recognize from the GL: 281x + 6583 / 21x).
#[tauri::command]
pub async fn dispose_asset(
    state: State<'_, AppState>,
    company_id: String,
    asset_id: String,
    disposal_date: String,
) -> AppResult<()> {
    assets::dispose(&state.db, &company_id, &asset_id, &disposal_date).await
}

/// List the recorded monthly depreciation register (optionally one period).
#[tauri::command]
pub async fn list_depreciation(
    state: State<'_, AppState>,
    company_id: String,
    period: Option<String>,
) -> AppResult<Vec<AssetDepreciationRow>> {
    assets::list_depreciation(&state.db, &company_id, period).await
}

/// Compute the annual book + fiscal amortization schedule for one asset (for D101.rd.16 reporting).
/// Returns per-year rows with fiscal_amount, book_amount, and temp_diff (fiscal − book).
#[tauri::command]
pub async fn get_asset_fiscal_schedule(
    state: State<'_, AppState>,
    company_id: String,
    asset_id: String,
) -> AppResult<Vec<FiscalScheduleRow>> {
    let asset = assets::get(&state.db, &asset_id, &company_id).await?;
    assets::compute_fiscal_schedule(&asset)
}

/// Reevaluează un mijloc fix la valoarea justă (OMFP 1802/2014, pct.100 — metoda valorii nete).
/// Postează nota GL (ASSET_REVAL) și înregistrează evenimentul în `asset_revaluations`.
/// Necesită permisiunea PostGl.
#[tauri::command]
pub async fn revalue_fixed_asset(
    state: State<'_, AppState>,
    company_id: String,
    asset_id: String,
    fair_value: String,
    revaluation_date: String,
) -> AppResult<RevaluationResult> {
    assets::revalue_asset(
        &state.db,
        &company_id,
        &asset_id,
        &fair_value,
        &revaluation_date,
    )
    .await
}

/// Listează reevaluările înregistrate pentru un activ sau pentru toți activii companiei.
#[tauri::command]
pub async fn list_asset_revaluations(
    state: State<'_, AppState>,
    company_id: String,
    asset_id: Option<String>,
) -> AppResult<Vec<AssetRevaluation>> {
    assets::list_revaluations(&state.db, &company_id, asset_id.as_deref()).await
}
