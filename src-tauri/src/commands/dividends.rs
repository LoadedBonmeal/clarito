//! Tauri commands — dividende repartizate + impozit pe dividende (Legea 141/2025). Înregistrarea unui
//! dividend calculează cota (16% de la 2026 / 10% tranzitoriu) și postează nota 117/457/446 în GL.

use tauri::State;

use crate::db::dividends::{self, Dividend, DividendInput};
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub async fn list_dividends(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<Dividend>> {
    dividends::list(&state.db, &company_id).await
}

#[tauri::command]
pub async fn create_dividend(
    state: State<'_, AppState>,
    input: DividendInput,
) -> AppResult<Dividend> {
    dividends::create(&state.db, input).await
}

#[tauri::command]
pub async fn delete_dividend(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    dividends::delete(&state.db, &id, &company_id).await
}
