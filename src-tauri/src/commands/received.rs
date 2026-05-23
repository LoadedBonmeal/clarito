use tauri::State;

use crate::db::models::{Paginated, ReceivedStatus};
use crate::db::received::{self, ReceivedFilter, ReceivedInvoice};
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub async fn list_received_invoices(
    state: State<'_, AppState>,
    filter: Option<ReceivedFilter>,
) -> AppResult<Paginated<ReceivedInvoice>> {
    received::list(&state.db, filter.unwrap_or_default()).await
}

#[tauri::command]
pub async fn get_received_invoice(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<ReceivedInvoice> {
    received::get(&state.db, &id).await
}

#[tauri::command]
pub async fn update_received_status(
    state: State<'_, AppState>,
    id: String,
    status: ReceivedStatus,
) -> AppResult<()> {
    received::set_status(&state.db, &id, status).await
}
