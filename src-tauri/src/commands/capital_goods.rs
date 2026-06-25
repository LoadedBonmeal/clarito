//! Tauri commands — Registrul bunurilor de capital + ajustarea TVA (Cod fiscal art. 305).

use tauri::State;

use crate::db::capital_goods::{
    self, CapitalGood, CapitalGoodAdjustment, CreateCapitalGoodInput, RecordAdjustmentInput,
};
use crate::error::AppResult;
use crate::state::AppState;

/// Înregistrează un bun de capital în registru (perioada de ajustare 5/20 ani după tip).
#[tauri::command]
pub async fn create_capital_good(
    state: State<'_, AppState>,
    input: CreateCapitalGoodInput,
) -> AppResult<CapitalGood> {
    capital_goods::create(&state.db, input).await
}

/// Listează bunurile de capital ale companiei.
#[tauri::command]
pub async fn list_capital_goods(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<CapitalGood>> {
    capital_goods::list(&state.db, &company_id).await
}

/// Listează ajustările înregistrate pentru un bun de capital.
#[tauri::command]
pub async fn list_capital_good_adjustments(
    state: State<'_, AppState>,
    capital_good_id: String,
    company_id: String,
) -> AppResult<Vec<CapitalGoodAdjustment>> {
    capital_goods::list_adjustments(&state.db, &capital_good_id, &company_id).await
}

/// Calculează + înregistrează ajustarea unui an cu schimbare de utilizare și postează GL.
#[tauri::command]
pub async fn record_capital_good_adjustment(
    state: State<'_, AppState>,
    input: RecordAdjustmentInput,
) -> AppResult<CapitalGoodAdjustment> {
    capital_goods::record_adjustment(&state.db, input).await
}

/// Σ semnată a ajustărilor TVA bunuri de capital dintr-o perioadă (AAAA-LL), în lei — pentru D300.
#[tauri::command]
pub async fn capital_good_period_adjustment(
    state: State<'_, AppState>,
    company_id: String,
    period: String,
) -> AppResult<i64> {
    capital_goods::period_adjustment_lei(&state.db, &company_id, &period).await
}

/// Șterge un bun de capital (guard multi-tenant) + notele GL ale ajustărilor.
#[tauri::command]
pub async fn delete_capital_good(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    capital_goods::delete(&state.db, &id, &company_id).await
}
