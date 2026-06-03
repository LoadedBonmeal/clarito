//! Tauri commands for fixed assets (Assets SAF-T MasterFiles section).
//! MVP: manual entry capture. UI is out of scope for P6.

use tauri::State;

use crate::db::assets::{self, FixedAsset, FixedAssetInput};
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
