//! Tauri commands for contract management.
//!
//! Contracts are commercial/legal driver records — NOT document justificative.
//! NO GL postings occur on any contract operation.

use serde::Deserialize;
use tauri::State;

use crate::db::contracts::{self, Contract, CreateContractInput, UpdateContractInput};
use crate::db::recurring::RecurringInvoice;
use crate::error::AppResult;
use crate::state::AppState;

// ─── Create ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateContractArgs {
    pub company_id: String,
    pub contact_id: Option<String>,
    pub number: Option<String>,
    pub title: String,
    pub object: Option<String>,
    pub value: Option<String>,
    pub currency: Option<String>,
    pub start_date: String,
    pub end_date: Option<String>,
    pub status: Option<String>,
    pub payment_terms_days: Option<i64>,
    pub auto_renew: Option<bool>,
    pub renewal_notice_days: Option<i64>,
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn create_contract(
    state: State<'_, AppState>,
    args: CreateContractArgs,
) -> AppResult<Contract> {
    crate::commands::require_valid_date("Data de start", &args.start_date)?;
    crate::commands::require_valid_date_opt("Data de sfârșit", args.end_date.as_deref())?;

    contracts::create(
        &state.db,
        CreateContractInput {
            company_id: args.company_id,
            contact_id: args.contact_id,
            number: args.number,
            title: args.title,
            object: args.object,
            value: args.value,
            currency: args.currency,
            start_date: args.start_date,
            end_date: args.end_date,
            status: args.status,
            payment_terms_days: args.payment_terms_days,
            auto_renew: args.auto_renew,
            renewal_notice_days: args.renewal_notice_days,
            notes: args.notes,
        },
    )
    .await
}

// ─── List ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_contracts(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<Contract>> {
    contracts::list(&state.db, &company_id).await
}

// ─── Get ──────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_contract(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<Contract> {
    contracts::get_by_id(&state.db, &id, &company_id).await
}

// ─── Update ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateContractArgs {
    pub id: String,
    pub company_id: String,
    pub contact_id: Option<String>,
    pub number: Option<String>,
    pub title: String,
    pub object: Option<String>,
    pub value: Option<String>,
    pub currency: Option<String>,
    pub start_date: String,
    pub end_date: Option<String>,
    pub payment_terms_days: Option<i64>,
    pub auto_renew: Option<bool>,
    pub renewal_notice_days: Option<i64>,
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn update_contract(
    state: State<'_, AppState>,
    args: UpdateContractArgs,
) -> AppResult<()> {
    crate::commands::require_valid_date("Data de start", &args.start_date)?;
    crate::commands::require_valid_date_opt("Data de sfârșit", args.end_date.as_deref())?;

    contracts::update(
        &state.db,
        &args.id,
        &args.company_id,
        UpdateContractInput {
            contact_id: args.contact_id,
            number: args.number,
            title: args.title,
            object: args.object,
            value: args.value,
            currency: args.currency,
            start_date: args.start_date,
            end_date: args.end_date,
            payment_terms_days: args.payment_terms_days,
            auto_renew: args.auto_renew,
            renewal_notice_days: args.renewal_notice_days,
            notes: args.notes,
        },
    )
    .await?;

    let _ = crate::db::audit::log_user_action(
        &state.db,
        "contract_updated",
        "contract",
        &args.id,
        Some(&args.company_id),
        None,
    )
    .await;

    Ok(())
}

// ─── Set status ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn set_contract_status(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    status: String,
) -> AppResult<()> {
    contracts::set_status(&state.db, &id, &company_id, &status).await?;

    let _ = crate::db::audit::log_user_action(
        &state.db,
        "contract_status_changed",
        "contract",
        &id,
        Some(&company_id),
        Some(&status),
    )
    .await;

    Ok(())
}

// ─── Delete ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn delete_contract(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    contracts::delete(&state.db, &id, &company_id).await?;

    let _ = crate::db::audit::log_user_action(
        &state.db,
        "contract_deleted",
        "contract",
        &id,
        Some(&company_id),
        None,
    )
    .await;

    Ok(())
}

// ─── Linked recurring invoices ────────────────────────────────────────────────

#[tauri::command]
pub async fn list_contract_recurring(
    state: State<'_, AppState>,
    contract_id: String,
    company_id: String,
) -> AppResult<Vec<RecurringInvoice>> {
    contracts::list_linked_recurring(&state.db, &contract_id, &company_id).await
}
