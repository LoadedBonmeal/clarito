//! Tauri commands — NIR (Notă de Intrare Recepție).
//!
//! Thin dispatch layer to `db::nir`. All IPC-boundary validation (valid dates,
//! non-empty required strings) happens here; business logic stays in db::nir.

use tauri::State;

use crate::commands::require_valid_date;
use crate::db::nir::{self, NirDocument, NirInput, NirWithLines};
use crate::error::AppResult;
use crate::state::AppState;

/// Creează un NIR nou cu status 'draft'.
#[tauri::command]
pub async fn create_nir(
    state: State<'_, AppState>,
    company_id: String,
    input: NirInput,
) -> AppResult<NirDocument> {
    require_valid_date("Data NIR", &input.nir_date)?;
    nir::create_nir(&state.db, &company_id, input).await
}

/// Returnează un NIR (cu linii) după ID.
#[tauri::command]
pub async fn get_nir(
    state: State<'_, AppState>,
    company_id: String,
    nir_id: String,
) -> AppResult<NirWithLines> {
    nir::get_nir(&state.db, &company_id, &nir_id).await
}

/// Listează toate NIR-urile pentru o companie.
#[tauri::command]
pub async fn list_nir(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<NirDocument>> {
    nir::list_nir(&state.db, &company_id).await
}

/// Finalizează un NIR (draft → finalized): înregistrează stocul + nota GL.
#[tauri::command]
pub async fn finalize_nir(
    state: State<'_, AppState>,
    company_id: String,
    nir_id: String,
) -> AppResult<NirDocument> {
    nir::finalize_nir(&state.db, &company_id, &nir_id).await
}

/// Prefill-uiește un NirInput din datele unei facturi primite.
#[tauri::command]
pub async fn nir_from_received_invoice(
    state: State<'_, AppState>,
    company_id: String,
    received_invoice_id: String,
) -> AppResult<NirInput> {
    nir::nir_from_received_invoice(&state.db, &company_id, &received_invoice_id).await
}
