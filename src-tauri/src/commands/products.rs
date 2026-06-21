//! Tauri commands pentru catalog de produse/articole.
//!
//! Toate comenzile sunt company-scoped: `company_id` este obligatoriu
//! și este verificat în layer-ul DB. Cross-company access returnează NotFound.

use tauri::State;

use crate::db::products::{
    self, AccountMapping, EffectiveAccountMapping, Product, ProductGroup, ProductGroupInput,
    ProductInput, SetAccountMappingInput, UpdateProductInput,
};
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

// ─── P2 Wave 1: account mapping commands ─────────────────────────────────────

/// Resolve the effective account mapping for a (company, product_type) pair.
/// Returns the company override if present, else the code default.
#[tauri::command]
pub async fn resolve_accounts(
    state: State<'_, AppState>,
    company_id: String,
    product_type: String,
) -> AppResult<AccountMapping> {
    products::resolve_accounts(&state.db, &company_id, &product_type).await
}

/// List effective account mappings for all 5 canonical product types.
/// Returns defaults merged with any company overrides (5 rows always).
#[tauri::command]
pub async fn list_account_mappings(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<EffectiveAccountMapping>> {
    products::list_account_mappings(&state.db, &company_id).await
}

/// Upsert a company override for a product_type.
#[tauri::command]
pub async fn set_account_mapping(
    state: State<'_, AppState>,
    company_id: String,
    product_type: String,
    input: SetAccountMappingInput,
) -> AppResult<EffectiveAccountMapping> {
    products::set_account_mapping(&state.db, &company_id, &product_type, input).await
}

/// Delete the company override for a product_type → reverts to code default.
#[tauri::command]
pub async fn reset_account_mapping(
    state: State<'_, AppState>,
    company_id: String,
    product_type: String,
) -> AppResult<EffectiveAccountMapping> {
    products::reset_account_mapping(&state.db, &company_id, &product_type).await
}

// ─── P2 Wave 1: product groups commands ──────────────────────────────────────

/// List product groups for a company.
#[tauri::command]
pub async fn list_product_groups(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<ProductGroup>> {
    products::list_product_groups(&state.db, &company_id).await
}

/// Create a product group.
#[tauri::command]
pub async fn create_product_group(
    state: State<'_, AppState>,
    company_id: String,
    input: ProductGroupInput,
) -> AppResult<ProductGroup> {
    products::create_product_group(&state.db, &company_id, input).await
}

/// Delete a product group. Products referencing it keep their group_id (nullable FK).
#[tauri::command]
pub async fn delete_product_group(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    products::delete_product_group(&state.db, &id, &company_id).await
}
