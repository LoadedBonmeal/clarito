use tauri::State;

use crate::db::contacts::{
    self, Contact, ContactFilter, CreateContactInput, UpdateContactInput,
};
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub async fn list_contacts(
    state: State<'_, AppState>,
    filter: Option<ContactFilter>,
) -> AppResult<Vec<Contact>> {
    contacts::list(&state.db, filter.unwrap_or_default()).await
}

#[tauri::command]
pub async fn get_contact(state: State<'_, AppState>, id: String) -> AppResult<Contact> {
    contacts::get(&state.db, &id).await
}

#[tauri::command]
pub async fn create_contact(
    state: State<'_, AppState>,
    input: CreateContactInput,
) -> AppResult<Contact> {
    contacts::create(&state.db, input).await
}

#[tauri::command]
pub async fn update_contact(
    state: State<'_, AppState>,
    id: String,
    input: UpdateContactInput,
) -> AppResult<Contact> {
    contacts::update(&state.db, &id, input).await
}

#[tauri::command]
pub async fn delete_contact(state: State<'_, AppState>, id: String) -> AppResult<()> {
    contacts::delete(&state.db, &id).await
}

#[tauri::command]
pub async fn search_contacts(
    state: State<'_, AppState>,
    query: String,
) -> AppResult<Vec<Contact>> {
    contacts::list(
        &state.db,
        ContactFilter {
            query: Some(query),
            ..Default::default()
        },
    )
    .await
}
