//! Tauri commands pentru catalog de produse/articole.
//!
//! Toate comenzile sunt company-scoped: `company_id` este obligatoriu
//! și este verificat în layer-ul DB. Cross-company access returnează NotFound.

use tauri::State;

use crate::db::products::{self, Product, ProductInput, UpdateProductInput};
use crate::error::AppResult;
use crate::state::AppState;

/// R15: List all products for a company, with optional search query.
#[tauri::command]
pub async fn list_products(
    state: State<'_, AppState>,
    company_id: String,
    query: Option<String>,
) -> AppResult<Vec<Product>> {
    products::list(&state.db, &company_id, query.as_deref()).await
}

/// R15: Get a single product by id. Returns NotFound for wrong company.
#[tauri::command]
pub async fn get_product(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<Product> {
    products::get(&state.db, &id, &company_id).await
}

/// R15: Create a product for the given company.
#[tauri::command]
pub async fn create_product(
    state: State<'_, AppState>,
    company_id: String,
    input: ProductInput,
) -> AppResult<Product> {
    products::create(&state.db, &company_id, input).await
}

/// R15: Update a product. Cross-company update returns NotFound.
#[tauri::command]
pub async fn update_product(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: UpdateProductInput,
) -> AppResult<Product> {
    products::update(&state.db, &id, &company_id, input).await
}

/// R15: Delete a product. Cross-company deletion returns NotFound.
#[tauri::command]
pub async fn delete_product(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    products::delete(&state.db, &id, &company_id).await
}

/// R15: Search products by name/code for the picker. Scoped to company.
#[tauri::command]
pub async fn search_products(
    state: State<'_, AppState>,
    company_id: String,
    query: String,
) -> AppResult<Vec<Product>> {
    products::list(&state.db, &company_id, Some(&query)).await
}
