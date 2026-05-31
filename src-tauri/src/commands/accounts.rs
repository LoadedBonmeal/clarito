//! Tauri commands pentru planul de conturi (chart of accounts).
//!
//! Toate comenzile sunt company-scoped: `company_id` este obligatoriu
//! și este verificat în layer-ul DB. Cross-company access returnează NotFound.

use tauri::State;

use crate::db::accounts::{self, Account, AccountInput, UpdateAccountInput};
use crate::error::AppResult;
use crate::state::AppState;

/// R15 Wave 4: List all accounts for a company, ordered by account_code.
#[tauri::command]
pub async fn list_accounts(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<Account>> {
    accounts::list(&state.db, &company_id).await
}

/// R15 Wave 4: Get a single account by id. Returns NotFound for wrong company.
#[tauri::command]
pub async fn get_account(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<Account> {
    accounts::get(&state.db, &id, &company_id).await
}

/// R15 Wave 4: Create an account for the given company.
/// Returns Conflict if the account code already exists for this company.
#[tauri::command]
pub async fn create_account(
    state: State<'_, AppState>,
    company_id: String,
    input: AccountInput,
) -> AppResult<Account> {
    accounts::create(&state.db, &company_id, input).await
}

/// R15 Wave 4: Update an account. Cross-company update returns NotFound.
#[tauri::command]
pub async fn update_account(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: UpdateAccountInput,
) -> AppResult<Account> {
    accounts::update(&state.db, &id, &company_id, input).await
}

/// R15 Wave 4: Delete an account. Cross-company deletion returns NotFound.
#[tauri::command]
pub async fn delete_account(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    accounts::delete(&state.db, &id, &company_id).await
}

/// R15 Wave 4: Seed the standard Romanian chart of accounts for a company.
/// Idempotent — only inserts when the company has no accounts yet.
/// Returns the number of accounts inserted (0 if already seeded).
#[tauri::command]
pub async fn seed_standard_accounts(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<usize> {
    accounts::seed_standard(&state.db, &company_id).await
}
