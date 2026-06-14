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
    // Durata de viață trebuie să fie ≥ 1 lună — amortizarea lunară împarte la life_months, iar un
    // 0 ar fi doar mascat de guard-ul din calcul (activ care nu se amortizează niciodată).
    if let Some(lm) = input.life_months {
        if lm < 1 {
            return Err(AppError::Validation(
                "Durata de amortizare trebuie să fie de cel puțin 1 lună.".into(),
            ));
        }
    }
    // EDGE-002 — date-quality guard: a malformed acquisition/start-up/disposal date would otherwise
    // silently make `parse_ym` compute depreciation from year 0. Reject at the input boundary.
    if !valid_ymd(&input.date_of_acquisition) {
        return Err(AppError::Validation(
            "Data achiziției invalidă — folosiți formatul AAAA-LL-ZZ.".into(),
        ));
    }
    for (label, opt) in [
        ("Data punerii în funcțiune", input.start_up_date.as_deref()),
        ("Data scoaterii din uz", input.disposal_date.as_deref()),
    ] {
        if let Some(d) = opt {
            if !d.is_empty() && !valid_ymd(d) {
                return Err(AppError::Validation(format!(
                    "{label} invalidă — folosiți formatul AAAA-LL-ZZ."
                )));
            }
        }
    }
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

    // Validate + normalize the cost at the boundary, mirroring update(). Without this, RO-typed
    // values rust_decimal can't parse ("5.000,00", "5000,50", "5 000") would bind raw, then
    // compute_depreciation's `from_str(...).unwrap_or(ZERO)` would silently yield ZERO cost → no
    // depreciation + a false SAF-T D406 acquisition value. Empty → "0" (asset with no cost yet).
    let acquisition_cost = {
        let raw = input.acquisition_cost.trim();
        if raw.is_empty() {
            "0".to_string()
        } else {
            let d = Decimal::from_str(raw)
                .map_err(|_| AppError::Validation("Cost invalid — folosiți 1234.56.".into()))?;
            if d.is_sign_negative() {
                return Err(AppError::Validation("Costul nu poate fi negativ.".into()));
            }
            d.to_string()
        }
    };

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
    .bind(&acquisition_cost)
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

    // Commercial rounding (MidpointAwayFromZero) — never bare round_dp for money.
    let monthly = crate::db::invoices::round2(cost / Decimal::from(asset.life_months));

    // Amortizarea începe din luna URMĂTOARE punerii în funcțiune (start_up_date / PIF) — art. 28
    // alin. (12) Cod fiscal + OMFP 1802/2014. n = months from PIF; the asset is depreciable in a
    // month iff 1 <= n <= life_months. accumulated_begin is "before this period"; accumulated_end is
    // "after end month". The final month (n == life_months) absorbs the rounding remainder → cost.
    let (pif_y, pif_m) = parse_ym(&asset.start_up_date);
    let pif = pif_y * 12 + pif_m as i64;
    let acc_after = |as_of: &str| -> Decimal {
        let (y, m) = parse_ym(as_of);
        let n = (y * 12 + m as i64) - pif; // depreciable-month index at this month
        if n < 1 {
            Decimal::ZERO
        } else if n >= asset.life_months {
            cost // last-month remainder folded in → exactly cost
        } else {
            Decimal::from(n) * monthly
        }
    };
    // accumulated at period start = accumulated through the month BEFORE `begin_date`.
    let (by, bm) = parse_ym(begin_date);
    let before_begin = format!(
        "{:04}-{:02}-01",
        if bm == 1 { by - 1 } else { by },
        if bm == 1 { 12 } else { bm - 1 }
    );
    let acc_begin = acc_after(&before_begin);
    let acc_end = acc_after(end_date);
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

/// Map a 21x asset account to its 281x amortization mirror (OMFP 1802 chart).
pub fn amort_account_for(asset_account: &str) -> &'static str {
    match asset_account {
        a if a.starts_with("212") => "2812",
        a if a.starts_with("213") => "2813",
        a if a.starts_with("214") => "2814",
        a if a.starts_with("205") => "2805",
        a if a.starts_with("208") => "2808",
        _ => "2813",
    }
}

/// Parse YYYY-MM-DD into (year: i64, month: u32). Returns (0, 1) on parse failure.
/// `true` for a well-formed ISO date `YYYY-MM-DD` (month 1-12, day 1-31). Asset dates feed the
/// depreciation month math ([`parse_ym`]); a malformed one would otherwise compute from year 0.
fn valid_ymd(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    if p.len() != 3 || p[0].len() != 4 || p[1].len() != 2 || p[2].len() != 2 {
        return false;
    }
    if !p.iter().all(|seg| seg.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let m = p[1].parse::<u32>().unwrap_or(0);
    let d = p[2].parse::<u32>().unwrap_or(0);
    (1..=12).contains(&m) && (1..=31).contains(&d)
}

/// `("YYYY-MM-DD")` → `(year, month)`. Asset dates are guarded by [`valid_ymd`] at create/update, so a
/// fallback here means a legacy/corrupt row — we WARN (not silently compute depreciation from year 0).
fn parse_ym(date: &str) -> (i64, u32) {
    let parts: Vec<&str> = date.splitn(3, '-').collect();
    if parts.len() >= 2 {
        if let (Ok(y), Ok(m)) = (parts[0].parse::<i64>(), parts[1].parse::<u32>()) {
            return (y, m);
        }
    }
    tracing::warn!(
        date,
        "parse_ym: dată invalidă pe un mijloc fix — folosesc (0,1); verificați datele activului"
    );
    (0, 1)
}

// ─── Update + monthly depreciation run + disposal ────────────────────────────

/// Partial update of a fixed asset (mirrors the payroll partial-update + money validation).
pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: FixedAssetInput,
) -> AppResult<FixedAsset> {
    let cur = get(pool, id, company_id).await?;
    // EDGE-002 — same date-quality guard as create (the UPDATE binds these dates directly).
    if !valid_ymd(&input.date_of_acquisition) {
        return Err(AppError::Validation(
            "Data achiziției invalidă — folosiți formatul AAAA-LL-ZZ.".into(),
        ));
    }
    for (label, opt) in [
        ("Data punerii în funcțiune", input.start_up_date.as_deref()),
        ("Data scoaterii din uz", input.disposal_date.as_deref()),
    ] {
        if let Some(d) = opt {
            if !d.is_empty() && !valid_ymd(d) {
                return Err(AppError::Validation(format!(
                    "{label} invalidă — folosiți formatul AAAA-LL-ZZ."
                )));
            }
        }
    }
    let cost = if input.acquisition_cost.trim().is_empty() {
        cur.acquisition_cost.clone()
    } else {
        let d = Decimal::from_str(input.acquisition_cost.trim())
            .map_err(|_| AppError::Validation("Cost invalid — folosiți 1234.56.".into()))?;
        if d.is_sign_negative() {
            return Err(AppError::Validation("Costul nu poate fi negativ.".into()));
        }
        d.to_string()
    };
    sqlx::query(
        "UPDATE fixed_assets SET asset_code=?3, account_id=?4, description=?5, \
         date_of_acquisition=?6, start_up_date=?7, acquisition_cost=?8, life_months=?9, \
         depreciation_method=?10, disposal_date=?11, active=?12, updated_at=?13 \
         WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .bind(&input.asset_code)
    .bind(input.account_id.as_deref().unwrap_or(&cur.account_id))
    .bind(&input.description)
    .bind(&input.date_of_acquisition)
    .bind(input.start_up_date.as_deref().unwrap_or(&cur.start_up_date))
    .bind(&cost)
    .bind(input.life_months.unwrap_or(cur.life_months))
    .bind(
        input
            .depreciation_method
            .as_deref()
            .unwrap_or(&cur.depreciation_method),
    )
    .bind(input.disposal_date.or(cur.disposal_date))
    .bind(input.active.unwrap_or(cur.active))
    .bind(now_unix())
    .execute(pool)
    .await?;
    get(pool, id, company_id).await
}

/// One asset's computed depreciation for the month.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDepreciationState {
    pub asset_id: String,
    pub asset_code: String,
    pub description: String,
    pub monthly_charge: String,
    pub accumulated: String,
    pub book_value: String,
    pub expense_acct: String,
    pub amort_acct: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepreciationRun {
    pub states: Vec<AssetDepreciationState>,
    pub total_amount: String,
    pub posted: bool,
    pub entry_date: String,
}

fn ym_of(date: &str) -> i64 {
    let (y, m) = parse_ym(date);
    y * 12 + m as i64
}

/// Compute + record the monthly straight-line depreciation for every active asset and post the
/// aggregate to the GL (D 6811 / C 281x). Idempotent per (company, month).
pub async fn run_depreciation(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<DepreciationRun> {
    let period_ym = ym_of(period_from);
    let period = &period_from[..7]; // YYYY-MM
    let assets = list(pool, company_id).await?;
    let mut states = Vec::new();
    let mut total = Decimal::ZERO;
    // Aggregate per (expense, amort) account pair for the GL note.
    let mut by_pair: std::collections::BTreeMap<(String, String), Decimal> =
        std::collections::BTreeMap::new();

    // Idempotent: clear this period's register rows first, then rebuild — so a re-run after a
    // disposal (or any change) leaves no stale rows and the register matches the re-posted GL note.
    sqlx::query("DELETE FROM asset_depreciation WHERE company_id=?1 AND period=?2")
        .bind(company_id)
        .bind(period)
        .execute(pool)
        .await?;

    // Depreciate every asset that is amortizable in THIS period — keyed on the disposal month, not
    // the `active` flag (a disposed asset has active=0 but must still appear in its pre-disposal
    // months when those months are re-run).
    for a in assets.iter().filter(|a| a.depreciation_method == "liniara") {
        // Skip assets disposed before this month.
        if let Some(dd) = &a.disposal_date {
            if ym_of(dd) < period_ym {
                continue;
            }
        }
        let calc = compute_depreciation(a, period_from, period_to);
        if calc.for_period.is_zero() {
            continue;
        }
        let amort = amort_account_for(&a.account_id).to_string();
        total += calc.for_period;
        *by_pair
            .entry(("6811".to_string(), amort.clone()))
            .or_default() += calc.for_period;

        // Idempotent UPSERT into the register.
        sqlx::query(
            "INSERT INTO asset_depreciation (id, company_id, asset_id, period, amount, accumulated, \
             book_value, expense_acct, amort_acct, created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,'6811',?8,?9) \
             ON CONFLICT(company_id, asset_id, period) DO UPDATE SET \
             amount=?5, accumulated=?6, book_value=?7, amort_acct=?8",
        )
        .bind(new_id())
        .bind(company_id)
        .bind(&a.id)
        .bind(period)
        .bind(format!("{:.2}", calc.for_period))
        .bind(format!("{:.2}", calc.accumulated_end))
        .bind(format!("{:.2}", calc.book_value_end))
        .bind(&amort)
        .bind(now_unix())
        .execute(pool)
        .await?;

        states.push(AssetDepreciationState {
            asset_id: a.id.clone(),
            asset_code: a.asset_code.clone(),
            description: a.description.clone(),
            monthly_charge: format!("{:.2}", calc.for_period),
            accumulated: format!("{:.2}", calc.accumulated_end),
            book_value: format!("{:.2}", calc.book_value_end),
            expense_acct: "6811".into(),
            amort_acct: amort,
        });
    }

    let lines: Vec<(String, String, Decimal)> = by_pair
        .into_iter()
        .map(|((exp, amort), amt)| (exp, amort, amt))
        .collect();
    let post =
        crate::db::gl::post_depreciation(pool, company_id, period_from, period_to, lines).await?;

    Ok(DepreciationRun {
        states,
        total_amount: format!("{:.2}", total),
        posted: post.posted,
        entry_date: post.entry_date,
    })
}

/// Dispose of an asset: de-recognize it from the GL (D 281x accumulated + D 6583 residual / C 21x
/// cost) using the accumulated already in the register, and mark it disposed.
pub async fn dispose(
    pool: &SqlitePool,
    company_id: &str,
    asset_id: &str,
    disposal_date: &str,
) -> AppResult<()> {
    let asset = get(pool, asset_id, company_id).await?;
    let cost = Decimal::from_str(asset.acquisition_cost.trim()).unwrap_or(Decimal::ZERO);
    // Accumulated = Σ register amounts through the disposal month (single source of truth so GL ties).
    // Sum the Decimal-as-TEXT amounts in Rust to avoid f64 precision loss.
    let disp_ym = &disposal_date[..7];
    let amounts: Vec<String> = sqlx::query_scalar::<_, String>(
        "SELECT amount FROM asset_depreciation \
         WHERE company_id=?1 AND asset_id=?2 AND period<=?3",
    )
    .bind(company_id)
    .bind(asset_id)
    .bind(disp_ym)
    .fetch_all(pool)
    .await?;
    let accumulated: Decimal = amounts
        .iter()
        .filter_map(|s| Decimal::from_str(s.trim()).ok())
        .sum();
    let accumulated = if accumulated > cost {
        cost
    } else {
        accumulated
    };

    crate::db::gl::post_asset_disposal(
        pool,
        company_id,
        asset_id,
        disposal_date,
        cost,
        accumulated,
        &asset.account_id,
        amort_account_for(&asset.account_id),
    )
    .await?;

    sqlx::query(
        "UPDATE fixed_assets SET disposal_date=?3, active=0, updated_at=?4 \
         WHERE id=?1 AND company_id=?2",
    )
    .bind(asset_id)
    .bind(company_id)
    .bind(disposal_date)
    .bind(now_unix())
    .execute(pool)
    .await?;
    Ok(())
}

/// List the recorded monthly depreciation register for a company (optionally a period).
#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AssetDepreciationRow {
    pub asset_id: String,
    pub period: String,
    pub amount: String,
    pub accumulated: String,
    pub book_value: String,
}

pub async fn list_depreciation(
    pool: &SqlitePool,
    company_id: &str,
    period: Option<String>,
) -> AppResult<Vec<AssetDepreciationRow>> {
    let rows =
        match period {
            Some(p) => sqlx::query_as::<_, AssetDepreciationRow>(
                "SELECT asset_id, period, amount, accumulated, book_value FROM asset_depreciation \
                 WHERE company_id=?1 AND period=?2 ORDER BY asset_id",
            )
            .bind(company_id)
            .bind(p)
            .fetch_all(pool)
            .await?,
            None => sqlx::query_as::<_, AssetDepreciationRow>(
                "SELECT asset_id, period, amount, accumulated, book_value FROM asset_depreciation \
                 WHERE company_id=?1 ORDER BY period DESC, asset_id",
            )
            .bind(company_id)
            .fetch_all(pool)
            .await?,
        };
    Ok(rows)
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
    fn depreciation_starts_month_after_pif() {
        // cost=1200, life=12m, PIF Jan 2025 → NO charge in Jan (PIF month); first charge in Feb.
        let asset = sample_asset("1200.00", 12, "2025-01-01");
        let jan = compute_depreciation(&asset, "2025-01-01", "2025-01-31");
        assert_eq!(jan.monthly, Decimal::from_str("100.00").unwrap());
        assert_eq!(jan.for_period, Decimal::ZERO); // PIF month: not depreciated
        let feb = compute_depreciation(&asset, "2025-02-01", "2025-02-28");
        assert_eq!(feb.for_period, Decimal::from_str("100.00").unwrap()); // first charge
    }

    #[test]
    fn depreciation_last_month_absorbs_remainder() {
        // cost=1000, life=3 → monthly=333.33; months 1,2 = 333.33; month 3 = 333.34; Σ=1000.00.
        let asset = sample_asset("1000.00", 3, "2025-01-01");
        assert_eq!(
            compute_depreciation(&asset, "2025-02-01", "2025-02-28").for_period,
            Decimal::from_str("333.33").unwrap()
        );
        let m3 = compute_depreciation(&asset, "2025-04-01", "2025-04-30"); // 3rd depreciable month
        assert_eq!(m3.for_period, Decimal::from_str("333.34").unwrap());
        assert_eq!(m3.accumulated_end, Decimal::from_str("1000.00").unwrap());
        assert_eq!(m3.book_value_end, Decimal::ZERO);
    }

    #[test]
    fn depreciation_after_one_year() {
        // cost=1200, life=12m, PIF 2024-01-01 → 12th (final) charge in Jan 2025.
        let asset = sample_asset("1200.00", 12, "2024-01-01");
        let calc = compute_depreciation(&asset, "2025-01-01", "2025-01-31");
        assert_eq!(calc.accumulated_end, Decimal::from_str("1200.00").unwrap());
        assert_eq!(calc.book_value_end, Decimal::ZERO);
        assert_eq!(calc.for_period, Decimal::from_str("100.00").unwrap()); // final month charge
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

    #[test]
    fn valid_ymd_accepts_iso_rejects_garbage() {
        assert!(valid_ymd("2025-01-15"));
        assert!(valid_ymd("2026-12-31"));
        assert!(!valid_ymd("")); // empty
        assert!(!valid_ymd("2025-1-5")); // not zero-padded
        assert!(!valid_ymd("15-01-2025")); // wrong order
        assert!(!valid_ymd("2025-13-01")); // month 13
        assert!(!valid_ymd("2025-00-10")); // month 0
        assert!(!valid_ymd("abcd-ef-gh")); // non-numeric
                                           // parse_ym never silently yields year 0 for a valid date.
        assert_eq!(parse_ym("2026-06-15"), (2026, 6));
    }

    #[tokio::test]
    async fn create_rejects_malformed_acquisition_date() {
        // EDGE-002: a garbage acquisition date must be rejected, not silently stored (→ year-0 deprec).
        let pool = setup_asset_pool().await;
        let mut bad = sample_input();
        bad.date_of_acquisition = "not-a-date".into();
        let err = create(&pool, "co-1", bad).await.unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
        // A malformed disposal date is likewise rejected.
        let mut bad2 = sample_input();
        bad2.asset_code = "MF-002".into();
        bad2.disposal_date = Some("2025-99-99".into());
        assert!(matches!(
            create(&pool, "co-1", bad2).await.unwrap_err(),
            AppError::Validation(_)
        ));
    }

    #[tokio::test]
    async fn create_rejects_unparseable_cost_and_normalizes_valid() {
        // MONEY-015/017: a RO-typed cost rust_decimal can't parse must be rejected, NOT bound raw and
        // silently read back as ZERO (→ no depreciation + a false SAF-T acquisition value). Mirrors
        // the create-side date guard (EDGE-002) and the existing update() cost validation.
        let pool = setup_asset_pool().await;
        for bad_cost in ["5.000,00", "5000,50", "abc", "5 000"] {
            let mut bad = sample_input();
            bad.asset_code = format!("MF-{bad_cost}");
            bad.acquisition_cost = bad_cost.into();
            assert!(
                matches!(
                    create(&pool, "co-1", bad).await.unwrap_err(),
                    AppError::Validation(_)
                ),
                "cost {bad_cost:?} must be rejected"
            );
        }
        // A negative cost is rejected too.
        let mut neg = sample_input();
        neg.asset_code = "MF-neg".into();
        neg.acquisition_cost = "-100".into();
        assert!(matches!(
            create(&pool, "co-1", neg).await.unwrap_err(),
            AppError::Validation(_)
        ));
        // A valid cost is stored normalized and round-trips to a non-zero Decimal.
        let mut ok = sample_input();
        ok.asset_code = "MF-ok".into();
        ok.acquisition_cost = "5000.50".into();
        let asset = create(&pool, "co-1", ok).await.unwrap();
        assert_eq!(
            Decimal::from_str(asset.acquisition_cost.trim()).unwrap(),
            Decimal::from_str("5000.50").unwrap()
        );
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

    // Full migrate-based pool (GL + register tables) for the posting tests.
    async fn migrate_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co-1','RO99','T SRL','S','C','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn depreciation_then_disposal_post_balanced_gl() {
        let pool = migrate_pool().await;
        // Cost 3.600, viață 36 luni → amortizare lunară 100; cont 213 → amortizare 2813.
        let asset = create(
            &pool,
            "co-1",
            FixedAssetInput {
                acquisition_cost: "3600.00".into(),
                life_months: Some(36),
                date_of_acquisition: "2026-01-10".into(),
                start_up_date: Some("2026-01-10".into()),
                ..sample_input()
            },
        )
        .await
        .unwrap();

        // One month of depreciation → D 6811 100 / C 2813 100.
        run_depreciation(&pool, "co-1", "2026-02-01", "2026-02-28")
            .await
            .unwrap();
        let tb = crate::db::gl::trial_balance(&pool, "co-1", "2026-02-01", "2026-02-28")
            .await
            .unwrap();
        let bal = |c: &str| {
            tb.rows
                .iter()
                .find(|r| r.account_code == c)
                .map(|r| (r.closing_debit.clone(), r.closing_credit.clone()))
        };
        assert_eq!(bal("6811"), Some(("100.00".into(), "0.00".into())));
        assert_eq!(bal("2813"), Some(("0.00".into(), "100.00".into())));
        assert!(tb.balanced);

        // Dispose at end of Feb: accumulated 100, valoare rămasă 3.500 → D 2813 100 + D 6583 3500 /
        // C 213 3600. Over the full period the GL stays balanced.
        dispose(&pool, "co-1", &asset.id, "2026-02-28")
            .await
            .unwrap();
        let tb2 = crate::db::gl::trial_balance(&pool, "co-1", "2026-01-01", "2026-12-31")
            .await
            .unwrap();
        assert!(tb2.balanced);
        let bal2 = |c: &str| {
            tb2.rows
                .iter()
                .find(|r| r.account_code == c)
                .map(|r| r.closing_debit.clone())
        };
        // 6583 (cheltuieli din cedarea activelor) carries the residual book value.
        assert_eq!(bal2("6583"), Some("3500.00".into()));
        // The asset is now inactive.
        let a = get(&pool, &asset.id, "co-1").await.unwrap();
        assert!(!a.active);
        assert_eq!(a.disposal_date.as_deref(), Some("2026-02-28"));
    }
}
