//! GL auto-posting commands — Tauri interface pentru registrul jurnal.
//!
//! Comenzile din acest modul sunt înregistrate în `lib.rs` via `generate_handler!`.
//! Frontend-ul (Wave P7) va folosi `invoke("generate_gl_entries", {...})` și
//! `invoke("reconcile_gl", {...})`.

use tauri::State;

use crate::db::gl::profit_and_loss as db_profit_and_loss;
use crate::db::gl::ProfitLoss;
use crate::db::gl::{bilant as db_bilant, BilantReport};
use crate::db::gl::{general_ledger as db_general_ledger, LedgerAccount};
use crate::db::gl::{generate_gl_entries as db_generate, reconcile as db_reconcile};
use crate::db::gl::{journal_register as db_journal_register, JournalRegister};
use crate::db::gl::{post_period_close as db_close_period, ClosePeriodResult};
use crate::db::gl::{post_vat_settlement as db_close_vat, VatSettlementResult};
use crate::db::gl::{trial_balance as db_trial_balance, TrialBalance};
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

/// Balanța de verificare (cod 14-6-30, patru egalități) pentru perioadă — din GL.
#[tauri::command]
pub async fn trial_balance(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<TrialBalance> {
    db_trial_balance(&state.db, &company_id, &period_from, &period_to).await
}

/// Contul de profit și pierdere (P&L) pentru perioadă — venituri (clasa 7) și cheltuieli
/// (clasa 6) din balanță, rezultatul brut/net + impozitul (înregistrat sau estimat după regimul
/// fiscal al companiei) și notele de închidere 6/7 → 121 (OMFP 1802/2014).
#[tauri::command]
pub async fn profit_and_loss(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<ProfitLoss> {
    let company = crate::db::companies::get(&state.db, &company_id).await?;
    db_profit_and_loss(
        &state.db,
        &company_id,
        &company.tax_regime,
        &period_from,
        &period_to,
    )
    .await
}

/// Bilanț contabil (balance sheet) pentru perioadă — agregă clasele 1-5 din balanță (active,
/// capitaluri, datorii), cu rezultatul exercițiului inclus în capitaluri. OMFP 1802/2014.
#[tauri::command]
pub async fn bilant(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<BilantReport> {
    db_bilant(&state.db, &company_id, &period_from, &period_to).await
}

/// Închiderea conturilor de venituri și cheltuieli (clasele 6 și 7) în 121 «Profit sau pierdere»
/// pentru perioadă (OMFP 1802/2014). Idempotentă per perioadă (source_type='PNL_CLOSE').
#[tauri::command]
pub async fn close_period(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<ClosePeriodResult> {
    db_close_period(&state.db, &company_id, &period_from, &period_to).await
}

/// Registru-jurnal (cod 14-1-1) — lista cronologică a notelor contabile din perioadă.
#[tauri::command]
pub async fn journal_register(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<JournalRegister> {
    db_journal_register(&state.db, &company_id, &period_from, &period_to).await
}

/// Cartea mare (cod 14-1-3 / fișă de cont) — câte o filă pe cont sintetic.
#[tauri::command]
pub async fn general_ledger(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<Vec<LedgerAccount>> {
    db_general_ledger(&state.db, &company_id, &period_from, &period_to).await
}
