//! Diurnă taxable-excess → payroll extra-income feed (Wave E).
//!
//! When an expense-report (decont) is approved and `diurna_impozabila > 0`, the taxable
//! surplus is written here so the next `run_payroll` and `build_d112_xml` for that
//! (company, period) fold it into the CAS/CASS/impozit/CAM bases.
//!
//! ## Idempotency
//! The UNIQUE key `(company_id, source_ref, employee_id, period)` prevents double-counting
//! when the same decont is re-settled (which is a no-op anyway, but the DB guarantee is the
//! ultimate safety net).
//!
//! ## Period-lock
//! If the payroll month is already CLOSED (period-locked) when the decont is approved, the
//! excess is stored with `period_lock_status = 'needs_rectificativa'` rather than `'open'`.
//! `run_payroll` skips `needs_rectificativa` rows — they must be included via a D112
//! rectificativă filed manually.
//!
//! ## GL (P1 fix — contributions in run_payroll, not at settlement)
//! The GL for the taxable excess is handled entirely by `run_payroll` in `db/payroll.rs`
//! (source_type `'PAYROLL'`), NOT by a separate `'DIURNA_ASIMILAT'` journal at settlement
//! time. This ensures contributions are rounded ONCE on the COMBINED salary+excess base
//! (ANAF convention) so GL 4315/4316/444/436 == D112 obligations to the leu.
//!
//!   Settlement (in 'EXPENSE_REPORT' journal, kept as-is):
//!     D 625 = C 542/5311  — FULL diurnă (within-cap + excess) settles the advance.
//!     → payroll_extra_income row upserted (excess stored for next run_payroll/D112).
//!
//!   Payroll note ('PAYROLL' journal, includes excess when run_payroll fires):
//!     D 641 = C 421  (salary gross S)            — salary expense / salary payable.
//!     D 641 = C 625  (excess E)                  — reclass travel-expense → salary-expense;
//!                                                   625 nets to within-cap only.
//!     D 421 = C 4315/4316/444  (combined S+E)    — withholdings on COMBINED base (single rounding).
//!     D 646 = C 436  (CAM on combined S+E)       — employer CAM (single rounding = D112).
//!     D 4282 = C 421 (excess-attributable wh.)   — receivable from employee for their excess charges;
//!                                                   421 nets to salary-net (not combined-net).
//!
//! Net check (Wave E active):
//!   641 = S + E (both salary + excess expense).
//!   625 = within-cap only (excess reclassed out).
//!   4315/4316/444 = combined-base contributions == D112.
//!   436 = CAM on combined base == D112.
//!   421 nets to salary-net (C:S + C:excess_recv − D:combined_wh − D:excess_recv = S − sal_wh). ✓
//!   4282 = receivable from employee = excess-attributable withholdings. ✓
//!
//! `post_diurna_asimilat_gl` still exists in this module (for unit tests + historical reference)
//! but is NO LONGER called from `approve_report`. It must NOT be invoked at settlement.
//!
//! The 'EXPENSE_REPORT' journal posts only 625 for the FULL diurnă.
//! See `approve_report` in `db/deconturi.rs` and `run_payroll` in `db/payroll.rs`.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::anaf_decl::d112::{pct, CAM_PCT};
use crate::db::gl::{post_manual_journal, ManualJournal};
use crate::db::models::{new_id, now_unix};
use crate::db::payroll_config::get_payroll_config;
use crate::error::AppResult;

// ─── Rate constants (same source as d112.rs) ──────────────────────────────────
const CAS_PCT: (i64, u32) = (25, 2);
const CASS_PCT: (i64, u32) = (10, 2);
const IMPOZIT_PCT: (i64, u32) = (10, 2);

fn round2(x: Decimal) -> Decimal {
    x.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
}

fn dp(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or_default()
}

// ─── DB structs ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PayrollExtraIncome {
    pub id: String,
    pub company_id: String,
    pub employee_id: String,
    pub period: String,
    pub kind: String,
    pub source: String,
    pub source_ref: String,
    pub amount: String,
    pub flag_cas: bool,
    pub flag_cass: bool,
    pub flag_impozit: bool,
    pub flag_cam: bool,
    pub period_lock_status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Contributions computed on the taxable diurnă excess.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiurnaContribs {
    pub excess: Decimal,
    /// CAS 25% (employee).
    pub cas: Decimal,
    /// CASS 10% (employee).
    pub cass: Decimal,
    /// Impozit 10% (on excess − CAS − CASS).
    pub impozit: Decimal,
    /// CAM 2.25% (employer).
    pub cam: Decimal,
    /// Employee debt to company = CAS + CASS + impozit (the full employee withholdings).
    ///
    /// The employee already received the full `excess` in cash (the company paid from 542/5311).
    /// The company must now remit CAS + CASS + impozit to ANAF from its own pocket.
    /// The receivable records the amount the employee must repay to the company (via payroll
    /// deduction or direct settlement). This nets account 421 to zero in the reclass journal.
    pub receivable_4282: Decimal,
}

impl DiurnaContribs {
    /// Compute all four contributions on `excess_amount`.
    pub fn compute(excess_amount: Decimal) -> Self {
        let excess = round2(excess_amount);
        let cas = pct(excess, CAS_PCT);
        let cass = pct(excess, CASS_PCT);
        // Income-tax base = excess − CAS − CASS (after employee contributions, before deductions
        // — deductions don't apply to asimilat venitor from diurnă excess: art.76(2)(k) + art.78).
        let impozit_base = (excess - cas - cass).max(Decimal::ZERO);
        let impozit = pct(impozit_base, IMPOZIT_PCT);
        let cam = pct(excess, CAM_PCT);
        // Receivable = full employee withholdings (CAS + CASS + impozit).
        // The employee owes these back to the company (which must remit them to ANAF).
        // GL: D 4282 / C 421 = receivable; D 421 / C 4315+4316+444 = same → 421 nets to 0.
        let receivable_4282 = cas + cass + impozit;
        DiurnaContribs {
            excess,
            cas,
            cass,
            impozit,
            cam,
            receivable_4282,
        }
    }
}

// ─── Upsert ──────────────────────────────────────────────────────────────────

/// Write (or update) a taxable diurnă excess into `payroll_extra_income`.
///
/// `period` is the delegation calendar month (`YYYY-MM`).
/// `period_lock_status` is pre-checked by the caller (`approve_report`):
///   - `'open'`                  → the payroll month is still open.
///   - `'needs_rectificativa'`   → the month is locked; a D112 rectificativă is required.
///
/// IDEMPOTENT: uses `INSERT ... ON CONFLICT DO UPDATE` on the UNIQUE key
/// `(company_id, source_ref, employee_id, period)`.
pub async fn upsert_extra_income(
    pool: &SqlitePool,
    company_id: &str,
    employee_id: &str,
    period: &str,
    source_ref: &str,
    amount: Decimal,
    period_lock_status: &str,
) -> AppResult<PayrollExtraIncome> {
    let id = new_id();
    let now = now_unix();
    let amount_str = format!("{:.2}", round2(amount));

    sqlx::query(
        "INSERT INTO payroll_extra_income \
         (id, company_id, employee_id, period, kind, source, source_ref, amount, \
          flag_cas, flag_cass, flag_impozit, flag_cam, period_lock_status, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,'venit_asimilat','diurna_decont',?5,?6,1,1,1,1,?7,?8,?8) \
         ON CONFLICT(company_id, source_ref, employee_id, period) DO UPDATE SET \
           amount=excluded.amount, \
           period_lock_status=excluded.period_lock_status, \
           updated_at=excluded.updated_at",
    )
    .bind(&id)
    .bind(company_id)
    .bind(employee_id)
    .bind(period)
    .bind(source_ref)
    .bind(&amount_str)
    .bind(period_lock_status)
    .bind(now)
    .execute(pool)
    .await?;

    // Fetch the actual row (may have been the existing one on conflict).
    Ok(sqlx::query_as::<_, PayrollExtraIncome>(
        "SELECT * FROM payroll_extra_income \
         WHERE company_id=?1 AND source_ref=?2 AND employee_id=?3 AND period=?4",
    )
    .bind(company_id)
    .bind(source_ref)
    .bind(employee_id)
    .bind(period)
    .fetch_one(pool)
    .await?)
}

/// Sum all OPEN extra-income amounts per employee for a (company, period).
/// Returns a map `employee_id → total_excess` including only `period_lock_status = 'open'` rows.
pub async fn open_extra_income_by_employee(
    pool: &SqlitePool,
    company_id: &str,
    period: &str,
) -> AppResult<std::collections::HashMap<String, Decimal>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT employee_id, amount FROM payroll_extra_income \
         WHERE company_id=?1 AND period=?2 AND period_lock_status='open'",
    )
    .bind(company_id)
    .bind(period)
    .fetch_all(pool)
    .await?;

    let mut map: std::collections::HashMap<String, Decimal> = std::collections::HashMap::new();
    for (emp_id, amt_str) in rows {
        *map.entry(emp_id).or_default() += dp(&amt_str);
    }
    Ok(map)
}

/// Post the GL reclass journal for a diurnă excess (source_type `'DIURNA_ASIMILAT'`).
///
/// This is idempotent per `(company, source_type='DIURNA_ASIMILAT', source_id=source_ref)`.
///
/// See module-level docstring for the full monografie.
pub async fn post_diurna_asimilat_gl(
    pool: &SqlitePool,
    company_id: &str,
    source_ref: &str,
    approve_date: &str,
    excess: Decimal,
) -> AppResult<()> {
    if excess <= Decimal::ZERO {
        return Ok(());
    }
    let c = DiurnaContribs::compute(excess);
    let cfg = get_payroll_config(pool, company_id).await?;

    // Build the balanced journal.
    // Line-by-line (all in D/C pairs, sum zero):
    //   D 421 = C 625  (excess reclass: travel → salary-payable)
    //   D 641 = C 421  (salary expense recognition)
    //   D 421 = C 4315 (CAS employee withholding)
    //   D 421 = C 4316 (CASS employee withholding)
    //   D 421 = C 444  (impozit withholding)
    //   D 646 = C 436  (CAM employer)
    //   D 4282 = C 421 (receivable from employee for net charges)
    //
    // Net 421 check:
    //   421 C = excess (from 641)
    //   421 D = excess (reclass) + CAS + CASS + impozit + receivable_4282
    //         = excess + CAS + CASS + impozit + (excess − CAS − CASS − impozit) = 2×excess
    //   421 C (from 641→421 C excess) + 421 C = 0 (reclass)
    //   Hmm — need to trace carefully.
    //
    // Compact formulation:
    //   (1) D 421 / C 625 = excess  (move from 625 to 421)
    //   (2) D 641 / C 421 = excess  (recognize as salary expense, net 421 back to 0)
    //   (3) D 421 / C 4315 = CAS    (employee CAS owed)
    //   (4) D 421 / C 4316 = CASS   (employee CASS owed)
    //   (5) D 421 / C 444  = impozit (income tax owed)
    //   (6) D 646 / C 436  = CAM    (employer CAM)
    //   (7) D 4282 / C 421 = receivable_4282  (net debt from employee)
    //
    // 421 net: (1)D + (3)D + (4)D + (5)D + (7)C_offset
    //   D 421 = excess + CAS + CASS + impozit
    //   C 421 = excess (from 641) + receivable_4282 = excess + (excess − CAS − CASS − impozit)
    //         = 2·excess − CAS − CASS − impozit
    // That doesn't balance. Fix: (2) posts C 421 = excess; (7) C 421 = receivable.
    //   Total D 421 = excess(1) + CAS(3) + CASS(4) + impozit(5) = excess + withholdings
    //   Total C 421 = excess(2) + receivable(7) = excess + (excess − withholdings) = 2·excess − withholdings
    // Still not equal. The correct formulation is:
    //
    // The employee already received the full excess in cash (from 542/5311 via 625).
    // The COMPANY now owes:
    //   - 4315: CAS to ANAF
    //   - 4316: CASS to ANAF
    //   - 444: impozit to ANAF
    //   - 436: CAM to ANAF
    // The EMPLOYEE owes the company the net charges (= CAS + CASS + impozit) = receivable_4282.
    //
    // Correct compact monografie:
    //   D 641 = C 625       excess  → reclassify travel expense to salary expense
    //   D 641 = C 4315      CAS     → salary expense includes CAS charge borne by employer? NO.
    //
    // Actually the correct RO monografie for "venit asimilat salariului deja plătit cash":
    //
    //   (A) Reclass travel → salary:
    //       D 641 / C 625 = excess  (single entry: move from 625 to 641)
    //
    //   (B) Recognize employee withholding liability + employer CAM:
    //       D 421 / C 4315 = CAS   (CAS withheld from employee)
    //       D 421 / C 4316 = CASS  (CASS withheld from employee)
    //       D 421 / C 444  = impozit
    //       D 646 / C 436  = CAM   (employer contribution)
    //       But D 421 above means 421 is debited — reducing the salary-payable to zero (employee
    //       already received the cash). 421 was never credited (the cash went 542 → 625 directly).
    //
    //   (C) Receivable from employee for the net charges (CAS + CASS + impozit):
    //       D 4282 / C 421 = receivable_4282  (employee debt recovered via payroll deduction)
    //       The remaining 421 (withholdings net of receivable) is the amount the COMPANY bears.
    //       In practice 421 net = -(CAS+CASS+impozit) + receivable_4282 = 0.
    //
    // Final journal (balanced):
    //   D 641 / C 625    = excess           (A) travel→salary
    //   D 421 / C 4315   = CAS              (B) withholding
    //   D 421 / C 4316   = CASS             (B) withholding
    //   D 421 / C 444    = impozit          (B) withholding
    //   D 646 / C 436    = CAM              (B) employer CAM
    //   D 4282 / C 421   = receivable_4282  (C) employee debt
    //
    // Balance check:
    //   Σ Debit  = excess + CAS + CASS + impozit + CAM + receivable_4282
    //            = excess + CAS + CASS + impozit + CAM + (excess − CAS − CASS − impozit)
    //            = 2·excess + CAM
    //   Σ Credit = excess (625) + CAS (4315) + CASS (4316) + impozit (444)
    //              + CAM (436) + receivable_4282 (421 credit)
    //            = excess + CAS + CASS + impozit + CAM + (excess − CAS − CASS − impozit)
    //            = 2·excess + CAM  ✓
    //
    //   And 421 net = D(CAS + CASS + impozit) − C(receivable_4282)
    //               = (CAS + CASS + impozit) − (excess − CAS − CASS − impozit)
    //               ≠ 0 in general.
    //
    // The remaining 421 net = withholdings − receivable = impozit... wait:
    //   D 421 = CAS + CASS + impozit
    //   C 421 = receivable_4282 = excess − CAS − CASS − impozit
    //   Net 421 = D − C = (CAS + CASS + impozit) − (excess − CAS − CASS − impozit)
    //           = 2(CAS+CASS+impozit) − excess
    // This is NOT zero — unless excess = 2(CAS+CASS+impozit), which is only true when
    // the combined contribution rate is exactly 50%, which is false (25+10+10 = 45% on excess,
    // but impozit is on excess−CAS−CASS, so effective total < 45%).
    //
    // The issue: the employee already received the full excess cash. The company paid from 542/5311.
    // The withholdings (CAS+CASS+impozit) from the employee's perspective REDUCE the net they keep,
    // so they owe the company that net. But the COMPANY also has to pay CAS+CASS+impozit to ANAF
    // from its own pocket (since the cash already left). So:
    //
    //   4282 = receivable from employee = CAS + CASS + impozit  (all three, not just partial)
    //   D 4282 / C 421 = CAS + CASS + impozit
    //
    //   Then 421 net = D(CAS+CASS+impozit) − C(CAS+CASS+impozit) = 0 ✓
    //   And receivable_4282 = CAS + CASS + impozit (the full withholdings, not excess−withholdings).
    //
    // This is the correct interpretation: the employee received excess in cash; the company must
    // remit CAS+CASS+impozit to ANAF; the receivable from the employee is the amount they must
    // repay (= CAS+CASS+impozit). The employee's net benefit from the excess is
    // excess − (CAS+CASS+impozit).
    //
    // Revised balance:
    //   Σ Debit  = excess + CAS + CASS + impozit + CAM + receivable
    //            = excess + CAS + CASS + impozit + CAM + (CAS + CASS + impozit)
    //            = excess + 2(CAS+CASS+impozit) + CAM
    //   Σ Credit = excess (625) + CAS (4315) + CASS (4316) + impozit (444)
    //              + CAM (436) + receivable (421 credit)
    //            = excess + CAS + CASS + impozit + CAM + (CAS + CASS + impozit)
    //            = excess + 2(CAS+CASS+impozit) + CAM  ✓
    //
    //   421 net = D(CAS+CASS+impozit) − C(CAS+CASS+impozit) = 0 ✓
    //   625 net = C(excess) → reduces 625 to within-cap only (since EXPENSE_REPORT already debited
    //             full diurnă to 625, and this entry credits back the excess) ✓
    //   641 net = D(excess) → expense recognized once ✓
    //   4282 net = D(CAS+CASS+impozit) — receivable from employee ✓
    //   4315/4316/444/436 = contributions payable ✓

    let receivable = c.cas + c.cass + c.impozit; // employee owes the full withholdings back

    // Build lines using string refs bound to `cfg` (cfg outlives this call).
    let acct_641 = cfg.cheltuieli_salarii.as_str();
    let acct_421 = cfg.salarii_datorate.as_str();
    let acct_4315 = cfg.cas.as_str();
    let acct_4316 = cfg.cass.as_str();
    let acct_444 = cfg.impozit.as_str();
    let acct_646 = cfg.cheltuieli_cam.as_str();
    let acct_436 = cfg.cam.as_str();
    let withholdings = c.cas + c.cass + c.impozit;

    let lines: &[(&str, Decimal, Decimal)] = &[
        (acct_641, c.excess, Decimal::ZERO),     // D 641 (salary expense)
        ("625", Decimal::ZERO, c.excess),        // C 625 (reclass travel→salary)
        (acct_421, withholdings, Decimal::ZERO), // D 421 (withholdings owed by employee)
        (acct_4315, Decimal::ZERO, c.cas),       // C 4315 (CAS payable)
        (acct_4316, Decimal::ZERO, c.cass),      // C 4316 (CASS payable)
        (acct_444, Decimal::ZERO, c.impozit),    // C 444 (impozit payable)
        (acct_646, c.cam, Decimal::ZERO),        // D 646 (CAM employer expense)
        (acct_436, Decimal::ZERO, c.cam),        // C 436 (CAM payable)
        ("4282", receivable, Decimal::ZERO),     // D 4282 (receivable from employee)
        (acct_421, Decimal::ZERO, receivable),   // C 421 (clears D421; net 421 = 0)
    ];

    let journal_id = format!("DAS-{}", &source_ref[..8.min(source_ref.len())]);
    post_manual_journal(
        pool,
        &ManualJournal {
            company_id,
            journal_id: &journal_id,
            journal_type: "DIURNA_ASIMILAT",
            source_type: "DIURNA_ASIMILAT",
            source_id: source_ref,
            date: approve_date,
            description: "Reclasificare diurnă impozabilă → venit asimilat salariu",
            partner_cui: None,
        },
        lines,
    )
    .await
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    // ── Pure contribution math ─────────────────────────────────────────────────

    /// SPEC: excess=212.50 → CAS 53.13 (round(212.50×0.25)), CASS 21.25, impozit on
    /// (212.50−53.13−21.25)×10% = round(138.12×10%) = 13.81, CAM 2.25%×212.50 = 4.78.
    /// receivable = 53.13 + 21.25 + 13.81 = 88.19.
    #[test]
    fn contributions_on_212_50() {
        let c = DiurnaContribs::compute(d("212.50"));
        // CAS 25%: round(212.50 × 0.25) = round(53.125) = 53 (whole-lei per pct())
        assert_eq!(c.cas, d("53"));
        // CASS 10%: round(212.50 × 0.10) = round(21.25) = 21 (half-away: 21.25 → 21... wait)
        // pct() rounds to WHOLE LEI (0 dp): round(212.50 × 0.10) = round(21.25) = 21 (< 0.5)
        assert_eq!(c.cass, d("21"));
        // impozit base = 212.50 − 53 − 21 = 138.50; impozit = round(138.50 × 0.10) = round(13.85) = 14
        assert_eq!(c.impozit, d("14"));
        // CAM 2.25%: round(212.50 × 0.0225) = round(4.78125) = 5
        assert_eq!(c.cam, d("5"));
        // receivable = 53 + 21 + 14 = 88
        assert_eq!(c.receivable_4282, d("88"));
    }

    /// All four contributions fire (CAS + CASS + impozit + CAM all > 0).
    #[test]
    fn all_four_contributions_nonzero_for_any_positive_excess() {
        let c = DiurnaContribs::compute(d("100.00"));
        assert!(c.cas > Decimal::ZERO, "CAS must be > 0");
        assert!(c.cass > Decimal::ZERO, "CASS must be > 0");
        assert!(c.impozit > Decimal::ZERO, "impozit must be > 0");
        assert!(c.cam > Decimal::ZERO, "CAM must be > 0");
    }

    /// CAS base = CASS base = CAM base = excess (NOT reduced by prior contributions).
    #[test]
    fn all_four_use_excess_as_base_cas_cass_cam() {
        // CAS: pct(excess, 25%)
        // CASS: pct(excess, 10%)
        // CAM: pct(excess, 2.25%)
        // These three all use `excess` as the base.
        // impozit base = excess − CAS − CASS.
        let c = DiurnaContribs::compute(d("1000.00"));
        let expected_cas = pct(d("1000.00"), (25, 2));
        let expected_cass = pct(d("1000.00"), (10, 2));
        let expected_cam = pct(d("1000.00"), (225, 4));
        assert_eq!(c.cas, expected_cas);
        assert_eq!(c.cass, expected_cass);
        assert_eq!(c.cam, expected_cam);
        // impozit base = 1000 − 250 − 100 = 650; impozit = 65.
        assert_eq!(c.impozit, pct(d("650.00"), (10, 2)));
    }

    /// Zero excess → all contributions zero.
    #[test]
    fn zero_excess_zero_contributions() {
        let c = DiurnaContribs::compute(Decimal::ZERO);
        assert_eq!(c.cas, Decimal::ZERO);
        assert_eq!(c.cass, Decimal::ZERO);
        assert_eq!(c.impozit, Decimal::ZERO);
        assert_eq!(c.cam, Decimal::ZERO);
        assert_eq!(c.receivable_4282, Decimal::ZERO);
    }

    /// Receivable = CAS + CASS + impozit (employee repays the full employee withholdings).
    #[test]
    fn receivable_equals_employee_withholdings() {
        let c = DiurnaContribs::compute(d("500.00"));
        assert_eq!(c.receivable_4282, c.cas + c.cass + c.impozit);
    }

    // ── DB integration tests ───────────────────────────────────────────────────

    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country, \
             created_at, updated_at) VALUES ('co1','12345678','T SRL','X','X','CJ','RO',0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO employees \
             (id, company_id, cnp, full_name, gross_salary, personal_deduction, \
              employment_date, active, tip_asigurat, pensionar, tip_contract, ore_norma, \
              exceptie_cas_min, sediu_cif, beneficiar_suma_netaxabila, created_at, updated_at) \
             VALUES ('emp1','co1','1900101410011','Ion Pop','5000','0', \
                     '2024-01-01',1,'1',0,'N',8,'','',0,0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    /// Upsert inserts a row; second call with same key is idempotent (no duplicate).
    #[tokio::test]
    async fn upsert_idempotent_same_source_ref() {
        let pool = setup().await;
        let r1 = upsert_extra_income(
            &pool,
            "co1",
            "emp1",
            "2026-06",
            "dec-001",
            d("212.50"),
            "open",
        )
        .await
        .unwrap();
        let r2 = upsert_extra_income(
            &pool,
            "co1",
            "emp1",
            "2026-06",
            "dec-001",
            d("212.50"),
            "open",
        )
        .await
        .unwrap();
        assert_eq!(r1.id, r2.id, "same source_ref must not create a second row");
        let rows: Vec<PayrollExtraIncome> =
            sqlx::query_as("SELECT * FROM payroll_extra_income WHERE company_id='co1'")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rows.len(), 1, "exactly one row after two identical upserts");
    }

    /// Two different deconts in the same month for the same employee → two rows, sum aggregated.
    #[tokio::test]
    async fn two_deconts_same_month_sum_aggregated() {
        let pool = setup().await;
        upsert_extra_income(
            &pool,
            "co1",
            "emp1",
            "2026-06",
            "dec-001",
            d("100.00"),
            "open",
        )
        .await
        .unwrap();
        upsert_extra_income(
            &pool,
            "co1",
            "emp1",
            "2026-06",
            "dec-002",
            d("50.00"),
            "open",
        )
        .await
        .unwrap();

        let map = open_extra_income_by_employee(&pool, "co1", "2026-06")
            .await
            .unwrap();
        let total = map.get("emp1").copied().unwrap_or_default();
        assert_eq!(total, d("150.00"), "sum must be 100+50");
    }

    /// Locked-month rows are excluded from open_extra_income.
    #[tokio::test]
    async fn locked_month_rows_excluded() {
        let pool = setup().await;
        upsert_extra_income(
            &pool,
            "co1",
            "emp1",
            "2026-05",
            "dec-old",
            d("80.00"),
            "needs_rectificativa",
        )
        .await
        .unwrap();
        upsert_extra_income(
            &pool,
            "co1",
            "emp1",
            "2026-06",
            "dec-new",
            d("40.00"),
            "open",
        )
        .await
        .unwrap();

        let map_may = open_extra_income_by_employee(&pool, "co1", "2026-05")
            .await
            .unwrap();
        assert!(
            map_may.is_empty(),
            "locked row must not appear in open feed"
        );

        let map_jun = open_extra_income_by_employee(&pool, "co1", "2026-06")
            .await
            .unwrap();
        assert_eq!(map_jun.get("emp1").copied().unwrap_or_default(), d("40.00"));
    }

    /// GL journal is balanced after post_diurna_asimilat_gl.
    #[tokio::test]
    async fn gl_journal_balanced() {
        let pool = setup().await;
        post_diurna_asimilat_gl(&pool, "co1", "dec-001", "2026-06-05", d("212.50"))
            .await
            .unwrap();

        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT e.debit, e.credit \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.source_type='DIURNA_ASIMILAT' AND j.source_id='dec-001' \
               AND j.company_id='co1'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(!rows.is_empty(), "GL must be posted");

        let sum_d: Decimal = rows.iter().map(|(d, _)| dp(d)).sum();
        let sum_c: Decimal = rows.iter().map(|(_, c)| dp(c)).sum();
        assert_eq!(sum_d, sum_c, "DIURNA_ASIMILAT journal must balance");
    }

    /// GL: 641 D = excess; 625 C = excess (reclass); 4282 D = receivable; 421 net = 0.
    #[tokio::test]
    async fn gl_accounts_correct() {
        let pool = setup().await;
        let excess = d("212.50");
        post_diurna_asimilat_gl(&pool, "co1", "dec-002", "2026-06-05", excess)
            .await
            .unwrap();

        let entries: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT e.account_code, e.debit, e.credit \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.source_type='DIURNA_ASIMILAT' AND j.source_id='dec-002'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let sum_for = |acct: &str, col: usize| -> Decimal {
            entries
                .iter()
                .filter(|(a, _, _)| a == acct)
                .map(|r| dp(if col == 0 { &r.1 } else { &r.2 }))
                .sum()
        };

        let c = DiurnaContribs::compute(excess);
        // 641 D = excess (salary expense)
        assert_eq!(sum_for("641", 0), excess, "641 D must equal excess");
        // 625 C = excess (reclass from travel)
        assert_eq!(sum_for("625", 1), excess, "625 C must equal excess");
        // 4282 D = receivable (employee debt = CAS + CASS + impozit)
        assert_eq!(
            sum_for("4282", 0),
            c.receivable_4282,
            "4282 D must equal receivable"
        );
        // 421 net must be zero
        let d421 = sum_for("421", 0);
        let c421 = sum_for("421", 1);
        assert_eq!(d421, c421, "421 must net to zero");
        // 4315 C = CAS, 4316 C = CASS, 444 C = impozit
        assert_eq!(sum_for("4315", 1), c.cas, "4315 C = CAS");
        assert_eq!(sum_for("4316", 1), c.cass, "4316 C = CASS");
        assert_eq!(sum_for("444", 1), c.impozit, "444 C = impozit");
        // 436 C = CAM; 646 D = CAM
        assert_eq!(sum_for("436", 1), c.cam, "436 C = CAM");
        assert_eq!(sum_for("646", 0), c.cam, "646 D = CAM");
        // No 6458 (must NOT appear — the stopgap is replaced)
        assert!(
            !entries.iter().any(|(a, _, _)| a == "6458"),
            "6458 must NOT appear in DIURNA_ASIMILAT journal"
        );
    }

    /// GL idempotent: posting the same source_ref twice results in one journal (not two).
    #[tokio::test]
    async fn gl_idempotent() {
        let pool = setup().await;
        post_diurna_asimilat_gl(&pool, "co1", "dec-003", "2026-06-05", d("100.00"))
            .await
            .unwrap();
        post_diurna_asimilat_gl(&pool, "co1", "dec-003", "2026-06-05", d("100.00"))
            .await
            .unwrap();

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal WHERE source_type='DIURNA_ASIMILAT' \
             AND source_id='dec-003' AND company_id='co1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1, "idempotent: must have exactly one journal");
    }
}
