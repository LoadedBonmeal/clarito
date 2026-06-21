//! Tauri commands for inter-gestiune stock transfers (bon de transfer 14-3-3A).

use tauri::State;

use crate::db::stock_transfer::{self, StockTransfer, TransferInput};
use crate::error::AppResult;
use crate::state::AppState;

/// Execute a stock transfer from one gestiune to another.
///
/// Validates: from ≠ to, both gestiuni owned by company, product owned,
/// qty > 0, on-hand in from_gestiune ≥ qty. The transfer is GL-neutral:
/// no 607 turnover is generated (only analytic gestiune movement).
#[tauri::command]
pub async fn transfer_stock(
    state: State<'_, AppState>,
    company_id: String,
    input: TransferInput,
) -> AppResult<StockTransfer> {
    stock_transfer::transfer_stock(&state.db, &company_id, input).await
}

/// List all stock transfers for a company (newest first).
#[tauri::command]
pub async fn list_stock_transfers(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<StockTransfer>> {
    stock_transfer::list_transfers(&state.db, &company_id).await
}

/// Get a single stock transfer by id (multi-tenant guard).
#[tauri::command]
pub async fn get_stock_transfer(
    state: State<'_, AppState>,
    company_id: String,
    id: String,
) -> AppResult<StockTransfer> {
    stock_transfer::get_transfer(&state.db, &company_id, &id).await
}
