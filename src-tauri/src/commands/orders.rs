//! Tauri commands for orders / comenzi management.

use tauri::State;

use crate::db::invoices::Invoice;
use crate::db::orders::{self, CreateOrderInput, Order, OrderWithLines, UpdateOrderInput};
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub async fn create_order(state: State<'_, AppState>, args: CreateOrderInput) -> AppResult<Order> {
    orders::create(&state.db, args).await
}

#[tauri::command]
pub async fn list_orders(state: State<'_, AppState>, company_id: String) -> AppResult<Vec<Order>> {
    orders::list(&state.db, &company_id).await
}

#[tauri::command]
pub async fn get_order(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<OrderWithLines> {
    orders::get_with_lines(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn update_order(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: UpdateOrderInput,
) -> AppResult<Order> {
    orders::update(&state.db, &id, &company_id, input).await
}

#[tauri::command]
pub async fn delete_order(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    orders::delete(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn set_order_status(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    status: String,
) -> AppResult<Order> {
    orders::set_status(&state.db, &id, &company_id, &status).await
}

#[tauri::command]
pub async fn convert_order_to_invoice(
    state: State<'_, AppState>,
    company_id: String,
    order_id: String,
) -> AppResult<Invoice> {
    orders::convert_to_invoice(&state.db, &company_id, &order_id).await
}
