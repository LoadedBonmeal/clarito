//! Tauri commands for recurring invoice management.

use serde::Deserialize;
use tauri::State;

use crate::db::recurring::{self, CreateRecurringInput, RecurringInvoice};
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRecurringArgs {
    pub company_id: String,
    pub template_name: String,
    pub client_id: String,
    pub frequency: String,
    pub next_issue_date: String,
    pub day_of_month: i64,
    pub auto_submit_anaf: bool,
    pub series: String,
    pub lines_json: String,
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn create_recurring_invoice(
    state: State<'_, AppState>,
    args: CreateRecurringArgs,
) -> AppResult<RecurringInvoice> {
    recurring::create(
        &state.db,
        CreateRecurringInput {
            company_id: args.company_id,
            template_name: args.template_name,
            client_id: args.client_id,
            frequency: args.frequency,
            next_issue_date: args.next_issue_date,
            day_of_month: args.day_of_month,
            auto_submit_anaf: args.auto_submit_anaf,
            series: args.series,
            lines_json: args.lines_json,
            notes: args.notes,
        },
    )
    .await
}

#[tauri::command]
pub async fn list_recurring_invoices(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<RecurringInvoice>> {
    recurring::list(&state.db, &company_id).await
}

#[tauri::command]
pub async fn delete_recurring_invoice(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    recurring::delete(&state.db, &id, &company_id).await
}
