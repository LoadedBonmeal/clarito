//! Tauri commands for payment tracking.

use serde::Deserialize;
use tauri::State;

use crate::db::payments::{self, CreatePaymentInput, Payment, PaymentSummary};
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddPaymentArgs {
    pub invoice_id: String,
    pub company_id: String,
    pub amount: String,
    pub currency: Option<String>,
    pub paid_at: String,
    pub method: Option<String>,
    pub reference: Option<String>,
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn add_payment(state: State<'_, AppState>, args: AddPaymentArgs) -> AppResult<Payment> {
    payments::create(
        &state.db,
        CreatePaymentInput {
            invoice_id: args.invoice_id,
            company_id: args.company_id,
            amount: args.amount,
            currency: args.currency,
            paid_at: args.paid_at,
            method: args.method,
            reference: args.reference,
            notes: args.notes,
        },
    )
    .await
}

#[tauri::command]
pub async fn list_payments(
    state: State<'_, AppState>,
    invoice_id: String,
    company_id: String,
) -> AppResult<Vec<Payment>> {
    payments::list_for_invoice(&state.db, &invoice_id, &company_id).await
}

#[tauri::command]
pub async fn delete_payment(
    state: State<'_, AppState>,
    payment_id: String,
    company_id: String,
) -> AppResult<()> {
    payments::delete(&state.db, &payment_id, &company_id).await
}

#[tauri::command]
pub async fn get_payment_summary(
    state: State<'_, AppState>,
    invoice_id: String,
    company_id: String,
) -> AppResult<PaymentSummary> {
    payments::summary_for_invoice(&state.db, &invoice_id, &company_id).await
}

#[tauri::command]
pub async fn list_payment_summaries(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<PaymentSummary>> {
    payments::list_all_summaries(&state.db, &company_id).await
}
