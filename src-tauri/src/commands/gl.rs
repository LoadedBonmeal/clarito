//! GL auto-posting commands — Tauri interface pentru registrul jurnal.
//!
//! Comenzile din acest modul sunt înregistrate în `lib.rs` via `generate_handler!`.
//! Frontend-ul (Wave P7) va folosi `invoke("generate_gl_entries", {...})` și
//! `invoke("reconcile_gl", {...})`.

use tauri::State;

use crate::db::gl::{generate_gl_entries as db_generate, reconcile as db_reconcile};
use crate::db::gl::{post_vat_settlement as db_close_vat, VatSettlementResult};
use crate::db::gl::{GlPostResult, ReconcileReport};
use crate::error::AppResult;
use crate::state::AppState;

/// Generează (sau re-generează idempotent) notele contabile GL pentru o perioadă.
///
/// Acoperă:
/// - Facturi emise (VALIDATED / STORNED) în `[period_from, period_to]`.
/// - Facturi primite cu defalcare TVA (received_invoice_vat_lines) în perioadă.
/// - Plăți clienți înregistrate în perioadă.
///
/// Înregistrările existente pentru aceleași documente sunt șterse și re-înregistrate
/// (idempotent prin UNIQUE index pe `(company_id, source_type, source_id)`).
#[tauri::command]
pub async fn generate_gl_entries(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<GlPostResult> {
    db_generate(&state.db, &company_id, &period_from, &period_to).await
}

/// Reconciliază GL-ul cu D300 pentru o perioadă.
///
/// Verifică:
/// 1. Σdebit_total == Σcredit_total (principiul dublei înregistrări).
/// 2. Σcredit cont 4427 (TVA colectată GL) == TVA colectată D300.
/// 3. Σdebit cont 4426 (TVA deductibilă GL) == TVA deductibilă D300.
///
/// Returnează `ReconcileReport` cu flag-ul `balanced`, totaluri și lista de discrepanțe.
#[tauri::command]
pub async fn reconcile_gl(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<ReconcileReport> {
    db_reconcile(&state.db, &company_id, &period_from, &period_to).await
}

/// Închiderea/regularizarea TVA: netează 4426/4427 → 4423 (de plată) sau 4424 (de recuperat)
/// la sfârșitul perioadei. Idempotentă; nu atinge 4428 «TVA neexigibilă».
#[tauri::command]
pub async fn close_vat_period(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<VatSettlementResult> {
    db_close_vat(&state.db, &company_id, &period_from, &period_to).await
}
