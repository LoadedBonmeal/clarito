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
    pub stock_qty: Option<String>,
    /// Art. 331 reverse-charge product category code (D394 op11 codPR).
    /// Allowed values: cereal NC codes + category codes 22–31,36 (tp=1) / 22,23,32–35 (tp=2).
    /// NULL = use default 22 in D394.
    pub art331_code: Option<String>,
    /// Stock valuation policy (OMFP 1802/2014 pct. 96): 'FIFO' | 'CMP' | 'LIFO' (default CMP).
    pub valuation_method: Option<String>,
    /// GL stock account for this product (371 mărfuri / 301 materii prime / 345 produse…).
    pub stock_account: Option<String>,
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
    pub stock_qty: Option<String>,
    /// Art. 331 reverse-charge product category code for D394 codPR.
    pub art331_code: Option<String>,
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
    pub stock_qty: Option<String>,
    /// Art. 331 reverse-charge product category code for D394 codPR.
    pub art331_code: Option<String>,
    pub active: Option<bool>,
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
         code, stock_qty, art331_code, valuation_method, stock_account, active, created_at, updated_at \
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
         code, stock_qty, art331_code, valuation_method, stock_account, active, created_at, updated_at \
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

    sqlx::query(
        "INSERT INTO products (
            id, company_id, name, unit, unit_price, vat_rate, vat_category,
            code, stock_qty, art331_code, active, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10, ?11, ?12, ?12
        )",
    )
    .bind(&id)
    .bind(company_id)
    .bind(&input.name)
    .bind(input.unit.as_deref().unwrap_or("buc"))
    .bind(input.unit_price.as_deref().unwrap_or("0.00"))
    // 2026 standard rate (Legea 141/2025) when the caller omits it.
    .bind(input.vat_rate.as_deref().unwrap_or("21"))
    .bind(input.vat_category.as_deref().unwrap_or("S"))
    .bind(&code_trimmed)
    .bind(&input.stock_qty)
    .bind(&input.art331_code)
    .bind(input.active.unwrap_or(true))
    .bind(now)
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

    sqlx::query(
        "UPDATE products SET
            name         = ?2,
            unit         = ?3,
            unit_price   = ?4,
            vat_rate     = ?5,
            vat_category = ?6,
            code         = ?7,
            stock_qty    = ?8,
            art331_code  = ?9,
            active       = ?10,
            updated_at   = ?11
        WHERE id = ?1 AND company_id = ?12",
    )
    .bind(id)
    .bind(input.name.as_deref().unwrap_or(&current.name))
    .bind(input.unit.as_deref().unwrap_or(&current.unit))
    .bind(input.unit_price.as_deref().unwrap_or(&current.unit_price))
    .bind(input.vat_rate.as_deref().unwrap_or(&current.vat_rate))
    .bind(
        input
            .vat_category
            .as_deref()
            .unwrap_or(&current.vat_category),
    )
    .bind(input.code.or(current.code))
    .bind(input.stock_qty.or(current.stock_qty))
    .bind(input.art331_code.or(current.art331_code))
    .bind(input.active.unwrap_or(current.active))
    .bind(now)
    .bind(company_id)
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

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Minimal in-memory schema for products Wave 1 tests.
    async fn setup_products_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        sqlx::query(
            "CREATE TABLE products (
                id           TEXT    PRIMARY KEY NOT NULL,
                company_id   TEXT    NOT NULL,
                name         TEXT    NOT NULL,
                unit         TEXT    NOT NULL DEFAULT 'buc',
                unit_price   TEXT    NOT NULL DEFAULT '0.00',
                vat_rate     TEXT    NOT NULL DEFAULT '19',
                vat_category TEXT    NOT NULL DEFAULT 'S',
                code         TEXT,
                stock_qty    TEXT,
                art331_code  TEXT,
                valuation_method TEXT,
                stock_account    TEXT,
                active       INTEGER NOT NULL DEFAULT 1,
                created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at   INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Seed: two companies, two products — one per company.
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
            stock_qty: None,
            art331_code: None,
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
            stock_qty: None,
            art331_code: None,
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
            stock_qty: None,
            art331_code: None,
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
            stock_qty: None,
            art331_code: None,
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
            stock_qty: None,
            art331_code: None,
            active: None,
        };
        let input2 = ProductInput {
            name: "Fara Cod 2".to_string(),
            unit: None,
            unit_price: None,
            vat_rate: None,
            vat_category: None,
            code: None,
            stock_qty: None,
            art331_code: None,
            active: None,
        };
        let r1 = create(&pool, "comp-1", input1).await;
        let r2 = create(&pool, "comp-1", input2).await;
        assert!(r1.is_ok(), "product without code must be allowed");
        assert!(r2.is_ok(), "second product without code must be allowed");
    }
}
