//! Tauri commands pentru Dezmembrare stocuri (OMFP 1802/2014).

use tauri::State;

use crate::db::dezmembrari::{self, CreateDezmembrareInput, Dezmembrare, DezmembrareWithLines};
use crate::error::AppResult;
use crate::state::AppState;

/// Creează un bon de dezmembrare ca ciornă (status='DRAFT').
/// Validează stocul disponibil + apartenența produselor la companie.
/// RBAC: CreateDraft
#[tauri::command]
pub async fn create_dezmembrare(
    state: State<'_, AppState>,
    input: CreateDezmembrareInput,
) -> AppResult<DezmembrareWithLines> {
    dezmembrari::create_dezmembrare(&state.db, input).await
}

/// Returnează un bon de dezmembrare cu liniile sale (guard multi-tenant).
#[tauri::command]
pub async fn get_dezmembrare(
    state: State<'_, AppState>,
    company_id: String,
    dezmembrare_id: String,
) -> AppResult<DezmembrareWithLines> {
    dezmembrari::get_dezmembrare_with_lines(&state.db, &company_id, &dezmembrare_id).await
}

/// Listează bonurile de dezmembrare pentru o companie, descrescător după dată.
#[tauri::command]
pub async fn list_dezmembrari(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<Dezmembrare>> {
    dezmembrari::list_dezmembrari(&state.db, &company_id).await
}

/// Postează dezmembrarea (DRAFT → POSTED):
///   - Stoc OUT produs dezasamblat la valoare contabilă (D607=C371)
///   - Stoc IN componente recuperate la valoare justă (D371=C7588)
///   - Nota GL echilibrată cu assert_balanced
/// RBAC: PostGl
#[tauri::command]
pub async fn post_dezmembrare(
    state: State<'_, AppState>,
    company_id: String,
    dezmembrare_id: String,
) -> AppResult<Dezmembrare> {
    dezmembrari::post_dezmembrare(&state.db, &company_id, &dezmembrare_id).await
}
