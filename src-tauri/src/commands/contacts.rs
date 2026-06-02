use tauri::State;

use crate::db::contacts::{self, Contact, ContactFilter, CreateContactInput, UpdateContactInput};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
pub async fn list_contacts(
    state: State<'_, AppState>,
    filter: Option<ContactFilter>,
) -> AppResult<Vec<Contact>> {
    let f = filter.unwrap_or_default();
    // Defence-in-depth: reject a null/empty company_id so a missing active
    // company never leaks cross-company data via the IS-NULL SQL shortcut.
    if f.company_id.as_ref().is_none_or(|s| s.is_empty()) {
        return Err(AppError::Validation(
            "Selectați o companie activă.".to_string(),
        ));
    }
    contacts::list(&state.db, f).await
}

/// S1: `company_id` is required — cross-company fetch returns NotFound.
#[tauri::command]
pub async fn get_contact(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<Contact> {
    contacts::get(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn create_contact(
    state: State<'_, AppState>,
    input: CreateContactInput,
) -> AppResult<Contact> {
    contacts::create(&state.db, input).await
}

/// R14 Wave A: `company_id` is required. Cross-company update returns NotFound.
#[tauri::command]
pub async fn update_contact(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: UpdateContactInput,
) -> AppResult<Contact> {
    contacts::update(&state.db, &id, &company_id, input).await
}

/// R14 Wave A: `company_id` is required. Cross-company deletion returns NotFound.
#[tauri::command]
pub async fn delete_contact(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    contacts::delete(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn search_contacts(
    state: State<'_, AppState>,
    query: String,
    company_id: String,
) -> AppResult<Vec<Contact>> {
    contacts::list(
        &state.db,
        ContactFilter {
            query: Some(query),
            company_id: Some(company_id),
        },
    )
    .await
}
