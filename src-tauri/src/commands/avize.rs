//! Tauri commands pentru Avize de însoțire a mărfii (OMFP 2634/2015 formular 14-3-6A).

use tauri::State;

use crate::db::avize::{self, Aviz, AvizWithLines, CreateAvizInput};
use crate::error::AppResult;
use crate::state::AppState;

/// Creează un aviz ca ciornă (status='DRAFT').
/// RBAC: CreateDraft
#[tauri::command]
pub async fn create_aviz(
    state: State<'_, AppState>,
    input: CreateAvizInput,
) -> AppResult<AvizWithLines> {
    avize::create_aviz(&state.db, input).await
}

/// Returnează un aviz cu liniile sale (guard multi-tenant).
#[tauri::command]
pub async fn get_aviz(
    state: State<'_, AppState>,
    company_id: String,
    aviz_id: String,
) -> AppResult<AvizWithLines> {
    avize::get_aviz_with_lines(&state.db, &company_id, &aviz_id).await
}

/// Listează avizele pentru o companie, descrescător după dată.
#[tauri::command]
pub async fn list_avize(state: State<'_, AppState>, company_id: String) -> AppResult<Vec<Aviz>> {
    avize::list_avize(&state.db, &company_id).await
}

/// Emite avizul (DRAFT → ISSUED): postează GL D418/C707/C4428 + D607/C371 + stoc OUT.
/// RBAC: PostGl
#[tauri::command]
pub async fn issue_aviz(
    state: State<'_, AppState>,
    company_id: String,
    aviz_id: String,
) -> AppResult<Aviz> {
    avize::issue_aviz(&state.db, &company_id, &aviz_id).await
}

/// Convertește avizul la factură (ISSUED → INVOICED): reclasifică 418→4111, 4428→4427.
/// Venitul (707) este recunoscut O SINGURĂ dată (la emiterea avizului) — NU se dublez.
/// RBAC: PostGl
#[tauri::command]
pub async fn convert_aviz_to_invoice(
    state: State<'_, AppState>,
    company_id: String,
    aviz_id: String,
    invoice_id: String,
) -> AppResult<Aviz> {
    avize::convert_aviz_to_invoice(&state.db, &company_id, &aviz_id, &invoice_id).await
}
