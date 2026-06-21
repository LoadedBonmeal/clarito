//! Tauri commands for stock movements (MovementOfGoods SAF-T section).
//! MVP: manual entry capture. UI is out of scope for P6.

use tauri::State;

use crate::db::gestiune::{self, Gestiune, GestiuneInput};
use crate::db::stock::{self, StockMovementInput, StockMovementWithLines};
use crate::db::stock_valuation::{self, Dir, LedgerRow, StockMovementInput as StockValInput};
use crate::error::AppResult;
use crate::state::AppState;

/// Record a stock receipt (IN, at purchase cost) + revalue the product + post the GL leg. Returns an
/// optional warning (gestiune negativa).
#[tauri::command]
pub async fn record_stock_receipt(
    state: State<'_, AppState>,
    input: StockValInput,
) -> AppResult<Option<String>> {
    stock_valuation::record_movement(&state.db, &input, Dir::In).await
}

/// Record a stock issue / descarcare gestiune (OUT, valued via FIFO/CMP) + post the GL leg. Returns
/// an optional warning (gestiune negativa).
#[tauri::command]
pub async fn record_stock_issue(
    state: State<'_, AppState>,
    input: StockValInput,
) -> AppResult<Option<String>> {
    stock_valuation::record_movement(&state.db, &input, Dir::Out).await
}

/// The valued stock ledger (fisa de magazie) for a product, optionally filtered by gestiune_id.
#[tauri::command]
pub async fn stock_ledger(
    state: State<'_, AppState>,
    company_id: String,
    product_id: String,
    gestiune_id: Option<String>,
) -> AppResult<Vec<LedgerRow>> {
    stock_valuation::ledger(&state.db, &company_id, &product_id, gestiune_id.as_deref()).await
}

/// Set a product's valuation method ('FIFO'|'LIFO'|'CMP') + stock account, then revalue.
#[tauri::command]
pub async fn set_stock_valuation(
    state: State<'_, AppState>,
    company_id: String,
    product_id: String,
    method: String,
    stock_account: String,
) -> AppResult<()> {
    stock_valuation::assert_product_owned(&state.db, &company_id, &product_id).await?;
    let m = match method.as_str() {
        "FIFO" => "FIFO",
        "LIFO" => "LIFO",
        _ => "CMP",
    };
    sqlx::query(
        "UPDATE products SET valuation_method=?2, stock_account=?3 WHERE id=?1 AND company_id=?4",
    )
    .bind(&product_id)
    .bind(m)
    .bind(&stock_account)
    .bind(&company_id)
    .execute(&state.db)
    .await?;
    stock_valuation::recompute_product(&state.db, &company_id, &product_id).await?;
    Ok(())
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

// ── Gestiuni (warehouses) commands ───────────────────────────────────────────

/// List all gestiuni for a company.
#[tauri::command]
pub async fn list_gestiuni(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<Gestiune>> {
    gestiune::list(&state.db, &company_id).await
}

/// Create a new gestiune.
#[tauri::command]
pub async fn create_gestiune(
    state: State<'_, AppState>,
    company_id: String,
    input: GestiuneInput,
) -> AppResult<Gestiune> {
    gestiune::create(&state.db, &company_id, input).await
}

/// Update an existing gestiune.
#[tauri::command]
pub async fn update_gestiune(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: GestiuneInput,
) -> AppResult<Gestiune> {
    gestiune::update(&state.db, &id, &company_id, input).await
}

/// Delete a gestiune (blocked if default or has stock movements).
#[tauri::command]
pub async fn delete_gestiune(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    gestiune::delete(&state.db, &id, &company_id).await
}

/// Returns the default gestiune id for a company.
#[tauri::command]
pub async fn get_default_gestiune_id(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<String> {
    gestiune::default_gestiune_id(&state.db, &company_id).await
}

/// On-hand quantity and value for a product in a specific gestiune (or total if gestiune_id is None).
/// Returns (qty, value) as formatted strings.
#[tauri::command]
pub async fn stock_on_hand(
    state: State<'_, AppState>,
    company_id: String,
    product_id: String,
    gestiune_id: Option<String>,
) -> AppResult<(String, String)> {
    gestiune::stock_on_hand(&state.db, &company_id, &product_id, gestiune_id.as_deref()).await
}
