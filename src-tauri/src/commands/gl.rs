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
use crate::db::gl::{post_annual_close as db_annual_close, AnnualCloseResult};
use crate::db::gl::{post_income_tax as db_income_tax, IncomeTaxResult};
use crate::db::gl::{post_period_close as db_close_period, ClosePeriodResult};
use crate::db::gl::{post_vat_settlement as db_close_vat, VatSettlementResult};
use crate::db::gl::{trial_balance as db_trial_balance, TrialBalance};
use crate::db::gl::{GlPostResult, ReconcileReport};
use crate::error::{AppError, AppResult};
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

/// Exportă bilanțul în format XML oficial ANAF (S1005 «UU» micro / S1003 «BS» entitate mică) cu
/// blocurile F10 (bilanț) + F20 (cont de profit și pierdere) din contabilitate, pentru import în
/// PDF-ul inteligent ANAF. Header-ul (cod fiscal teritorial, întocmitor, audit) + F30 «Date
/// informative» se completează în aplicația ANAF după import. Returnează calea fișierului scris.
#[tauri::command]
pub async fn export_bilant_xml(
    state: State<'_, AppState>,
    company_id: String,
    year: i32,
    caen: String,
    dest_path: String,
) -> AppResult<String> {
    use crate::anaf_decl::bilant_xml::{
        compute_f10, compute_f10_developed, compute_f20, compute_f20_full, generate_bilant_xml,
        BilantHeader,
    };
    // CAEN is a required, enum-validated header field — must be a 4-digit code.
    let caen = caen.trim().to_string();
    if caen.len() != 4 || !caen.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::Validation(
            "Cod CAEN invalid — introduceți 4 cifre (ex. 6201).".into(),
        ));
    }
    let company = crate::db::companies::get(&state.db, &company_id).await?;
    // county_code() falls back to 40 (București) for unknown codes — reject anything that isn't a
    // real 2-letter auto-code (other than the legitimate "B") so codTT isn't silently wrong.
    let county_norm = company.county.trim().to_uppercase();
    if county_norm != "B" && crate::anaf_decl::bilant_xml::county_code(&county_norm) == 40 {
        return Err(AppError::Validation(format!(
            "Cod județ invalid: '{}'. Folosiți codul auto din 2 litere (ex. CJ, B, IF, TM).",
            company.county
        )));
    }
    let from = format!("{year}-01-01");
    let to = format!("{year}-12-31");
    let tb = db_trial_balance(&state.db, &company_id, &from, &to).await?;
    let pnl = db_profit_and_loss(&state.db, &company_id, &company.tax_regime, &from, &to).await?;
    // Prior-year P&L for the F20 comparative column (best-effort).
    let pyear = year - 1;
    let prior = db_profit_and_loss(
        &state.db,
        &company_id,
        &company.tax_regime,
        &format!("{pyear}-01-01"),
        &format!("{pyear}-12-31"),
    )
    .await
    .ok();

    // Entity size → form (OMFP-1802, simplified on total assets): ≤ 2.250.000 lei →
    // microîntreprindere S1005/UU; ≤ 25.000.000 lei → entitate mică S1003/BS; else entitate
    // mare/mijlocie S1002/BL. The full F20 (small/large) + the developed F10 (large, rd.1-103) are
    // completed in the ANAF app after import.
    let bil = db_bilant(&state.db, &company_id, &from, &to).await?;
    let total_assets: f64 = bil.total_assets.parse().unwrap_or(0.0);
    let form = if total_assets <= 2_250_000.0 {
        "UU"
    } else if total_assets <= 25_000_000.0 {
        "BS"
    } else {
        "BL"
    };
    let micro = form == "UU";

    // UU + BS share the prescurtat F10 + simplified F20; BL (entitate mare) files the DEVELOPED F10
    // (rd.1-103) + the full F20 (rd.1-70), which use a different row layout.
    let (f10, f20) = if form == "BL" {
        // Prior-year trial balance for the F20 comparative column (best-effort).
        let prior_tb = db_trial_balance(
            &state.db,
            &company_id,
            &format!("{pyear}-01-01"),
            &format!("{pyear}-12-31"),
        )
        .await
        .ok();
        (
            compute_f10_developed(&tb),
            compute_f20_full(&tb, prior_tb.as_ref()),
        )
    } else {
        (compute_f10(&tb), compute_f20(&pnl, prior.as_ref(), micro))
    };
    let header = BilantHeader {
        year,
        cui: company.cui.clone(),
        den: company.legal_name.clone(),
        adresa: format!("{}, {}, {}", company.address, company.city, company.county),
        reg_com: company.registry_number.clone().unwrap_or_default(),
        caen,
        county: company.county.clone(),
        nume_admin: company.legal_name.clone(),
    };
    let xml = generate_bilant_xml(&header, &f10, &f20, form);
    std::fs::write(&dest_path, xml).map_err(|e| AppError::Other(e.to_string()))?;
    Ok(dest_path)
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

/// Postează impozitul pe venit/profit (D 698/691 = C 4418/4411) pentru perioadă, după regimul
/// companiei. `amount` (string Decimal) suprascrie estimarea (ex. cifra exactă din D101).
#[tauri::command]
pub async fn post_income_tax(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
    amount: Option<String>,
) -> AppResult<IncomeTaxResult> {
    let company = crate::db::companies::get(&state.db, &company_id).await?;
    let amt = amount.and_then(|s| s.parse::<rust_decimal::Decimal>().ok());
    db_income_tax(
        &state.db,
        &company_id,
        &company.tax_regime,
        &period_from,
        &period_to,
        amt,
    )
    .await
}

/// Închiderea anuală: transferă soldul contului 121 în 117 «Rezultatul reportat» (OMFP 1802/2014).
/// Idempotentă per an (source_type='ANNUAL_CLOSE'); nota se datează 1 ianuarie a anului următor.
#[tauri::command]
pub async fn post_annual_close(
    state: State<'_, AppState>,
    company_id: String,
    year: i32,
) -> AppResult<AnnualCloseResult> {
    db_annual_close(&state.db, &company_id, year).await
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
