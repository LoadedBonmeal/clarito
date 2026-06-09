//! D112 payroll — the per-employee salary computation that the D112 nominal annex is built from.
//!
//! 2026 rates (verified; the IT/construcții/agri exemptions were removed by OUG 156/2024):
//! CAS (pensie, salariat) 25%; CASS (sănătate, salariat) 10%; impozit pe venit 10% (pe baza după
//! CAS+CASS și deducerea personală); CAM (asigurătorie pentru muncă, angajator) 2,25%. Salariu
//! minim 2026: 4.050 lei (sem. I) / 4.325 lei (de la 1 iulie).
//!
//! This module computes ONE salary state (brut → net + contribuții + cost angajator). The full
//! D112 (evidența nominală a salariaților, stările lunare, exportul XML cu cele două versiuni de
//! schemă din 2026 și notele GL 641/421, 4315, 4316, 444, 646/436) este o extensie ulterioară —
//! acesta este nucleul de calcul reutilizabil.

use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;
use serde::{Deserialize, Serialize};

/// 2026 contribution + tax rates (percent).
const CAS_PCT: (i64, u32) = (25, 2); // 0.25
const CASS_PCT: (i64, u32) = (10, 2); // 0.10
const INCOME_TAX_PCT: (i64, u32) = (10, 2); // 0.10
const CAM_PCT: (i64, u32) = (225, 4); // 0.0225

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PayrollInput {
    /// Salariul brut lunar.
    pub gross: Decimal,
    /// Deducerea personală (din tabelul ANAF, în funcție de venit + persoane în întreținere).
    #[serde(default)]
    pub personal_deduction: Decimal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayrollResult {
    pub gross: String,
    pub cas: String,
    pub cass: String,
    pub personal_deduction: String,
    pub taxable_base: String,
    pub income_tax: String,
    pub net: String,
    pub cam: String,
    pub total_employer_cost: String,
}

fn pct(d: Decimal, (n, s): (i64, u32)) -> Decimal {
    // Contributions/tax rounded to whole lei with COMMERCIAL rounding (half away from zero), the
    // ANAF convention — e.g. 5.000 × 2,25% = 112,5 → 113 (banker's would give 112).
    (d * Decimal::new(n, s)).round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
}
fn fmt(d: Decimal) -> String {
    let d = d.round_dp(2);
    let d = if d.is_zero() { Decimal::ZERO } else { d };
    format!("{:.2}", d)
}

/// Compute one monthly salary state from the gross + personal deduction (2026 rates).
pub fn compute_payroll(input: &PayrollInput) -> PayrollResult {
    let z = Decimal::ZERO;
    let gross = input.gross.max(z);
    let cas = pct(gross, CAS_PCT);
    let cass = pct(gross, CASS_PCT);
    let after_contrib = gross - cas - cass;
    let deduction = input.personal_deduction.max(z).min(after_contrib.max(z));
    let taxable_base = (after_contrib - deduction).max(z);
    let income_tax = pct(taxable_base, INCOME_TAX_PCT);
    let net = gross - cas - cass - income_tax;
    let cam = pct(gross, CAM_PCT);
    let total_employer_cost = gross + cam;

    PayrollResult {
        gross: fmt(gross),
        cas: fmt(cas),
        cass: fmt(cass),
        personal_deduction: fmt(deduction),
        taxable_base: fmt(taxable_base),
        income_tax: fmt(income_tax),
        net: fmt(net),
        cam: fmt(cam),
        total_employer_cost: fmt(total_employer_cost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn payroll_2026_rates_gross_to_net() {
        // Gross 5.000, no personal deduction.
        // CAS 25% = 1.250; CASS 10% = 500; base = 5.000 − 1.250 − 500 = 3.250; impozit 10% = 325.
        // Net = 5.000 − 1.250 − 500 − 325 = 2.925. CAM 2,25% = 113 (rounded). Cost = 5.113.
        let r = compute_payroll(&PayrollInput {
            gross: d("5000"),
            personal_deduction: d("0"),
        });
        assert_eq!(r.cas, "1250.00");
        assert_eq!(r.cass, "500.00");
        assert_eq!(r.taxable_base, "3250.00");
        assert_eq!(r.income_tax, "325.00");
        assert_eq!(r.net, "2925.00");
        assert_eq!(r.cam, "113.00"); // 5000 × 0.0225 = 112.5 → 113
        assert_eq!(r.total_employer_cost, "5113.00");
    }

    #[test]
    fn personal_deduction_reduces_the_income_tax_base() {
        // Gross 4.050 (min wage H1), deduction 700.
        // CAS 1.013 (4050×0.25=1012.5→1013); CASS 405; after = 4050−1013−405 = 2632.
        // base = 2632 − 700 = 1932; impozit 10% = 193. Net = 2632 − 193 = 2439.
        let r = compute_payroll(&PayrollInput {
            gross: d("4050"),
            personal_deduction: d("700"),
        });
        assert_eq!(r.cas, "1013.00");
        assert_eq!(r.cass, "405.00");
        assert_eq!(r.taxable_base, "1932.00");
        assert_eq!(r.income_tax, "193.00");
        assert_eq!(r.net, "2439.00");
    }
}
