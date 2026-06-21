//! Tauri commands for payment instruments (CEC & BO).
//!
//! Thin layer: validates dates at the IPC boundary, dispatches to db layer.
//! GL postings are done inside the db functions (idempotent, see payment_instruments module).

use serde::Deserialize;
use tauri::State;

use crate::db::payment_instruments::{
    self, CreatePaymentInstrumentInput, PaymentInstrument, UpdatePaymentInstrumentInput,
};
use crate::error::AppResult;
use crate::state::AppState;

// ─── Create ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePaymentInstrumentArgs {
    pub company_id: String,
    pub kind: String,
    pub direction: String,
    pub partner_id: Option<String>,
    pub partner_cui: Option<String>,
    pub number: Option<String>,
    pub amount: String,
    pub currency: Option<String>,
    pub issue_date: String,
    pub scadenta: Option<String>,
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn create_payment_instrument(
    state: State<'_, AppState>,
    args: CreatePaymentInstrumentArgs,
) -> AppResult<PaymentInstrument> {
    crate::commands::require_valid_date("Data emiterii", &args.issue_date)?;
    crate::commands::require_valid_date_opt("Scadența", args.scadenta.as_deref())?;

    payment_instruments::create(
        &state.db,
        CreatePaymentInstrumentInput {
            company_id: args.company_id,
            kind: args.kind,
            direction: args.direction,
            partner_id: args.partner_id,
            partner_cui: args.partner_cui,
            number: args.number,
            amount: args.amount,
            currency: args.currency,
            issue_date: args.issue_date,
            scadenta: args.scadenta,
            notes: args.notes,
        },
    )
    .await
}

// ─── List ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_payment_instruments(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<PaymentInstrument>> {
    payment_instruments::list(&state.db, &company_id).await
}

// ─── Get ──────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_payment_instrument(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<PaymentInstrument> {
    payment_instruments::fetch_one(&state.db, &id, &company_id).await
}

// ─── Update ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePaymentInstrumentArgs {
    pub id: String,
    pub company_id: String,
    pub partner_id: Option<String>,
    pub partner_cui: Option<String>,
    pub number: Option<String>,
    pub amount: String,
    pub currency: Option<String>,
    pub issue_date: String,
    pub scadenta: Option<String>,
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn update_payment_instrument(
    state: State<'_, AppState>,
    args: UpdatePaymentInstrumentArgs,
) -> AppResult<PaymentInstrument> {
    crate::commands::require_valid_date("Data emiterii", &args.issue_date)?;
    crate::commands::require_valid_date_opt("Scadența", args.scadenta.as_deref())?;

    payment_instruments::update(
        &state.db,
        &args.id,
        &args.company_id,
        UpdatePaymentInstrumentInput {
            partner_id: args.partner_id,
            partner_cui: args.partner_cui,
            number: args.number,
            amount: args.amount,
            currency: args.currency,
            issue_date: args.issue_date,
            scadenta: args.scadenta,
            notes: args.notes,
        },
    )
    .await
}

// ─── Delete ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn delete_payment_instrument(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    payment_instruments::delete(&state.db, &id, &company_id).await
}

// ─── Lifecycle events ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn deposit_payment_instrument(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    date: String,
) -> AppResult<PaymentInstrument> {
    crate::commands::require_valid_date("Data depunerii", &date)?;
    payment_instruments::event_deposit(&state.db, &id, &company_id, &date).await
}

#[tauri::command]
pub async fn collect_payment_instrument(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    date: String,
) -> AppResult<PaymentInstrument> {
    crate::commands::require_valid_date("Data încasării", &date)?;
    payment_instruments::event_collect(&state.db, &id, &company_id, &date).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscountArgs {
    pub id: String,
    pub company_id: String,
    pub date: String,
    pub discount_amount: String,
    pub commission_amount: Option<String>,
}

#[tauri::command]
pub async fn discount_payment_instrument(
    state: State<'_, AppState>,
    args: DiscountArgs,
) -> AppResult<PaymentInstrument> {
    crate::commands::require_valid_date("Data scontării", &args.date)?;
    payment_instruments::event_discount(
        &state.db,
        &args.id,
        &args.company_id,
        &args.date,
        &args.discount_amount,
        args.commission_amount.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn dishonor_payment_instrument(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    date: String,
) -> AppResult<PaymentInstrument> {
    crate::commands::require_valid_date("Data refuzului", &date)?;
    payment_instruments::event_dishonor(&state.db, &id, &company_id, &date).await
}

#[tauri::command]
pub async fn pay_payment_instrument(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    date: String,
) -> AppResult<PaymentInstrument> {
    crate::commands::require_valid_date("Data plății", &date)?;
    payment_instruments::event_pay(&state.db, &id, &company_id, &date).await
}
