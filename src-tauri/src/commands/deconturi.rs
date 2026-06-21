//! Tauri commands — avansuri de trezorerie + deconturi de cheltuieli (P3 Wave D).

use tauri::State;

use crate::db::deconturi::{
    self, CreateAdvanceInput, CreateReportInput, DiurnaCalc, ExpenseReport, ExpenseReportFull,
    TreasuryAdvance,
};
use crate::db::payroll_config::get_payroll_config;
use crate::error::AppResult;
use crate::state::AppState;

// ─── Treasury advances ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn create_treasury_advance(
    state: State<'_, AppState>,
    input: CreateAdvanceInput,
) -> AppResult<TreasuryAdvance> {
    deconturi::create_advance(&state.db, input).await
}

#[tauri::command]
pub async fn list_treasury_advances(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<TreasuryAdvance>> {
    deconturi::list_advances(&state.db, &company_id).await
}

#[tauri::command]
pub async fn get_treasury_advance(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<TreasuryAdvance> {
    deconturi::get_advance(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn return_treasury_advance(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    return_date: String,
) -> AppResult<TreasuryAdvance> {
    deconturi::return_advance(&state.db, &id, &company_id, &return_date).await
}

#[tauri::command]
pub async fn delete_treasury_advance(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    deconturi::delete_advance(&state.db, &id, &company_id).await
}

// ─── Expense reports ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn create_expense_report(
    state: State<'_, AppState>,
    mut input: CreateReportInput,
) -> AppResult<ExpenseReportFull> {
    // Auto-fill diurna_interna from payroll_config if not provided
    if input.diurna_interna.is_none() {
        let cfg = get_payroll_config(&state.db, &input.company_id).await?;
        input.diurna_interna = Some(cfg.diurna_interna);
    }
    deconturi::create_report(&state.db, input).await
}

#[tauri::command]
pub async fn list_expense_reports(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<ExpenseReport>> {
    deconturi::list_reports(&state.db, &company_id).await
}

#[tauri::command]
pub async fn get_expense_report(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<ExpenseReportFull> {
    deconturi::get_report_full(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn approve_expense_report(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    approve_date: String,
) -> AppResult<ExpenseReportFull> {
    deconturi::approve_report(&state.db, &id, &company_id, &approve_date).await
}

#[tauri::command]
pub async fn delete_expense_report(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    deconturi::delete_report(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn compute_diurna(
    state: State<'_, AppState>,
    company_id: String,
    diurna_acordata: String,
    zile_delegare: u32,
    salariu_brut: String,
    year: i32,
    month: u32,
) -> AppResult<DiurnaCalc> {
    let cfg = get_payroll_config(&state.db, &company_id).await?;
    Ok(deconturi::compute_diurna(
        &diurna_acordata,
        zile_delegare,
        &salariu_brut,
        year,
        month,
        &cfg.diurna_interna,
    ))
}
