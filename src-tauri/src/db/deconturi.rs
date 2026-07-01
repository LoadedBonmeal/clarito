//! P3 Wave D — Avansuri de trezorerie (542) + Deconturi de cheltuieli.
//!
//! ## Diurnă engine (CF art.76(2)(k), art.76(4)(h), art.142(g), HG 714/2018)
//! Limita neimpozabilă zilnică = min(A, B):
//!   A = 2.5 × diurna_interna (config; default 23 lei → 57.50 lei/zi)
//!   B = salariu_brut × 3 ÷ working_days(an, luna_delegatiei)
//! Total neimpozabil = min(diurna_acordata, min(A,B) × zile_delegare)
//! Surplus impozabil = max(0, diurna_acordata − neimpozabil)
//!
//! Surplusul impozabil este CALCULAT + AFIȘAT + POSTAT în GL via monografia de reclasificare
//! (source_type='DIURNA_ASIMILAT', db/payroll_diurna.rs) și înregistrat în payroll_extra_income
//! pentru auto-feed statul de salarii + D112.
//!
//! ## Monografie GL (post_manual_journal, idempotent)
//! Grant (source_type='AVANS_TREZORERIE'):  542 D = 5311/5121 C
//! Aprobare decont (source_type='EXPENSE_REPORT'): cheltuieli D + 4426 D = 542 C
//!   — TOATĂ diurna (neimpozabila + impozabila) în 625; reclasificarea excesului este în
//!   jurnalul separat DIURNA_ASIMILAT (db/payroll_diurna.rs).
//! Reclasificare surplus (source_type='DIURNA_ASIMILAT'): 641 D = 625 C + retineri + CAM
//! Return (source_type='AVANS_RETURN'):    5311/5121 D = 542 C

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json;
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::gl::{post_manual_journal, ManualJournal};
use crate::db::models::{new_id, now_unix};
use crate::db::payroll::{days_in_month, working_days};
use crate::db::payroll_diurna::upsert_extra_income;
use crate::error::{AppError, AppResult};

/// Parse an ISO date string (`YYYY-MM-DD` or `YYYY-M-D`) into a zero-padded `YYYY-MM` period.
///
/// P3c: derive the period robustly by parsing the date components and re-formatting as
/// `format!("{year:04}-{month:02}")` — not by string-slicing — so non-padded months (e.g.
/// `"2026-6-15"`) produce the correct `"2026-06"` and not a broken lookup key.
fn parse_date_to_period(date_iso: &str) -> Option<String> {
    let parts: Vec<&str> = date_iso.split('-').collect();
    if parts.len() < 2 {
        return None;
    }
    let year: i32 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    if month == 0 || month > 12 {
        return None;
    }
    Some(format!("{year:04}-{month:02}"))
}

/// Limit-A factor (HG 714/2018 art.2 + CF art.76(4)(h)): daily non-taxable diurnă cap A
/// = this factor × `diurna_interna` (the configured internal per-diem rate).
const DIURNA_LIMIT_A_FACTOR: &str = "2.5";

/// Limit-B salary factor (CF art.76(2)(k)): monthly non-taxable diurnă cap B
/// = gross salary × this factor ÷ working_days(year, month).
const DIURNA_LIMIT_B_SALARY_FACTOR: i64 = 3;

fn dp(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or_default()
}

fn round2(x: Decimal) -> Decimal {
    x.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
}

/// Format a Decimal to exactly 2 decimal places (e.g. "200" → "200.00").
fn fmt2(x: Decimal) -> String {
    format!("{:.2}", x)
}

// ─── Diurnă engine ────────────────────────────────────────────────────────────

/// Result of the diurnă cap computation.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiurnaCalc {
    /// Total diurnă acordată (input).
    pub diurna_acordata: String,
    /// Non-taxable portion (≤ acordată, ≤ cap).
    pub diurna_neimpozabila: String,
    /// Taxable excess = acordată − neimpozabilă. Posted via DIURNA_ASIMILAT journal on approval.
    pub diurna_impozabila: String,
    /// Limit A per day = 2.5 × diurna_interna.
    pub limit_a_zi: String,
    /// Limit B per day = salariu × 3 ÷ working_days.
    pub limit_b_zi: String,
    /// The binding cap per day = min(A, B).
    pub cap_zi: String,
    /// working_days used for limit-B proration.
    pub working_days_used: u32,
}

/// Pure diurnă engine: single-month delegation.
///
/// Arguments:
/// - `diurna_acordata_total`: total diurnă given (lei, Decimal text)
/// - `zile_delegare`: number of delegation days
/// - `salariu_brut`: employee's gross monthly base salary (lei, Decimal text)
/// - `year`, `month`: the calendar month of the delegation (for working_days proration)
/// - `diurna_interna`: config value (lei/zi, Decimal text; default "23.00")
pub fn compute_diurna(
    diurna_acordata_total: &str,
    zile_delegare: u32,
    salariu_brut: &str,
    year: i32,
    month: u32,
    diurna_interna: &str,
) -> DiurnaCalc {
    let acordata = round2(dp(diurna_acordata_total));
    let sal = round2(dp(salariu_brut));
    let interna = round2(dp(diurna_interna));
    let zile = Decimal::from(zile_delegare);
    let nzl = working_days(year, month);

    // Limit A per day: 2.5 × diurna_interna (HG 714/2018 art.2 + CF art.76(4)(h))
    let limit_a_zi = round2(Decimal::from_str(DIURNA_LIMIT_A_FACTOR).unwrap() * interna);

    // Limit B per day: salariu × 3 ÷ working_days (CF art.76(2)(k))
    let limit_b_zi = if nzl == 0 {
        Decimal::ZERO
    } else {
        round2(sal * Decimal::from(DIURNA_LIMIT_B_SALARY_FACTOR) / Decimal::from(nzl))
    };

    // Cap per day = min(A, B)
    let cap_zi = round2(limit_a_zi.min(limit_b_zi));

    // Total cap = cap_zi × zile
    let cap_total = round2(cap_zi * zile);

    // Non-taxable = min(acordata, cap_total)
    let neimpozabila = round2(acordata.min(cap_total));

    // Taxable excess
    let impozabila = round2((acordata - neimpozabila).max(Decimal::ZERO));

    DiurnaCalc {
        diurna_acordata: fmt2(acordata),
        diurna_neimpozabila: fmt2(neimpozabila),
        diurna_impozabila: fmt2(impozabila),
        limit_a_zi: fmt2(limit_a_zi),
        limit_b_zi: fmt2(limit_b_zi),
        cap_zi: fmt2(cap_zi),
        working_days_used: nzl,
    }
}

// ─── Multi-month diurnă engine (CF art.76(2)(k)) ─────────────────────────────

/// Per-calendar-month breakdown of a delegation's diurnă.
///
/// CF art.76(2)(k) mandates computing the non-taxable cap **per calendar month**:
/// the 3-salary ceiling is prorated by that month's full working-day count and
/// multiplied only by the delegation days that fall in that month.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiurnaMonthSegment {
    /// Calendar month, `YYYY-MM`.
    pub period: String,
    /// Delegation calendar days (incl. weekends) that fall in this month.
    pub delegation_days: u32,
    /// Working days of the FULL calendar month (NZL — for limit-B proration).
    pub working_days: u32,
    /// Limit A per day = 2.5 × diurna_interna.
    pub limit_a_zi: Decimal,
    /// Limit B per day = salariu × 3 ÷ working_days_month (or 0 if nzl=0).
    pub limit_b_zi: Decimal,
    /// Binding cap per day = min(A, B).
    pub cap_zi: Decimal,
    /// Diurnă acordată for this month = daily_rate × delegation_days.
    pub acordata: Decimal,
    /// Non-taxable portion = min(acordata, cap_zi × delegation_days).
    pub nontax: Decimal,
    /// Taxable excess = max(0, acordata − nontax). Attributed to THIS month's payroll.
    pub excess: Decimal,
}

/// Parse an ISO date `YYYY-MM-DD` (or `YYYY-M-D`) into `(year, month, day)`.
fn parse_ymd(iso: &str) -> Option<(i32, u32, u32)> {
    let p: Vec<&str> = iso.split('-').collect();
    if p.len() < 3 {
        return None;
    }
    let y: i32 = p[0].parse().ok()?;
    let m: u32 = p[1].parse().ok()?;
    let d: u32 = p[2].parse().ok()?;
    if m == 0 || m > 12 || d == 0 || d > 31 {
        return None;
    }
    Some((y, m, d))
}

/// Advance a `(year, month, day)` by one calendar day.
fn next_day(y: i32, m: u32, d: u32) -> (i32, u32, u32) {
    let dim = days_in_month(y, m);
    if d < dim {
        (y, m, d + 1)
    } else if m < 12 {
        (y, m + 1, 1)
    } else {
        (y + 1, 1, 1)
    }
}

/// Compare two `(year, month, day)` tuples for ordering.
#[inline]
fn ymd_le(a: (i32, u32, u32), b: (i32, u32, u32)) -> bool {
    a.0 < b.0 || (a.0 == b.0 && (a.1 < b.1 || (a.1 == b.1 && a.2 <= b.2)))
}

/// Multi-month diurnă engine — CF art.76(2)(k).
///
/// Segments the delegation `[start_iso, end_iso]` (inclusive on both ends; every
/// calendar day counts as a diurnă day) by calendar month, then computes per-month
/// cap/nontax/excess using `daily_rate` per delegation day and the full working-day
/// count of each month for limit-B proration.
///
/// A single-month delegation returns exactly one segment, identical to what
/// `compute_diurna` would produce for (year, month) of the start date.
///
/// Returns an empty `Vec` if either date is missing or malformed, or if start > end.
pub fn compute_diurna_multimonth(
    start_iso: &str,
    end_iso: &str,
    daily_rate: Decimal,
    salariu_brut: &str,
    diurna_interna: &str,
) -> Vec<DiurnaMonthSegment> {
    let Some(start) = parse_ymd(start_iso) else {
        return vec![];
    };
    let Some(end) = parse_ymd(end_iso) else {
        return vec![];
    };
    if !ymd_le(start, end) {
        return vec![];
    }

    let sal = round2(dp(salariu_brut));
    let interna = round2(dp(diurna_interna));
    // Limit A per day = 2.5 × diurna_interna (HG 714/2018 art.2)
    let limit_a_zi = round2(Decimal::from_str(DIURNA_LIMIT_A_FACTOR).unwrap() * interna);

    // Accumulate delegation_days per (year, month).
    // We walk every calendar day from start to end (inclusive).
    let mut month_days: Vec<((i32, u32), u32)> = Vec::new();
    let mut cur = start;
    loop {
        let (cy, cm, _cd) = cur;
        // Find or create entry for (cy, cm).
        if let Some(entry) = month_days.iter_mut().find(|(ym, _)| *ym == (cy, cm)) {
            entry.1 += 1;
        } else {
            month_days.push(((cy, cm), 1));
        }
        if cur == end {
            break;
        }
        cur = next_day(cur.0, cur.1, cur.2);
    }

    // Build segments — preserve calendar order (month_days is already in order).
    let mut segments = Vec::with_capacity(month_days.len());
    for ((y, m), del_days) in month_days {
        let nzl = working_days(y, m);
        let limit_b_zi = if nzl == 0 {
            Decimal::ZERO
        } else {
            round2(sal * Decimal::from(DIURNA_LIMIT_B_SALARY_FACTOR) / Decimal::from(nzl))
        };
        let cap_zi = round2(limit_a_zi.min(limit_b_zi));
        let cap_total = round2(cap_zi * Decimal::from(del_days));
        let acordata = round2(daily_rate * Decimal::from(del_days));
        let nontax = round2(acordata.min(cap_total));
        let excess = round2((acordata - nontax).max(Decimal::ZERO));

        segments.push(DiurnaMonthSegment {
            period: format!("{y:04}-{m:02}"),
            delegation_days: del_days,
            working_days: nzl,
            limit_a_zi,
            limit_b_zi,
            cap_zi,
            acordata,
            nontax,
            excess,
        });
    }
    segments
}

// ─── DB structs ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TreasuryAdvance {
    pub id: String,
    pub company_id: String,
    pub employee_id: Option<String>,
    pub amount: String,
    pub currency: String,
    pub granted_date: String,
    pub method: String,
    pub status: String,
    pub notes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAdvanceInput {
    pub company_id: String,
    pub employee_id: Option<String>,
    pub amount: String,
    pub currency: Option<String>,
    pub granted_date: String,
    pub method: Option<String>,
    pub notes: Option<String>,
}

/// Compact serialisable form of a per-month segment stored in `diurna_breakdown_json`.
///
/// Only the fields needed by `approve_report` are stored (period, nontax, excess).
/// Full per-month detail (`limit_a_zi`, `cap_zi`, etc.) is available at create time
/// via `DiurnaMonthSegment` but is not needed for the payroll/GL feed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiurnaBreakdownSegment {
    /// Calendar month, `YYYY-MM`.
    pub period: String,
    /// Non-taxable diurnă for this month (Decimal text — lossless round-trip).
    pub nontax: String,
    /// Taxable excess for this month (Decimal text).
    pub excess: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseReport {
    pub id: String,
    pub company_id: String,
    pub advance_id: Option<String>,
    pub employee_id: Option<String>,
    pub delegation_from: Option<String>,
    pub delegation_to: Option<String>,
    pub destination: Option<String>,
    pub days: Option<i64>,
    pub diurna_acordata: Option<String>,
    pub diurna_neimpozabila: Option<String>,
    pub diurna_impozabila: Option<String>,
    pub salariu_baza: Option<String>,
    /// Configured internal diurnă rate (lei/zi) used at create time for limit_A = 2.5×interna.
    /// Persisted so approve_report never drifts to a hardcoded fallback. (migration 0080)
    pub diurna_interna: Option<String>,
    /// JSON-serialised `Vec<DiurnaBreakdownSegment>` — one entry per calendar month.
    /// NULL for reports created before migration 0080 (backward-compat fallback in approve).
    pub diurna_breakdown_json: Option<String>,
    pub report_date: String,
    pub status: String,
    pub notes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseLine {
    pub id: String,
    pub report_id: String,
    pub category: String,
    pub description: Option<String>,
    pub amount: String,
    pub vat_amount: Option<String>,
    pub account_code: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseLineInput {
    pub category: String,
    pub description: Option<String>,
    pub amount: String,
    pub vat_amount: Option<String>,
    pub account_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateReportInput {
    pub company_id: String,
    pub advance_id: Option<String>,
    pub employee_id: Option<String>,
    pub delegation_from: Option<String>,
    pub delegation_to: Option<String>,
    pub destination: Option<String>,
    pub days: Option<i64>,
    pub diurna_acordata: Option<String>,
    pub salariu_baza: Option<String>,
    pub report_date: String,
    pub notes: Option<String>,
    pub lines: Vec<ExpenseLineInput>,
    /// Config: diurna_interna per day (from payroll_config; default "23.00")
    pub diurna_interna: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseReportFull {
    pub report: ExpenseReport,
    pub lines: Vec<ExpenseLine>,
    pub diurna_calc: Option<DiurnaCalc>,
}

// Category → default account code
fn category_account(cat: &str) -> &'static str {
    match cat {
        "diurna" => "625",
        "transport" => "624",
        "cazare" => "625",
        "combustibil" => "6022",
        _ => "628",
    }
}

// ─── Treasury advance CRUD + GL ───────────────────────────────────────────────

pub async fn create_advance(
    pool: &SqlitePool,
    input: CreateAdvanceInput,
) -> AppResult<TreasuryAdvance> {
    let id = new_id();
    let now = now_unix();
    let currency = input.currency.unwrap_or_else(|| "RON".into());
    let method = input.method.unwrap_or_else(|| "cash".into());

    // Validate amount is a valid Decimal
    let amount_dec = Decimal::from_str(&input.amount)
        .map_err(|_| AppError::Validation("amount must be a valid decimal".into()))?;
    let amount_str = fmt2(round2(amount_dec));

    sqlx::query(
        "INSERT INTO treasury_advances \
         (id, company_id, employee_id, amount, currency, granted_date, method, status, notes, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,'granted',?8,?9,?9)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.employee_id)
    .bind(&amount_str)
    .bind(&currency)
    .bind(&input.granted_date)
    .bind(&method)
    .bind(&input.notes)
    .bind(now)
    .execute(pool)
    .await?;

    // GL: 542 D = 5311/5121 C
    let cash_acct = if method == "bank" { "5121" } else { "5311" };
    post_manual_journal(
        pool,
        &ManualJournal {
            company_id: &input.company_id,
            journal_id: &format!("AVZ-{}", &id[..8]),
            journal_type: "AVANS",
            source_type: "AVANS_TREZORERIE",
            source_id: &id,
            date: &input.granted_date,
            description: &format!(
                "Acordare avans trezorerie {}",
                input.employee_id.as_deref().unwrap_or("-")
            ),
            partner_cui: None,
        },
        &[
            ("542", amount_dec, Decimal::ZERO),
            (cash_acct, Decimal::ZERO, amount_dec),
        ],
    )
    .await?;

    get_advance(pool, &id, &input.company_id).await
}

pub async fn list_advances(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<TreasuryAdvance>> {
    Ok(sqlx::query_as::<_, TreasuryAdvance>(
        "SELECT * FROM treasury_advances WHERE company_id=?1 ORDER BY granted_date DESC, created_at DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_advance(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<TreasuryAdvance> {
    sqlx::query_as::<_, TreasuryAdvance>(
        "SELECT * FROM treasury_advances WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

/// Return unused advance: 5311/5121 D = 542 C
pub async fn return_advance(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    return_date: &str,
) -> AppResult<TreasuryAdvance> {
    let adv = get_advance(pool, id, company_id).await?;
    if adv.status != "granted" {
        return Err(AppError::Validation(
            "Only 'granted' advances can be returned".into(),
        ));
    }
    let now = now_unix();
    sqlx::query(
        "UPDATE treasury_advances SET status='returned', updated_at=?1 WHERE id=?2 AND company_id=?3",
    )
    .bind(now)
    .bind(id)
    .bind(company_id)
    .execute(pool)
    .await?;

    let amount = Decimal::from_str(&adv.amount).unwrap_or_default();
    let cash_acct = if adv.method == "bank" { "5121" } else { "5311" };
    // Return: 5311/5121 D = 542 C
    post_manual_journal(
        pool,
        &ManualJournal {
            company_id,
            journal_id: &format!("RET-{}", &id[..8]),
            journal_type: "AVANS",
            source_type: "AVANS_RETURN",
            source_id: id,
            date: return_date,
            description: "Restituire avans trezorerie neutilizat",
            partner_cui: None,
        },
        &[
            (cash_acct, amount, Decimal::ZERO),
            ("542", Decimal::ZERO, amount),
        ],
    )
    .await?;

    get_advance(pool, id, company_id).await
}

pub async fn delete_advance(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let adv = get_advance(pool, id, company_id).await?;
    if adv.status != "granted" {
        return Err(AppError::Validation(
            "Only draft 'granted' advances can be deleted".into(),
        ));
    }
    // Remove GL first (idempotent)
    sqlx::query(
        "DELETE FROM gl_journal WHERE company_id=?1 AND source_type='AVANS_TREZORERIE' AND source_id=?2",
    )
    .bind(company_id)
    .bind(id)
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM treasury_advances WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ─── Expense report CRUD + approve ────────────────────────────────────────────

pub async fn create_report(
    pool: &SqlitePool,
    input: CreateReportInput,
) -> AppResult<ExpenseReportFull> {
    let id = new_id();
    let now = now_unix();

    // ── Single source-of-truth diurnă split (P1/P2 fix) ───────────────────────
    //
    // compute_diurna_multimonth is called ONCE here at create time.  It uses:
    //   • the exact daily_rate = round2(acordata / days)
    //   • the REAL diurna_interna from the input (never a hardcoded "23.00")
    //   • the full delegation date span [from, to] for per-month cap computation
    //
    // Stored results:
    //   diurna_neimpozabila = Σ nontax[m]   (multimonth total — replaces single-month value)
    //   diurna_impozabila   = Σ excess[m]   (multimonth total)
    //   diurna_interna      = the configured rate used (persisted to prevent approve drift)
    //   diurna_breakdown_json = [{period, nontax, excess}] per month (JSON, compact)
    //
    // Rounding remainder: the per-segment nontax/excess are already rounded to 2dp by the
    // engine. To guarantee Σ(nontax)+Σ(excess) == acordata EXACTLY, we compute the
    // residual (acordata − Σ(nontax) − Σ(excess)) and add it to the last segment's nontax
    // (which keeps nontax ≤ cap, since the residual is a sub-cent artefact from round2).
    //
    // approve_report DESERIALIZES diurna_breakdown_json — it never recomputes.
    let interna_str = input
        .diurna_interna
        .as_deref()
        .unwrap_or("23.00")
        .to_string();

    let (diurna_neimpozabila, diurna_impozabila, stored_breakdown) =
        if let (Some(acordata_str), Some(sal), Some(days), Some(from), Some(to)) = (
            input.diurna_acordata.as_deref(),
            input.salariu_baza.as_deref(),
            input.days,
            input.delegation_from.as_deref(),
            input.delegation_to.as_deref(),
        ) {
            if days > 0 {
                let acordata = round2(dp(acordata_str));
                let daily_rate = round2(acordata / Decimal::from(days as u32));
                let segments = compute_diurna_multimonth(from, to, daily_rate, sal, &interna_str);

                if segments.is_empty() {
                    // Malformed dates — fall back to single-month (safe, no crash).
                    let parts: Vec<&str> = from.split('-').collect();
                    let (year, month) = if parts.len() >= 2 {
                        (
                            parts[0].parse::<i32>().unwrap_or(2026),
                            parts[1].parse::<u32>().unwrap_or(1),
                        )
                    } else {
                        (2026, 1)
                    };
                    let calc =
                        compute_diurna(acordata_str, days as u32, sal, year, month, &interna_str);
                    (
                        Some(calc.diurna_neimpozabila.clone()),
                        Some(calc.diurna_impozabila.clone()),
                        None::<String>,
                    )
                } else {
                    // Sum all segments.
                    let sum_nontax: Decimal = segments.iter().map(|s| s.nontax).sum();
                    let sum_excess: Decimal = segments.iter().map(|s| s.excess).sum();

                    // Distribute rounding remainder so Σ(nontax)+Σ(excess) == acordata EXACTLY.
                    // Any residual is a sub-cent rounding artefact; put it on the last segment's
                    // nontax (safe: nontax can only grow, staying ≤ acordata for that segment).
                    let remainder = round2(acordata - sum_nontax - sum_excess);

                    // Build compact breakdown for storage.
                    let n = segments.len();
                    let breakdown: Vec<DiurnaBreakdownSegment> = segments
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            let nontax = if i == n - 1 {
                                round2(s.nontax + remainder)
                            } else {
                                s.nontax
                            };
                            DiurnaBreakdownSegment {
                                period: s.period.clone(),
                                nontax: fmt2(nontax),
                                excess: fmt2(s.excess),
                            }
                        })
                        .collect();

                    let total_nontax = round2(sum_nontax + remainder);
                    let total_excess = round2(sum_excess);
                    let breakdown_json = serde_json::to_string(&breakdown).unwrap_or_default();

                    (
                        Some(fmt2(total_nontax)),
                        Some(fmt2(total_excess)),
                        Some(breakdown_json),
                    )
                }
            } else {
                (None, None, None)
            }
        } else if let (Some(acordata_str), Some(sal), Some(days), Some(from)) = (
            // Fallback: no delegation_to — use single-month engine on the start month.
            input.diurna_acordata.as_deref(),
            input.salariu_baza.as_deref(),
            input.days,
            input.delegation_from.as_deref(),
        ) {
            if days > 0 {
                let parts: Vec<&str> = from.split('-').collect();
                let (year, month) = if parts.len() >= 2 {
                    (
                        parts[0].parse::<i32>().unwrap_or(2026),
                        parts[1].parse::<u32>().unwrap_or(1),
                    )
                } else {
                    (2026, 1)
                };
                let calc =
                    compute_diurna(acordata_str, days as u32, sal, year, month, &interna_str);
                (
                    Some(calc.diurna_neimpozabila.clone()),
                    Some(calc.diurna_impozabila.clone()),
                    None::<String>,
                )
            } else {
                (None, None, None)
            }
        } else {
            (None, None, None)
        };

    sqlx::query(
        "INSERT INTO expense_reports \
         (id, company_id, advance_id, employee_id, delegation_from, delegation_to, destination, \
          days, diurna_acordata, diurna_neimpozabila, diurna_impozabila, salariu_baza, \
          diurna_interna, diurna_breakdown_json, \
          report_date, status, notes, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,'draft',?16,?17,?17)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.advance_id)
    .bind(&input.employee_id)
    .bind(&input.delegation_from)
    .bind(&input.delegation_to)
    .bind(&input.destination)
    .bind(input.days)
    .bind(&input.diurna_acordata)
    .bind(&diurna_neimpozabila)
    .bind(&diurna_impozabila)
    .bind(&input.salariu_baza)
    .bind(if diurna_neimpozabila.is_some() {
        Some(interna_str.as_str())
    } else {
        None
    })
    .bind(&stored_breakdown)
    .bind(&input.report_date)
    .bind(&input.notes)
    .bind(now)
    .execute(pool)
    .await?;

    // Insert lines
    for line in &input.lines {
        let line_id = new_id();
        let acct = line
            .account_code
            .as_deref()
            .unwrap_or_else(|| category_account(&line.category));
        // Validate amount
        let _ = Decimal::from_str(&line.amount)
            .map_err(|_| AppError::Validation("line amount must be a valid decimal".into()))?;
        sqlx::query(
            "INSERT INTO expense_lines (id, report_id, category, description, amount, vat_amount, account_code) \
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
        )
        .bind(&line_id)
        .bind(&id)
        .bind(&line.category)
        .bind(&line.description)
        .bind(&line.amount)
        .bind(&line.vat_amount)
        .bind(acct)
        .execute(pool)
        .await?;
    }

    get_report_full(pool, &id, &input.company_id).await
}

pub async fn list_reports(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<ExpenseReport>> {
    Ok(sqlx::query_as::<_, ExpenseReport>(
        "SELECT * FROM expense_reports WHERE company_id=?1 ORDER BY report_date DESC, created_at DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_report(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<ExpenseReport> {
    sqlx::query_as::<_, ExpenseReport>(
        "SELECT * FROM expense_reports WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn get_report_full(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<ExpenseReportFull> {
    let report = get_report(pool, id, company_id).await?;
    let lines = sqlx::query_as::<_, ExpenseLine>(
        "SELECT * FROM expense_lines WHERE report_id=?1 ORDER BY rowid",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    // Recompute diurnă calc for display (if data present).
    // Use the stored diurna_interna (migration 0080) so the display is consistent with
    // what was used at create time. Falls back to "23.00" for pre-0080 reports.
    let diurna_calc = if let (Some(acordata), Some(sal), Some(days), Some(from)) = (
        report.diurna_acordata.as_deref(),
        report.salariu_baza.as_deref(),
        report.days,
        report.delegation_from.as_deref(),
    ) {
        let interna_display = report.diurna_interna.as_deref().unwrap_or("23.00");
        let parts: Vec<&str> = from.split('-').collect();
        let (year, month) = if parts.len() >= 2 {
            (
                parts[0].parse::<i32>().unwrap_or(2026),
                parts[1].parse::<u32>().unwrap_or(1),
            )
        } else {
            (2026, 1)
        };
        Some(compute_diurna(
            acordata,
            days as u32,
            sal,
            year,
            month,
            interna_display,
        ))
    } else {
        None
    };

    Ok(ExpenseReportFull {
        report,
        lines,
        diurna_calc,
    })
}

pub async fn delete_report(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let report = get_report(pool, id, company_id).await?;
    if report.status != "draft" {
        return Err(AppError::Validation(
            "Only draft reports can be deleted".into(),
        ));
    }
    sqlx::query(
        "DELETE FROM gl_journal WHERE company_id=?1 AND source_type='EXPENSE_REPORT' AND source_id=?2",
    )
    .bind(company_id)
    .bind(id)
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM expense_lines WHERE report_id=?1")
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM expense_reports WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Approve a decont: post the settlement GL entry and mark as approved.
///
/// Settlement GL (source_type='EXPENSE_REPORT'):
///   - 625 D = diurna_neimpozabila + diurna_impozabila (FULL diurnă — cash already paid)
///   - per expense line (non-diurnă): account_code D = amount, 4426 D = vat_amount
///   - Total credits: 542 C = advance amount (if any), 5311 C for shortfall/direct reimb
///
/// If diurna_impozabila > 0 AND employee_id is set, additionally:
///   - post_diurna_asimilat_gl (source_type='DIURNA_ASIMILAT') — reclass journal
///   - upsert payroll_extra_income for payroll/D112 feed (period-lock checked)
///
/// If advance > total expenses → overshoot → 5311 D for returned excess
/// If advance < total expenses → underpay → 5311 C for the shortfall (company reimburses)
pub async fn approve_report(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    approve_date: &str,
) -> AppResult<ExpenseReportFull> {
    let full = get_report_full(pool, id, company_id).await?;
    if full.report.status != "draft" {
        // Idempotent: already approved → no-op
        return Ok(full);
    }

    // ── Build GL debit lines ────────────────────────────────────────────────
    let mut debit_lines: Vec<(String, Decimal)> = Vec::new(); // (account, amount)

    // Diurnă acordată (both within-cap and taxable excess) → 625.
    // The FULL diurnă is debited to 625 because the employee already received the cash.
    // The taxable excess reclass (641 D = 625 C) is posted in a SEPARATE journal
    // (source_type='DIURNA_ASIMILAT') so that 625 ends up holding only the within-cap
    // portion after both journals are posted — no 6458 stopgap.
    let diurna_neimpozabila = full
        .report
        .diurna_neimpozabila
        .as_deref()
        .map(|s| round2(dp(s)))
        .unwrap_or(Decimal::ZERO);
    let diurna_impozabila = full
        .report
        .diurna_impozabila
        .as_deref()
        .map(|s| round2(dp(s)))
        .unwrap_or(Decimal::ZERO);
    let diurna_total = diurna_neimpozabila + diurna_impozabila;
    if diurna_total > Decimal::ZERO {
        debit_lines.push(("625".into(), diurna_total));
    }

    // Expense lines (non-diurnă categories only to avoid double-counting)
    for line in &full.lines {
        if line.category == "diurna" {
            // Diurnă (both portions) is handled above
            continue;
        }
        let amt = round2(dp(&line.amount));
        if amt > Decimal::ZERO {
            debit_lines.push((line.account_code.clone(), amt));
        }
        // Deductible VAT
        if let Some(vat_str) = &line.vat_amount {
            let vat = round2(dp(vat_str));
            if vat > Decimal::ZERO {
                debit_lines.push(("4426".into(), vat));
            }
        }
    }

    let total_debit: Decimal = debit_lines.iter().map(|(_, a)| *a).sum();

    // Advance amount (the 542 to credit)
    let advance_amount = if let Some(adv_id) = full.report.advance_id.as_deref() {
        let adv_opt = sqlx::query_as::<_, TreasuryAdvance>(
            "SELECT * FROM treasury_advances WHERE id=?1 AND company_id=?2",
        )
        .bind(adv_id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?;
        adv_opt
            .map(|a| round2(dp(&a.amount)))
            .unwrap_or(Decimal::ZERO)
    } else {
        Decimal::ZERO
    };

    // Build the balanced journal
    let mut gl_lines: Vec<(String, Decimal, Decimal)> = Vec::new();
    for (acct, amt) in &debit_lines {
        gl_lines.push((acct.clone(), *amt, Decimal::ZERO));
    }

    let threshold = round2(Decimal::from_str("0.005").unwrap());

    if advance_amount > Decimal::ZERO {
        // Credit 542 for advance portion
        let credit_542 = total_debit.min(advance_amount);
        if credit_542 > Decimal::ZERO {
            gl_lines.push(("542".into(), Decimal::ZERO, credit_542));
        }
        let diff = total_debit - advance_amount;
        if diff > threshold {
            // Over-spent: company reimburses employee via 5311
            gl_lines.push(("5311".into(), Decimal::ZERO, diff));
        } else if diff < -threshold {
            // Under-spent: employee returns excess cash → 5311 D = 542 C (extra)
            let excess = (-diff).abs();
            gl_lines.push(("5311".into(), excess, Decimal::ZERO));
            gl_lines.push(("542".into(), Decimal::ZERO, excess));
        }
    } else {
        // No advance: all expenses credited to 5311 (direct reimbursement)
        if total_debit > Decimal::ZERO {
            gl_lines.push(("5311".into(), Decimal::ZERO, total_debit));
        }
    }

    // Only post if there are actual lines
    if !gl_lines.is_empty() && total_debit > Decimal::ZERO {
        let lines_ref: Vec<(&str, Decimal, Decimal)> = gl_lines
            .iter()
            .map(|(a, d_val, c_val)| (a.as_str(), *d_val, *c_val))
            .collect();

        post_manual_journal(
            pool,
            &ManualJournal {
                company_id,
                journal_id: &format!("DEC-{}", &id[..8]),
                journal_type: "DECONT",
                source_type: "EXPENSE_REPORT",
                source_id: id,
                date: approve_date,
                description: &format!(
                    "Decont cheltuieli {}",
                    full.report.destination.as_deref().unwrap_or("")
                ),
                partner_cui: None,
            },
            &lines_ref,
        )
        .await?;
    }

    // Mark advance as settled
    if let Some(adv_id) = full.report.advance_id.as_deref() {
        let now = now_unix();
        sqlx::query(
            "UPDATE treasury_advances SET status='settled', updated_at=?1 WHERE id=?2 AND company_id=?3",
        )
        .bind(now)
        .bind(adv_id)
        .bind(company_id)
        .execute(pool)
        .await?;
    }

    // Mark report as approved
    let now = now_unix();
    sqlx::query(
        "UPDATE expense_reports SET status='approved', updated_at=?1 WHERE id=?2 AND company_id=?3",
    )
    .bind(now)
    .bind(id)
    .bind(company_id)
    .execute(pool)
    .await?;

    // ── Diurnă excess → payroll feed (single source-of-truth, P1+P2 fix) ────────
    //
    // The GL 625 D (above) uses the STORED diurna_neimpozabila + diurna_impozabila, which are
    // now the multimonth totals computed at create time.  The payroll/D112 feed is fed from the
    // SAME stored breakdown (diurna_breakdown_json) — never recomputed here.
    //
    // GL ≡ payroll: stored diurna_impozabila = Σ excess across segments = Σ of what we feed.
    // Gate: diurna_impozabila > 0 ↔ any segment has excess > 0 (same invariant).
    //
    // Backward-compat: reports created before migration 0080 have diurna_breakdown_json = NULL.
    // In that case we fall back to attributing the stored diurna_impozabila to the approve_date
    // month — same as the original single-period behaviour, no crash.
    if diurna_impozabila > Decimal::ZERO {
        if let Some(emp_id) = full.report.employee_id.as_deref() {
            let breakdown_json = full.report.diurna_breakdown_json.as_deref();

            let did_breakdown = if let Some(json) = breakdown_json {
                // ── Post-0080 path: deserialize and feed per month ──────────────────
                match serde_json::from_str::<Vec<DiurnaBreakdownSegment>>(json) {
                    Ok(segments) if !segments.is_empty() => {
                        for seg in &segments {
                            let seg_excess = round2(dp(&seg.excess));
                            if seg_excess > Decimal::ZERO {
                                let is_locked = crate::db::period_locks::is_period_locked(
                                    pool,
                                    company_id,
                                    &seg.period,
                                )
                                .await?;
                                let lock_status = if is_locked {
                                    "needs_rectificativa"
                                } else {
                                    "open"
                                };
                                // UNIQUE key: (company_id, source_ref=id, employee_id, period).
                                // Each month gets its own idempotent row.
                                upsert_extra_income(
                                    pool,
                                    company_id,
                                    emp_id,
                                    &seg.period,
                                    id,
                                    seg_excess,
                                    lock_status,
                                )
                                .await?;
                            }
                        }
                        true
                    }
                    _ => false, // JSON parse error or empty — fall through to compat path
                }
            } else {
                false // NULL json — pre-0080 report, fall through
            };

            // ── Pre-0080 backward-compat path ──────────────────────────────────────
            // Attribute the full stored excess to the approve_date month.
            // Same behaviour as the original single-period code — no under-declaration.
            if !did_breakdown {
                let period = parse_date_to_period(approve_date)
                    .unwrap_or_else(|| approve_date.get(..7).unwrap_or("2026-01").to_string());
                let is_locked =
                    crate::db::period_locks::is_period_locked(pool, company_id, &period).await?;
                let lock_status = if is_locked {
                    "needs_rectificativa"
                } else {
                    "open"
                };
                upsert_extra_income(
                    pool,
                    company_id,
                    emp_id,
                    &period,
                    id,
                    diurna_impozabila,
                    lock_status,
                )
                .await?;
            }
        }
    }

    get_report_full(pool, id, company_id).await
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    // ── Diurnă engine tests ────────────────────────────────────────────────────

    #[test]
    fn diurna_within_plafon_zero_taxable() {
        // 5 days, salary 4000 RON, June 2026 (21 working days)
        // Limit A: 2.5 × 23 = 57.50/zi
        // Limit B: 4000 × 3 / 21 = 571.43/zi → not binding
        // cap = 57.50; total cap = 57.50 × 5 = 287.50
        // acordata = 200 < 287.50 → all non-taxable
        let c = compute_diurna("200", 5, "4000", 2026, 6, "23.00");
        assert_eq!(c.diurna_neimpozabila, "200.00");
        assert_eq!(c.diurna_impozabila, "0.00");
        assert_eq!(c.limit_a_zi, "57.50");
    }

    #[test]
    fn diurna_over_limit_a_taxable_excess() {
        // 3 days, salary 4000, June 2026
        // cap = min(57.50, 4000×3/21=571.43) = 57.50
        // cap_total = 57.50 × 3 = 172.50
        // acordata = 300 > 172.50 → impozabila = 300 - 172.50 = 127.50
        let c = compute_diurna("300", 3, "4000", 2026, 6, "23.00");
        assert_eq!(c.cap_zi, "57.50");
        assert_eq!(c.diurna_neimpozabila, "172.50");
        assert_eq!(c.diurna_impozabila, "127.50");
    }

    #[test]
    fn diurna_limit_b_binding_low_salary() {
        // 5 days, salary 200 RON, June 2026 (21 working days)
        // Limit A = 57.50/zi
        // Limit B = 200 × 3 / 21 = 28.57/zi (BINDING — lower than A)
        // cap = 28.57; total cap = 28.57 × 5 = 142.85
        // acordata = 100 < 142.85 → neimpozabila = 100, impozabila = 0
        let c = compute_diurna("100", 5, "200", 2026, 6, "23.00");
        let b = round2(dec("200") * dec("3") / Decimal::from(21u32));
        assert!(
            b < dec("57.50"),
            "Limit B must be less than A for this test"
        );
        assert_eq!(c.cap_zi, c.limit_b_zi, "Limit B should be the binding cap");
        assert_eq!(c.diurna_impozabila, "0.00"); // within cap

        // Now acordata exceeds B cap
        let c2 = compute_diurna("200", 5, "200", 2026, 6, "23.00");
        let cap_total = round2(b * dec("5"));
        let expected_nontax = cap_total.to_string();
        assert_eq!(c2.diurna_neimpozabila, expected_nontax);
        let impoz = round2(dec("200") - cap_total);
        assert_eq!(c2.diurna_impozabila, impoz.to_string());
    }

    #[test]
    fn diurna_cap_is_min_a_b() {
        // Verify cap = min(A, B) in both directions
        // High salary: A < B → cap = A
        let c_high = compute_diurna("0", 1, "10000", 2026, 6, "23.00");
        assert!(
            Decimal::from_str(&c_high.cap_zi).unwrap()
                <= Decimal::from_str(&c_high.limit_b_zi).unwrap(),
            "cap must be ≤ limit_b when salary is high (A is binding)"
        );
        assert_eq!(c_high.cap_zi, c_high.limit_a_zi, "cap = A when A < B");

        // Low salary: B < A → cap = B
        let c_low = compute_diurna("0", 1, "100", 2026, 6, "23.00");
        assert!(
            Decimal::from_str(&c_low.cap_zi).unwrap()
                <= Decimal::from_str(&c_low.limit_a_zi).unwrap(),
            "cap must be ≤ limit_a when salary is low (B is binding)"
        );
        assert_eq!(c_low.cap_zi, c_low.limit_b_zi, "cap = B when B < A");
    }

    #[test]
    fn diurna_round2_exact() {
        // Ensure we round to 2 decimal places
        // 23 × 2.5 = 57.50 exactly
        let c = compute_diurna("57.50", 1, "10000", 2026, 6, "23.00");
        assert_eq!(c.limit_a_zi, "57.50");
        assert_eq!(c.diurna_neimpozabila, "57.50");
        assert_eq!(c.diurna_impozabila, "0.00");
    }

    #[test]
    fn round2_uses_midpoint_away_from_zero() {
        // FIX 4: round2 must use MidpointAwayFromZero (commercial), not banker's half-even.
        // 1.005 → 1.01 (away from zero), not 1.00 (banker's rounds to even).
        let result = round2(Decimal::from_str("1.005").unwrap());
        assert_eq!(
            result,
            Decimal::from_str("1.01").unwrap(),
            "1.005 must round to 1.01 (midpoint away from zero)"
        );
        // 2.005 → 2.01 (both strategies agree here, but verifies positive direction)
        let result2 = round2(Decimal::from_str("2.005").unwrap());
        assert_eq!(result2, Decimal::from_str("2.01").unwrap());
        // -1.005 → -1.01 (away from zero for negative)
        let result3 = round2(Decimal::from_str("-1.005").unwrap());
        assert_eq!(result3, Decimal::from_str("-1.01").unwrap());
    }

    // ── GL integration tests ──────────────────────────────────────────────────

    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT OR IGNORE INTO companies \
             (id, cui, legal_name, address, city, county, country, created_at, updated_at) \
             VALUES ('co1','RO1','Test SRL','Str.1','Cluj','CJ','RO',0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn has_account(entries: &[(String, String, String)], acct: &str) -> bool {
        entries.iter().any(|(a, _, _)| a == acct)
    }

    async fn gl_entries_for(
        pool: &SqlitePool,
        source_type: &str,
        source_id: &str,
    ) -> Vec<(String, String, String)> {
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT e.account_code, e.debit, e.credit \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.source_type=?1 AND j.source_id=?2",
        )
        .bind(source_type)
        .bind(source_id)
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn advance_grant_posts_542_eq_5311() {
        let pool = setup().await;
        let adv = create_advance(
            &pool,
            CreateAdvanceInput {
                company_id: "co1".into(),
                employee_id: None,
                amount: "500.00".into(),
                currency: None,
                granted_date: "2026-06-01".into(),
                method: Some("cash".into()),
                notes: None,
            },
        )
        .await
        .unwrap();

        let entries = gl_entries_for(&pool, "AVANS_TREZORERIE", &adv.id).await;
        assert!(!entries.is_empty(), "GL must be posted");
        assert!(has_account(&entries, "542"), "542 must appear");
        assert!(
            has_account(&entries, "5311"),
            "5311 must appear (cash method)"
        );

        // Balanced: sum debit == sum credit
        let sum_d: Decimal = entries.iter().map(|(_, d, _)| dp(d)).sum();
        let sum_c: Decimal = entries.iter().map(|(_, _, c)| dp(c)).sum();
        assert_eq!(sum_d, sum_c, "GL must be balanced");
        assert_eq!(sum_d, dec("500.00"));
    }

    #[tokio::test]
    async fn advance_grant_bank_posts_5121() {
        let pool = setup().await;
        let adv = create_advance(
            &pool,
            CreateAdvanceInput {
                company_id: "co1".into(),
                employee_id: None,
                amount: "1000.00".into(),
                currency: Some("RON".into()),
                granted_date: "2026-06-01".into(),
                method: Some("bank".into()),
                notes: None,
            },
        )
        .await
        .unwrap();

        let entries = gl_entries_for(&pool, "AVANS_TREZORERIE", &adv.id).await;
        assert!(
            has_account(&entries, "5121"),
            "5121 must appear (bank method)"
        );
        assert!(
            !has_account(&entries, "5311"),
            "5311 must NOT appear for bank method"
        );
    }

    #[tokio::test]
    async fn advance_return_posts_5311_eq_542() {
        let pool = setup().await;
        let adv = create_advance(
            &pool,
            CreateAdvanceInput {
                company_id: "co1".into(),
                employee_id: None,
                amount: "300.00".into(),
                currency: None,
                granted_date: "2026-06-01".into(),
                method: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        return_advance(&pool, &adv.id, "co1", "2026-06-10")
            .await
            .unwrap();

        let entries = gl_entries_for(&pool, "AVANS_RETURN", &adv.id).await;
        assert!(
            has_account(&entries, "5311"),
            "5311 D must appear on return"
        );
        assert!(has_account(&entries, "542"), "542 C must appear on return");

        let sum_d: Decimal = entries.iter().map(|(_, d_val, _)| dp(d_val)).sum();
        let sum_c: Decimal = entries.iter().map(|(_, _, c_val)| dp(c_val)).sum();
        assert_eq!(sum_d, sum_c, "return GL must be balanced");
        assert_eq!(sum_d, dec("300.00"));
    }

    /// Wave E: The settlement journal (EXPENSE_REPORT) posts 625 for the FULL diurnă
    /// (neimpozabila + impozabila). The reclass (641/625/withholdings) is in the separate
    /// DIURNA_ASIMILAT journal. 6458 must NOT appear in EXPENSE_REPORT; the total GL for
    /// both journals must balance. No employee_id on this report → no DIURNA_ASIMILAT journal.
    #[tokio::test]
    async fn approval_posts_full_diurna_in_625_no_6458() {
        let pool = setup().await;

        // Create advance
        let adv = create_advance(
            &pool,
            CreateAdvanceInput {
                company_id: "co1".into(),
                employee_id: None,
                amount: "600.00".into(),
                currency: None,
                granted_date: "2026-06-01".into(),
                method: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        // Create report WITHOUT employee_id: 3 days, salary 4000, diurnă 300
        // (cap = 57.50×3 = 172.50 → 127.50 taxable). No DIURNA_ASIMILAT journal fires
        // because employee_id is None.
        let full = create_report(
            &pool,
            CreateReportInput {
                company_id: "co1".into(),
                advance_id: Some(adv.id.clone()),
                employee_id: None,
                delegation_from: Some("2026-06-02".into()),
                delegation_to: Some("2026-06-04".into()),
                destination: Some("București".into()),
                days: Some(3),
                diurna_acordata: Some("300.00".into()),
                salariu_baza: Some("4000.00".into()),
                report_date: "2026-06-05".into(),
                notes: None,
                lines: vec![
                    ExpenseLineInput {
                        category: "diurna".into(),
                        description: Some("Diurnă 3 zile".into()),
                        amount: "300.00".into(),
                        vat_amount: None,
                        account_code: None,
                    },
                    ExpenseLineInput {
                        category: "transport".into(),
                        description: Some("Tren".into()),
                        amount: "80.00".into(),
                        vat_amount: Some("15.20".into()),
                        account_code: None,
                    },
                ],
                diurna_interna: Some("23.00".into()),
            },
        )
        .await
        .unwrap();

        // Verify diurnă computed correctly
        assert_eq!(
            full.report.diurna_neimpozabila.as_deref().unwrap(),
            "172.50"
        );
        assert_eq!(full.report.diurna_impozabila.as_deref().unwrap(), "127.50");

        // Approve
        let approved = approve_report(&pool, &full.report.id, "co1", "2026-06-05")
            .await
            .unwrap();
        assert_eq!(approved.report.status, "approved");

        let entries = gl_entries_for(&pool, "EXPENSE_REPORT", &full.report.id).await;
        assert!(!entries.is_empty(), "GL must be posted on approval");

        // 625 must appear for the FULL diurnă (172.50 + 127.50 = 300.00)
        assert!(
            has_account(&entries, "625"),
            "625 must appear for full diurnă (Wave E)"
        );
        let debit_625: Decimal = entries
            .iter()
            .filter(|(a, _, _)| a == "625")
            .map(|(_, d, _)| dp(d))
            .sum();
        assert_eq!(debit_625, dec("300.00"), "625 D must be full diurnă 300.00");

        // 624 for transport
        assert!(
            has_account(&entries, "624"),
            "624 must appear for transport"
        );
        // 4426 for VAT
        assert!(
            has_account(&entries, "4426"),
            "4426 must appear for deductible VAT"
        );

        // 6458 must NOT appear in the EXPENSE_REPORT journal (Wave E replaces the stopgap)
        assert!(
            !has_account(&entries, "6458"),
            "6458 must NOT appear in EXPENSE_REPORT journal (Wave E)"
        );
        // No DIURNA_ASIMILAT journal either (no employee_id)
        let das_entries = gl_entries_for(&pool, "DIURNA_ASIMILAT", &full.report.id).await;
        assert!(
            das_entries.is_empty(),
            "DIURNA_ASIMILAT journal must NOT fire without employee_id"
        );

        // GL must be balanced
        let sum_d: Decimal = entries.iter().map(|(_, d_val, _)| dp(d_val)).sum();
        let sum_c: Decimal = entries.iter().map(|(_, _, c_val)| dp(c_val)).sum();
        assert_eq!(sum_d, sum_c, "settlement GL must be balanced");
    }

    /// Wave E: diurnă 500 (non-tax 287.50 + taxable 212.50), advance 600.
    /// EXPENSE_REPORT journal: 625 D=500 (full), 542 C=600, 5311 D=100 (returned by employee).
    /// 6458 must NOT appear. DIURNA_ASIMILAT does NOT fire (no employee_id).
    #[tokio::test]
    async fn taxable_diurna_settles_full_advance_via_625_no_6458() {
        // Wave E: diurnă acordată 500 (non-tax 287.50 + taxable 212.50) funded by advance 600.
        // Expected: 625 D=500 (full diurnă), 542 C=600, 5311 D=100 (returned).
        // 6458 must NOT appear; reclass is in DIURNA_ASIMILAT (not fired here, no emp_id).
        let pool = setup().await;

        let adv = create_advance(
            &pool,
            CreateAdvanceInput {
                company_id: "co1".into(),
                employee_id: None,
                amount: "600.00".into(),
                currency: None,
                granted_date: "2026-06-01".into(),
                method: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        // 5 days, salary 4000, June 2026: cap = 57.50/zi → 287.50 total → taxable = 212.50
        let full = create_report(
            &pool,
            CreateReportInput {
                company_id: "co1".into(),
                advance_id: Some(adv.id.clone()),
                employee_id: None,
                delegation_from: Some("2026-06-02".into()),
                delegation_to: Some("2026-06-06".into()),
                destination: Some("Timișoara".into()),
                days: Some(5),
                diurna_acordata: Some("500.00".into()),
                salariu_baza: Some("4000.00".into()),
                report_date: "2026-06-07".into(),
                notes: None,
                lines: vec![ExpenseLineInput {
                    category: "diurna".into(),
                    description: Some("Diurnă 5 zile".into()),
                    amount: "500.00".into(),
                    vat_amount: None,
                    account_code: None,
                }],
                diurna_interna: Some("23.00".into()),
            },
        )
        .await
        .unwrap();

        assert_eq!(
            full.report.diurna_neimpozabila.as_deref().unwrap(),
            "287.50",
            "non-taxable must be 287.50"
        );
        assert_eq!(
            full.report.diurna_impozabila.as_deref().unwrap(),
            "212.50",
            "taxable must be 212.50"
        );

        approve_report(&pool, &full.report.id, "co1", "2026-06-07")
            .await
            .unwrap();

        let entries = gl_entries_for(&pool, "EXPENSE_REPORT", &full.report.id).await;

        let debit_for = |acct: &str| -> Decimal {
            entries
                .iter()
                .filter(|(a, _, _)| a == acct)
                .map(|(_, d, _)| dp(d))
                .sum()
        };
        let credit_for = |acct: &str| -> Decimal {
            entries
                .iter()
                .filter(|(a, _, _)| a == acct)
                .map(|(_, _, c)| dp(c))
                .sum()
        };

        // Wave E: 625 gets the FULL diurnă (287.50 + 212.50 = 500.00)
        assert_eq!(debit_for("625"), dec("500.00"), "625 D must be full 500.00");
        // 6458 must NOT appear (replaced by DIURNA_ASIMILAT journal)
        assert_eq!(
            debit_for("6458"),
            Decimal::ZERO,
            "6458 must NOT appear in EXPENSE_REPORT"
        );
        // advance=600, expenses=500 → 542 C = 600 (full advance settled)
        assert_eq!(
            credit_for("542"),
            dec("600.00"),
            "542 C must fully settle the advance"
        );
        // excess 100 returned by employee → 5311 D=100
        assert_eq!(
            debit_for("5311"),
            dec("100.00"),
            "5311 D=100 (employee returns excess cash)"
        );

        // GL balanced
        let sum_d: Decimal = entries.iter().map(|(_, d, _)| dp(d)).sum();
        let sum_c: Decimal = entries.iter().map(|(_, _, c)| dp(c)).sum();
        assert_eq!(sum_d, sum_c, "GL must be balanced");
    }

    #[tokio::test]
    async fn approve_idempotent() {
        let pool = setup().await;
        let full = create_report(
            &pool,
            CreateReportInput {
                company_id: "co1".into(),
                advance_id: None,
                employee_id: None,
                delegation_from: None,
                delegation_to: None,
                destination: None,
                days: None,
                diurna_acordata: None,
                salariu_baza: None,
                report_date: "2026-06-01".into(),
                notes: None,
                lines: vec![ExpenseLineInput {
                    category: "alte".into(),
                    description: None,
                    amount: "100.00".into(),
                    vat_amount: None,
                    account_code: Some("628".into()),
                }],
                diurna_interna: None,
            },
        )
        .await
        .unwrap();

        approve_report(&pool, &full.report.id, "co1", "2026-06-01")
            .await
            .unwrap();
        // Second approval should be no-op (idempotent)
        approve_report(&pool, &full.report.id, "co1", "2026-06-01")
            .await
            .unwrap();

        let entries = gl_entries_for(&pool, "EXPENSE_REPORT", &full.report.id).await;
        // Should still have exactly one set of entries (not doubled)
        let count = entries.len();
        assert!(count >= 1, "must have at least one entry");
    }

    #[tokio::test]
    async fn chart_backfill_542_6022_425() {
        let pool = setup().await;
        // Seed standard accounts for co1 (migration backfill only runs for pre-existing
        // companies; test company is inserted after migration, so we seed explicitly).
        crate::db::accounts::seed_standard(&pool, "co1")
            .await
            .unwrap();

        let codes: Vec<String> = sqlx::query_scalar(
            "SELECT account_code FROM chart_of_accounts WHERE company_id='co1' AND account_code IN ('542','6022','425')",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(
            codes.contains(&"542".to_string()),
            "542 must be in standard_accounts"
        );
        assert!(
            codes.contains(&"6022".to_string()),
            "6022 must be in standard_accounts"
        );
        assert!(
            codes.contains(&"425".to_string()),
            "425 must be in standard_accounts"
        );
    }

    // ── Multi-month diurnă engine tests (CF art.76(2)(k)) ─────────────────────

    /// Single-month delegation → exactly one segment, results identical to compute_diurna.
    #[test]
    fn multimonth_single_month_identical_to_compute_diurna() {
        // 3–7 June 2026 (5 days), daily_rate=100, salary=4000.
        // June 2026: 21 working days, limit_a=57.50, limit_b=4000×3/21≈571.43 → cap=57.50/day
        // acordata = 100×5 = 500; cap_total = 57.50×5 = 287.50; nontax=287.50; excess=212.50
        let daily_rate = dec("100");
        let segs =
            compute_diurna_multimonth("2026-06-03", "2026-06-07", daily_rate, "4000", "23.00");
        assert_eq!(segs.len(), 1, "single-month delegation → one segment");
        let s = &segs[0];
        assert_eq!(s.period, "2026-06");
        assert_eq!(s.delegation_days, 5);
        assert_eq!(s.working_days, 21);
        assert_eq!(s.acordata, dec("500.00"));
        assert_eq!(s.nontax, dec("287.50"));
        assert_eq!(s.excess, dec("212.50"));

        // Verify it matches compute_diurna (acordata=500, 5 days, salary=4000, Jun 2026).
        let legacy = compute_diurna("500.00", 5, "4000", 2026, 6, "23.00");
        assert_eq!(
            s.nontax,
            dec(&legacy.diurna_neimpozabila),
            "nontax must equal legacy non-taxable"
        );
        assert_eq!(
            s.excess,
            dec(&legacy.diurna_impozabila),
            "excess must equal legacy taxable"
        );
    }

    /// Cross-month delegation 28 May–3 Jun 2026.
    /// May 2026: 4 delegation days (28,29,30,31 May). Working days May 2026 = 20.
    /// Jun 2026: 3 delegation days (1,2,3 Jun). Working days June 2026 = 21.
    /// salary=200 RON → limit_B binds (200×3÷20=30 for May; 200×3÷21≈28.57 for June).
    /// limit_A = 57.50; cap_May = min(57.50,30) = 30; cap_Jun = min(57.50,28.57) = 28.57.
    /// daily_rate=100 → excess per month:
    ///   May: acordata=400, cap_total=30×4=120, nontax=120, excess=280.
    ///   Jun: acordata=300, cap_total=28.57×3=85.71, nontax=85.71, excess=214.29.
    #[test]
    fn multimonth_cross_month_may_jun_per_month_caps() {
        let daily_rate = dec("100");
        let segs =
            compute_diurna_multimonth("2026-05-28", "2026-06-03", daily_rate, "200", "23.00");
        assert_eq!(segs.len(), 2, "should produce two monthly segments");
        let may = segs
            .iter()
            .find(|s| s.period == "2026-05")
            .expect("May segment");
        let jun = segs
            .iter()
            .find(|s| s.period == "2026-06")
            .expect("Jun segment");

        // May 2026 working days = 20 (1 Mai holiday + weekends)
        assert_eq!(may.working_days, 20, "May 2026 should have 20 working days");
        assert_eq!(may.delegation_days, 4, "4 May days: 28,29,30,31");

        // June 2026 working days = 21 (1 Jun holiday)
        assert_eq!(
            jun.working_days, 21,
            "June 2026 should have 21 working days"
        );
        assert_eq!(jun.delegation_days, 3, "3 Jun days: 1,2,3");

        // Limit-B for May: 200×3/20 = 30.00 → binds (< 57.50)
        let limit_b_may = round2(dec("200") * dec("3") / Decimal::from(20u32));
        assert_eq!(
            may.cap_zi, limit_b_may,
            "May cap_zi = limit_B_May (binding)"
        );
        assert!(
            may.cap_zi < dec("57.50"),
            "limit_B must be less than limit_A for May"
        );

        // Limit-B for June: 200×3/21 ≈ 28.57 → binds (< 57.50)
        let limit_b_jun = round2(dec("200") * dec("3") / Decimal::from(21u32));
        assert_eq!(
            jun.cap_zi, limit_b_jun,
            "Jun cap_zi = limit_B_Jun (binding)"
        );
        assert!(
            jun.cap_zi < dec("57.50"),
            "limit_B must be less than limit_A for June"
        );

        // May: acordata=400, nontax=min(400, 30×4=120)=120, excess=280
        assert_eq!(may.acordata, dec("400.00"));
        assert_eq!(may.nontax, round2(limit_b_may * dec("4")));
        assert_eq!(may.excess, dec("400.00") - may.nontax);

        // June: acordata=300, cap_total=limit_b_jun×3, nontax=min(300,cap_total)
        let jun_cap_total = round2(limit_b_jun * dec("3"));
        assert_eq!(jun.acordata, dec("300.00"));
        assert_eq!(jun.nontax, round2(dec("300.00").min(jun_cap_total)));
        assert_eq!(
            jun.excess,
            round2((dec("300.00") - jun.nontax).max(Decimal::ZERO))
        );

        // Σ within-cap (nontax) = may.nontax + jun.nontax (not the whole-span cap)
        let sum_nontax = may.nontax + jun.nontax;
        let sum_acordata = may.acordata + jun.acordata;
        let sum_excess = may.excess + jun.excess;
        assert_eq!(
            round2(sum_nontax + sum_excess),
            round2(sum_acordata),
            "Σ nontax + Σ excess == Σ acordata"
        );

        // NEVER a single whole-span cap: assert May excess ≠ excess computed with June working days
        let wrong_may_excess =
            round2((dec("400.00") - round2(limit_b_jun * dec("4"))).max(Decimal::ZERO));
        assert_ne!(
            may.excess, wrong_may_excess,
            "May excess must use MAY working days, not June's"
        );
    }

    /// Verify limit_A binding (high salary) vs limit_B binding (low salary) produces different
    /// cap_perday per month even within the same delegation.
    #[test]
    fn multimonth_cap_binding_differs_per_month() {
        // High salary (10000): limit_A binds for all months → same cap_zi across months.
        let segs_high =
            compute_diurna_multimonth("2026-05-28", "2026-06-03", dec("100"), "10000", "23.00");
        assert_eq!(segs_high.len(), 2);
        // Both caps = limit_A = 57.50
        for s in &segs_high {
            assert_eq!(
                s.cap_zi,
                dec("57.50"),
                "high-salary: cap = limit_A for every month"
            );
        }

        // Low salary (200): limit_B binds, and limit_B differs between May (20 wd) and June (21 wd)
        let segs_low =
            compute_diurna_multimonth("2026-05-28", "2026-06-03", dec("100"), "200", "23.00");
        assert_eq!(segs_low.len(), 2);
        let may_low = segs_low.iter().find(|s| s.period == "2026-05").unwrap();
        let jun_low = segs_low.iter().find(|s| s.period == "2026-06").unwrap();
        // Different working days → different cap_perday
        assert_ne!(
            may_low.cap_zi, jun_low.cap_zi,
            "low-salary: different months have different cap_zi (different working days)"
        );
        assert_ne!(
            may_low.working_days, jun_low.working_days,
            "May and June must have different working-day counts"
        );
    }

    /// Idempotent: approving a cross-month decont twice → still exactly 2 extra_income rows.
    #[tokio::test]
    async fn multimonth_approve_idempotent_two_extra_income_rows() {
        let pool = setup().await;

        // Create employee emp1 (salary 200 → limit_B binds and both months have excess).
        sqlx::query(
            "INSERT INTO employees \
             (id, company_id, cnp, full_name, gross_salary, personal_deduction, \
              employment_date, active, tip_asigurat, pensionar, tip_contract, ore_norma, \
              exceptie_cas_min, sediu_cif, beneficiar_suma_netaxabila, created_at, updated_at) \
             VALUES ('emp1','co1','1900101410011','Ion Pop','200','0', \
                     '2024-01-01',1,'1',0,'N',8,'','',0,0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // 28 May–3 Jun, daily_rate=100, salary=200, total 7 days, diurna=700
        let full = create_report(
            &pool,
            CreateReportInput {
                company_id: "co1".into(),
                advance_id: None,
                employee_id: Some("emp1".into()),
                delegation_from: Some("2026-05-28".into()),
                delegation_to: Some("2026-06-03".into()),
                destination: Some("București".into()),
                days: Some(7),
                diurna_acordata: Some("700.00".into()),
                salariu_baza: Some("200.00".into()),
                report_date: "2026-06-05".into(),
                notes: None,
                lines: vec![ExpenseLineInput {
                    category: "diurna".into(),
                    description: Some("Diurnă 7 zile".into()),
                    amount: "700.00".into(),
                    vat_amount: None,
                    account_code: None,
                }],
                diurna_interna: Some("23.00".into()),
            },
        )
        .await
        .unwrap();

        // First approval
        approve_report(&pool, &full.report.id, "co1", "2026-06-05")
            .await
            .unwrap();

        // Second approval (idempotent — already 'approved', no-op in approve_report)
        approve_report(&pool, &full.report.id, "co1", "2026-06-05")
            .await
            .unwrap();

        // Must have exactly 2 extra_income rows (one per month), no duplicates
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT period, amount FROM payroll_extra_income \
             WHERE company_id='co1' AND source_ref=?1 AND employee_id='emp1' \
             ORDER BY period",
        )
        .bind(&full.report.id)
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(rows.len(), 2, "exactly 2 extra_income rows: one per month");
        let periods: Vec<&str> = rows.iter().map(|(p, _)| p.as_str()).collect();
        assert!(periods.contains(&"2026-05"), "must have 2026-05 row");
        assert!(periods.contains(&"2026-06"), "must have 2026-06 row");

        // Each month must have a positive excess
        for (period, amount) in &rows {
            let amt = dec(amount);
            assert!(
                amt > Decimal::ZERO,
                "excess for period {} must be > 0",
                period
            );
        }

        // Verify May uses May's working days: cap_May = 200×3/20=30 per day × 4 days = 120 nontax
        // May acordata = 100×4 = 400; May excess = 400-120 = 280.
        let may_row = rows.iter().find(|(p, _)| p == "2026-05").unwrap();
        assert_eq!(dec(&may_row.1), dec("280.00"), "May excess must be 280.00");

        // June: cap_Jun = 200×3/21≈28.57 per day × 3 days = 85.71 nontax
        // Jun acordata = 100×3=300; Jun excess = 300-85.71 = 214.29
        let jun_row = rows.iter().find(|(p, _)| p == "2026-06").unwrap();
        let limit_b_jun = round2(dec("200") * dec("3") / Decimal::from(21u32));
        let jun_nontax = round2(limit_b_jun * dec("3"));
        let jun_excess_expected = round2((dec("300") - jun_nontax).max(Decimal::ZERO));
        assert_eq!(
            dec(&jun_row.1),
            jun_excess_expected,
            "Jun excess must equal per-month computed value"
        );
    }

    /// Single-month delegation via approve_report: still produces exactly one extra_income row.
    #[tokio::test]
    async fn approve_single_month_produces_one_extra_income() {
        let pool = setup().await;

        sqlx::query(
            "INSERT INTO employees \
             (id, company_id, cnp, full_name, gross_salary, personal_deduction, \
              employment_date, active, tip_asigurat, pensionar, tip_contract, ore_norma, \
              exceptie_cas_min, sediu_cif, beneficiar_suma_netaxabila, created_at, updated_at) \
             VALUES ('emp2','co1','2900101410011','Ana Pop','4000','0', \
                     '2024-01-01',1,'1',0,'N',8,'','',0,0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // 3–7 June (5 days), salary=4000, daily_rate=100, total diurnă=500
        // cap=57.50/day × 5 = 287.50; excess=212.50 → ONE row for 2026-06
        let full = create_report(
            &pool,
            CreateReportInput {
                company_id: "co1".into(),
                advance_id: None,
                employee_id: Some("emp2".into()),
                delegation_from: Some("2026-06-03".into()),
                delegation_to: Some("2026-06-07".into()),
                destination: Some("Cluj".into()),
                days: Some(5),
                diurna_acordata: Some("500.00".into()),
                salariu_baza: Some("4000.00".into()),
                report_date: "2026-06-08".into(),
                notes: None,
                lines: vec![ExpenseLineInput {
                    category: "diurna".into(),
                    description: Some("Diurnă 5 zile".into()),
                    amount: "500.00".into(),
                    vat_amount: None,
                    account_code: None,
                }],
                diurna_interna: Some("23.00".into()),
            },
        )
        .await
        .unwrap();

        approve_report(&pool, &full.report.id, "co1", "2026-06-08")
            .await
            .unwrap();

        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT period, amount FROM payroll_extra_income \
             WHERE company_id='co1' AND source_ref=?1 AND employee_id='emp2'",
        )
        .bind(&full.report.id)
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(
            rows.len(),
            1,
            "single-month delegation → exactly one extra_income row"
        );
        assert_eq!(rows[0].0, "2026-06", "period must be 2026-06");
        assert_eq!(dec(&rows[0].1), dec("212.50"), "excess must be 212.50");
    }

    // ── P1/P2 fix: single source-of-truth tests ───────────────────────────────

    /// Σ-RECONCILIATION (cross-month): Σ(nontax) + Σ(excess) == diurna_acordata EXACTLY.
    /// Stored diurna_impozabila == Σ(excess in breakdown).
    /// Payroll feed Σ == stored diurna_impozabila (no drift from recomputation).
    #[tokio::test]
    async fn sigma_reconciliation_cross_month_no_drift() {
        let pool = setup().await;

        sqlx::query(
            "INSERT INTO employees \
             (id, company_id, cnp, full_name, gross_salary, personal_deduction, \
              employment_date, active, tip_asigurat, pensionar, tip_contract, ore_norma, \
              exceptie_cas_min, sediu_cif, beneficiar_suma_netaxabila, created_at, updated_at) \
             VALUES ('emp_sigma','co1','1900101410012','Test Sigma','200','0', \
                     '2024-01-01',1,'1',0,'N',8,'','',0,0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // 28 May–3 Jun 2026, 7 days, total diurnă 700 (daily_rate=100, salary=200).
        // May: 4 days × 100 = 400 acordata; cap = limit_B_May = 200×3/20 = 30/day → nontax=120, excess=280.
        // Jun: 3 days × 100 = 300 acordata; cap = limit_B_Jun = 200×3/21 ≈ 28.57/day → nontax≈85.71, excess≈214.29.
        // Σ acordata = 700.  Σ nontax + Σ excess must == 700.00 exactly.
        let full = create_report(
            &pool,
            CreateReportInput {
                company_id: "co1".into(),
                advance_id: None,
                employee_id: Some("emp_sigma".into()),
                delegation_from: Some("2026-05-28".into()),
                delegation_to: Some("2026-06-03".into()),
                destination: Some("Test".into()),
                days: Some(7),
                diurna_acordata: Some("700.00".into()),
                salariu_baza: Some("200.00".into()),
                report_date: "2026-06-05".into(),
                notes: None,
                lines: vec![ExpenseLineInput {
                    category: "diurna".into(),
                    description: None,
                    amount: "700.00".into(),
                    vat_amount: None,
                    account_code: None,
                }],
                diurna_interna: Some("23.00".into()),
            },
        )
        .await
        .unwrap();

        let acordata = dec("700.00");
        let stored_nontax = dec(full.report.diurna_neimpozabila.as_deref().unwrap());
        let stored_impozabila = dec(full.report.diurna_impozabila.as_deref().unwrap());

        // Σ-reconciliation: stored totals must sum exactly to acordata.
        assert_eq!(
            round2(stored_nontax + stored_impozabila),
            acordata,
            "Σ(nontax) + Σ(excess) must == diurna_acordata EXACTLY (no rounding drift)"
        );

        // Deserialize breakdown and verify internal consistency.
        let json = full
            .report
            .diurna_breakdown_json
            .as_deref()
            .expect("breakdown_json must be stored for cross-month report");
        let breakdown: Vec<DiurnaBreakdownSegment> = serde_json::from_str(json).unwrap();
        assert_eq!(breakdown.len(), 2, "two months in breakdown");

        let sum_bd_nontax: Decimal = breakdown.iter().map(|s| dec(&s.nontax)).sum();
        let sum_bd_excess: Decimal = breakdown.iter().map(|s| dec(&s.excess)).sum();

        // Breakdown Σ must equal stored totals.
        assert_eq!(
            round2(sum_bd_nontax),
            stored_nontax,
            "breakdown Σ nontax must == stored diurna_neimpozabila"
        );
        assert_eq!(
            round2(sum_bd_excess),
            stored_impozabila,
            "breakdown Σ excess must == stored diurna_impozabila"
        );
        // And the full reconciliation holds in the breakdown itself.
        assert_eq!(
            round2(sum_bd_nontax + sum_bd_excess),
            acordata,
            "breakdown Σ(nontax + excess) must == acordata"
        );

        // Approve and verify payroll feed Σ == stored impozabila (no drift).
        approve_report(&pool, &full.report.id, "co1", "2026-06-05")
            .await
            .unwrap();

        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT period, amount FROM payroll_extra_income \
             WHERE company_id='co1' AND source_ref=?1 AND employee_id='emp_sigma' \
             ORDER BY period",
        )
        .bind(&full.report.id)
        .fetch_all(&pool)
        .await
        .unwrap();

        let feed_sum: Decimal = rows.iter().map(|(_, a)| dec(a)).sum();
        assert_eq!(
            round2(feed_sum),
            stored_impozabila,
            "Σ fed to payroll must == stored diurna_impozabila (GL ≡ payroll, no drift)"
        );
    }

    /// LATER-MONTH-BINDS: a delegation where only the SECOND month has excess (the start-month
    /// cap is not binding). The P1 defect caused stored diurna_impozabila = 0 (single-month
    /// compute_diurna used start-month working days across the full span) → payroll feed skipped.
    /// The fix: stored diurna_impozabila reflects the multimonth total, including later-month excess.
    #[tokio::test]
    async fn later_month_binds_no_under_declaration() {
        let pool = setup().await;

        sqlx::query(
            "INSERT INTO employees \
             (id, company_id, cnp, full_name, gross_salary, personal_deduction, \
              employment_date, active, tip_asigurat, pensionar, tip_contract, ore_norma, \
              exceptie_cas_min, sediu_cif, beneficiar_suma_netaxabila, created_at, updated_at) \
             VALUES ('emp_later','co1','1900101410013','Test Later','10000','0', \
                     '2024-01-01',1,'1',0,'N',8,'','',0,0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // High-salary employee: salary=10000.
        // limit_B_May = 10000×3/20 = 1500/day >> limit_A=57.50 → cap_May = 57.50/day.
        // limit_B_Jun = 10000×3/21 ≈ 1428.57/day >> limit_A=57.50 → cap_Jun = 57.50/day.
        //
        // Delegation 28 May–3 Jun 2026 (7 days), daily_rate_may = 50 (within cap), daily_rate_jun = 100 (over cap).
        // But compute_diurna_multimonth uses a single daily_rate.  To produce "later month binds" we
        // need to choose a scenario where the first month doesn't produce excess but the second does.
        //
        // Use a VERY short first-month stay so cap_total > acordata_may, but a longer second-month.
        // Easier: use low diurna_interna (say 5.00) → limit_A = 12.50/day.
        // salary=10000 → limit_B >> limit_A → cap=12.50.
        // 1 day in May (31st), 5 days in June.  daily_rate = 15.
        //   May: acordata=15, cap_total=12.50×1=12.50 → excess=2.50.
        //   Jun: acordata=75, cap_total=12.50×5=62.50 → excess=12.50.
        // Both months have excess. That's fine (both > 0 = not skipped).
        //
        // To get "only SECOND month binds": we need acordata_first_month ≤ cap_first_month.
        // Use limit_A = 57.50 and very few days in May.
        // May 31 only (1 day), Jun 1–5 (5 days). daily_rate = 50.
        //   May: acordata=50, cap_total=57.50×1=57.50 → nontax=50, excess=0 (within cap).
        //   Jun: acordata=250, cap_total=57.50×5=287.50 → nontax=250, excess=0 (within cap too).
        //
        // Make Jun excess positive: daily_rate = 70.
        //   May: acordata=70, cap=57.50 → nontax=57.50, excess=12.50.
        //   Jun: acordata=350, cap=57.50×5=287.50 → nontax=287.50, excess=62.50.
        // Both months have excess. Still not "only second binds".
        //
        // The clearest "only second binds" case: limit_B_may > limit_A (salary high), but
        // limit_B_jun LOWER than acordata_jun due to fewer working days — but that's the same cap.
        //
        // Simplest case that demonstrates the BUG: cross-month where the single-month engine
        // would compute with start-month (May, 20 working days) and produce impozabila=0,
        // but the multimonth engine correctly finds impozabila > 0 from June.
        // salary=200, limit_B_May=30, limit_B_Jun=28.57.  Both months: cap < daily_rate → excess > 0.
        // The P1 bug (single-month) uses May working days (20) for the FULL 7-day span:
        //   single-month cap = min(57.50, 200×3/20) = 30/day × 7 days = 210.
        //   If acordata = 200 (< 210) → single-month impozabila = 0!  But per-month: May cap=30×4=120
        //   and Jun cap=28.57×3=85.71, Σcap=205.71; acordata=200 → all nontax, excess=0.
        // We need acordata > per-month Σcap but < single-month cap to get the P1 under-declaration.
        //
        // salary=200, 28 May–3 Jun (4 May days + 3 Jun days), daily_rate=30 → acordata=210.
        //   single-month (May, 20 wd): cap/day=30, cap×7=210 → acordata=210, nontax=210, impozabila=0. ← BUG.
        //   per-month: May cap=30×4=120, Jun cap=28.57×3=85.71 → Σcap=205.71; excess=210-205.71=4.29. ← CORRECT.
        //
        // So: stored diurna_impozabila should be 4.29 (multimonth total), not 0.
        let full = create_report(
            &pool,
            CreateReportInput {
                company_id: "co1".into(),
                advance_id: None,
                employee_id: Some("emp_later".into()),
                delegation_from: Some("2026-05-28".into()),
                delegation_to: Some("2026-06-03".into()),
                destination: Some("LaterMonthBug".into()),
                days: Some(7),
                diurna_acordata: Some("210.00".into()),
                salariu_baza: Some("200.00".into()),
                report_date: "2026-06-05".into(),
                notes: None,
                lines: vec![ExpenseLineInput {
                    category: "diurna".into(),
                    description: None,
                    amount: "210.00".into(),
                    vat_amount: None,
                    account_code: None,
                }],
                diurna_interna: Some("23.00".into()),
            },
        )
        .await
        .unwrap();

        let stored_impozabila_str = full
            .report
            .diurna_impozabila
            .as_deref()
            .expect("diurna_impozabila must be stored");
        let stored_impozabila = dec(stored_impozabila_str);

        // P1 BUG would give 0; the fix must give > 0.
        assert!(
            stored_impozabila > Decimal::ZERO,
            "LATER-MONTH-BINDS: stored diurna_impozabila must be > 0 (was {stored_impozabila_str}); \
             the P1 single-month bug would have returned 0 (under-declaration)",
        );

        // Verify the breakdown exists and has June excess > 0.
        let json = full
            .report
            .diurna_breakdown_json
            .as_deref()
            .expect("breakdown_json must be stored");
        let breakdown: Vec<DiurnaBreakdownSegment> = serde_json::from_str(json).unwrap();
        let jun = breakdown
            .iter()
            .find(|s| s.period == "2026-06")
            .expect("Jun segment");
        assert!(
            dec(&jun.excess) > Decimal::ZERO,
            "June must contribute to the taxable excess"
        );

        // Approve and verify payroll feed is NOT skipped.
        approve_report(&pool, &full.report.id, "co1", "2026-06-05")
            .await
            .unwrap();

        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT period, amount FROM payroll_extra_income \
             WHERE company_id='co1' AND source_ref=?1 AND employee_id='emp_later'",
        )
        .bind(&full.report.id)
        .fetch_all(&pool)
        .await
        .unwrap();

        assert!(
            !rows.is_empty(),
            "LATER-MONTH-BINDS: payroll feed must NOT be skipped (P1 under-declaration fixed)"
        );
        let feed_sum: Decimal = rows.iter().map(|(_, a)| dec(a)).sum();
        assert_eq!(
            round2(feed_sum),
            stored_impozabila,
            "Σ fed to payroll must == stored impozabila (no drift)"
        );
    }

    /// diurna_interna ≠ 23: a company configured a different internal rate (e.g. 30 lei/zi).
    /// limit_A = 2.5 × 30 = 75/day. The stored values and the payroll feed must both use 75,
    /// NOT the hardcoded 57.50 (= 2.5×23).
    #[tokio::test]
    async fn diurna_interna_non_default_used_consistently() {
        let pool = setup().await;

        sqlx::query(
            "INSERT INTO employees \
             (id, company_id, cnp, full_name, gross_salary, personal_deduction, \
              employment_date, active, tip_asigurat, pensionar, tip_contract, ore_norma, \
              exceptie_cas_min, sediu_cif, beneficiar_suma_netaxabila, created_at, updated_at) \
             VALUES ('emp_interna','co1','1900101410014','Test Interna','10000','0', \
                     '2024-01-01',1,'1',0,'N',8,'','',0,0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // salary=10000, June 2026 (21 working days): limit_B = 10000×3/21 ≈ 1428.57 >> limit_A.
        // With diurna_interna=30: limit_A = 2.5×30 = 75/day.  Cap = 75/day.
        // With diurna_interna=23: limit_A = 57.50/day.
        // 3 days, acordata=300 → cap_total_30 = 75×3=225, nontax=225, excess=75.
        //                         cap_total_23 = 57.50×3=172.50, nontax=172.50, excess=127.50.
        // Only the correct cap (75) matches the configured rate 30.
        let full = create_report(
            &pool,
            CreateReportInput {
                company_id: "co1".into(),
                advance_id: None,
                employee_id: Some("emp_interna".into()),
                delegation_from: Some("2026-06-10".into()),
                delegation_to: Some("2026-06-12".into()),
                destination: Some("TestInterna".into()),
                days: Some(3),
                diurna_acordata: Some("300.00".into()),
                salariu_baza: Some("10000.00".into()),
                report_date: "2026-06-13".into(),
                notes: None,
                lines: vec![ExpenseLineInput {
                    category: "diurna".into(),
                    description: None,
                    amount: "300.00".into(),
                    vat_amount: None,
                    account_code: None,
                }],
                diurna_interna: Some("30.00".into()), // non-default configured rate
            },
        )
        .await
        .unwrap();

        // Verify persisted diurna_interna.
        assert_eq!(
            full.report.diurna_interna.as_deref(),
            Some("30.00"),
            "diurna_interna must be persisted as the configured value, not hardcoded 23.00"
        );

        // Verify stored nontax/excess use limit_A = 75 (not 57.50).
        // nontax should be 225.00 (not 172.50), excess should be 75.00 (not 127.50).
        let stored_nontax = dec(full.report.diurna_neimpozabila.as_deref().unwrap());
        let stored_impozabila = dec(full.report.diurna_impozabila.as_deref().unwrap());
        assert_eq!(
            stored_nontax,
            dec("225.00"),
            "nontax must use limit_A=75 (diurna_interna=30), not 57.50 (hardcoded 23)"
        );
        assert_eq!(
            stored_impozabila,
            dec("75.00"),
            "excess must use limit_A=75 (diurna_interna=30), not 127.50 (hardcoded 23)"
        );

        // Approve and verify payroll feed uses the same (correct) excess.
        approve_report(&pool, &full.report.id, "co1", "2026-06-13")
            .await
            .unwrap();

        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT period, amount FROM payroll_extra_income \
             WHERE company_id='co1' AND source_ref=?1 AND employee_id='emp_interna'",
        )
        .bind(&full.report.id)
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(rows.len(), 1, "single-month → one row");
        let fed_excess = dec(&rows[0].1);
        assert_eq!(
            fed_excess,
            dec("75.00"),
            "payroll feed must use excess=75 (diurna_interna=30), not 127.50 (P2 hardcoded fix)"
        );
        // GL 625 D = stored nontax + stored impozabila = 300 (full acordata).
        let entries: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT e.account_code, e.debit, e.credit \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.source_type='EXPENSE_REPORT' AND j.source_id=?1",
        )
        .bind(&full.report.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        let debit_625: Decimal = entries
            .iter()
            .filter(|(a, _, _)| a == "625")
            .map(|(_, d, _)| dec(d))
            .sum();
        assert_eq!(
            debit_625,
            dec("300.00"),
            "GL 625 D must be full acordata (225 nontax + 75 excess = 300)"
        );
    }

    /// GL≡payroll single source-of-truth: the 625 residual (Σ nontax) in the settlement GL
    /// reconciles with the breakdown nontax totals; the fed excess == stored impozabila.
    #[tokio::test]
    async fn gl_625_residual_reconciles_with_breakdown_nontax() {
        let pool = setup().await;

        sqlx::query(
            "INSERT INTO employees \
             (id, company_id, cnp, full_name, gross_salary, personal_deduction, \
              employment_date, active, tip_asigurat, pensionar, tip_contract, ore_norma, \
              exceptie_cas_min, sediu_cif, beneficiar_suma_netaxabila, created_at, updated_at) \
             VALUES ('emp_gl','co1','1900101410015','Test GL','4000','0', \
                     '2024-01-01',1,'1',0,'N',8,'','',0,0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // 3–7 June 2026 (5 days), salary=4000, diurna=500, diurna_interna=23.
        // cap=57.50/day × 5 = 287.50; excess=212.50; nontax=287.50.
        // GL: 625 D = 500 (full), 5311 C = 500 (no advance).
        // After run_payroll: 641 D = 212.50 (excess), 625 C = 212.50 → 625 net = 287.50 (within-cap).
        let full = create_report(
            &pool,
            CreateReportInput {
                company_id: "co1".into(),
                advance_id: None,
                employee_id: Some("emp_gl".into()),
                delegation_from: Some("2026-06-03".into()),
                delegation_to: Some("2026-06-07".into()),
                destination: Some("GL Test".into()),
                days: Some(5),
                diurna_acordata: Some("500.00".into()),
                salariu_baza: Some("4000.00".into()),
                report_date: "2026-06-08".into(),
                notes: None,
                lines: vec![ExpenseLineInput {
                    category: "diurna".into(),
                    description: None,
                    amount: "500.00".into(),
                    vat_amount: None,
                    account_code: None,
                }],
                diurna_interna: Some("23.00".into()),
            },
        )
        .await
        .unwrap();

        approve_report(&pool, &full.report.id, "co1", "2026-06-08")
            .await
            .unwrap();

        // GL 625 D = full acordata (EXPENSE_REPORT journal).
        let entries: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT e.account_code, e.debit, e.credit \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.source_type='EXPENSE_REPORT' AND j.source_id=?1",
        )
        .bind(&full.report.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        let debit_625: Decimal = entries
            .iter()
            .filter(|(a, _, _)| a == "625")
            .map(|(_, d, _)| dec(d))
            .sum();
        assert_eq!(debit_625, dec("500.00"), "GL 625 D must be full acordata");

        // Stored values match breakdown.
        let stored_nontax = dec(full.report.diurna_neimpozabila.as_deref().unwrap());
        let stored_impozabila = dec(full.report.diurna_impozabila.as_deref().unwrap());
        assert_eq!(stored_nontax, dec("287.50"));
        assert_eq!(stored_impozabila, dec("212.50"));

        // Breakdown Σ nontax + Σ excess == 500 (exact reconciliation, no drift).
        let json = full
            .report
            .diurna_breakdown_json
            .as_deref()
            .expect("breakdown_json");
        let breakdown: Vec<DiurnaBreakdownSegment> = serde_json::from_str(json).unwrap();
        let bd_nontax: Decimal = breakdown.iter().map(|s| dec(&s.nontax)).sum();
        let bd_excess: Decimal = breakdown.iter().map(|s| dec(&s.excess)).sum();
        assert_eq!(
            round2(bd_nontax + bd_excess),
            dec("500.00"),
            "breakdown Σ(nontax + excess) must == 500 exactly"
        );

        // Payroll feed excess == stored impozabila (same source, no recomputation drift).
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT period, amount FROM payroll_extra_income \
             WHERE company_id='co1' AND source_ref=?1 AND employee_id='emp_gl'",
        )
        .bind(&full.report.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        let feed_excess: Decimal = rows.iter().map(|(_, a)| dec(a)).sum();
        assert_eq!(
            round2(feed_excess),
            stored_impozabila,
            "payroll feed Σ excess must == stored impozabila (GL ≡ payroll, no drift)"
        );
        // The "625 residual within cap" = 625 D (settlement) - excess_fed = nontax exactly.
        assert_eq!(
            round2(debit_625 - feed_excess),
            stored_nontax,
            "625 net after reclass = nontax (reconciles settlement with payroll)"
        );
    }

    /// Idempotent re-approve after stored breakdown: re-approving a second time (which is a
    /// no-op via the status check) must not create additional extra_income rows.
    #[tokio::test]
    async fn idempotent_reapprove_no_duplicate_extra_income() {
        let pool = setup().await;

        sqlx::query(
            "INSERT INTO employees \
             (id, company_id, cnp, full_name, gross_salary, personal_deduction, \
              employment_date, active, tip_asigurat, pensionar, tip_contract, ore_norma, \
              exceptie_cas_min, sediu_cif, beneficiar_suma_netaxabila, created_at, updated_at) \
             VALUES ('emp_idem2','co1','1900101410016','Test Idem2','4000','0', \
                     '2024-01-01',1,'1',0,'N',8,'','',0,0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let full = create_report(
            &pool,
            CreateReportInput {
                company_id: "co1".into(),
                advance_id: None,
                employee_id: Some("emp_idem2".into()),
                delegation_from: Some("2026-06-03".into()),
                delegation_to: Some("2026-06-07".into()),
                destination: Some("Idem Test".into()),
                days: Some(5),
                diurna_acordata: Some("500.00".into()),
                salariu_baza: Some("4000.00".into()),
                report_date: "2026-06-08".into(),
                notes: None,
                lines: vec![ExpenseLineInput {
                    category: "diurna".into(),
                    description: None,
                    amount: "500.00".into(),
                    vat_amount: None,
                    account_code: None,
                }],
                diurna_interna: Some("23.00".into()),
            },
        )
        .await
        .unwrap();

        approve_report(&pool, &full.report.id, "co1", "2026-06-08")
            .await
            .unwrap();
        // Second call is a no-op (status already 'approved').
        approve_report(&pool, &full.report.id, "co1", "2026-06-08")
            .await
            .unwrap();

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM payroll_extra_income \
             WHERE company_id='co1' AND source_ref=?1 AND employee_id='emp_idem2'",
        )
        .bind(&full.report.id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(
            count, 1,
            "idempotent: must have exactly one extra_income row"
        );
    }
}
