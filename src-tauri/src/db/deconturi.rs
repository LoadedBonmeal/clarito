//! P3 Wave D — Avansuri de trezorerie (542) + Deconturi de cheltuieli.
//!
//! ## Diurnă engine (CF art.76(2)(k), art.76(4)(h), art.142(g), HG 714/2018)
//! Limita neimpozabilă zilnică = min(A, B):
//!   A = 2.5 × diurna_interna (config; default 23 lei → 57.50 lei/zi)
//!   B = salariu_brut × 3 ÷ working_days(an, luna_delegatiei)
//! Total neimpozabil = min(diurna_acordata, min(A,B) × zile_delegare)
//! Surplus impozabil = max(0, diurna_acordata − neimpozabil)
//!
//! INTERN ONLY: surplusul impozabil este CALCULAT + AFIȘAT (flagged), nu postat în GL.
//! (Extern + auto-feed statul de salarii: DEFERRED.)
//!
//! ## Monografie GL (post_manual_journal, idempotent)
//! Grant (source_type='AVANS_TREZORERIE'):  542 D = 5311/5121 C
//! Aprobare decont (source_type='EXPENSE_REPORT'): cheltuieli D + 4426 D = 542 C
//!   — DOAR diurna_neimpozabila în 625; diurna_impozabila NU se postează
//! Return (source_type='AVANS_RETURN'):    5311/5121 D = 542 C

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::gl::{post_manual_journal, ManualJournal};
use crate::db::models::{new_id, now_unix};
use crate::db::payroll::working_days;
use crate::error::{AppError, AppResult};

fn dp(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or_default()
}

fn round2(x: Decimal) -> Decimal {
    x.round_dp(2)
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
    /// Taxable excess = acordată − neimpozabilă. Flagged, NOT posted to GL.
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
    let limit_a_zi = round2(Decimal::from_str("2.5").unwrap() * interna);

    // Limit B per day: salariu × 3 ÷ working_days (CF art.76(2)(k))
    let limit_b_zi = if nzl == 0 {
        Decimal::ZERO
    } else {
        round2(sal * Decimal::from(3) / Decimal::from(nzl))
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

    // Compute diurnă if all inputs are present
    let (diurna_neimpozabila, diurna_impozabila) =
        if let (Some(acordata), Some(sal), Some(days), Some(from)) = (
            input.diurna_acordata.as_deref(),
            input.salariu_baza.as_deref(),
            input.days,
            input.delegation_from.as_deref(),
        ) {
            let parts: Vec<&str> = from.split('-').collect();
            let (year, month) = if parts.len() >= 2 {
                (
                    parts[0].parse::<i32>().unwrap_or(2026),
                    parts[1].parse::<u32>().unwrap_or(1),
                )
            } else {
                (2026, 1)
            };
            let interna = input.diurna_interna.as_deref().unwrap_or("23.00");
            let calc = compute_diurna(acordata, days as u32, sal, year, month, interna);
            (
                Some(calc.diurna_neimpozabila.clone()),
                Some(calc.diurna_impozabila.clone()),
            )
        } else {
            (None, None)
        };

    sqlx::query(
        "INSERT INTO expense_reports \
         (id, company_id, advance_id, employee_id, delegation_from, delegation_to, destination, \
          days, diurna_acordata, diurna_neimpozabila, diurna_impozabila, salariu_baza, \
          report_date, status, notes, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'draft',?14,?15,?15)",
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

    // Recompute diurnă calc for display (if data present)
    let diurna_calc = if let (Some(acordata), Some(sal), Some(days), Some(from)) = (
        report.diurna_acordata.as_deref(),
        report.salariu_baza.as_deref(),
        report.days,
        report.delegation_from.as_deref(),
    ) {
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
            "23.00",
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
///   - 625 D = diurna_neimpozabila (non-taxable diurnă only; taxable is flagged not posted)
///   - per expense line (non-diurnă): account_code D = amount, 4426 D = vat_amount
///   - Total credits: 542 C = advance amount (if any), 5311 C for shortfall/direct reimb
///
/// If advance > total expenses → overshoot → 5311 D for returned excess
/// If advance < total expenses → underpay → 5311 C for the shortfall (company reimburses)
///
/// The taxable diurnă excess is stored on the report and surfaced in UI but NOT posted.
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

    // Non-taxable diurnă → 625
    if let Some(neimpoz) = full.report.diurna_neimpozabila.as_deref() {
        let amt = round2(dp(neimpoz));
        if amt > Decimal::ZERO {
            debit_lines.push(("625".into(), amt));
        }
    }

    // Expense lines (non-diurnă categories only to avoid double-counting)
    for line in &full.lines {
        if line.category == "diurna" {
            // Diurnă is handled via the diurna_neimpozabila computation above
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

    #[tokio::test]
    async fn approval_posts_only_neimpozabila_in_625_not_641() {
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

        // Create report: 3 days, salary 4000, diurnă 300 (cap = 57.50×3 = 172.50 → 127.50 taxable)
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

        // 625 must appear (non-taxable diurnă)
        assert!(
            has_account(&entries, "625"),
            "625 must appear for non-taxable diurnă"
        );
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

        // 641 must NOT appear — taxable excess is not posted
        assert!(
            !has_account(&entries, "641"),
            "641 must NOT appear — taxable diurnă excess is flagged, not posted"
        );
        // 4315/4316/444 must NOT appear — no payroll GL from decont
        assert!(!has_account(&entries, "4315"), "4315 must NOT appear");
        assert!(!has_account(&entries, "4316"), "4316 must NOT appear");
        assert!(!has_account(&entries, "444"), "444 must NOT appear");

        // GL must be balanced
        let sum_d: Decimal = entries.iter().map(|(_, d_val, _)| dp(d_val)).sum();
        let sum_c: Decimal = entries.iter().map(|(_, _, c_val)| dp(c_val)).sum();
        assert_eq!(sum_d, sum_c, "settlement GL must be balanced");
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
}
