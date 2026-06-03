//! Tauri commands for stock movements (MovementOfGoods SAF-T section).
//! MVP: manual entry capture. UI is out of scope for P6.

use tauri::State;

use crate::db::stock::{self, StockMovementInput, StockMovementWithLines};
use crate::error::AppResult;
use crate::state::AppState;

/// Create a new stock movement with lines.
#[tauri::command]
pub async fn create_stock_movement(
    state: State<'_, AppState>,
    company_id: String,
    input: StockMovementInput,
) -> AppResult<StockMovementWithLines> {
    stock::create(&state.db, &company_id, input).await
}

/// List stock movements for a company in a date range.
#[tauri::command]
pub async fn list_stock_movements(
    state: State<'_, AppState>,
    company_id: String,
    date_from: String,
    date_to: String,
) -> AppResult<Vec<StockMovementWithLines>> {
    stock::list(&state.db, &company_id, &date_from, &date_to).await
}

/// Delete a stock movement (cascades to lines).
#[tauri::command]
pub async fn delete_stock_movement(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    stock::delete(&state.db, &id, &company_id).await
}
