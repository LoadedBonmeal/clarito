//! Articole / catalog de produse (company-scoped).
//!
//! Fiecare produs aparține unei companii (company_id). Toate operațiunile
//! sunt scoped pe company_id — cross-company access returnează NotFound.
//!
//! Valorile monetare și cantitățile sunt stocate ca TEXT (convenția
//! Decimal-as-TEXT a aplicației).

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Product types + account mapping ──────────────────────────────────────────

/// Canonical product types (OMFP 1802/2014 plan de conturi).
pub const PRODUCT_TYPES: &[&str] = &[
    "marfa",
    "produs_finit",
    "materie_prima",
    "material_consumabil",
    "serviciu",
];

/// Effective account mapping returned by `resolve_accounts`.
/// Mirrors the account_mapping DB row but is always populated (falls back to
/// code defaults when no company override exists).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountMapping {
    /// GL stock account (e.g. "371", "345"). None for services.
    pub stock_account: Option<String>,
    /// GL expense account for stock issues / cost of sales (e.g. "607", "711"). None for services.
    pub expense_account: Option<String>,
    /// GL income account for sales revenue (e.g. "707", "704"). None for raw-material types.
    pub income_account: Option<String>,
    /// Whether this type maintains a stock balance.
    pub uses_stock: bool,
    /// Whether this type can be sold at retail (amănunt) prices (371-class only).
    pub retail_capable: bool,
}

/// Standard code defaults per product_type — OMFP 1802/2014.
/// This is the single source of truth; no DB seeding is needed.
pub fn default_account_mapping(product_type: &str) -> AccountMapping {
    match product_type {
        "marfa" => AccountMapping {
            stock_account: Some("371".into()),
            expense_account: Some("607".into()),
            income_account: Some("707".into()),
            uses_stock: true,
            retail_capable: true,
        },
        "produs_finit" => AccountMapping {
            stock_account: Some("345".into()),
            expense_account: Some("711".into()),
            income_account: Some("701".into()),
            uses_stock: true,
            retail_capable: false,
        },
        "materie_prima" => AccountMapping {
            stock_account: Some("301".into()),
            expense_account: Some("601".into()),
            income_account: None,
            uses_stock: true,
            retail_capable: false,
        },
        "material_consumabil" => AccountMapping {
            stock_account: Some("302".into()),
            expense_account: Some("602".into()),
            income_account: None,
            uses_stock: true,
            retail_capable: false,
        },
        "serviciu" => AccountMapping {
            stock_account: None,
            expense_account: None,
            income_account: Some("704".into()),
            uses_stock: false,
            retail_capable: false,
        },
        // Unknown type: fall back to marfa defaults (defensive).
        _ => AccountMapping {
            stock_account: Some("371".into()),
            expense_account: Some("607".into()),
            income_account: Some("707".into()),
            uses_stock: true,
            retail_capable: true,
        },
    }
}

// ─── AccountMapping DB row (for override CRUD) ────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AccountMappingRow {
    pub id: String,
    pub company_id: String,
    pub product_type: String,
    pub stock_account: Option<String>,
    pub expense_account: Option<String>,
    pub income_account: Option<String>,
    pub uses_stock: bool,
    pub retail_capable: bool,
    pub updated_at: i64,
}

/// Effective account mapping row — defaults merged with any company override.
/// Always returns one entry per product_type (5 rows total).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveAccountMapping {
    pub product_type: String,
    /// True when a company-specific override row exists.
    pub is_override: bool,
    pub stock_account: Option<String>,
    pub expense_account: Option<String>,
    pub income_account: Option<String>,
    pub uses_stock: bool,
    pub retail_capable: bool,
}

// ─── ProductGroup ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ProductGroup {
    pub id: String,
    pub company_id: String,
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductGroupInput {
    pub name: String,
}

// ─── SetAccountMappingInput ────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAccountMappingInput {
    pub stock_account: Option<String>,
    pub expense_account: Option<String>,
    pub income_account: Option<String>,
    pub uses_stock: bool,
    pub retail_capable: bool,
}

// ─── Model ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub id: String,
    pub company_id: String,
    pub name: String,
    pub unit: String,
    pub unit_price: String,
    pub vat_rate: String,
    pub vat_category: String,
    pub code: Option<String>,
    /// EAN-13 / GTIN barcode — preferred cross-system product dedup key (see Wave C importer).
    pub barcode: Option<String>,
    pub stock_qty: Option<String>,
    /// Art. 331 reverse-charge product category code (D394 op11 codPR).
    /// Allowed values: cereal NC codes + category codes 22–31,36 (tp=1) / 22,23,32–35 (tp=2).
    /// NULL = use default 22 in D394.
    pub art331_code: Option<String>,
    /// Stock valuation policy (OMFP 1802/2014 pct. 96): 'FIFO' | 'CMP' | 'LIFO' (default CMP).
    pub valuation_method: Option<String>,
    /// GL stock account for this product (371 mărfuri / 301 materii prime / 345 produse…).
    pub stock_account: Option<String>,
    /// True when this product is a service (non-stocabil): no fișă de magazie, no stock qty/valuation.
    /// GL revenue default: serviciu → account 704; marfă → account 707 (see db/gl.rs revenue_account).
    pub is_service: bool,
    /// Canonical product type (OMFP 1802/2014): marfa | produs_finit | materie_prima |
    /// material_consumabil | serviciu. Drives the default GL account mapping for NIR/producție.
    /// Kept consistent with is_service: serviciu ⇔ is_service=true.
    pub product_type: String,
    /// Optional product group (FK to product_groups.id). NULL = no group.
    pub product_group_id: Option<String>,
    pub active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

// ─── Inputs ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductInput {
    pub name: String,
    pub unit: Option<String>,
    pub unit_price: Option<String>,
    pub vat_rate: Option<String>,
    pub vat_category: Option<String>,
    pub code: Option<String>,
    /// EAN-13 / GTIN barcode — set by the Wave C importer; manual UI field is a later wave.
    pub barcode: Option<String>,
    pub stock_qty: Option<String>,
    /// Art. 331 reverse-charge product category code for D394 codPR.
    pub art331_code: Option<String>,
    /// True when this product is a service (non-stocabil). Defaults to false (goods).
    pub is_service: Option<bool>,
    /// Canonical product type. Defaults to "serviciu" when is_service=true, else "marfa".
    pub product_type: Option<String>,
    /// Optional product group id.
    pub product_group_id: Option<String>,
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProductInput {
    pub name: Option<String>,
    pub unit: Option<String>,
    pub unit_price: Option<String>,
    pub vat_rate: Option<String>,
    pub vat_category: Option<String>,
    pub code: Option<String>,
    /// EAN-13 / GTIN barcode.
    pub barcode: Option<String>,
    pub stock_qty: Option<String>,
    /// Art. 331 reverse-charge product category code for D394 codPR.
    pub art331_code: Option<String>,
    /// True when this product is a service (non-stocabil). None = leave unchanged (keeps current value).
    pub is_service: Option<bool>,
    /// Canonical product type. None = leave unchanged.
    pub product_type: Option<String>,
    /// Optional product group id. None = leave unchanged.
    pub product_group_id: Option<String>,
    pub active: Option<bool>,
}

/// Derive a canonical product_type from explicit input or fallback to is_service flag.
/// Rules:
///  - If `product_type` is explicitly set to a valid value → use it.
///  - If `is_service=true` and no explicit type → "serviciu".
///  - Otherwise → "marfa".
fn effective_product_type(product_type: Option<&str>, is_service: bool) -> String {
    if let Some(pt) = product_type {
        let pt = pt.trim();
        if PRODUCT_TYPES.contains(&pt) {
            return pt.to_string();
        }
    }
    if is_service {
        "serviciu".to_string()
    } else {
        "marfa".to_string()
    }
}

/// Câmpurile numerice FURNIZATE trebuie să fie valide — altfel un preț corupt ar fi stocat ca
/// atare și ar deveni tăcut 0 la fiecare citire ulterioară (dec()-fallback). Folosit identic la
/// create() și update() (paritate de validare).
fn validate_numeric_fields(unit_price: Option<&str>, vat_rate: Option<&str>) -> AppResult<()> {
    if let Some(p) = unit_price {
        let d = <rust_decimal::Decimal as std::str::FromStr>::from_str(p.trim()).map_err(|_| {
            AppError::Validation("Preț unitar invalid — folosiți formatul 1234.56.".into())
        })?;
        if d.is_sign_negative() {
            return Err(AppError::Validation(
                "Prețul unitar nu poate fi negativ.".into(),
            ));
        }
    }
    if let Some(r) = vat_rate {
        let ok = r
            .trim()
            .parse::<i64>()
            .map(|n| crate::db::models::VALID_VAT_RATES.contains(&n))
            .unwrap_or(false);
        if !ok {
            return Err(AppError::Validation(format!(
                "Cotă TVA invalidă: {r}. Valori permise: 0, 5, 9, 11, 19, 21."
            )));
        }
    }
    Ok(())
}

// ─── Queries ───────────────────────────────────────────────────────────────

/// List products for a company, with optional name/code search.
/// Always company-scoped: every row is filtered by `company_id = ?`.
pub async fn list(
    pool: &SqlitePool,
    company_id: &str,
    query: Option<&str>,
) -> AppResult<Vec<Product>> {
    let query_term = query.filter(|s| !s.is_empty());
    let items = sqlx::query_as::<_, Product>(
        "SELECT id, company_id, name, unit, unit_price, vat_rate, vat_category, \
         code, barcode, stock_qty, art331_code, valuation_method, stock_account, is_service, \
         product_type, product_group_id, active, created_at, updated_at \
         FROM products \
         WHERE company_id = ?1 \
           AND (?2 IS NULL OR name LIKE '%' || ?2 || '%' OR code LIKE '%' || ?2 || '%') \
         ORDER BY name",
    )
    .bind(company_id)
    .bind(query_term)
    .fetch_all(pool)
    .await?;
    Ok(items)
}

/// Fetch a single product by id; then verify ownership.
pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Product> {
    let product = sqlx::query_as::<_, Product>(
        "SELECT id, company_id, name, unit, unit_price, vat_rate, vat_category, \
         code, barcode, stock_qty, art331_code, valuation_method, stock_account, is_service, \
         product_type, product_group_id, active, created_at, updated_at \
         FROM products WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    // R15 company isolation: cross-company access returns NotFound.
    if product.company_id != company_id {
        return Err(AppError::NotFound);
    }
    Ok(product)
}

/// Create a new product for the given company.
pub async fn create(
    pool: &SqlitePool,
    company_id: &str,
    input: ProductInput,
) -> AppResult<Product> {
    // Task 3: prevent duplicate (company_id, code) per company.
    // Skip if code is empty/None.
    // Bind the trimmed code so a whitespace-padded value can't slip past the dup check.
    let code_trimmed: Option<String> = input
        .code
        .as_ref()
        .map(|c| c.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Some(ref code) = code_trimmed {
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM products WHERE company_id = ?1 AND code = ?2 LIMIT 1",
        )
        .bind(company_id)
        .bind(code)
        .fetch_optional(pool)
        .await?;
        if existing.is_some() {
            return Err(AppError::Validation(format!(
                "Există deja un produs cu codul '{}' pentru această companie.",
                code
            )));
        }
    }

    validate_numeric_fields(input.unit_price.as_deref(), input.vat_rate.as_deref())?;

    let id = new_id();
    let now = now_unix();
    let is_service = input.is_service.unwrap_or(false);
    let product_type = effective_product_type(input.product_type.as_deref(), is_service);
    // Keep is_service consistent: serviciu ⇔ is_service.
    let is_service_eff = is_service || product_type == "serviciu";

    sqlx::query(
        "INSERT INTO products (
            id, company_id, name, unit, unit_price, vat_rate, vat_category,
            code, barcode, stock_qty, art331_code, is_service,
            product_type, product_group_id, active, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10, ?11, ?12,
            ?13, ?14, ?15, ?16, ?16
        )",
    )
    .bind(&id) // ?1
    .bind(company_id) // ?2
    .bind(&input.name) // ?3
    .bind(input.unit.as_deref().unwrap_or("buc")) // ?4
    .bind(input.unit_price.as_deref().unwrap_or("0.00")) // ?5
    // 2026 standard rate (Legea 141/2025) when the caller omits it.
    .bind(input.vat_rate.as_deref().unwrap_or("21")) // ?6
    .bind(input.vat_category.as_deref().unwrap_or("S")) // ?7
    .bind(&code_trimmed) // ?8
    .bind(&input.barcode) // ?9
    .bind(&input.stock_qty) // ?10
    .bind(&input.art331_code) // ?11
    .bind(is_service_eff as i64) // ?12
    .bind(&product_type) // ?13
    .bind(&input.product_group_id) // ?14
    .bind(input.active.unwrap_or(true)) // ?15
    .bind(now) // ?16 (created_at = updated_at)
    .execute(pool)
    .await?;

    get(pool, &id, company_id).await
}

/// Update a product. Verifies ownership via `get` first.
/// The UPDATE SQL is also scoped with `AND company_id = ?` as defence-in-depth.
pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: UpdateProductInput,
) -> AppResult<Product> {
    let current = get(pool, id, company_id).await?;

    // Validation parity with create(): the new effective code must not duplicate
    // another product in the same company (excluding this product's own row).
    let effective_code: Option<String> = input
        .code
        .clone()
        .or_else(|| current.code.clone())
        .map(|c| c.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(ref code) = effective_code {
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM products WHERE company_id = ?1 AND code = ?2 AND id != ?3 LIMIT 1",
        )
        .bind(company_id)
        .bind(code)
        .bind(id)
        .fetch_optional(pool)
        .await?;
        if existing.is_some() {
            return Err(AppError::Validation(format!(
                "Există deja un produs cu codul '{}' pentru această companie.",
                code
            )));
        }
    }

    validate_numeric_fields(input.unit_price.as_deref(), input.vat_rate.as_deref())?;

    let now = now_unix();
    let is_service_new = input.is_service.unwrap_or(current.is_service);

    // Derive the effective product_type:
    // - If product_type is explicitly set in the input → use it (validates in effective_product_type).
    // - If is_service is explicitly set to false AND no explicit product_type AND current type is
    //   "serviciu" → switch to "marfa" (caller cleared the service flag).
    // - Otherwise inherit the current type.
    let product_type_new = if let Some(ref pt) = input.product_type {
        effective_product_type(Some(pt.as_str()), is_service_new)
    } else if input.is_service == Some(false) && current.product_type == "serviciu" {
        // Explicit is_service=false with no type override: move out of serviciu → marfa.
        "marfa".to_string()
    } else {
        effective_product_type(Some(&current.product_type), is_service_new)
    };
    // Keep is_service consistent.
    let is_service_eff = is_service_new || product_type_new == "serviciu";

    sqlx::query(
        "UPDATE products SET
            name             = ?2,
            unit             = ?3,
            unit_price       = ?4,
            vat_rate         = ?5,
            vat_category     = ?6,
            code             = ?7,
            barcode          = ?8,
            stock_qty        = ?9,
            art331_code      = ?10,
            is_service       = ?11,
            product_type     = ?12,
            product_group_id = ?13,
            active           = ?14,
            updated_at       = ?15
        WHERE id = ?1 AND company_id = ?16",
    )
    .bind(id) // ?1
    .bind(input.name.as_deref().unwrap_or(&current.name)) // ?2
    .bind(input.unit.as_deref().unwrap_or(&current.unit)) // ?3
    .bind(input.unit_price.as_deref().unwrap_or(&current.unit_price)) // ?4
    .bind(input.vat_rate.as_deref().unwrap_or(&current.vat_rate)) // ?5
    .bind(
        // ?6
        input
            .vat_category
            .as_deref()
            .unwrap_or(&current.vat_category),
    )
    .bind(input.code.or(current.code)) // ?7
    .bind(input.barcode.or(current.barcode)) // ?8
    .bind(input.stock_qty.or(current.stock_qty)) // ?9
    .bind(input.art331_code.or(current.art331_code)) // ?10
    .bind(is_service_eff as i64) // ?11
    .bind(&product_type_new) // ?12
    .bind(input.product_group_id.or(current.product_group_id)) // ?13
    .bind(input.active.unwrap_or(current.active)) // ?14
    .bind(now) // ?15
    .bind(company_id) // ?16
    .execute(pool)
    .await?;

    get(pool, id, company_id).await
}

/// Delete a product. Verifies ownership first; cross-company returns NotFound.
pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    // Verify ownership first for a clear NotFound on cross-company attempts.
    let product = get(pool, id, company_id).await?;
    if product.company_id != company_id {
        return Err(AppError::NotFound);
    }
    let res = sqlx::query("DELETE FROM products WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ─── Account mapping ───────────────────────────────────────────────────────────

/// Resolve the effective account mapping for (company, product_type):
/// - Returns the company OVERRIDE from `account_mapping` when present.
/// - Otherwise returns the code default from `default_account_mapping`.
///
/// This is the NEW lookup layer for future NIR/producție waves.
/// It does NOT touch `revenue_account` or `stock_expense_account` in gl.rs.
pub async fn resolve_accounts(
    pool: &SqlitePool,
    company_id: &str,
    product_type: &str,
) -> AppResult<AccountMapping> {
    let row = sqlx::query_as::<_, AccountMappingRow>(
        "SELECT id, company_id, product_type, stock_account, expense_account, income_account, \
         uses_stock, retail_capable, updated_at \
         FROM account_mapping \
         WHERE company_id = ?1 AND product_type = ?2 \
         LIMIT 1",
    )
    .bind(company_id)
    .bind(product_type)
    .fetch_optional(pool)
    .await?;

    Ok(match row {
        Some(r) => AccountMapping {
            stock_account: r.stock_account,
            expense_account: r.expense_account,
            income_account: r.income_account,
            uses_stock: r.uses_stock,
            retail_capable: r.retail_capable,
        },
        None => default_account_mapping(product_type),
    })
}

/// List effective account mappings for all 5 canonical product types.
/// Each row is either the company override or the code default.
/// The UI shows all 5 rows, marking which ones are overridden.
pub async fn list_account_mappings(
    pool: &SqlitePool,
    company_id: &str,
) -> AppResult<Vec<EffectiveAccountMapping>> {
    // Fetch all override rows for this company in one query.
    let overrides: Vec<AccountMappingRow> = sqlx::query_as::<_, AccountMappingRow>(
        "SELECT id, company_id, product_type, stock_account, expense_account, income_account, \
         uses_stock, retail_capable, updated_at \
         FROM account_mapping \
         WHERE company_id = ?1",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let result = PRODUCT_TYPES
        .iter()
        .map(
            |&pt| match overrides.iter().find(|r| r.product_type == pt) {
                Some(r) => EffectiveAccountMapping {
                    product_type: pt.to_string(),
                    is_override: true,
                    stock_account: r.stock_account.clone(),
                    expense_account: r.expense_account.clone(),
                    income_account: r.income_account.clone(),
                    uses_stock: r.uses_stock,
                    retail_capable: r.retail_capable,
                },
                None => {
                    let def = default_account_mapping(pt);
                    EffectiveAccountMapping {
                        product_type: pt.to_string(),
                        is_override: false,
                        stock_account: def.stock_account,
                        expense_account: def.expense_account,
                        income_account: def.income_account,
                        uses_stock: def.uses_stock,
                        retail_capable: def.retail_capable,
                    }
                }
            },
        )
        .collect();

    Ok(result)
}

/// Upsert a company override for a given product_type.
pub async fn set_account_mapping(
    pool: &SqlitePool,
    company_id: &str,
    product_type: &str,
    input: SetAccountMappingInput,
) -> AppResult<EffectiveAccountMapping> {
    if !PRODUCT_TYPES.contains(&product_type) {
        return Err(AppError::Validation(format!(
            "Tip produs invalid: {product_type}. Valori permise: {}",
            PRODUCT_TYPES.join(", ")
        )));
    }
    let id = new_id();
    let now = now_unix();

    sqlx::query(
        "INSERT INTO account_mapping \
         (id, company_id, product_type, stock_account, expense_account, income_account, \
          uses_stock, retail_capable, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(company_id, product_type) DO UPDATE SET \
           stock_account   = excluded.stock_account, \
           expense_account = excluded.expense_account, \
           income_account  = excluded.income_account, \
           uses_stock      = excluded.uses_stock, \
           retail_capable  = excluded.retail_capable, \
           updated_at      = excluded.updated_at",
    )
    .bind(&id)
    .bind(company_id)
    .bind(product_type)
    .bind(&input.stock_account)
    .bind(&input.expense_account)
    .bind(&input.income_account)
    .bind(input.uses_stock as i64)
    .bind(input.retail_capable as i64)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(EffectiveAccountMapping {
        product_type: product_type.to_string(),
        is_override: true,
        stock_account: input.stock_account,
        expense_account: input.expense_account,
        income_account: input.income_account,
        uses_stock: input.uses_stock,
        retail_capable: input.retail_capable,
    })
}

/// Delete the company override for a product_type → reverts to code default.
pub async fn reset_account_mapping(
    pool: &SqlitePool,
    company_id: &str,
    product_type: &str,
) -> AppResult<EffectiveAccountMapping> {
    sqlx::query("DELETE FROM account_mapping WHERE company_id = ?1 AND product_type = ?2")
        .bind(company_id)
        .bind(product_type)
        .execute(pool)
        .await?;

    let def = default_account_mapping(product_type);
    Ok(EffectiveAccountMapping {
        product_type: product_type.to_string(),
        is_override: false,
        stock_account: def.stock_account,
        expense_account: def.expense_account,
        income_account: def.income_account,
        uses_stock: def.uses_stock,
        retail_capable: def.retail_capable,
    })
}

// ─── Product groups ────────────────────────────────────────────────────────────

/// List product groups for a company.
pub async fn list_product_groups(
    pool: &SqlitePool,
    company_id: &str,
) -> AppResult<Vec<ProductGroup>> {
    let groups = sqlx::query_as::<_, ProductGroup>(
        "SELECT id, company_id, name, created_at \
         FROM product_groups \
         WHERE company_id = ?1 \
         ORDER BY name",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(groups)
}

/// Create a product group.
pub async fn create_product_group(
    pool: &SqlitePool,
    company_id: &str,
    input: ProductGroupInput,
) -> AppResult<ProductGroup> {
    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO product_groups (id, company_id, name, created_at) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(&id)
    .bind(company_id)
    .bind(input.name.trim())
    .bind(now)
    .execute(pool)
    .await?;

    let group = sqlx::query_as::<_, ProductGroup>(
        "SELECT id, company_id, name, created_at FROM product_groups WHERE id = ?1",
    )
    .bind(&id)
    .fetch_one(pool)
    .await?;
    Ok(group)
}

/// Delete a product group. Products referencing it keep the id (FK is nullable + no CASCADE on products).
pub async fn delete_product_group(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let res = sqlx::query("DELETE FROM product_groups WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    /// Run real migrations then seed two companies + one product each.
    async fn setup_products_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Seed two companies with valid production-schema columns.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county) VALUES \
             ('comp-1', 'RO12345674', 'Firma Unu SRL', 'Str. Test 1', 'București', 'B'), \
             ('comp-2', 'RO98765438', 'Firma Doi SRL', 'Str. Test 2', 'Cluj', 'CJ')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Seed one product per company.
        // vat_rate '19' and '9' are both valid per VALID_VAT_RATES.
        sqlx::query(
            "INSERT INTO products (id, company_id, name, unit_price, vat_rate, vat_category) \
             VALUES ('p1', 'comp-1', 'Produs Comp1', '100.00', '19', 'S'), \
                    ('p2', 'comp-2', 'Produs Comp2', '200.00', '9', 'S')",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    // ── get: wrong company → NotFound ───────────────────────────────────────

    #[tokio::test]
    async fn wave1_product_get_wrong_company_returns_not_found() {
        let pool = setup_products_pool().await;
        let result = get(&pool, "p1", "comp-2").await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "get with wrong company_id must return NotFound"
        );
    }

    #[tokio::test]
    async fn wave1_product_get_correct_company_succeeds() {
        let pool = setup_products_pool().await;
        let result = get(&pool, "p1", "comp-1").await;
        assert!(result.is_ok(), "get with correct company_id must succeed");
        assert_eq!(result.unwrap().name, "Produs Comp1");
    }

    // ── update: wrong company → NotFound ────────────────────────────────────

    #[tokio::test]
    async fn wave1_product_update_wrong_company_returns_not_found() {
        let pool = setup_products_pool().await;
        let input = UpdateProductInput {
            name: Some("Renamed".to_string()),
            ..Default::default()
        };
        // comp-2 tries to update comp-1's product.
        let result = update(&pool, "p1", "comp-2", input).await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "update with wrong company_id must return NotFound"
        );
        // Name must be unchanged.
        let product = get(&pool, "p1", "comp-1").await.unwrap();
        assert_eq!(product.name, "Produs Comp1", "name must not change");
    }

    #[tokio::test]
    async fn wave1_product_update_correct_company_succeeds() {
        let pool = setup_products_pool().await;
        let input = UpdateProductInput {
            name: Some("Produs Redenumit".to_string()),
            ..Default::default()
        };
        let result = update(&pool, "p1", "comp-1", input).await;
        assert!(
            result.is_ok(),
            "update with correct company_id must succeed"
        );
        let product = get(&pool, "p1", "comp-1").await.unwrap();
        assert_eq!(product.name, "Produs Redenumit", "name must be updated");
    }

    // ── delete: wrong company → NotFound ────────────────────────────────────

    #[tokio::test]
    async fn wave1_product_delete_wrong_company_returns_not_found() {
        let pool = setup_products_pool().await;
        // comp-2 tries to delete comp-1's product.
        let result = delete(&pool, "p1", "comp-2").await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "delete with wrong company_id must return NotFound"
        );
        // Product must still exist.
        let still_there = get(&pool, "p1", "comp-1").await;
        assert!(still_there.is_ok(), "product must not have been deleted");
    }

    #[tokio::test]
    async fn wave1_product_delete_correct_company_succeeds() {
        let pool = setup_products_pool().await;
        let result = delete(&pool, "p1", "comp-1").await;
        assert!(
            result.is_ok(),
            "delete with correct company_id must succeed"
        );
        let gone = get(&pool, "p1", "comp-1").await;
        assert!(
            matches!(gone, Err(AppError::NotFound)),
            "product must be gone after correct-company delete"
        );
    }

    // ── create + list round-trip ─────────────────────────────────────────────

    #[tokio::test]
    async fn wave1_product_create_list_round_trip() {
        let pool = setup_products_pool().await;
        let input = ProductInput {
            name: "Serviciu Consulting".to_string(),
            unit: Some("ora".to_string()),
            unit_price: Some("350.00".to_string()),
            vat_rate: Some("19".to_string()),
            vat_category: Some("S".to_string()),
            code: Some("SVC-001".to_string()),
            barcode: None,
            stock_qty: None,
            art331_code: None,
            is_service: None,
            product_type: None,
            product_group_id: None,
            active: Some(true),
        };
        let created = create(&pool, "comp-1", input).await.unwrap();
        assert_eq!(created.company_id, "comp-1");
        assert_eq!(created.name, "Serviciu Consulting");
        assert_eq!(created.unit, "ora");
        assert_eq!(created.code, Some("SVC-001".to_string()));

        let list_result = list(&pool, "comp-1", None).await.unwrap();
        // comp-1 now has p1 + newly created
        assert_eq!(list_result.len(), 2);
        assert!(list_result.iter().any(|p| p.id == created.id));

        // comp-2 must not see comp-1's products.
        let list_comp2 = list(&pool, "comp-2", None).await.unwrap();
        assert_eq!(list_comp2.len(), 1);
        assert_eq!(list_comp2[0].id, "p2");
    }

    // ── search filters by name/code ──────────────────────────────────────────

    #[tokio::test]
    async fn wave1_product_search_filters_by_name() {
        let pool = setup_products_pool().await;
        let result = list(&pool, "comp-1", Some("Comp1")).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "p1");

        let empty = list(&pool, "comp-1", Some("xyznotexists")).await.unwrap();
        assert!(empty.is_empty());
    }

    // ── Task 3: duplicate (company_id, code) rejected ────────────────────────

    #[tokio::test]
    async fn task3_duplicate_product_code_per_company_rejected() {
        let pool = setup_products_pool().await;

        // First product with code SVC-001 for comp-1 — should succeed.
        let input1 = ProductInput {
            name: "Serviciu A".to_string(),
            unit: Some("ora".to_string()),
            unit_price: Some("100.00".to_string()),
            vat_rate: Some("19".to_string()),
            vat_category: Some("S".to_string()),
            code: Some("SVC-001".to_string()),
            barcode: None,
            stock_qty: None,
            art331_code: None,
            is_service: None,
            product_type: None,
            product_group_id: None,
            active: Some(true),
        };
        let r1 = create(&pool, "comp-1", input1).await;
        assert!(r1.is_ok(), "first product with code SVC-001 must succeed");

        // Second product with same code for same company — must fail.
        let input2 = ProductInput {
            name: "Serviciu B".to_string(),
            unit: Some("ora".to_string()),
            unit_price: Some("200.00".to_string()),
            vat_rate: Some("19".to_string()),
            vat_category: Some("S".to_string()),
            code: Some("SVC-001".to_string()),
            barcode: None,
            stock_qty: None,
            art331_code: None,
            is_service: None,
            product_type: None,
            product_group_id: None,
            active: Some(true),
        };
        let r2 = create(&pool, "comp-1", input2).await;
        assert!(
            matches!(r2, Err(AppError::Validation(_))),
            "duplicate code for same company must return Validation error"
        );

        // Same code for a different company — must succeed.
        let input3 = ProductInput {
            name: "Serviciu C".to_string(),
            unit: Some("ora".to_string()),
            unit_price: Some("300.00".to_string()),
            vat_rate: Some("19".to_string()),
            vat_category: Some("S".to_string()),
            code: Some("SVC-001".to_string()),
            barcode: None,
            stock_qty: None,
            art331_code: None,
            is_service: None,
            product_type: None,
            product_group_id: None,
            active: Some(true),
        };
        let r3 = create(&pool, "comp-2", input3).await;
        assert!(r3.is_ok(), "same code for a different company must succeed");
    }

    #[tokio::test]
    async fn task3_empty_code_allows_duplicates() {
        let pool = setup_products_pool().await;

        // Products with no code (None) are always allowed, even with same name.
        let input1 = ProductInput {
            name: "Fara Cod 1".to_string(),
            unit: None,
            unit_price: None,
            vat_rate: None,
            vat_category: None,
            code: None,
            barcode: None,
            stock_qty: None,
            art331_code: None,
            is_service: None,
            product_type: None,
            product_group_id: None,
            active: None,
        };
        let input2 = ProductInput {
            name: "Fara Cod 2".to_string(),
            unit: None,
            unit_price: None,
            vat_rate: None,
            vat_category: None,
            code: None,
            barcode: None,
            stock_qty: None,
            art331_code: None,
            is_service: None,
            product_type: None,
            product_group_id: None,
            active: None,
        };
        let r1 = create(&pool, "comp-1", input1).await;
        let r2 = create(&pool, "comp-1", input2).await;
        assert!(r1.is_ok(), "product without code must be allowed");
        assert!(r2.is_ok(), "second product without code must be allowed");
    }

    // ── is_service round-trip: create → get → update ─────────────────────────

    /// A normal product defaults is_service=false.
    #[tokio::test]
    async fn p1b_is_service_default_is_false() {
        let pool = setup_products_pool().await;
        // p1 was seeded without explicit is_service → DEFAULT 0 → false
        let p = get(&pool, "p1", "comp-1").await.unwrap();
        assert!(
            !p.is_service,
            "seeded product must default to is_service=false"
        );
    }

    /// create is_service=true → get returns true → update to false → get returns false.
    #[tokio::test]
    async fn p1b_is_service_create_get_update_round_trip() {
        let pool = setup_products_pool().await;

        // 1. Create a service product (is_service = true).
        let input = ProductInput {
            name: "Consultanță IT".to_string(),
            unit: Some("ora".to_string()),
            unit_price: Some("150.00".to_string()),
            vat_rate: Some("21".to_string()),
            vat_category: Some("S".to_string()),
            code: Some("CONS-IT-01".to_string()),
            barcode: None,
            stock_qty: None,
            art331_code: None,
            is_service: Some(true),
            product_type: None,
            product_group_id: None,
            active: Some(true),
        };
        let created = create(&pool, "comp-1", input).await.unwrap();
        assert!(
            created.is_service,
            "created product must have is_service=true"
        );

        // 2. get() must persist the flag.
        let fetched = get(&pool, &created.id, "comp-1").await.unwrap();
        assert!(
            fetched.is_service,
            "get() after create must return is_service=true"
        );

        // 3. update() to is_service=false.
        let upd = UpdateProductInput {
            is_service: Some(false),
            ..Default::default()
        };
        let updated = update(&pool, &created.id, "comp-1", upd).await.unwrap();
        assert!(!updated.is_service, "update() must set is_service=false");

        // 4. get() must reflect the change.
        let refetched = get(&pool, &created.id, "comp-1").await.unwrap();
        assert!(
            !refetched.is_service,
            "get() after update must return is_service=false"
        );
    }

    // ── P2 Wave 1: product_type round-trips ─────────────────────────────────

    /// product_type defaults to 'marfa' for goods.
    #[tokio::test]
    async fn p2w1_product_type_default_marfa() {
        let pool = setup_products_pool().await;
        let p = get(&pool, "p1", "comp-1").await.unwrap();
        assert_eq!(
            p.product_type, "marfa",
            "seeded product without explicit type must be 'marfa'"
        );
    }

    /// When is_service=true and product_type is None → 'serviciu'.
    #[tokio::test]
    async fn p2w1_product_type_derived_from_is_service() {
        let pool = setup_products_pool().await;
        let input = ProductInput {
            name: "Serviciu transport".to_string(),
            unit: Some("km".to_string()),
            unit_price: Some("2.50".to_string()),
            vat_rate: Some("21".to_string()),
            vat_category: Some("S".to_string()),
            code: None,
            barcode: None,
            stock_qty: None,
            art331_code: None,
            is_service: Some(true),
            product_type: None, // should derive 'serviciu'
            product_group_id: None,
            active: Some(true),
        };
        let created = create(&pool, "comp-1", input).await.unwrap();
        assert_eq!(created.product_type, "serviciu");
        assert!(created.is_service);
    }

    /// Explicit product_type wins over is_service derivation.
    #[tokio::test]
    async fn p2w1_product_type_explicit_wins() {
        let pool = setup_products_pool().await;
        let input = ProductInput {
            name: "Materie prima fier".to_string(),
            unit: Some("kg".to_string()),
            unit_price: Some("10.00".to_string()),
            vat_rate: Some("21".to_string()),
            vat_category: Some("S".to_string()),
            code: None,
            barcode: None,
            stock_qty: None,
            art331_code: None,
            is_service: Some(false),
            product_type: Some("materie_prima".to_string()),
            product_group_id: None,
            active: Some(true),
        };
        let created = create(&pool, "comp-1", input).await.unwrap();
        assert_eq!(created.product_type, "materie_prima");
        assert!(!created.is_service);
    }

    /// Update product_type round-trip: marfa → produs_finit.
    #[tokio::test]
    async fn p2w1_product_type_update_round_trip() {
        let pool = setup_products_pool().await;
        let upd = UpdateProductInput {
            product_type: Some("produs_finit".to_string()),
            ..Default::default()
        };
        let updated = update(&pool, "p1", "comp-1", upd).await.unwrap();
        assert_eq!(updated.product_type, "produs_finit");
        let refetched = get(&pool, "p1", "comp-1").await.unwrap();
        assert_eq!(refetched.product_type, "produs_finit");
    }

    /// serviciu ⇔ is_service consistency: setting product_type=serviciu sets is_service=true.
    #[tokio::test]
    async fn p2w1_serviciu_sets_is_service() {
        let pool = setup_products_pool().await;
        // p1 starts as marfa + is_service=false.
        let upd = UpdateProductInput {
            product_type: Some("serviciu".to_string()),
            is_service: Some(false), // explicit false, but type=serviciu should override
            ..Default::default()
        };
        let updated = update(&pool, "p1", "comp-1", upd).await.unwrap();
        assert_eq!(updated.product_type, "serviciu");
        assert!(
            updated.is_service,
            "product_type=serviciu must set is_service=true"
        );
    }

    // ── P2 Wave 1: resolve_accounts ─────────────────────────────────────────

    /// No override → returns code default for marfa (371/607/707).
    #[tokio::test]
    async fn p2w1_resolve_accounts_marfa_default() {
        let pool = setup_products_pool().await;
        let m = resolve_accounts(&pool, "comp-1", "marfa").await.unwrap();
        assert_eq!(m.stock_account.as_deref(), Some("371"));
        assert_eq!(m.expense_account.as_deref(), Some("607"));
        assert_eq!(m.income_account.as_deref(), Some("707"));
        assert!(m.uses_stock);
        assert!(m.retail_capable);
    }

    /// No override → returns code default for produs_finit (345/711/701).
    #[tokio::test]
    async fn p2w1_resolve_accounts_produs_finit_default() {
        let pool = setup_products_pool().await;
        let m = resolve_accounts(&pool, "comp-1", "produs_finit")
            .await
            .unwrap();
        assert_eq!(m.stock_account.as_deref(), Some("345"));
        assert_eq!(m.expense_account.as_deref(), Some("711"));
        assert_eq!(m.income_account.as_deref(), Some("701"));
        assert!(m.uses_stock);
        assert!(!m.retail_capable);
    }

    /// No override → returns code default for serviciu (None/None/704, no stock).
    #[tokio::test]
    async fn p2w1_resolve_accounts_serviciu_default() {
        let pool = setup_products_pool().await;
        let m = resolve_accounts(&pool, "comp-1", "serviciu").await.unwrap();
        assert!(m.stock_account.is_none());
        assert!(m.expense_account.is_none());
        assert_eq!(m.income_account.as_deref(), Some("704"));
        assert!(!m.uses_stock);
        assert!(!m.retail_capable);
    }

    /// With an override row → returns the override values.
    #[tokio::test]
    async fn p2w1_resolve_accounts_override_wins() {
        let pool = setup_products_pool().await;
        let input = SetAccountMappingInput {
            stock_account: Some("3711".to_string()),
            expense_account: Some("6071".to_string()),
            income_account: Some("7071".to_string()),
            uses_stock: true,
            retail_capable: true,
        };
        set_account_mapping(&pool, "comp-1", "marfa", input)
            .await
            .unwrap();

        let m = resolve_accounts(&pool, "comp-1", "marfa").await.unwrap();
        assert_eq!(
            m.stock_account.as_deref(),
            Some("3711"),
            "override stock account must be returned"
        );
        assert_eq!(m.expense_account.as_deref(), Some("6071"));
        assert_eq!(m.income_account.as_deref(), Some("7071"));
    }

    /// After reset_account_mapping → returns code default again.
    #[tokio::test]
    async fn p2w1_reset_account_mapping_reverts_to_default() {
        let pool = setup_products_pool().await;
        let input = SetAccountMappingInput {
            stock_account: Some("3711".to_string()),
            expense_account: Some("6071".to_string()),
            income_account: Some("7071".to_string()),
            uses_stock: true,
            retail_capable: false,
        };
        set_account_mapping(&pool, "comp-1", "marfa", input)
            .await
            .unwrap();
        reset_account_mapping(&pool, "comp-1", "marfa")
            .await
            .unwrap();

        let m = resolve_accounts(&pool, "comp-1", "marfa").await.unwrap();
        assert_eq!(
            m.stock_account.as_deref(),
            Some("371"),
            "after reset must return code default"
        );
        assert_eq!(m.income_account.as_deref(), Some("707"));
    }

    // ── P2 Wave 1: list_account_mappings returns all 5 rows ─────────────────

    #[tokio::test]
    async fn p2w1_list_account_mappings_returns_all_five() {
        let pool = setup_products_pool().await;
        let rows = list_account_mappings(&pool, "comp-1").await.unwrap();
        assert_eq!(
            rows.len(),
            5,
            "must always return exactly 5 rows (all product types)"
        );
        let types: Vec<&str> = rows.iter().map(|r| r.product_type.as_str()).collect();
        for pt in PRODUCT_TYPES {
            assert!(types.contains(pt), "must include product_type={pt}");
        }
        // All must be non-override (no overrides set).
        assert!(
            rows.iter().all(|r| !r.is_override),
            "all must be defaults when no overrides exist"
        );
    }

    #[tokio::test]
    async fn p2w1_list_account_mappings_shows_override_flag() {
        let pool = setup_products_pool().await;
        let input = SetAccountMappingInput {
            stock_account: Some("3711".to_string()),
            expense_account: Some("6071".to_string()),
            income_account: Some("7071".to_string()),
            uses_stock: true,
            retail_capable: true,
        };
        set_account_mapping(&pool, "comp-1", "marfa", input)
            .await
            .unwrap();

        let rows = list_account_mappings(&pool, "comp-1").await.unwrap();
        assert_eq!(rows.len(), 5);
        let marfa = rows.iter().find(|r| r.product_type == "marfa").unwrap();
        assert!(marfa.is_override, "marfa row must be marked as override");
        let serviciu = rows.iter().find(|r| r.product_type == "serviciu").unwrap();
        assert!(!serviciu.is_override, "serviciu row must remain default");
    }

    // ── P2 Wave 1: confirm no existing GL posting paths changed ─────────────

    /// Verifies that the revenue_account / stock_expense_account functions in gl.rs
    /// are separate from resolve_accounts: resolve_accounts is a new function that
    /// does not interfere with existing invoice/stock posting logic.
    /// This test calls resolve_accounts in isolation and verifies it doesn't panic or error.
    #[tokio::test]
    async fn p2w1_resolve_accounts_is_additive_only() {
        let pool = setup_products_pool().await;
        // Call resolve_accounts for all types — must succeed with no side effects.
        for pt in PRODUCT_TYPES {
            let result = resolve_accounts(&pool, "comp-1", pt).await;
            assert!(result.is_ok(), "resolve_accounts({pt}) must not error");
        }
        // No data in products table was changed.
        let p = get(&pool, "p1", "comp-1").await.unwrap();
        assert_eq!(p.name, "Produs Comp1", "existing product must be untouched");
    }
}
