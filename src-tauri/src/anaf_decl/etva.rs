//! RO e-TVA — reconciliation of the app-computed D300 against the ANAF "decont precompletat"
//! (P300ETVA), as a PRE-FILING SELF-CHECK.
//!
//! 2026 legal context (deep-researched): OUG 89/2025 (from 1 Jan 2026) removed the obligation to
//! respond to the "Notificarea de conformare RO e-TVA" and its penalties; OUG 13/2026 (in force
//! 9 Mar 2026) repealed Art. 5/8/16 of OUG 70/2024 — the conformance notification is ABOLISHED.
//! The decont precompletat is still produced and made available via SPV (by the 5th of the month
//! after the D300 deadline) but is now PURELY INFORMATIVE. The taxpayer still computes and files
//! their own D300 (the only juridically-binding return). This module therefore implements an
//! internal self-check, not a notification-response flow.
//!
//! "Diferență semnificativă" (the threshold ANAF historically used, kept here as the self-check
//! guideline): a per-line difference exceeding BOTH ≥ 20% AND an absolute ≥ 5.000 lei.
//!
//! NOTE: ANAF delivers the precompletat as JSON-in-a-zip via a dedicated SPV endpoint, with NO
//! published XSD. Live retrieval (SPV auth) and mapping the actual P300ETVA JSON fields are out of
//! scope here (need a real sample + credentials); this module reconciles a precompletat whose
//! key values the caller supplies (imported/entered from the SPV download).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Absolute floor for a "significant" difference (lei).
const SIGNIFICANT_ABS_LEI: i64 = 5_000;
/// Percentage floor for a "significant" difference.
const SIGNIFICANT_PCT: i64 = 20;

/// Precompletat values supplied by the caller (from the SPV P300ETVA download). Strings are
/// 2-decimal RON, matching the D300 report fields.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EtvaPrecompletat {
    /// TVA colectată total (precompletat, P300ETVA).
    #[serde(default)]
    pub collected_vat: String,
    /// TVA deductibilă total (precompletat, P300ETVA).
    #[serde(default)]
    pub deductible_vat: String,
}

/// One reconciled line: the D300 value vs the precompletat value + the flagged difference.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EtvaLine {
    pub label: String,
    pub d300: String,
    pub precompletat: String,
    pub diff: String,
    pub diff_pct: String,
    /// True when |diff| ≥ 5.000 lei AND |diff%| ≥ 20% (the significance guideline).
    pub significant: bool,
    /// Explanatory note (e.g. cash-VAT divergence is expected).
    pub note: Option<String>,
}

/// The reconciliation result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EtvaReconciliation {
    pub period_from: String,
    pub period_to: String,
    pub lines: Vec<EtvaLine>,
    /// Any line flagged significant.
    pub any_significant: bool,
    /// Company is on TVA la încasare → divergences from the precompletat (built off e-Factura
    /// issue dates, not collection) are EXPECTED, not errors.
    pub cash_vat: bool,
}

/// Build a reconciled line for a (d300, precompletat) value pair.
pub fn reconcile_line(
    label: &str,
    d300: Decimal,
    precompletat: Decimal,
    note: Option<String>,
) -> EtvaLine {
    let diff = d300 - precompletat;
    let abs_diff = diff.abs();
    // pct relative to the precompletat (ANAF's reference); guard div-by-zero. Keep it UNROUNDED
    // for the threshold test (rounding first would flag a true 19.95% as 20%); round for display.
    let pct_exact = if precompletat.is_zero() {
        if diff.is_zero() {
            Decimal::ZERO
        } else {
            Decimal::from(100)
        }
    } else {
        abs_diff / precompletat.abs() * Decimal::from(100)
    };
    let significant = abs_diff >= Decimal::from(SIGNIFICANT_ABS_LEI)
        && pct_exact >= Decimal::from(SIGNIFICANT_PCT);
    EtvaLine {
        label: label.to_string(),
        d300: fmt2(d300),
        precompletat: fmt2(precompletat),
        diff: fmt2(diff),
        diff_pct: format!("{}", pct_exact.round_dp(1)),
        significant,
        note,
    }
}

fn fmt2(d: Decimal) -> String {
    // Round to 2dp first so a sub-cent negative (e.g. -0.001) renders "0.00", never "-0.00".
    let d = d.round_dp(2);
    let d = if d.is_zero() { Decimal::ZERO } else { d };
    format!("{:.2}", d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn flags_significant_only_when_both_thresholds_met() {
        // 30.000 vs 20.000 → diff 10.000 (≥5.000) and 50% (≥20%) → significant.
        let l = reconcile_line("TVA colectată", dec("30000"), dec("20000"), None);
        assert!(l.significant);
        assert_eq!(l.diff, "10000.00");

        // big % but small absolute: 100 vs 0 → 100% but only 100 lei → NOT significant.
        let l = reconcile_line("x", dec("100"), dec("0"), None);
        assert!(!l.significant);

        // big absolute but small %: 106.000 vs 100.000 → 6.000 lei (≥5.000) but 6% (<20%) → not.
        let l = reconcile_line("y", dec("106000"), dec("100000"), None);
        assert!(!l.significant);

        // exact match → 0 diff, not significant, renders 0.00 (never -0.00).
        let l = reconcile_line("z", dec("5000"), dec("5000"), None);
        assert!(!l.significant);
        assert_eq!(l.diff, "0.00");
        assert_eq!(l.diff_pct, "0");

        // sub-cent negative diff must render "0.00", not "-0.00".
        let l = reconcile_line("m1", dec("100.00"), dec("100.001"), None);
        assert_eq!(l.diff, "0.00");

        // a true 19.95% (≥5.000 lei) must NOT be flagged — threshold is strict ≥20% on the
        // unrounded ratio (regression guard against rounding 19.95 → 20.0 before the test).
        let l = reconcile_line("m2", dec("119950"), dec("100000"), None);
        assert_eq!(l.diff, "19950.00");
        assert!(!l.significant, "19.95% is below the 20% threshold");
    }
}
