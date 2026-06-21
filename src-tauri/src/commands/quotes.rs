//! Tauri commands for quotes / devize management.

use tauri::State;

use crate::db::invoices::Invoice;
use crate::db::quotes::{self, CreateQuoteInput, Quote, QuoteWithLines, UpdateQuoteInput};
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub async fn create_quote(state: State<'_, AppState>, args: CreateQuoteInput) -> AppResult<Quote> {
    quotes::create(&state.db, args).await
}

#[tauri::command]
pub async fn list_quotes(state: State<'_, AppState>, company_id: String) -> AppResult<Vec<Quote>> {
    quotes::list(&state.db, &company_id).await
}

#[tauri::command]
pub async fn get_quote(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<QuoteWithLines> {
    quotes::get_with_lines(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn update_quote(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: UpdateQuoteInput,
) -> AppResult<Quote> {
    quotes::update(&state.db, &id, &company_id, input).await
}

#[tauri::command]
pub async fn delete_quote(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    quotes::delete(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn set_quote_status(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    status: String,
) -> AppResult<Quote> {
    quotes::set_status(&state.db, &id, &company_id, &status).await
}

#[tauri::command]
pub async fn convert_quote_to_invoice(
    state: State<'_, AppState>,
    company_id: String,
    quote_id: String,
) -> AppResult<Invoice> {
    quotes::convert_to_invoice(&state.db, &company_id, &quote_id).await
}
