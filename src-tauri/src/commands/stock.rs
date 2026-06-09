//! Tauri commands for stock movements (MovementOfGoods SAF-T section).
//! MVP: manual entry capture. UI is out of scope for P6.

use tauri::State;

use crate::db::stock::{self, StockMovementInput, StockMovementWithLines};
use crate::db::stock_valuation::{self, Dir, LedgerRow, StockMovementInput as StockValInput};
use crate::error::AppResult;
use crate::state::AppState;

/// Record a stock receipt (IN, at purchase cost) + revalue the product + post the GL leg.
#[tauri::command]
pub async fn record_stock_receipt(
    state: State<'_, AppState>,
    input: StockValInput,
) -> AppResult<()> {
    stock_valuation::record_movement(&state.db, &input, Dir::In).await
}

/// Record a stock issue / descărcare gestiune (OUT, valued via FIFO/CMP) + post the GL leg.
#[tauri::command]
pub async fn record_stock_issue(state: State<'_, AppState>, input: StockValInput) -> AppResult<()> {
    stock_valuation::record_movement(&state.db, &input, Dir::Out).await
}

/// The valued stock ledger (fișa de magazie) for a product.
#[tauri::command]
pub async fn stock_ledger(
    state: State<'_, AppState>,
    company_id: String,
    product_id: String,
) -> AppResult<Vec<LedgerRow>> {
    stock_valuation::ledger(&state.db, &company_id, &product_id).await
}

/// Set a product's valuation method ('FIFO'|'CMP') + stock account, then revalue.
#[tauri::command]
pub async fn set_stock_valuation(
    state: State<'_, AppState>,
    company_id: String,
    product_id: String,
    method: String,
    stock_account: String,
) -> AppResult<()> {
    let m = if method == "FIFO" { "FIFO" } else { "CMP" };
    sqlx::query("UPDATE products SET valuation_method=?2, stock_account=?3 WHERE id=?1")
        .bind(&product_id)
        .bind(m)
        .bind(&stock_account)
        .execute(&state.db)
        .await?;
    stock_valuation::recompute_product(&state.db, &company_id, &product_id).await
}

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
