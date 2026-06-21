//! Mijloace fixe (Assets SAF-T MasterFiles section).
//!
//! Fiecare mijloc fix aparține unei companii (company_id).
//! Calculul amortizării: liniară (implicită), degresivă (AD), accelerată, super-accelerată (OUG
//! 8/2026). Valorile monetare sunt stocate ca TEXT (convenția Decimal-as-TEXT).
//!
//! # Metoda de amortizare (book vs fiscal)
//! `depreciation_method` = metoda contabilă (dictează nota 6811 = 281x).
//! `fiscal_method` = metoda fiscală opțională (diferă în general de cea contabilă; alimentează
//! Registrul de evidență fiscală și D101.rd.16). Dacă NULL, se consideră identică cu cea contabilă.
//!
//! # Noi metode adăugate (Cod Fiscal art.28, Legea 15/1994, HG 2139/2004; OUG 8/2026)
//! - `degresiva` — cotă degresivă (Cd = Cl × k) cu switch la liniară în primul an
//!   în care amortizarea liniară depășește cea degresivă.
//! - `accelerata` — 50% în primul an, restul liniar.
//! - `super_accelerata` — 65% în primul an (numai active NOI, subgrupa 2.1, PIF 2026).

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
    /// Metoda fiscală (diferă de cea contabilă → diferență temporară D101). NULL = identică.
    pub fiscal_method: Option<String>,
    /// 1 = activ NOU; 0 = second-hand. Condiție eligibilitate super-accelerată.
    pub is_new: bool,
    /// Subgrupa HG 2139/2004 (ex. "2.1"). Condiție eligibilitate super-accelerată.
    pub subgroup: Option<String>,
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
    pub fiscal_method: Option<String>,
    pub is_new: Option<bool>,
    pub subgroup: Option<String>,
}

// ─── Queries ───────────────────────────────────────────────────────────────

const ASSET_COLS: &str = "id, company_id, asset_code, account_id, description, valuation_class, \
     supplier_id, supplier_name, date_of_acquisition, start_up_date, \
     acquisition_cost, life_months, depreciation_method, depreciation_pct, \
     disposal_date, active, created_at, updated_at, \
     fiscal_method, is_new, subgroup";

/// List all active fixed assets for a company.
pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<FixedAsset>> {
    let sql = format!(
        "SELECT {ASSET_COLS} FROM fixed_assets \
         WHERE company_id = ?1 ORDER BY asset_code ASC"
    );
    let items = sqlx::query_as::<_, FixedAsset>(&sql)
        .bind(company_id)
        .fetch_all(pool)
        .await?;
    Ok(items)
}

/// Fetch a single fixed asset by id; verifies company ownership.
pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<FixedAsset> {
    let sql = format!("SELECT {ASSET_COLS} FROM fixed_assets WHERE id = ?1");
    let asset = sqlx::query_as::<_, FixedAsset>(&sql)
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
    // Validate + normalize the depreciation method. None → "liniara".
    let method = input
        .depreciation_method
        .as_deref()
        .unwrap_or("liniara")
        .trim();
    validate_method(method)?;
    let fiscal_method_str = input.fiscal_method.as_deref().unwrap_or("").trim();
    if !fiscal_method_str.is_empty() {
        validate_method(fiscal_method_str)?;
    }
    // Eligibility: super_accelerata requires a new asset in service in 2026, subgroup 2.1.
    if method == "super_accelerata" {
        validate_super_accelerata(&input)?;
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
    // Normalize empty strings → None so the fallback (unwrap_or) fires correctly.
    // Some("") is not the same as None, so we filter it out explicitly (EDGE-002).
    let start_up = input
        .start_up_date
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&input.date_of_acquisition)
        .to_string();
    // Store None rather than Some("") for disposal_date so downstream slicing is safe.
    let disposal_date = input.disposal_date.filter(|s| !s.trim().is_empty());

    let fiscal_method_stored = {
        let s = input
            .fiscal_method
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    };

    sqlx::query(
        "INSERT INTO fixed_assets (
            id, company_id, asset_code, account_id, description, valuation_class,
            supplier_id, supplier_name, date_of_acquisition, start_up_date,
            acquisition_cost, life_months, depreciation_method, depreciation_pct,
            disposal_date, active, created_at, updated_at,
            fiscal_method, is_new, subgroup
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?17,?18,?19,?20)",
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
    .bind(&disposal_date)
    .bind(input.active.unwrap_or(true) as i32)
    .bind(now)
    .bind(&fiscal_method_stored)
    .bind(input.is_new.unwrap_or(true) as i32)
    .bind(&input.subgroup)
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
    // Scoaterea din funcțiune: NU se mai amortizează în luna scoaterii sau după (art. 28 Cod fiscal /
    // OMFP 1802/2014 — simetric cu „începe în luna următoare PIF"). Ultima lună amortizată = luna
    // DINAINTEA scoaterii ⇒ indexul-cap = (luna scoaterii) − PIF − 1. Valoarea rămasă neamortizată se
    // descarcă prin scoatere (câștig/pierdere), nu prin amortizare.
    let disp_last_index: Option<i64> = asset.disposal_date.as_deref().map(|dd| {
        let (dy, dm) = parse_ym(dd);
        (dy * 12 + dm as i64) - pif - 1
    });
    let acc_after = |as_of: &str| -> Decimal {
        let (y, m) = parse_ym(as_of);
        let mut n = (y * 12 + m as i64) - pif; // depreciable-month index at this month
        if let Some(last) = disp_last_index {
            n = n.min(last); // nu se acumulează în/​după luna scoaterii din funcțiune
        }
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

/// `true` for a well-formed, calendar-valid ISO date `YYYY-MM-DD`. Asset dates feed the
/// depreciation month math ([`parse_ym`]); a malformed one would otherwise compute from year 0.
/// Uses `chrono::NaiveDate` so impossible days like 2025-02-31 are also rejected.
fn valid_ymd(s: &str) -> bool {
    // Strict ISO `YYYY-MM-DD`: chrono's `%Y-%m-%d` accepts non-zero-padded ("2025-1-5"), so also
    // require length 10 to enforce the AAAA-LL-ZZ promise; chrono rejects impossible calendar days
    // (e.g. 2025-02-31) and months (13).
    s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

/// Parse YYYY-MM-DD into (year: i64, month: u32). Returns (0, 1) on parse failure.
/// Asset dates are guarded by [`valid_ymd`] at create/update, so a
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

// ─── Depreciation schedule helpers ───────────────────────────────────────────

/// Recognized depreciation method strings (book or fiscal).
pub fn is_recognized_method(m: &str) -> bool {
    matches!(
        m,
        "liniara" | "degresiva" | "accelerata" | "super_accelerata"
    )
}

fn validate_method(m: &str) -> AppResult<()> {
    if !is_recognized_method(m) {
        return Err(AppError::Validation(format!(
            "Metodă de amortizare nesuportată: '{m}'. \
             Valori acceptate: liniara, degresiva, accelerata, super_accelerata."
        )));
    }
    Ok(())
}

/// Enforce super-accelerată eligibility constraints (OUG 8/2026):
/// - asset must be new (is_new = true)
/// - subgroup must be "2.1"
/// - PIF year must be 2026
///
/// Returns `Err(Validation)` if conditions are not met.
fn validate_super_accelerata(input: &FixedAssetInput) -> AppResult<()> {
    if !input.is_new.unwrap_or(true) {
        return Err(AppError::Validation(
            "Super-accelerată (OUG 8/2026) se aplică doar activelor NOI (is_new = true).".into(),
        ));
    }
    if input.subgroup.as_deref().map(|s| s.trim()) != Some("2.1") {
        return Err(AppError::Validation(
            "Super-accelerată (OUG 8/2026) se aplică doar activelor din subgrupa 2.1 \
             (HG 2139/2004)."
                .into(),
        ));
    }
    let pif = input
        .start_up_date
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&input.date_of_acquisition);
    let (pif_year, _) = parse_ym(pif);
    if pif_year != 2026 {
        return Err(AppError::Validation(
            "Super-accelerată (OUG 8/2026) se aplică doar activelor puse în funcțiune în 2026."
                .into(),
        ));
    }
    Ok(())
}

// ─── Annual schedule builders ─────────────────────────────────────────────────
//
// Each fn returns a Vec of yearly amounts (Decimal), one entry per DNU year.
// Σ of all entries == VI exactly (last-year absorbs rounding residual).
//
// NOTE: "yearly" here means each full calendar year of the depreciation life,
// starting from the PIF year. Monthly amounts are derived by the run as
// year_amount / 12 for the first/last year (the on-month logic in run_depreciation
// handles sub-year starts exactly like the linear path does).

/// Degresivă (AD) per Cod Fiscal art. 28 alin. (7)–(9).
///
/// k factor: 2≤DNU≤5 → 1.5; 5<DNU≤10 → 2.0; DNU>10 → 2.5.
/// DNU<2: degresivă nu se aplică → error (fall back to liniară at call site if needed).
///
/// Switch-to-linear: first year where (remaining × Cd) ≤ (remaining / remaining_years).
pub fn degressive_schedule(vi: Decimal, dnu: i64) -> AppResult<Vec<Decimal>> {
    if dnu < 2 {
        return Err(AppError::Validation(
            "Metoda degresivă nu se aplică pentru DNU < 2 ani.".into(),
        ));
    }
    let r = crate::db::invoices::round2;
    let cl = r(Decimal::ONE / Decimal::from(dnu)); // linear rate
    let k = if dnu <= 5 {
        Decimal::new(15, 1) // 1.5
    } else if dnu <= 10 {
        Decimal::TWO // 2.0
    } else {
        Decimal::new(25, 1) // 2.5
    };
    let cd = r(cl * k); // degressive rate

    let mut schedule: Vec<Decimal> = Vec::with_capacity(dnu as usize);
    let mut remaining = vi;

    for year in 1..=(dnu as usize) {
        let remaining_years = Decimal::from((dnu as usize - year + 1) as i64);
        let degr = r(remaining * cd);
        let lin = r(remaining / remaining_years);
        // Switch in first year where linear ≥ degressive.
        if lin >= degr {
            // From this year on, spread remaining equally.
            // We re-enter the same logic: n years remain, divide equally.
            // We compute all remaining years right here.
            let n_remaining = (dnu as usize) - year + 1;
            let per_year = r(remaining / Decimal::from(n_remaining as i64));
            for i in 0..n_remaining {
                if i == n_remaining - 1 {
                    // Last year absorbs residual.
                    schedule.push(remaining - per_year * Decimal::from((n_remaining - 1) as i64));
                } else {
                    schedule.push(per_year);
                }
            }
            remaining = Decimal::ZERO;
            break;
        } else {
            schedule.push(degr);
            remaining -= degr;
        }
    }
    // Safety: if floating calc leaves tiny non-zero remainder, fold into last entry.
    if !remaining.is_zero() && !schedule.is_empty() {
        let last = schedule.len() - 1;
        schedule[last] += remaining;
    }
    Ok(schedule)
}

/// Accelerată per Cod Fiscal art. 28 alin. (8)(a): 50% în Y1, restul liniar Y2..Yn.
pub fn accelerated_schedule(vi: Decimal, dnu: i64) -> AppResult<Vec<Decimal>> {
    if dnu < 1 {
        return Err(AppError::Validation(
            "DNU trebuie să fie ≥ 1 an pentru amortizarea accelerată.".into(),
        ));
    }
    let r = crate::db::invoices::round2;
    let y1 = r(vi * Decimal::new(5, 1)); // 50%
    let remaining = vi - y1;
    let mut schedule = vec![y1];
    if dnu == 1 {
        // Life = 1 year: everything in year 1 (remaining = 0).
        if !remaining.is_zero() {
            schedule[0] += remaining;
        }
    } else {
        let n_remain = dnu - 1;
        let per_year = r(remaining / Decimal::from(n_remain));
        for i in 1..=n_remain {
            if i == n_remain {
                schedule.push(remaining - per_year * Decimal::from(n_remain - 1));
            } else {
                schedule.push(per_year);
            }
        }
    }
    Ok(schedule)
}

/// Super-accelerată per OUG 8/2026: 65% în Y1, restul liniar Y2..Yn.
/// `in_service_year` must be 2026 (enforced at create/update; here it's informational).
pub fn super_accelerated_schedule(vi: Decimal, dnu: i64) -> AppResult<Vec<Decimal>> {
    if dnu < 1 {
        return Err(AppError::Validation(
            "DNU trebuie să fie ≥ 1 an pentru amortizarea super-accelerată.".into(),
        ));
    }
    let r = crate::db::invoices::round2;
    let y1 = r(vi * Decimal::new(65, 2)); // 65%
    let remaining = vi - y1;
    let mut schedule = vec![y1];
    if dnu == 1 {
        if !remaining.is_zero() {
            schedule[0] += remaining;
        }
    } else {
        let n_remain = dnu - 1;
        let per_year = r(remaining / Decimal::from(n_remain));
        for i in 1..=n_remain {
            if i == n_remain {
                schedule.push(remaining - per_year * Decimal::from(n_remain - 1));
            } else {
                schedule.push(per_year);
            }
        }
    }
    Ok(schedule)
}

/// Straight-line (liniară) annual schedule — added for consistency with the other builders.
pub fn linear_schedule(vi: Decimal, dnu: i64) -> AppResult<Vec<Decimal>> {
    if dnu < 1 {
        return Err(AppError::Validation("DNU trebuie să fie ≥ 1 an.".into()));
    }
    let r = crate::db::invoices::round2;
    let per_year = r(vi / Decimal::from(dnu));
    let mut schedule: Vec<Decimal> = (0..dnu).map(|_| per_year).collect();
    // Absorb rounding residual in last year.
    let sum: Decimal = schedule.iter().copied().sum();
    let diff = vi - sum;
    if let Some(last) = schedule.last_mut() {
        *last += diff;
    }
    Ok(schedule)
}

// ─── Monthly dispatch ─────────────────────────────────────────────────────────
//
// These functions map from a FixedAsset + a period-month to the monthly charge.
// They replace the liniară-only `compute_depreciation` for the run loop, but
// `compute_depreciation` is kept for backward compatibility (external callers).

/// DNU in whole years (ceiling of life_months / 12).
fn dnu_from_months(life_months: i64) -> i64 {
    (life_months + 11) / 12
}

/// Cumulative depreciation accumulated through end-of-month of `as_of_date` (YYYY-MM-DD),
/// capped at cost. Dispatches by depreciation_method; falls back to liniară on unknown methods.
fn compute_accumulated(asset: &FixedAsset, as_of_date: &str) -> Decimal {
    let cost = Decimal::from_str(asset.acquisition_cost.trim()).unwrap_or(Decimal::ZERO);
    if cost <= Decimal::ZERO || asset.life_months <= 0 {
        return Decimal::ZERO;
    }
    let (pif_y, pif_m) = parse_ym(&asset.start_up_date);
    let pif = pif_y * 12 + pif_m as i64;
    let (as_y, as_m) = parse_ym(as_of_date);
    let as_of = as_y * 12 + as_m as i64;

    // Disposal cap: amortizarea se oprește ÎNAINTE de luna scoaterii din funcțiune.
    let as_of = if let Some(dd) = &asset.disposal_date {
        let (dy, dm) = parse_ym(dd);
        let disp = dy * 12 + dm as i64;
        as_of.min(disp - 1)
    } else {
        as_of
    };

    let n = as_of - pif; // depreciable months elapsed (1 = first month after PIF)
    if n < 1 {
        return Decimal::ZERO;
    }
    // Cap: once fully through the depreciation life, accumulated == cost regardless of method.
    // This guards non-12-multiple life_months (e.g. 18, 30, 42) where the schedule-based path
    // would otherwise return a partial value at month life_months and over-run past life.
    if n >= asset.life_months {
        return cost;
    }

    let dnu = dnu_from_months(asset.life_months);
    let n_years_elapsed = n / 12; // complete years elapsed (0-indexed from PIF)
    let n_months_in_year = n % 12; // additional months in the current year

    match asset.depreciation_method.as_str() {
        "degresiva" => {
            let schedule = match degressive_schedule(cost, dnu) {
                Ok(s) => s,
                Err(_) => return compute_accumulated_linear(cost, asset.life_months, n),
            };
            accumulated_from_schedule(&schedule, n_years_elapsed, n_months_in_year, cost)
        }
        "accelerata" => {
            let schedule = match accelerated_schedule(cost, dnu) {
                Ok(s) => s,
                Err(_) => return compute_accumulated_linear(cost, asset.life_months, n),
            };
            accumulated_from_schedule(&schedule, n_years_elapsed, n_months_in_year, cost)
        }
        "super_accelerata" => {
            let schedule = match super_accelerated_schedule(cost, dnu) {
                Ok(s) => s,
                Err(_) => return compute_accumulated_linear(cost, asset.life_months, n),
            };
            accumulated_from_schedule(&schedule, n_years_elapsed, n_months_in_year, cost)
        }
        _ => compute_accumulated_linear(cost, asset.life_months, n),
    }
}

/// Straight-line accumulated through month n (1-indexed depreciable-month offset).
fn compute_accumulated_linear(cost: Decimal, life_months: i64, n: i64) -> Decimal {
    let r = crate::db::invoices::round2;
    let monthly = r(cost / Decimal::from(life_months));
    if n >= life_months {
        cost
    } else {
        Decimal::from(n) * monthly
    }
}

/// Accumulated through a given year+month offset from a yearly schedule.
/// `n_years_elapsed` = how many COMPLETE years have passed (0 = still in year 1).
/// `n_months_in_year` = additional months into the CURRENT year (0 = none).
fn accumulated_from_schedule(
    schedule: &[Decimal],
    n_years_elapsed: i64,
    n_months_in_year: i64,
    cost: Decimal,
) -> Decimal {
    if schedule.is_empty() {
        return Decimal::ZERO;
    }
    let total_years = schedule.len() as i64;
    if n_years_elapsed >= total_years {
        return cost; // fully depreciated
    }
    let r = crate::db::invoices::round2;
    // Sum complete years.
    let mut acc: Decimal = schedule[..n_years_elapsed as usize].iter().copied().sum();
    // Add partial current year (n_months_in_year / 12 of that year's charge).
    if n_months_in_year > 0 {
        let cur_year_annual = schedule[n_years_elapsed as usize];
        acc += r(cur_year_annual * Decimal::from(n_months_in_year) / Decimal::from(12));
    }
    acc.min(cost)
}

/// Monthly depreciation charge for the month containing `period_date` (YYYY-MM-DD).
/// Returns ZERO if the asset has not started, is fully depreciated, or is disposed.
fn compute_period_charge(asset: &FixedAsset, period_date: &str) -> Decimal {
    // Month index at period_date.
    let (py, pm) = parse_ym(period_date);
    let period_abs = py * 12 + pm as i64;

    // Disposal: not charged in disposal month or after.
    if let Some(dd) = &asset.disposal_date {
        let (dy, dm) = parse_ym(dd);
        if dy * 12 + dm as i64 <= period_abs {
            return Decimal::ZERO;
        }
    }

    // Month BEFORE this period (for "beginning-of-month" accumulated).
    let prev = format!(
        "{:04}-{:02}-01",
        if pm == 1 { py - 1 } else { py },
        if pm == 1 { 12 } else { pm - 1 }
    );
    let acc_before = compute_accumulated(asset, &prev);
    let acc_after = compute_accumulated(asset, period_date);
    (acc_after - acc_before).max(Decimal::ZERO)
}

// ─── Fiscal schedule exposure ─────────────────────────────────────────────────

/// Per-asset fiscal amortization schedule (annual), used for D101.rd.16 computation.
/// Returns yearly amounts matching the fiscal_method (falls back to depreciation_method).
/// Also returns the book-vs-fiscal difference per year for temporary-difference reporting.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiscalScheduleRow {
    /// 1-based year index.
    pub year: usize,
    /// Fiscal amortization for this year (Decimal-as-TEXT).
    pub fiscal_amount: String,
    /// Book amortization for this year (Decimal-as-TEXT).
    pub book_amount: String,
    /// Temporary difference (fiscal − book). Positive = fiscal deducts more.
    pub temp_diff: String,
}

/// Compute the annual fiscal + book schedules for an asset, returning per-year rows.
///
/// # D101 wiring note
/// `fiscal_deductions` (rd.16) should include the EXCESS of fiscal amortization over book
/// amortization: Σ(fiscal_amount − book_amount) for the tax year. When fiscal == book, diff = 0.
/// The caller (D101 form) must aggregate this over all assets for the year.
pub fn compute_fiscal_schedule(asset: &FixedAsset) -> AppResult<Vec<FiscalScheduleRow>> {
    let cost = Decimal::from_str(asset.acquisition_cost.trim()).unwrap_or(Decimal::ZERO);
    if cost <= Decimal::ZERO || asset.life_months <= 0 {
        return Ok(vec![]);
    }
    let dnu = dnu_from_months(asset.life_months);
    let fiscal_m = asset
        .fiscal_method
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&asset.depreciation_method);

    let book_schedule = schedule_for_method(&asset.depreciation_method, cost, dnu)?;
    let fiscal_schedule = schedule_for_method(fiscal_m, cost, dnu)?;

    let n = book_schedule.len().max(fiscal_schedule.len());
    let rows = (0..n)
        .map(|i| {
            let book = book_schedule.get(i).copied().unwrap_or(Decimal::ZERO);
            let fiscal = fiscal_schedule.get(i).copied().unwrap_or(Decimal::ZERO);
            let diff = fiscal - book;
            FiscalScheduleRow {
                year: i + 1,
                fiscal_amount: format!("{:.2}", fiscal),
                book_amount: format!("{:.2}", book),
                temp_diff: format!("{:.2}", diff),
            }
        })
        .collect();
    Ok(rows)
}

fn schedule_for_method(method: &str, cost: Decimal, dnu: i64) -> AppResult<Vec<Decimal>> {
    match method {
        "degresiva" => degressive_schedule(cost, dnu),
        "accelerata" => accelerated_schedule(cost, dnu),
        "super_accelerata" => super_accelerated_schedule(cost, dnu),
        _ => linear_schedule(cost, dnu),
    }
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
    // Validate method (if provided).
    if let Some(m) = input.depreciation_method.as_deref() {
        validate_method(m.trim())?;
    }
    if let Some(fm) = input.fiscal_method.as_deref() {
        if !fm.trim().is_empty() {
            validate_method(fm.trim())?;
        }
    }
    let new_method = input
        .depreciation_method
        .as_deref()
        .unwrap_or(&cur.depreciation_method);
    if new_method == "super_accelerata" {
        // Only run eligibility validation when the method is being newly set or changed.
        // A partial update of an already-valid super_accelerata asset (e.g. changing description
        // while leaving depreciation_method as None in the payload) must not re-validate against
        // the input's potentially absent/non-2026 date fields.
        let method_changing = input
            .depreciation_method
            .as_deref()
            .map(|m| m != cur.depreciation_method)
            .unwrap_or(false);
        if method_changing || cur.depreciation_method != "super_accelerata" {
            validate_super_accelerata(&input)?;
        }
    }
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
    // Normalize empty strings → None/fallback so the EDGE-002 guard covers update() too.
    let start_up_update = input
        .start_up_date
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&cur.start_up_date)
        .to_string();
    let disposal_date_update = input
        .disposal_date
        .filter(|s| !s.trim().is_empty())
        .or(cur.disposal_date);

    let fiscal_method_update = match &input.fiscal_method {
        Some(fm) if !fm.trim().is_empty() => Some(fm.trim().to_string()),
        Some(_) => None, // empty string → clear it
        None => cur.fiscal_method.clone(),
    };
    let is_new_update = input.is_new.unwrap_or(cur.is_new);
    let subgroup_update = input.subgroup.as_deref().or(cur.subgroup.as_deref());

    sqlx::query(
        "UPDATE fixed_assets SET asset_code=?3, account_id=?4, description=?5, \
         date_of_acquisition=?6, start_up_date=?7, acquisition_cost=?8, life_months=?9, \
         depreciation_method=?10, disposal_date=?11, active=?12, updated_at=?13, \
         fiscal_method=?14, is_new=?15, subgroup=?16 \
         WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .bind(&input.asset_code)
    .bind(input.account_id.as_deref().unwrap_or(&cur.account_id))
    .bind(&input.description)
    .bind(&input.date_of_acquisition)
    .bind(&start_up_update)
    .bind(&cost)
    .bind(input.life_months.unwrap_or(cur.life_months))
    .bind(
        input
            .depreciation_method
            .as_deref()
            .unwrap_or(&cur.depreciation_method),
    )
    .bind(disposal_date_update)
    .bind(input.active.unwrap_or(cur.active))
    .bind(now_unix())
    .bind(&fiscal_method_update)
    .bind(is_new_update as i32)
    .bind(subgroup_update)
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
    if !valid_ymd(period_from) {
        return Err(AppError::Validation(
            "Data de început a perioadei este invalidă — folosiți AAAA-LL-ZZ.".into(),
        ));
    }
    if !valid_ymd(period_to) {
        return Err(AppError::Validation(
            "Data de sfârșit a perioadei este invalidă — folosiți AAAA-LL-ZZ.".into(),
        ));
    }
    let period_ym = ym_of(period_from);
    let period = period_from
        .get(..7)
        .ok_or_else(|| AppError::Validation("Dată invalidă — folosiți AAAA-LL-ZZ.".into()))?; // YYYY-MM
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
    // months when those months are re-run). All recognized methods are processed.
    for a in assets
        .iter()
        .filter(|a| is_recognized_method(&a.depreciation_method))
    {
        // Skip assets disposed before this month.
        if let Some(dd) = &a.disposal_date {
            if ym_of(dd) < period_ym {
                continue;
            }
        }
        let for_period = compute_period_charge(a, period_from);
        if for_period.is_zero() {
            continue;
        }
        // Re-compute the full accumulated & book-value state for the register.
        let cost = Decimal::from_str(a.acquisition_cost.trim()).unwrap_or(Decimal::ZERO);
        let accumulated = compute_accumulated(a, period_from);
        let book_value = (cost - accumulated).max(Decimal::ZERO);
        let amort = amort_account_for(&a.account_id).to_string();
        total += for_period;
        *by_pair
            .entry(("6811".to_string(), amort.clone()))
            .or_default() += for_period;

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
        .bind(format!("{:.2}", for_period))
        .bind(format!("{:.2}", accumulated))
        .bind(format!("{:.2}", book_value))
        .bind(&amort)
        .bind(now_unix())
        .execute(pool)
        .await?;

        states.push(AssetDepreciationState {
            asset_id: a.id.clone(),
            asset_code: a.asset_code.clone(),
            description: a.description.clone(),
            monthly_charge: format!("{:.2}", for_period),
            accumulated: format!("{:.2}", accumulated),
            book_value: format!("{:.2}", book_value),
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
    if !valid_ymd(disposal_date) {
        return Err(AppError::Validation(
            "Data scoaterii din uz este invalidă — folosiți AAAA-LL-ZZ.".into(),
        ));
    }
    let asset = get(pool, asset_id, company_id).await?;
    let cost = Decimal::from_str(asset.acquisition_cost.trim()).unwrap_or(Decimal::ZERO);
    // Accumulated = Σ register amounts through the disposal month (single source of truth so GL ties).
    // Sum the Decimal-as-TEXT amounts in Rust to avoid f64 precision loss.
    let disp_ym = disposal_date
        .get(..7)
        .ok_or_else(|| AppError::Validation("Dată invalidă — folosiți AAAA-LL-ZZ.".into()))?;
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
    use sqlx::SqlitePool;

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
            fiscal_method: None,
            is_new: true,
            subgroup: None,
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
    fn depreciation_stops_in_disposal_month() {
        // cost=1200, life=12m, PIF Jan 2025 (charges Feb..). Disposed 2025-06-10 → NU se amortizează
        // în iunie (luna scoaterii); ultima lună amortizată = mai. Σ Feb-Mai = 4×100 = 400.
        let mut asset = sample_asset("1200.00", 12, "2025-01-01");
        asset.disposal_date = Some("2025-06-10".into());
        // mai (a 4-a lună amortizabilă): se încarcă 100.
        let may = compute_depreciation(&asset, "2025-05-01", "2025-05-31");
        assert_eq!(may.for_period, Decimal::from_str("100.00").unwrap());
        assert_eq!(may.accumulated_end, Decimal::from_str("400.00").unwrap());
        // iunie (luna scoaterii din funcțiune): 0 — înainte de fix se încărca o lună întreagă.
        let jun = compute_depreciation(&asset, "2025-06-01", "2025-06-30");
        assert_eq!(jun.for_period, Decimal::ZERO);
        assert_eq!(jun.accumulated_end, Decimal::from_str("400.00").unwrap());
        // valoarea rămasă (800) se descarcă prin scoatere, nu prin amortizare.
        assert_eq!(jun.book_value_end, Decimal::from_str("800.00").unwrap());
        // după scoatere: tot 0.
        let jul = compute_depreciation(&asset, "2025-07-01", "2025-07-31");
        assert_eq!(jul.for_period, Decimal::ZERO);
    }

    #[test]
    fn depreciation_zero_life_months_returns_zero() {
        let asset = sample_asset("5000.00", 0, "2025-01-01");
        let calc = compute_depreciation(&asset, "2025-01-01", "2025-12-31");
        assert_eq!(calc.monthly, Decimal::ZERO);
        assert_eq!(calc.accumulated_end, Decimal::ZERO);
    }

    async fn setup_asset_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Seed one company with valid production-schema columns.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county) \
             VALUES ('co-1', 'RO12345674', 'Test SRL', 'Str. 1', 'București', 'B')",
        )
        .execute(&pool)
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
            fiscal_method: None,
            is_new: Some(true),
            subgroup: None,
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
                                           // Chrono-level calendar check: impossible days must be rejected.
        assert!(!valid_ymd("2025-02-31")); // Feb 31 doesn't exist
        assert!(!valid_ymd("2025-13-01")); // month 13 (double-check via chrono)
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
    async fn create_empty_start_up_date_falls_back_to_acquisition() {
        // EDGE-002: Some("") must behave like None — start_up_date should fall back to
        // date_of_acquisition, not be stored as "" (which would make parse_ym compute from year 0).
        let pool = setup_asset_pool().await;
        let mut input = sample_input();
        input.start_up_date = Some("".into());
        let asset = create(&pool, "co-1", input).await.unwrap();
        assert_eq!(
            asset.start_up_date, asset.date_of_acquisition,
            "empty start_up_date must fall back to date_of_acquisition"
        );
        assert!(
            !asset.start_up_date.is_empty(),
            "start_up_date must not be stored as empty"
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

    /// Method validation: unknown methods are rejected; all four recognized methods are accepted.
    #[tokio::test]
    async fn create_rejects_unknown_depreciation_method() {
        let pool = setup_asset_pool().await;
        // Unknown method → Validation error.
        let mut bad = sample_input();
        bad.depreciation_method = Some("inventata".into());
        assert!(matches!(
            create(&pool, "co-1", bad).await.unwrap_err(),
            AppError::Validation(_)
        ));
        // None → implicit "liniara" — accepted.
        let mut ok_none = sample_input();
        ok_none.asset_code = "MF-none-method".into();
        ok_none.depreciation_method = None;
        create(&pool, "co-1", ok_none).await.unwrap();
        // "liniara" — accepted.
        let mut ok_lin = sample_input();
        ok_lin.asset_code = "MF-lin-method".into();
        ok_lin.depreciation_method = Some("liniara".into());
        create(&pool, "co-1", ok_lin).await.unwrap();
        // "degresiva" with DNU >= 2 (36 months = 3 yr) — accepted.
        let mut ok_deg = sample_input();
        ok_deg.asset_code = "MF-deg-method".into();
        ok_deg.depreciation_method = Some("degresiva".into());
        create(&pool, "co-1", ok_deg).await.unwrap();
        // "accelerata" — accepted.
        let mut ok_acc = sample_input();
        ok_acc.asset_code = "MF-acc-method".into();
        ok_acc.depreciation_method = Some("accelerata".into());
        create(&pool, "co-1", ok_acc).await.unwrap();
    }

    /// update() must reject unknown methods but accept all four recognized methods.
    #[tokio::test]
    async fn update_rejects_unknown_depreciation_method() {
        let pool = setup_asset_pool().await;
        let asset = create(&pool, "co-1", sample_input()).await.unwrap();
        let mut bad_upd = sample_input();
        bad_upd.depreciation_method = Some("grешita".into());
        assert!(matches!(
            update(&pool, &asset.id, "co-1", bad_upd).await.unwrap_err(),
            AppError::Validation(_)
        ));
        // "accelerata" must now be accepted on update too.
        let mut ok_upd = sample_input();
        ok_upd.depreciation_method = Some("accelerata".into());
        // No error expected.
        update(&pool, &asset.id, "co-1", ok_upd).await.unwrap();
    }

    // ─── Worked examples from spec ────────────────────────────────────────────

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    /// Degresivă VI=10000, DNU=5 yr.
    /// Cl=20%, k=1.5 (DNU∈[2,5]), Cd=30%.
    /// Y1=3000 (rem 7000); Y2=2100 (rem 4900);
    /// Y3: degr 4900×30%=1470 vs lin 4900/3=1633.33 → switch → Y3=Y4=Y5=4900/3.
    /// Σ=10000 exactly.
    #[test]
    fn degressive_worked_example_5yr() {
        let schedule = degressive_schedule(d("10000"), 5).unwrap();
        assert_eq!(schedule.len(), 5);
        assert_eq!(schedule[0], d("3000.00"), "Y1");
        assert_eq!(schedule[1], d("2100.00"), "Y2");
        // Switch at Y3: remaining 4900 / 3 years = 1633.33.
        // The three switch-years must sum to exactly 4900.
        let switch_sum: Decimal = schedule[2] + schedule[3] + schedule[4];
        assert_eq!(
            switch_sum,
            d("4900.00"),
            "Y3+Y4+Y5 must equal remaining 4900"
        );
        // Each switch-year should be 1633.33 (except last which absorbs residual).
        assert_eq!(schedule[2], d("1633.33"), "Y3 after switch");
        assert_eq!(schedule[3], d("1633.33"), "Y4");
        assert_eq!(schedule[4], d("1633.34"), "Y5 absorbs residual");
        // Total = VI exactly.
        let total: Decimal = schedule.iter().copied().sum();
        assert_eq!(total, d("10000.00"), "Σ must equal VI");
    }

    /// Degresivă band selection: k factor for DNU = 4, 8, 15.
    /// Note: rates are rounded to 2 decimal places (MidpointAwayFromZero) per round2.
    #[test]
    fn degressive_k_factor_bands() {
        // DNU=4 (2≤DNU≤5) → k=1.5, Cl=round2(1/4)=0.25, Cd=round2(0.25×1.5)=round2(0.375)=0.38.
        // Y1 = 10000 × 0.38 = 3800.
        let s4 = degressive_schedule(d("10000"), 4).unwrap();
        assert_eq!(
            s4[0],
            d("3800.00"),
            "DNU=4 Y1: 10000×38% (Cd=round2(0.375)=0.38)"
        );
        // DNU=8 (5<DNU≤10) → k=2.0, Cl=round2(1/8)=0.13, Cd=round2(0.13×2.0)=0.26.
        // Y1 = 10000 × 0.26 = 2600.
        let s8 = degressive_schedule(d("10000"), 8).unwrap();
        assert_eq!(
            s8[0],
            d("2600.00"),
            "DNU=8 Y1: 10000×26% (Cd=round2(0.25)=0.25→×2=0.26)"
        );
        // DNU=15 (DNU>10) → k=2.5, Cl=round2(1/15)=round2(0.0667)=0.07.
        // Cd=round2(0.07×2.5)=round2(0.175)=0.18. Y1 = 10000 × 0.18 = 1800.
        let s15 = degressive_schedule(d("10000"), 15).unwrap();
        assert_eq!(
            s15[0],
            d("1800.00"),
            "DNU=15 Y1: 10000×18% (Cd=round2(0.175)=0.18)"
        );
        // Σ must equal 10000 in all cases.
        let sum4: Decimal = s4.iter().copied().sum();
        let sum8: Decimal = s8.iter().copied().sum();
        let sum15: Decimal = s15.iter().copied().sum();
        assert_eq!(sum4, d("10000.00"));
        assert_eq!(sum8, d("10000.00"));
        assert_eq!(sum15, d("10000.00"));
    }

    /// Degresivă DNU<2 → error.
    #[test]
    fn degressive_dnu_lt2_returns_error() {
        assert!(degressive_schedule(d("5000"), 1).is_err());
        assert!(degressive_schedule(d("5000"), 0).is_err());
    }

    /// Accelerată VI=2000000, DNU=12 yr.
    /// Y1=1000000 (50%); Y2..Y12 = 1000000/11 = 90909.09/yr.
    #[test]
    fn accelerated_worked_example_12yr() {
        let schedule = accelerated_schedule(d("2000000"), 12).unwrap();
        assert_eq!(schedule.len(), 12);
        assert_eq!(schedule[0], d("1000000.00"), "Y1 = 50%");
        // Remaining 1000000 over 11 years: per year = round2(1000000/11) = 90909.09.
        // Last year absorbs residual: 1000000 - 90909.09*10 = 1000000 - 909090.90 = 90909.10.
        for (idx, val) in schedule.iter().enumerate().take(11).skip(1) {
            assert_eq!(*val, d("90909.09"), "Y{}", idx + 1);
        }
        assert_eq!(schedule[11], d("90909.10"), "Y12 absorbs residual");
        let total: Decimal = schedule.iter().copied().sum();
        assert_eq!(total, d("2000000.00"), "Σ must equal VI");
    }

    /// Super-accelerată VI=1000000. Y1=65%; rest liniar.
    #[test]
    fn super_accelerated_worked_example() {
        // DNU unspecified in spec example — use DNU=5 as a concrete test.
        let schedule = super_accelerated_schedule(d("1000000"), 5).unwrap();
        assert_eq!(schedule.len(), 5);
        assert_eq!(schedule[0], d("650000.00"), "Y1 = 65%");
        // Remaining 350000 / 4 years = 87500 each.
        assert_eq!(schedule[1], d("87500.00"), "Y2");
        assert_eq!(schedule[2], d("87500.00"), "Y3");
        assert_eq!(schedule[3], d("87500.00"), "Y4");
        assert_eq!(schedule[4], d("87500.00"), "Y5");
        let total: Decimal = schedule.iter().copied().sum();
        assert_eq!(total, d("1000000.00"), "Σ must equal VI");
    }

    /// Liniară schedule: Σ == VI, each year equal (last absorbs residual).
    #[test]
    fn linear_schedule_sum_equals_vi() {
        let schedule = linear_schedule(d("10000"), 3).unwrap();
        assert_eq!(schedule.len(), 3);
        let total: Decimal = schedule.iter().copied().sum();
        assert_eq!(total, d("10000.00"));
    }

    /// compute_period_charge for degresivă asset: monthly charge in year 1 = Y1_annual/12.
    #[test]
    fn degressive_monthly_charge_year1() {
        // VI=10000, DNU=5yr (life_months=60), PIF 2025-01-01.
        // Y1 annual = 3000 → monthly = 3000/12 = 250.
        let mut asset = sample_asset("10000.00", 60, "2025-01-01");
        asset.depreciation_method = "degresiva".into();
        // First depreciable month = Feb 2025 (month after PIF).
        let charge = compute_period_charge(&asset, "2025-02-01");
        assert_eq!(charge, d("250.00"), "monthly Y1 charge");
        // Jan 2025 (PIF month): 0.
        let pif_month = compute_period_charge(&asset, "2025-01-01");
        assert_eq!(pif_month, Decimal::ZERO, "PIF month = 0");
    }

    /// compute_period_charge for accelerată asset: first month = Y1_annual/12.
    #[test]
    fn accelerated_monthly_charge_year1() {
        // VI=2000000, DNU=12yr (life=144m), PIF 2025-01-01.
        // Y1 annual = 1000000 → monthly = 83333.33.
        let mut asset = sample_asset("2000000.00", 144, "2025-01-01");
        asset.depreciation_method = "accelerata".into();
        let charge = compute_period_charge(&asset, "2025-02-01");
        assert_eq!(charge, d("83333.33"), "monthly Y1 accelerata charge");
    }

    /// P2 — non-12-multiple life_months: Σ of monthly charges over the asset's actual life
    /// must equal VI exactly, accumulated at month life_months must equal VI, and there must
    /// be no over-run past life_months.
    #[test]
    fn accelerated_non12_multiple_life_months_no_overrun() {
        // life_months=18 → DNU=ceil(18/12)=2.  VI=12000.
        // PIF 2025-01-01 → first depreciable month = Feb 2025.
        // Months 1..18 are Feb 2025 .. Jul 2026.
        let vi = d("12000.00");
        let mut asset = sample_asset("12000.00", 18, "2025-01-01");
        asset.depreciation_method = "accelerata".into();

        // Sum monthly charges for months 1..18 and assert == VI.
        let months = [
            "2025-02-01",
            "2025-03-01",
            "2025-04-01",
            "2025-05-01",
            "2025-06-01",
            "2025-07-01",
            "2025-08-01",
            "2025-09-01",
            "2025-10-01",
            "2025-11-01",
            "2025-12-01",
            "2026-01-01",
            "2026-02-01",
            "2026-03-01",
            "2026-04-01",
            "2026-05-01",
            "2026-06-01",
            "2026-07-01",
        ];
        let sum: Decimal = months
            .iter()
            .map(|m| compute_period_charge(&asset, m))
            .sum();
        assert_eq!(sum, vi, "Σ of 18 monthly charges must equal VI");

        // accumulated at month 18 (depreciable month 18 = 2026-07) == VI.
        let acc_at_18 = compute_accumulated(&asset, "2026-07-01");
        assert_eq!(acc_at_18, vi, "accumulated at life end must equal VI");

        // No over-run: months 19+ must also return VI.
        let acc_at_19 = compute_accumulated(&asset, "2026-08-01");
        assert_eq!(acc_at_19, vi, "accumulated past life must not exceed VI");
        let acc_at_24 = compute_accumulated(&asset, "2026-12-01");
        assert_eq!(
            acc_at_24, vi,
            "accumulated 6 months past life must equal VI"
        );
    }

    /// P2 — degressive with life_months=30 (DNU=3, but life ends at month 30, not 36).
    #[test]
    fn degressive_non12_multiple_life_months_no_overrun() {
        // life_months=30 → DNU=ceil(30/12)=3. VI=9000.
        // PIF 2025-01-01 → month 30 = 2027-07-01.
        let vi = d("9000.00");
        let mut asset = sample_asset("9000.00", 30, "2025-01-01");
        asset.depreciation_method = "degresiva".into();

        // accumulated at month 30 must == VI.
        let acc_at_30 = compute_accumulated(&asset, "2027-07-01");
        assert_eq!(acc_at_30, vi, "accumulated at life_months=30 must equal VI");

        // No over-run at month 31+.
        let acc_past = compute_accumulated(&asset, "2027-08-01");
        assert_eq!(acc_past, vi, "no over-run past life_months=30");
    }

    /// P3b — updating an existing super_accelerata asset's description (without changing method)
    /// succeeds even when the input's date fields don't satisfy 2026 eligibility on their own.
    #[tokio::test]
    async fn update_super_accelerata_description_without_method_change_succeeds() {
        let pool = setup_asset_pool().await;
        // Create a valid super_accelerata asset (2026, is_new, subgroup 2.1).
        let mut inp = sample_input();
        inp.asset_code = "MF-super".into();
        inp.depreciation_method = Some("super_accelerata".into());
        inp.date_of_acquisition = "2026-03-01".into();
        inp.start_up_date = Some("2026-03-01".into());
        inp.is_new = Some(true);
        inp.subgroup = Some("2.1".into());
        let asset = create(&pool, "co-1", inp).await.unwrap();

        // Partial update: only change description; leave depreciation_method as None
        // (i.e., don't re-supply it) and don't supply 2026 dates.
        // The update must succeed without triggering the 2026-PIF eligibility check.
        let mut upd = sample_input();
        upd.asset_code = asset.asset_code.clone(); // keep same code
        upd.description = "Laptop Dell (updated)".into();
        upd.depreciation_method = None; // not changing method
                                        // Supply the same dates so date-format validation passes.
        upd.date_of_acquisition = asset.date_of_acquisition.clone();
        upd.start_up_date = None; // trigger the stored-fallback path
        upd.is_new = Some(asset.is_new);
        upd.subgroup = asset.subgroup.clone();
        // Must not fail with "puse în funcțiune în 2026" error.
        let updated = update(&pool, &asset.id, "co-1", upd).await.unwrap();
        assert_eq!(updated.description, "Laptop Dell (updated)");
        assert_eq!(updated.depreciation_method, "super_accelerata");
    }
}
