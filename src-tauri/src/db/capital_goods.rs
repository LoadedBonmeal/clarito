//! Registrul bunurilor de capital + ajustarea TVA (Cod fiscal art. 305).
//!
//! A capital good carries a multi-year VAT-adjustment period: 5 years (movables / services on
//! immovables) or 20 years (acquisition/construction of immovables). When the deduction right
//! changes during that period (use-change toward/away from taxable operations — art. 305(4)),
//! 1/5 (resp. 1/20) of the initially-deducted VAT is adjusted for each affected year:
//!
//!   adjustment(year) = (vat_deducted / N) × (new_deduction_pct − initial_deduction_pct) / 100
//!
//! `new_pct < initial_pct` → negative (clawback: repay part of the deducted VAT);
//! `new_pct > initial_pct` → positive (additional deductible VAT).
//!
//! GL (idempotent per adjustment id, `source_type='CAPGOOD_ADJ'`):
//!   clawback (amount < 0): D 635 / C 4426   — deducted VAT becomes a cost
//!   positive (amount > 0): D 4426 / C 758    — additional deductible VAT, recognized as income
//! The signed amount is also reported on the D300 deductible-adjustment row (art. 305 + OPANAF 174/2026).

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::gl::post_register_lines;
use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CapitalGood {
    pub id: String,
    pub company_id: String,
    pub asset_id: Option<String>,
    pub description: String,
    pub kind: String, // 'movable' | 'immovable'
    pub acquisition_date: String,
    pub base_value: String,
    pub vat_deducted: String,
    pub adjustment_years: i64,
    pub initial_deduction_pct: f64,
    pub status: String,
    pub disposed_date: Option<String>,
    pub notes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CapitalGoodAdjustment {
    pub id: String,
    pub company_id: String,
    pub capital_good_id: String,
    pub year: i64,
    pub new_deduction_pct: f64,
    pub adjustment_amount: String,
    pub period: String,
    pub posted: bool,
    pub notes: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCapitalGoodInput {
    pub company_id: String,
    pub asset_id: Option<String>,
    pub description: String,
    pub kind: String,
    pub acquisition_date: String,
    pub base_value: String,
    pub vat_deducted: String,
    #[serde(default = "default_pct")]
    pub initial_deduction_pct: f64,
    pub notes: Option<String>,
}

fn default_pct() -> f64 {
    100.0
}

fn is_ymd(s: &str) -> bool {
    s.len() == 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-'
}

fn is_ym(s: &str) -> bool {
    s.len() == 7
        && s.as_bytes()[4] == b'-'
        && s[..4].chars().all(|c| c.is_ascii_digit())
        && s[5..].chars().all(|c| c.is_ascii_digit())
}

fn period_end(ym: &str) -> String {
    use chrono::NaiveDate;
    let y: i32 = ym[..4].parse().unwrap_or(2026);
    let m: u32 = ym[5..].parse().unwrap_or(1);
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| format!("{ym}-28"))
}

fn pct_ok(p: f64) -> bool {
    (0.0..=100.0).contains(&p)
}

/// art. 305 annual adjustment for one use-change year, signed (negative = clawback).
/// `vat_deducted` is the VAT subject to adjustment; `n` the 5/20-year period.
pub fn compute_adjustment(
    vat_deducted: Decimal,
    n: i64,
    initial_pct: f64,
    new_pct: f64,
) -> Decimal {
    if n <= 0 {
        return Decimal::ZERO;
    }
    let delta = Decimal::from_f64_retain(new_pct - initial_pct).unwrap_or(Decimal::ZERO);
    (vat_deducted / Decimal::from(n) * delta / Decimal::from(100))
        .round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

fn validate(input: &CreateCapitalGoodInput) -> AppResult<(Decimal, Decimal, i64)> {
    if input.description.trim().is_empty() {
        return Err(AppError::Validation("Descrierea este obligatorie.".into()));
    }
    let n = match input.kind.as_str() {
        "movable" => 5,
        "immovable" => 20,
        _ => {
            return Err(AppError::Validation(
                "Tipul trebuie 'movable' (5 ani) sau 'immovable' (20 ani).".into(),
            ))
        }
    };
    if !is_ymd(&input.acquisition_date) {
        return Err(AppError::Validation(
            "Data achiziției trebuie în format AAAA-LL-ZZ.".into(),
        ));
    }
    if !pct_ok(input.initial_deduction_pct) {
        return Err(AppError::Validation(
            "Procentul inițial de deducere trebuie între 0 și 100.".into(),
        ));
    }
    let base = Decimal::from_str(input.base_value.trim())
        .map_err(|_| AppError::Validation("Valoarea de bază este invalidă.".into()))?;
    let vat = Decimal::from_str(input.vat_deducted.trim())
        .map_err(|_| AppError::Validation("TVA dedusă este invalidă.".into()))?;
    if vat < Decimal::ZERO || base < Decimal::ZERO {
        return Err(AppError::Validation("Valorile nu pot fi negative.".into()));
    }
    Ok((base, vat, n))
}

pub async fn create(pool: &SqlitePool, input: CreateCapitalGoodInput) -> AppResult<CapitalGood> {
    let (base, vat, n) = validate(&input)?;
    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO capital_goods \
         (id, company_id, asset_id, description, kind, acquisition_date, base_value, vat_deducted, \
          adjustment_years, initial_deduction_pct, status, notes, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'active',?11,?12,?12)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(input.asset_id.as_deref().map(str::trim))
    .bind(input.description.trim())
    .bind(&input.kind)
    .bind(&input.acquisition_date)
    .bind(format!("{base:.2}"))
    .bind(format!("{vat:.2}"))
    .bind(n)
    .bind(input.initial_deduction_pct)
    .bind(input.notes.as_deref().map(str::trim))
    .bind(now)
    .execute(pool)
    .await?;
    fetch(pool, &id, &input.company_id).await
}

pub async fn fetch(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<CapitalGood> {
    sqlx::query_as::<_, CapitalGood>("SELECT * FROM capital_goods WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<CapitalGood>> {
    Ok(sqlx::query_as::<_, CapitalGood>(
        "SELECT * FROM capital_goods WHERE company_id=?1 ORDER BY acquisition_date DESC, created_at DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

pub async fn list_adjustments(
    pool: &SqlitePool,
    capital_good_id: &str,
    company_id: &str,
) -> AppResult<Vec<CapitalGoodAdjustment>> {
    Ok(sqlx::query_as::<_, CapitalGoodAdjustment>(
        "SELECT * FROM capital_good_adjustments WHERE capital_good_id=?1 AND company_id=?2 ORDER BY year",
    )
    .bind(capital_good_id)
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordAdjustmentInput {
    pub company_id: String,
    pub capital_good_id: String,
    pub year: i64,
    pub new_deduction_pct: f64,
    pub period: String, // 'YYYY-MM'
    pub notes: Option<String>,
}

/// Compute + record one use-change year's adjustment, post the GL, and mark it posted.
pub async fn record_adjustment(
    pool: &SqlitePool,
    input: RecordAdjustmentInput,
) -> AppResult<CapitalGoodAdjustment> {
    let g = fetch(pool, &input.capital_good_id, &input.company_id).await?;
    if !pct_ok(input.new_deduction_pct) {
        return Err(AppError::Validation(
            "Procentul de deducere trebuie între 0 și 100.".into(),
        ));
    }
    if input.year < 1 || input.year > g.adjustment_years {
        return Err(AppError::Validation(format!(
            "Anul ajustării trebuie între 1 și {} (perioada de ajustare).",
            g.adjustment_years
        )));
    }
    if !is_ym(&input.period) {
        return Err(AppError::Validation(
            "Perioada trebuie în format AAAA-LL.".into(),
        ));
    }
    let vat = Decimal::from_str(&g.vat_deducted).unwrap_or(Decimal::ZERO);
    let amount = compute_adjustment(
        vat,
        g.adjustment_years,
        g.initial_deduction_pct,
        input.new_deduction_pct,
    );

    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO capital_good_adjustments \
         (id, company_id, capital_good_id, year, new_deduction_pct, adjustment_amount, period, posted, notes, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,0,?8,?9) \
         ON CONFLICT(company_id, capital_good_id, year) DO UPDATE SET \
           new_deduction_pct=excluded.new_deduction_pct, adjustment_amount=excluded.adjustment_amount, \
           period=excluded.period, notes=excluded.notes",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.capital_good_id)
    .bind(input.year)
    .bind(input.new_deduction_pct)
    .bind(format!("{amount:.2}"))
    .bind(&input.period)
    .bind(input.notes.as_deref().map(str::trim))
    .bind(now)
    .execute(pool)
    .await?;

    // Resolve the (possibly pre-existing) row id for this (good, year).
    let row: CapitalGoodAdjustment = sqlx::query_as(
        "SELECT * FROM capital_good_adjustments WHERE company_id=?1 AND capital_good_id=?2 AND year=?3",
    )
    .bind(&input.company_id)
    .bind(&input.capital_good_id)
    .bind(input.year)
    .fetch_one(pool)
    .await?;

    // Post (or re-post) the GL. post_register_lines is idempotent per (source_type, source_id).
    let lines: Vec<(String, String, Decimal)> = if amount < Decimal::ZERO {
        vec![("635".into(), "4426".into(), amount.abs())] // clawback
    } else if amount > Decimal::ZERO {
        vec![("4426".into(), "758".into(), amount)] // positive adjustment
    } else {
        vec![]
    };
    if !lines.is_empty() {
        post_register_lines(
            pool,
            &input.company_id,
            "DIVERSE",
            "CAPGOOD_ADJ",
            &row.id,
            &period_end(&input.period),
            &format!(
                "Ajustare TVA bun de capital (an {}/{}): {}",
                input.year, g.adjustment_years, g.description
            ),
            lines,
        )
        .await?;
        sqlx::query("UPDATE capital_good_adjustments SET posted=1 WHERE id=?1")
            .bind(&row.id)
            .execute(pool)
            .await?;
    }

    sqlx::query_as::<_, CapitalGoodAdjustment>("SELECT * FROM capital_good_adjustments WHERE id=?1")
        .bind(&row.id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

/// Signed Σ of the capital-goods VAT adjustments recorded in a reporting period (YYYY-MM), in lei,
/// for the D300 deductible-adjustment row. + = additional deduction, − = clawback.
pub async fn period_adjustment_lei(
    pool: &SqlitePool,
    company_id: &str,
    period: &str,
) -> AppResult<i64> {
    period_adjustment_lei_range(pool, company_id, period, period).await
}

/// Like [`period_adjustment_lei`] but over an inclusive YYYY-MM range — the D300 reporting period can
/// span several months (quarterly filers). `period` is YYYY-MM, lexically comparable.
pub async fn period_adjustment_lei_range(
    pool: &SqlitePool,
    company_id: &str,
    from_ym: &str,
    to_ym: &str,
) -> AppResult<i64> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT adjustment_amount FROM capital_good_adjustments \
         WHERE company_id=?1 AND period >= ?2 AND period <= ?3",
    )
    .bind(company_id)
    .bind(from_ym)
    .bind(to_ym)
    .fetch_all(pool)
    .await?;
    let sum: Decimal = rows
        .iter()
        .filter_map(|(s,)| Decimal::from_str(s).ok())
        .sum();
    Ok(sum
        .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        .to_i64()
        .unwrap_or(0))
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let res = sqlx::query("DELETE FROM capital_goods WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    // Clean up GL journals for this good's adjustments (CASCADE removes the ledger rows themselves).
    sqlx::query(
        "DELETE FROM gl_journal WHERE company_id=?1 AND source_type='CAPGOOD_ADJ' AND source_id IN \
         (SELECT id FROM capital_good_adjustments WHERE capital_good_id=?2)",
    )
    .bind(company_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co','RO1','T','S','C','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn input() -> CreateCapitalGoodInput {
        CreateCapitalGoodInput {
            company_id: "co".into(),
            asset_id: None,
            description: "Hală producție".into(),
            kind: "immovable".into(),
            acquisition_date: "2026-03-10".into(),
            base_value: "1000000.00".into(),
            vat_deducted: "210000.00".into(),
            initial_deduction_pct: 100.0,
            notes: None,
        }
    }

    #[test]
    fn formula_clawback_and_positive() {
        let vat = Decimal::from_str("100000.00").unwrap();
        // movable, N=5, deducted 100% then used 0% for taxable → full clawback of 1/5 = -20000
        let a = compute_adjustment(vat, 5, 100.0, 0.0);
        assert_eq!(a, Decimal::from_str("-20000.00").unwrap());
        // immovable, N=20, 0%→100% → +1/20 = +5000
        let b = compute_adjustment(vat, 20, 0.0, 100.0);
        assert_eq!(b, Decimal::from_str("5000.00").unwrap());
        // partial: N=5, 100%→40% (−60pp) → (100000/5)*(-60/100) = -12000
        let c = compute_adjustment(vat, 5, 100.0, 40.0);
        assert_eq!(c, Decimal::from_str("-12000.00").unwrap());
    }

    #[tokio::test]
    async fn create_and_immovable_is_20yr() {
        let pool = pool().await;
        let g = create(&pool, input()).await.unwrap();
        assert_eq!(g.adjustment_years, 20);
        assert_eq!(g.status, "active");
        assert_eq!(list(&pool, "co").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn record_clawback_posts_635_to_4426_and_feeds_d300() {
        let pool = pool().await;
        let g = create(&pool, input()).await.unwrap(); // immovable N=20, vat 210000, init 100%
        let adj = record_adjustment(
            &pool,
            RecordAdjustmentInput {
                company_id: "co".into(),
                capital_good_id: g.id.clone(),
                year: 3,
                new_deduction_pct: 0.0, // used for exempt → clawback 1/20 = -10500
                period: "2028-12".into(),
                notes: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(adj.adjustment_amount, "-10500.00");
        assert!(adj.posted);
        // GL: D 635 / C 4426 for 10500, balanced
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT e.account_code, e.debit, e.credit FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk WHERE j.source_type='CAPGOOD_ADJ'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let dr: Decimal = rows.iter().map(|r| Decimal::from_str(&r.1).unwrap()).sum();
        let cr: Decimal = rows.iter().map(|r| Decimal::from_str(&r.2).unwrap()).sum();
        assert_eq!(dr, cr);
        assert!(rows.iter().any(|r| r.0 == "635" && r.1 == "10500.00"));
        assert!(rows.iter().any(|r| r.0 == "4426" && r.2 == "10500.00"));
        // D300 feed: signed lei for the period
        let d300 = period_adjustment_lei(&pool, "co", "2028-12").await.unwrap();
        assert_eq!(d300, -10500);
    }

    #[tokio::test]
    async fn record_is_idempotent_per_year() {
        let pool = pool().await;
        let g = create(&pool, input()).await.unwrap();
        let mk = |pct: f64| RecordAdjustmentInput {
            company_id: "co".into(),
            capital_good_id: g.id.clone(),
            year: 5,
            new_deduction_pct: pct,
            period: "2030-12".into(),
            notes: None,
        };
        record_adjustment(&pool, mk(0.0)).await.unwrap();
        record_adjustment(&pool, mk(50.0)).await.unwrap(); // re-record same year → overwrite
        let adjs = list_adjustments(&pool, &g.id, "co").await.unwrap();
        assert_eq!(adjs.len(), 1); // one row per year
        assert_eq!(adjs[0].adjustment_amount, "-5250.00"); // 210000/20 * (50-100)/100
                                                           // GL not double-posted
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM gl_journal WHERE source_type='CAPGOOD_ADJ'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn period_range_sum_covers_quarter() {
        let pool = pool().await;
        let g = create(&pool, input()).await.unwrap(); // immovable N=20, vat 210000
                                                       // two clawbacks in different months of Q2
        for (yr, per) in [(3, "2028-04"), (4, "2028-06")] {
            record_adjustment(
                &pool,
                RecordAdjustmentInput {
                    company_id: "co".into(),
                    capital_good_id: g.id.clone(),
                    year: yr,
                    new_deduction_pct: 0.0, // each = -10500
                    period: per.into(),
                    notes: None,
                },
            )
            .await
            .unwrap();
        }
        // single April month
        assert_eq!(
            period_adjustment_lei(&pool, "co", "2028-04").await.unwrap(),
            -10500
        );
        // Q2 range catches both
        assert_eq!(
            period_adjustment_lei_range(&pool, "co", "2028-04", "2028-06")
                .await
                .unwrap(),
            -21000
        );
        // a quarter with no adjustments → 0
        assert_eq!(
            period_adjustment_lei_range(&pool, "co", "2028-07", "2028-09")
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn cross_company_isolation() {
        let pool = pool().await;
        let g = create(&pool, input()).await.unwrap();
        assert!(matches!(
            delete(&pool, &g.id, "intrus").await,
            Err(AppError::NotFound)
        ));
        assert!(list(&pool, "intrus").await.unwrap().is_empty());
        assert!(delete(&pool, &g.id, "co").await.is_ok());
    }
}
