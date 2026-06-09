//! D101 — Declarația privind impozitul pe profit (Formular 101, OPANAF 206/2025).
//!
//! For PROFIT-tax companies (not micro). Annual; deadline 25 March of the following year. This
//! module computes the worksheet (art. 19 Cod fiscal): rezultat fiscal = rezultat contabil −
//! venituri neimpozabile − deduceri fiscale + cheltuieli nedeductibile − pierdere reportată;
//! impozit = 16% × profit impozabil; minus the sponsorship credit (the smaller of 0,75% × cifra de
//! afaceri and 20% × impozit) and the anticipated payments. Submission stays manual via the ANAF
//! offline PDF inteligent + SPV (no public API), like D300/D394 — this is the computation only.
//!
//! Simplification: `accounting_result` is the PRE-TAX gross result (the P&L's rezultat brut, which
//! excludes the income-tax expense 691/698), so 691 is NOT re-added in `non_deductible_expenses`.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Worksheet inputs. Money as Decimal (RON). Adjustments default to zero (the user fills them).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct D101Input {
    /// rd.7 — rezultat brut contabil (pre-tax). Default: the P&L gross result.
    pub accounting_result: Decimal,
    /// rd.21 — venituri neimpozabile (dividende primite, reluări de provizioane etc.).
    #[serde(default)]
    pub non_taxable_revenue: Decimal,
    /// rd.16 — deduceri fiscale (amortizare fiscală, rezervă legală, ajustări etc.).
    #[serde(default)]
    pub fiscal_deductions: Decimal,
    /// rd.34 — total cheltuieli nedeductibile (protocol peste 2%, amenzi, 50% auto, social peste
    /// 5% etc.) — FĂRĂ 691, care e deja exclus din rezultatul brut.
    #[serde(default)]
    pub non_deductible_expenses: Decimal,
    /// rd.39 — pierderea fiscală de recuperat din anii precedenți.
    #[serde(default)]
    pub prior_loss: Decimal,
    /// Cheltuiala cu sponsorizarea efectuată (pentru creditul de la rd.43).
    #[serde(default)]
    pub sponsorship: Decimal,
    /// Cifra de afaceri (pentru plafonul de 0,75% al sponsorizării). Default: venituri din exploatare.
    #[serde(default)]
    pub turnover: Decimal,
    /// Plăți anticipate / impozit declarat prin D100 în cursul anului.
    #[serde(default)]
    pub anticipated_payments: Decimal,
}

/// Worksheet result. Strings are 2-decimal RON.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D101Result {
    pub accounting_result: String,
    pub non_taxable_revenue: String,
    pub fiscal_deductions: String,
    pub non_deductible_expenses: String,
    pub fiscal_result: String,
    pub prior_loss: String,
    pub taxable_profit: String,
    pub tax16: String,
    pub sponsorship_cap: String,
    pub sponsorship_credit: String,
    pub tax_after_credits: String,
    pub anticipated_payments: String,
    pub balance_due: String,
    pub balance_recoverable: String,
}

fn r2(d: Decimal) -> Decimal {
    d.round_dp(2)
}
fn fmt(d: Decimal) -> String {
    let d = r2(d);
    let d = if d.is_zero() { Decimal::ZERO } else { d };
    format!("{:.2}", d)
}

/// Compute the D101 worksheet from the inputs.
pub fn compute_d101(input: &D101Input) -> D101Result {
    let z = Decimal::ZERO;
    let fiscal_result =
        input.accounting_result - input.non_taxable_revenue - input.fiscal_deductions
            + input.non_deductible_expenses;
    let taxable_profit = (fiscal_result - input.prior_loss).max(z);
    let tax16 = r2(taxable_profit * Decimal::new(16, 2)); // 16%
                                                          // Sponsorship credit: min(0,75% × cifra de afaceri, 20% × impozit), then capped by the amount paid.
    let cap_turnover = r2(input.turnover * Decimal::new(75, 4)); // 0.0075
    let cap_tax = r2(tax16 * Decimal::new(20, 2)); // 0.20
    let sponsorship_cap = cap_turnover.min(cap_tax);
    let sponsorship_credit = input.sponsorship.min(sponsorship_cap).max(z);
    let tax_after_credits = (tax16 - sponsorship_credit).max(z);
    let balance = tax_after_credits - input.anticipated_payments;

    D101Result {
        accounting_result: fmt(input.accounting_result),
        non_taxable_revenue: fmt(input.non_taxable_revenue),
        fiscal_deductions: fmt(input.fiscal_deductions),
        non_deductible_expenses: fmt(input.non_deductible_expenses),
        fiscal_result: fmt(fiscal_result),
        prior_loss: fmt(input.prior_loss),
        taxable_profit: fmt(taxable_profit),
        tax16: fmt(tax16),
        sponsorship_cap: fmt(sponsorship_cap),
        sponsorship_credit: fmt(sponsorship_credit),
        tax_after_credits: fmt(tax_after_credits),
        anticipated_payments: fmt(input.anticipated_payments),
        balance_due: fmt(balance.max(z)),
        balance_recoverable: fmt((-balance).max(z)),
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
    fn computes_fiscal_result_tax_and_sponsorship_credit() {
        // Accounting result 100.000; +5.000 nedeductibile; −2.000 venituri neimpozabile.
        // Fiscal result = 100.000 − 2.000 − 0 + 5.000 = 103.000. Tax 16% = 16.480.
        // Turnover 500.000 → cap 0,75% = 3.750; 20% × 16.480 = 3.296 → cap = 3.296.
        // Sponsorship paid 5.000 → credit capped at 3.296. Tax after = 13.184.
        // Anticipated 10.000 → balance due 3.184.
        let r = compute_d101(&D101Input {
            accounting_result: d("100000"),
            non_taxable_revenue: d("2000"),
            fiscal_deductions: d("0"),
            non_deductible_expenses: d("5000"),
            prior_loss: d("0"),
            sponsorship: d("5000"),
            turnover: d("500000"),
            anticipated_payments: d("10000"),
        });
        assert_eq!(r.fiscal_result, "103000.00");
        assert_eq!(r.taxable_profit, "103000.00");
        assert_eq!(r.tax16, "16480.00");
        assert_eq!(r.sponsorship_cap, "3296.00");
        assert_eq!(r.sponsorship_credit, "3296.00");
        assert_eq!(r.tax_after_credits, "13184.00");
        assert_eq!(r.balance_due, "3184.00");
        assert_eq!(r.balance_recoverable, "0.00");
    }

    #[test]
    fn loss_yields_zero_tax_and_carries_prior_loss() {
        // Fiscal result negative → taxable 0 → tax 0.
        let r = compute_d101(&D101Input {
            accounting_result: d("-20000"),
            ..Default::default()
        });
        assert_eq!(r.taxable_profit, "0.00");
        assert_eq!(r.tax16, "0.00");
        // Prior loss exceeds a small profit → taxable 0.
        let r2 = compute_d101(&D101Input {
            accounting_result: d("5000"),
            prior_loss: d("8000"),
            ..Default::default()
        });
        assert_eq!(r2.taxable_profit, "0.00");
        assert_eq!(r2.tax16, "0.00");
    }
}
