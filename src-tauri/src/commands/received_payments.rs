//! Tauri commands for supplier-payment tracking (payments-out against received invoices).

use serde::Deserialize;
use tauri::State;

use crate::db::received_payments::{
    self, CreateReceivedPaymentInput, ReceivedPayment, ReceivedPaymentSummary,
};
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddReceivedPaymentArgs {
    pub received_invoice_id: String,
    pub company_id: String,
    pub amount: String,
    pub currency: Option<String>,
    pub paid_at: String,
    pub method: Option<String>,
    pub reference: Option<String>,
    pub notes: Option<String>,
    pub exchange_rate: Option<f64>,
}

#[tauri::command]
pub async fn add_received_payment(
    state: State<'_, AppState>,
    args: AddReceivedPaymentArgs,
) -> AppResult<ReceivedPayment> {
    received_payments::create(
        &state.db,
        CreateReceivedPaymentInput {
            received_invoice_id: args.received_invoice_id,
            company_id: args.company_id,
            amount: args.amount,
            currency: args.currency,
            paid_at: args.paid_at,
            method: args.method,
            reference: args.reference,
            notes: args.notes,
            exchange_rate: args.exchange_rate,
        },
    )
    .await
}

#[tauri::command]
pub async fn list_received_payments(
    state: State<'_, AppState>,
    received_invoice_id: String,
    company_id: String,
) -> AppResult<Vec<ReceivedPayment>> {
    received_payments::list_for_received_invoice(&state.db, &received_invoice_id, &company_id).await
}

#[tauri::command]
pub async fn delete_received_payment(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    received_payments::delete(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn get_received_payment_summary(
    state: State<'_, AppState>,
    received_invoice_id: String,
    company_id: String,
) -> AppResult<ReceivedPaymentSummary> {
    received_payments::summary_for_received_invoice(&state.db, &received_invoice_id, &company_id)
        .await
}
