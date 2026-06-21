//! P2 Wave 7: payroll config Tauri commands.

use tauri::State;

use crate::db::payroll_config::{
    get_payroll_config, reset_payroll_config, set_payroll_config, PayrollAccountMap,
    SetPayrollConfigInput,
};
use crate::error::AppResult;
use crate::AppState;

#[tauri::command]
pub async fn get_payroll_config_cmd(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<PayrollAccountMap> {
    get_payroll_config(&state.db, &company_id).await
}

#[tauri::command]
pub async fn set_payroll_config_cmd(
    state: State<'_, AppState>,
    company_id: String,
    input: SetPayrollConfigInput,
) -> AppResult<PayrollAccountMap> {
    set_payroll_config(&state.db, &company_id, input).await
}

#[tauri::command]
pub async fn reset_payroll_config_cmd(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<PayrollAccountMap> {
    reset_payroll_config(&state.db, &company_id).await
}
