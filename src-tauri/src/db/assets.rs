//! Mijloace fixe (Assets SAF-T MasterFiles section).
//!
//! Fiecare mijloc fix aparține unei companii (company_id).
//! Calculul amortizării liniare este integrat (monthly = cost / life_months).
//!
//! Valorile monetare sunt stocate ca TEXT (convenția Decimal-as-TEXT).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Models ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct FixedAsset {
    pub id: String,
    pub company_id: String,
    pub asset_code: String,
    pub account_id: String,
    pub description: String,
    pub valuation_class: String,
    pub supplier_id: String,
    pub supplier_name: String,
    pub date_of_acquisition: String, // YYYY-MM-DD
    pub start_up_date: String,       // YYYY-MM-DD
    pub acquisition_cost: String,
    pub life_months: i64,
    pub depreciation_method: String,
    pub depreciation_pct: String,
    pub disposal_date: Option<String>,
    pub active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AssetTransaction {
    pub id: String,
    pub company_id: String,
    pub asset_id: String,
    pub transaction_code: String,
    pub transaction_type: String, // DUK AssetTransactionType numeric code
    pub transaction_date: String,
    pub description: String,
    pub gl_transaction_id: Option<String>,
    pub acq_prod_cost: String,
    pub book_value: String,
    pub amount: String,
    pub created_at: i64,
}

// ─── Depreciation result ───────────────────────────────────────────────────

/// Result of a straight-line depreciation calculation for a given period.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepreciationCalc {
    /// Acquisition cost
    pub cost: Decimal,
    /// Monthly depreciation charge (cost / life_months)
    pub monthly: Decimal,
    /// Accumulated depreciation at the period start date (capped at cost)
    pub accumulated_begin: Decimal,
    /// Accumulated depreciation at the period end date (capped at cost)
    pub accumulated_end: Decimal,
    /// Depreciation charge for the period = accumulated_end − accumulated_begin
    pub for_period: Decimal,
    /// Book value at period start = cost − accumulated_begin
    pub book_value_begin: Decimal,
    /// Book value at period end = cost − accumulated_end
    pub book_value_end: Decimal,
}

// ─── Inputs ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixedAssetInput {
    pub asset_code: String,
    pub account_id: Option<String>,
    pub description: String,
    pub valuation_class: Option<String>,
    pub supplier_id: Option<String>,
    pub supplier_name: Option<String>,
    pub date_of_acquisition: String,
    pub start_up_date: Option<String>,
    pub acquisition_cost: String,
    pub life_months: Option<i64>,
    pub depreciation_method: Option<String>,
    pub depreciation_pct: Option<String>,
    pub disposal_date: Option<String>,
    pub active: Option<bool>,
}

// ─── Queries ───────────────────────────────────────────────────────────────

/// List all active fixed assets for a company.
pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<FixedAsset>> {
    let items = sqlx::query_as::<_, FixedAsset>(
        "SELECT id, company_id, asset_code, account_id, description, valuation_class, \
                supplier_id, supplier_name, date_of_acquisition, start_up_date, \
                acquisition_cost, life_months, depreciation_method, depreciation_pct, \
                disposal_date, active, created_at, updated_at \
         FROM fixed_assets \
         WHERE company_id = ?1 \
         ORDER BY asset_code ASC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(items)
}

/// Fetch a single fixed asset by id; verifies company ownership.
pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<FixedAsset> {
    let asset = sqlx::query_as::<_, FixedAsset>(
        "SELECT id, company_id, asset_code, account_id, description, valuation_class, \
                supplier_id, supplier_name, date_of_acquisition, start_up_date, \
                acquisition_cost, life_months, depreciation_method, depreciation_pct, \
                disposal_date, active, created_at, updated_at \
         FROM fixed_assets WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    if asset.company_id != company_id {
        return Err(AppError::NotFound);
    }
    Ok(asset)
}

/// Create a new fixed asset for the given company.
pub async fn create(
    pool: &SqlitePool,
    company_id: &str,
    input: FixedAssetInput,
) -> AppResult<FixedAsset> {
    // asset_code must be unique per company
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM fixed_assets WHERE company_id = ?1 AND asset_code = ?2 LIMIT 1",
    )
    .bind(company_id)
    .bind(&input.asset_code)
    .fetch_optional(pool)
    .await?;
    if existing.is_some() {
        return Err(AppError::Validation(format!(
            "Există deja un mijloc fix cu codul '{}' pentru această companie.",
            input.asset_code
        )));
    }

    let id = new_id();
    let now = now_unix();
    let start_up = input
        .start_up_date
        .as_deref()
        .unwrap_or(&input.date_of_acquisition)
        .to_string();

    sqlx::query(
        "INSERT INTO fixed_assets (
            id, company_id, asset_code, account_id, description, valuation_class,
            supplier_id, supplier_name, date_of_acquisition, start_up_date,
            acquisition_cost, life_months, depreciation_method, depreciation_pct,
            disposal_date, active, created_at, updated_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?17)",
    )
    .bind(&id)
    .bind(company_id)
    .bind(&input.asset_code)
    .bind(input.account_id.as_deref().unwrap_or("213"))
    .bind(&input.description)
    .bind(input.valuation_class.as_deref().unwrap_or("Corporala"))
    .bind(input.supplier_id.as_deref().unwrap_or("0"))
    .bind(input.supplier_name.as_deref().unwrap_or(""))
    .bind(&input.date_of_acquisition)
    .bind(&start_up)
    .bind(&input.acquisition_cost)
    .bind(input.life_months.unwrap_or(60))
    .bind(input.depreciation_method.as_deref().unwrap_or("liniara"))
    .bind(input.depreciation_pct.as_deref().unwrap_or("0.00"))
    .bind(&input.disposal_date)
    .bind(input.active.unwrap_or(true) as i32)
    .bind(now)
    .execute(pool)
    .await?;

    get(pool, &id, company_id).await
}

/// Delete a fixed asset (cascades to asset_transactions). Verifies ownership.
pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let asset = get(pool, id, company_id).await?;
    if asset.company_id != company_id {
        return Err(AppError::NotFound);
    }
    let res = sqlx::query("DELETE FROM fixed_assets WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// List asset transactions for a company in a date range.
pub async fn list_transactions(
    pool: &SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<Vec<AssetTransaction>> {
    let items = sqlx::query_as::<_, AssetTransaction>(
        "SELECT id, company_id, asset_id, transaction_code, transaction_type, \
                transaction_date, description, gl_transaction_id, \
                acq_prod_cost, book_value, amount, created_at \
         FROM asset_transactions \
         WHERE company_id = ?1 \
           AND transaction_date >= ?2 \
           AND transaction_date <= ?3 \
         ORDER BY transaction_date ASC",
    )
    .bind(company_id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(pool)
    .await?;
    Ok(items)
}

// ─── Straight-line depreciation calculator ────────────────────────────────

/// Compute straight-line depreciation for an asset over a given period.
///
/// The period is defined by (begin_date, end_date), both `YYYY-MM-DD`.
/// Months elapsed is calculated as (year*12+month) difference from acquisition.
///
/// # Rules
/// - monthly charge = acquisition_cost / life_months
/// - accumulated(date) = months_elapsed_since_acquisition(date) * monthly, capped at cost
/// - for_period = accumulated_end − accumulated_begin
/// - book_value = cost − accumulated
///
/// If life_months == 0, all depreciation amounts are zero (avoids divide-by-zero).
pub fn compute_depreciation(
    asset: &FixedAsset,
    begin_date: &str,
    end_date: &str,
) -> DepreciationCalc {
    let cost = Decimal::from_str(asset.acquisition_cost.trim()).unwrap_or(Decimal::ZERO);

    if asset.life_months <= 0 || cost <= Decimal::ZERO {
        return DepreciationCalc {
            cost,
            monthly: Decimal::ZERO,
            accumulated_begin: Decimal::ZERO,
            accumulated_end: Decimal::ZERO,
            for_period: Decimal::ZERO,
            book_value_begin: cost,
            book_value_end: cost,
        };
    }

    let monthly = (cost / Decimal::from(asset.life_months)).round_dp(2);

    let months_begin = months_elapsed_since_acquisition(&asset.date_of_acquisition, begin_date);
    let months_end = months_elapsed_since_acquisition(&asset.date_of_acquisition, end_date);

    // Accumulated depreciation: months × monthly, capped at cost, never negative.
    let acc_begin = cap_at_cost(Decimal::from(months_begin.max(0)) * monthly, cost);
    let acc_end = cap_at_cost(Decimal::from(months_end.max(0)) * monthly, cost);

    let for_period = (acc_end - acc_begin).max(Decimal::ZERO);

    DepreciationCalc {
        cost,
        monthly,
        accumulated_begin: acc_begin,
        accumulated_end: acc_end,
        for_period,
        book_value_begin: (cost - acc_begin).max(Decimal::ZERO),
        book_value_end: (cost - acc_end).max(Decimal::ZERO),
    }
}

/// Number of full months elapsed from `acquisition_date` to `as_of_date`.
/// Both dates are `YYYY-MM-DD`. Returns 0 if as_of_date is before acquisition.
fn months_elapsed_since_acquisition(acquisition_date: &str, as_of_date: &str) -> i64 {
    let (acq_y, acq_m) = parse_ym(acquisition_date);
    let (as_y, as_m) = parse_ym(as_of_date);
    let elapsed = (as_y * 12 + as_m as i64) - (acq_y * 12 + acq_m as i64);
    elapsed.max(0)
}

fn cap_at_cost(value: Decimal, cost: Decimal) -> Decimal {
    if value > cost {
        cost
    } else {
        value
    }
}

/// Parse YYYY-MM-DD into (year: i64, month: u32). Returns (0, 1) on parse failure.
fn parse_ym(date: &str) -> (i64, u32) {
    let parts: Vec<&str> = date.splitn(3, '-').collect();
    if parts.len() >= 2 {
        let y = parts[0].parse::<i64>().unwrap_or(0);
        let m = parts[1].parse::<u32>().unwrap_or(1);
        (y, m)
    } else {
        (0, 1)
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::Executor;

    fn sample_asset(cost: &str, life: i64, acquired: &str) -> FixedAsset {
        FixedAsset {
            id: "a1".into(),
            company_id: "co-1".into(),
            asset_code: "MF-001".into(),
            account_id: "213".into(),
            description: "Laptop test".into(),
            valuation_class: "Corporala".into(),
            supplier_id: "0".into(),
            supplier_name: "".into(),
            date_of_acquisition: acquired.into(),
            start_up_date: acquired.into(),
            acquisition_cost: cost.into(),
            life_months: life,
            depreciation_method: "liniara".into(),
            depreciation_pct: "0.00".into(),
            disposal_date: None,
            active: true,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn depreciation_basic_monthly() {
        // cost=1200, life=12m → monthly=100
        let asset = sample_asset("1200.00", 12, "2025-01-01");
        let calc = compute_depreciation(&asset, "2025-01-01", "2025-01-31");
        // At period start (2025-01-01) months elapsed from acquisition = 0
        // At period end (2025-01-31) months elapsed = 0 (same month)
        assert_eq!(calc.monthly, Decimal::from_str("100.00").unwrap());
        assert_eq!(calc.book_value_begin, Decimal::from_str("1200.00").unwrap());
    }

    #[test]
    fn depreciation_after_one_year() {
        // cost=1200, life=12m, acquired 2024-01-01, period 2025-01-01..2025-01-31
        // months_elapsed at begin = 12, at end = 12 → fully depreciated
        let asset = sample_asset("1200.00", 12, "2024-01-01");
        let calc = compute_depreciation(&asset, "2025-01-01", "2025-01-31");
        assert_eq!(calc.accumulated_end, Decimal::from_str("1200.00").unwrap());
        assert_eq!(calc.book_value_end, Decimal::ZERO);
        assert_eq!(calc.for_period, Decimal::ZERO); // already fully depreciated
    }

    #[test]
    fn depreciation_cap_at_cost() {
        // 60-month asset acquired 2020-01-01, period 2026-01-01..2026-12-31 → beyond life
        let asset = sample_asset("6000.00", 60, "2020-01-01");
        let calc = compute_depreciation(&asset, "2026-01-01", "2026-12-31");
        assert_eq!(calc.accumulated_end, Decimal::from_str("6000.00").unwrap());
        assert_eq!(calc.book_value_end, Decimal::ZERO);
    }

    #[test]
    fn depreciation_zero_life_months_returns_zero() {
        let asset = sample_asset("5000.00", 0, "2025-01-01");
        let calc = compute_depreciation(&asset, "2025-01-01", "2025-12-31");
        assert_eq!(calc.monthly, Decimal::ZERO);
        assert_eq!(calc.accumulated_end, Decimal::ZERO);
    }

    async fn setup_asset_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        pool.execute(sqlx::query(
            "CREATE TABLE companies (
                id TEXT PRIMARY KEY, cui TEXT, legal_name TEXT, trade_name TEXT,
                registry_number TEXT, vat_payer INTEGER, address TEXT, city TEXT,
                county TEXT, postal_code TEXT, country TEXT, email TEXT, phone TEXT,
                iban TEXT, bank_name TEXT, is_active INTEGER, spv_enabled INTEGER,
                invoice_series TEXT, last_invoice_number INTEGER, logo_path TEXT,
                created_at INTEGER, updated_at INTEGER
            )",
        ))
        .await
        .unwrap();

        pool.execute(sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country, \
                                   vat_payer, invoice_series, last_invoice_number, \
                                   is_active, spv_enabled, created_at, updated_at) \
             VALUES ('co-1','RO1234','Test SRL','Str 1','Buc','B','RO',1,'F',0,1,0,0,0)",
        ))
        .await
        .unwrap();

        pool.execute(sqlx::query(
            "CREATE TABLE fixed_assets (
                id TEXT NOT NULL PRIMARY KEY,
                company_id TEXT NOT NULL,
                asset_code TEXT NOT NULL,
                account_id TEXT NOT NULL DEFAULT '213',
                description TEXT NOT NULL,
                valuation_class TEXT NOT NULL DEFAULT 'Corporala',
                supplier_id TEXT NOT NULL DEFAULT '0',
                supplier_name TEXT NOT NULL DEFAULT '',
                date_of_acquisition TEXT NOT NULL,
                start_up_date TEXT NOT NULL,
                acquisition_cost TEXT NOT NULL DEFAULT '0.00',
                life_months INTEGER NOT NULL DEFAULT 60,
                depreciation_method TEXT NOT NULL DEFAULT 'liniara',
                depreciation_pct TEXT NOT NULL DEFAULT '0.00',
                disposal_date TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0,
                UNIQUE(company_id, asset_code)
            )",
        ))
        .await
        .unwrap();

        pool.execute(sqlx::query(
            "CREATE TABLE asset_transactions (
                id TEXT NOT NULL PRIMARY KEY,
                company_id TEXT NOT NULL,
                asset_id TEXT NOT NULL,
                transaction_code TEXT NOT NULL,
                transaction_type TEXT NOT NULL DEFAULT '10',
                transaction_date TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                gl_transaction_id TEXT,
                acq_prod_cost TEXT NOT NULL DEFAULT '0.00',
                book_value TEXT NOT NULL DEFAULT '0.00',
                amount TEXT NOT NULL DEFAULT '0.00',
                created_at INTEGER NOT NULL DEFAULT 0
            )",
        ))
        .await
        .unwrap();

        pool
    }

    fn sample_input() -> FixedAssetInput {
        FixedAssetInput {
            asset_code: "MF-001".into(),
            account_id: Some("213".into()),
            description: "Laptop Dell".into(),
            valuation_class: Some("Corporala".into()),
            supplier_id: Some("0".into()),
            supplier_name: Some("Dell SRL".into()),
            date_of_acquisition: "2025-01-15".into(),
            start_up_date: Some("2025-01-15".into()),
            acquisition_cost: "3000.00".into(),
            life_months: Some(36),
            depreciation_method: Some("liniara".into()),
            depreciation_pct: None,
            disposal_date: None,
            active: Some(true),
        }
    }

    #[tokio::test]
    async fn create_and_get_round_trip() {
        let pool = setup_asset_pool().await;
        let asset = create(&pool, "co-1", sample_input()).await.unwrap();
        assert_eq!(asset.asset_code, "MF-001");
        assert_eq!(asset.life_months, 36);

        let fetched = get(&pool, &asset.id, "co-1").await.unwrap();
        assert_eq!(fetched.id, asset.id);
    }

    #[tokio::test]
    async fn duplicate_code_rejected() {
        let pool = setup_asset_pool().await;
        create(&pool, "co-1", sample_input()).await.unwrap();
        let err = create(&pool, "co-1", sample_input()).await.unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn cross_company_returns_not_found() {
        let pool = setup_asset_pool().await;
        let asset = create(&pool, "co-1", sample_input()).await.unwrap();
        let err = get(&pool, &asset.id, "co-2").await.unwrap_err();
        assert!(matches!(err, AppError::NotFound));
    }

    #[tokio::test]
    async fn delete_removes_asset() {
        let pool = setup_asset_pool().await;
        let asset = create(&pool, "co-1", sample_input()).await.unwrap();
        delete(&pool, &asset.id, "co-1").await.unwrap();
        let err = get(&pool, &asset.id, "co-1").await.unwrap_err();
        assert!(matches!(err, AppError::NotFound));
    }
}
