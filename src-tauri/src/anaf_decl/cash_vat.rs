//! TVA la încasare (cash VAT) — the per-operation exigibility decision.
//!
//! Decides whether the cash-VAT deferral applies to a given operation (VAT exigible on
//! *collection*) or whether the operation keeps normal exigibility (faptul generator).
//! This is the pure primitive the settlement-event ledger + D300 routing + GL postings
//! build on (see ../../CASH_VAT_DESIGN.md). It changes no behaviour on its own.
//!
//! Legal basis: Cod fiscal art. 282 alin. (3)-(8). The deferral applies to taxable
//! supplies with place of supply in Romania; art. 282 alin. (6) carves out reverse-charge
//! (art. 307(2)-(6) / 331), VAT-exempt, special-regime (art. 311-313 margin) and
//! affiliated-party operations. Intra-EU / export / import follow their own exigibility
//! rules. We key on the CIUS VAT category (S/AE/E/Z/K/O/G), which captures the
//! category-driven exclusions; affiliation and margin schemes are not modelled (the app
//! does not track them) and would need extra metadata.

/// Cash-VAT status of a single operation (invoice line / per-rate VAT group).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CashVatStatus {
    /// Under cash VAT — VAT becomes exigibilă on collection.
    Applies,
    /// The company has not elected the cash-VAT regime.
    NotElected,
    /// Reverse charge (taxare inversă, art. 331 / 307) — the beneficiary is liable.
    ReverseCharge,
    /// VAT-exempt operation (scutit).
    Exempt,
    /// Zero-rated (export / intra-EU exempt supply).
    ZeroRated,
    /// Intra-EU acquisition (own exigibility rules).
    IntraEu,
    /// Other category outside the cash-VAT scope (O/G/…).
    OutOfScope,
}

impl CashVatStatus {
    /// True only when the operation is under cash VAT (exigibility deferred to collection).
    pub fn applies(self) -> bool {
        matches!(self, CashVatStatus::Applies)
    }

    /// Romanian exclusion reason (None when the operation IS under cash VAT). Used for
    /// diagnostics and to suppress the "TVA la încasare" invoice mention on excluded lines.
    pub fn exclusion_reason(self) -> Option<&'static str> {
        match self {
            CashVatStatus::Applies => None,
            CashVatStatus::NotElected => Some("regimul TVA la încasare nu este activat"),
            CashVatStatus::ReverseCharge => Some("taxare inversă (art. 331/307)"),
            CashVatStatus::Exempt => Some("operațiune scutită de TVA"),
            CashVatStatus::ZeroRated => Some("livrare cu cotă zero (export/intra-UE)"),
            CashVatStatus::IntraEu => Some("achiziție intracomunitară"),
            CashVatStatus::OutOfScope => Some("operațiune în afara sferei TVA la încasare"),
        }
    }
}

/// Decide the cash-VAT status of a SALES (output) operation, from the company's election
/// and the line's CIUS VAT category. Only standard domestic taxable supplies (category
/// "S") defer to collection; reverse-charge / exempt / zero-rated / intra-EU keep normal
/// exigibility per art. 282 alin. (6).
pub fn sales_status(company_cash_vat: bool, vat_category: &str) -> CashVatStatus {
    if !company_cash_vat {
        return CashVatStatus::NotElected;
    }
    match vat_category.trim() {
        "S" => CashVatStatus::Applies,
        "AE" => CashVatStatus::ReverseCharge,
        "E" => CashVatStatus::Exempt,
        "Z" => CashVatStatus::ZeroRated,
        "K" => CashVatStatus::IntraEu,
        _ => CashVatStatus::OutOfScope,
    }
}

/// Decide the cash-VAT status of a PURCHASE (input) operation. The buyer defers deduction
/// to *payment* when the SUPPLIER applies cash VAT (art. 297 alin. (2)/(3)) — independent
/// of the buyer's own election — except for reverse-charge / import / intra-EU, which
/// deduct immediately.
pub fn purchase_status(supplier_cash_vat: bool, vat_category: &str) -> CashVatStatus {
    if !supplier_cash_vat {
        return CashVatStatus::NotElected;
    }
    match vat_category.trim() {
        "S" => CashVatStatus::Applies,
        "AE" => CashVatStatus::ReverseCharge,
        "E" => CashVatStatus::Exempt,
        "Z" => CashVatStatus::ZeroRated,
        "K" => CashVatStatus::IntraEu,
        _ => CashVatStatus::OutOfScope,
    }
}

/// Whether the right to deduct input VAT on a PURCHASE line is DEFERRED to payment under
/// buyer-side TVA la încasare (Cod fiscal art. 297 alin. (2)-(3); Norme pct. 69). Two
/// independent triggers — deferral applies when the SUPPLIER applies cash VAT (alin. (2),
/// any buyer) OR the BUYER applies cash VAT (alin. (3), all its domestic purchases).
///
/// Only standard-rate domestic acquisitions (category "S") defer; the art. 297 alin. (3)
/// carve-outs — reverse-charge (art. 307(2)-(6) / 313(10) / 331 → "AE"), intra-EU acquisitions
/// ("K"), imports, and exempt/zero-rated/out-of-scope — deduct at the normal exigibility date
/// (their VAT is self-assessed or paid to customs, never "paid to the supplier").
pub fn purchase_deferred(
    supplier_cash_vat: bool,
    buyer_cash_vat: bool,
    vat_category: &str,
) -> bool {
    (supplier_cash_vat || buyer_cash_vat) && vat_category.trim() == "S"
}

/// Round-half-away-from-zero integer division `a / b` (for `b > 0`).
fn round_div(a: i64, b: i64) -> i64 {
    if b == 0 {
        return 0;
    }
    let half = b / 2;
    if a >= 0 {
        (a + half) / b
    } else {
        -((-a + half) / b)
    }
}

/// VAT made exigibilă by a single (partial) collection under cash VAT, with exact true-up
/// on the final receipt. Cumulative-proportional: the VAT exigible at the post-payment
/// cumulative collected minus at the pre-payment cumulative — so rounding cannot drift and
/// the receipt that fully collects the invoice releases the entire residual VAT. All money
/// in the same integer unit (bani).
pub fn vat_released(invoice_gross: i64, invoice_vat: i64, paid_before: i64, payment: i64) -> i64 {
    if invoice_gross <= 0 || payment <= 0 {
        return 0;
    }
    let before = paid_before.clamp(0, invoice_gross);
    let after = (paid_before + payment).clamp(0, invoice_gross);
    let exig_after = round_div(invoice_vat.saturating_mul(after), invoice_gross);
    let exig_before = round_div(invoice_vat.saturating_mul(before), invoice_gross);
    (exig_after - exig_before).max(0)
}

/// One rate group of an invoice, in integer bani, used as input to cash-VAT release
/// allocation. `rate_key` is the caller's D300 grouping key (e.g. `(rate × 100).round()`),
/// carried through opaquely.
#[derive(Debug, Clone, Copy)]
pub struct RateBucket {
    pub rate_key: i64,
    pub base_bani: i64,
    pub vat_bani: i64,
}

/// The base + VAT released into one rate group by a single collection, in bani.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleasedBucket {
    pub rate_key: i64,
    pub base_bani: i64,
    pub vat_bani: i64,
}

/// Allocate a single (partial) collection across an invoice's cash-VAT rate buckets.
///
/// `gross_bani` is the FULL invoice gross (all categories, including any exempt lines) — the
/// denominator, because one payment settles the whole invoice proportionally. Only the
/// deferred buckets (the standard-rate "S" lines) are passed in and released here; excluded
/// lines (exempt / reverse-charge / intra-EU) keep invoice-date exigibility and are routed
/// elsewhere. Each component is released via `vat_released`, so every bucket trues up exactly
/// when the invoice is fully collected (no bani drift).
pub fn allocate_collection(
    gross_bani: i64,
    buckets: &[RateBucket],
    paid_before: i64,
    payment: i64,
) -> Vec<ReleasedBucket> {
    buckets
        .iter()
        .map(|b| ReleasedBucket {
            rate_key: b.rate_key,
            base_bani: vat_released(gross_bani, b.base_bani, paid_before, payment),
            vat_bani: vat_released(gross_bani, b.vat_bani, paid_before, payment),
        })
        .collect()
}

/// The cash-VAT eligibility/exit plafon in lei, in force on `date` (ISO "YYYY-MM-DD").
///
/// OUG 8/2026 staged it: 4.500.000 lei through 28.02.2026, 5.000.000 from 01.03.2026, and
/// 5.500.000 from 01.01.2027. For the EXIT test (cumulative current-year turnover) the OUG
/// 8/2026 art. 9 transitional protects a Jan–Feb 2026 breach that stays under 5.000.000, so
/// the practical 2026 exit plafon is 5.000.000 for the whole year — which is what this returns.
pub fn plafon_lei(date: &str) -> i64 {
    if date >= "2027-01-01" {
        5_500_000
    } else if date >= "2026-01-01" {
        5_000_000
    } else {
        4_500_000
    }
}

/// First month (`monthly_net_lei` is ascending `("YYYY-MM", net_lei)`) whose CUMULATIVE net
/// turnover strictly exceeds `plafon_lei` — i.e. the month the mandatory-exit obligation is
/// triggered (art. 282 alin. (4) / art. 324 alin. (14)). `None` if never breached.
pub fn plafon_breach_month(monthly_net_lei: &[(String, i64)], plafon_lei: i64) -> Option<String> {
    let mut cumulative: i64 = 0;
    for (month, net) in monthly_net_lei {
        cumulative = cumulative.saturating_add(*net);
        if cumulative > plafon_lei {
            return Some(month.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_elected_when_company_off() {
        assert_eq!(sales_status(false, "S"), CashVatStatus::NotElected);
        assert!(!sales_status(false, "S").applies());
    }

    #[test]
    fn standard_domestic_applies() {
        assert_eq!(sales_status(true, "S"), CashVatStatus::Applies);
        assert!(sales_status(true, "S").applies());
        assert_eq!(sales_status(true, "S").exclusion_reason(), None);
        // tolerate whitespace
        assert!(sales_status(true, " S ").applies());
    }

    #[test]
    fn excluded_categories_keep_normal_exigibility() {
        assert_eq!(sales_status(true, "AE"), CashVatStatus::ReverseCharge);
        assert_eq!(sales_status(true, "E"), CashVatStatus::Exempt);
        assert_eq!(sales_status(true, "Z"), CashVatStatus::ZeroRated);
        assert_eq!(sales_status(true, "K"), CashVatStatus::IntraEu);
        assert_eq!(sales_status(true, "O"), CashVatStatus::OutOfScope);
        for c in ["AE", "E", "Z", "K", "O", "G"] {
            assert!(
                !sales_status(true, c).applies(),
                "{c} must not be under cash VAT"
            );
            assert!(sales_status(true, c).exclusion_reason().is_some());
        }
    }

    #[test]
    fn purchase_keys_on_supplier_status() {
        // Buyer defers when the SUPPLIER applies cash VAT, regardless of own election.
        assert!(purchase_status(true, "S").applies());
        assert_eq!(purchase_status(false, "S"), CashVatStatus::NotElected);
        // Reverse-charge / intra-EU purchases deduct immediately even from a cash-VAT supplier.
        assert!(!purchase_status(true, "AE").applies());
        assert!(!purchase_status(true, "K").applies());
    }

    #[test]
    fn full_collection_releases_all_vat() {
        // 21% invoice: base 10000, VAT 2100, gross 12100. One full receipt clears it all.
        assert_eq!(vat_released(12100, 2100, 0, 12100), 2100);
    }

    #[test]
    fn partial_collections_true_up_to_invoice_vat() {
        // Two halves of a 12100 / 2100 invoice each release 1050; sum == 2100.
        let r1 = vat_released(12100, 2100, 0, 6050);
        let r2 = vat_released(12100, 2100, 6050, 6050);
        assert_eq!(r1, 1050);
        assert_eq!(r2, 1050);
        assert_eq!(r1 + r2, 2100);
    }

    #[test]
    fn uneven_thirds_sum_exactly_with_no_drift() {
        // 19% invoice: base 10000, VAT 1900, gross 11900. Pay 3967 + 3967 + 3966.
        let (g, v) = (11900, 1900);
        let r1 = vat_released(g, v, 0, 3967);
        let r2 = vat_released(g, v, 3967, 3967);
        let r3 = vat_released(g, v, 7934, 3966);
        assert_eq!(
            r1 + r2 + r3,
            v,
            "Σ releases must equal invoice VAT (true-up)"
        );
    }

    #[test]
    fn overpayment_caps_and_zero_inputs() {
        // Paying more than the invoice never releases more than the invoice VAT.
        assert_eq!(vat_released(12100, 2100, 0, 99999), 2100);
        // A receipt after the invoice is fully collected releases nothing.
        assert_eq!(vat_released(12100, 2100, 12100, 5000), 0);
        // Degenerate inputs are inert.
        assert_eq!(vat_released(0, 0, 0, 100), 0);
        assert_eq!(vat_released(12100, 2100, 0, 0), 0);
    }

    #[test]
    fn allocate_single_bucket_full() {
        // One 21% bucket, gross == bucket gross, paid in full.
        let b = [RateBucket {
            rate_key: 2100,
            base_bani: 10000,
            vat_bani: 2100,
        }];
        let out = allocate_collection(12100, &b, 0, 12100);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rate_key, 2100);
        assert_eq!(out[0].base_bani, 10000);
        assert_eq!(out[0].vat_bani, 2100);
    }

    #[test]
    fn allocate_uses_full_gross_with_exempt_line() {
        // Invoice: S 10000 + 2100 VAT, plus an exempt line 5000 / 0 VAT. Gross 17100.
        // Only the S bucket is passed; a half payment releases half the S base+VAT.
        let s = [RateBucket {
            rate_key: 2100,
            base_bani: 10000,
            vat_bani: 2100,
        }];
        let out = allocate_collection(17100, &s, 0, 8550);
        assert_eq!(out[0].base_bani, 5000);
        assert_eq!(out[0].vat_bani, 1050);
    }

    #[test]
    fn purchase_deferred_or_trigger_on_s_only() {
        // Either trigger defers a standard-rate purchase.
        assert!(purchase_deferred(true, false, "S")); // supplier on cash VAT (art. 297(2))
        assert!(purchase_deferred(false, true, "S")); // buyer on cash VAT (art. 297(3))
        assert!(purchase_deferred(true, true, "S"));
        assert!(purchase_deferred(true, false, " S ")); // whitespace tolerant
                                                        // Neither trigger → immediate deduction.
        assert!(!purchase_deferred(false, false, "S"));
        // Carve-outs never defer, even when a trigger is on.
        for cat in ["AE", "K", "E", "Z", "O", "G"] {
            assert!(
                !purchase_deferred(true, true, cat),
                "{cat} must deduct at the normal date"
            );
        }
    }

    #[test]
    fn plafon_lei_by_date() {
        assert_eq!(plafon_lei("2025-12-31"), 4_500_000);
        assert_eq!(plafon_lei("2026-02-15"), 5_000_000); // transitional → practical 5M
        assert_eq!(plafon_lei("2026-03-01"), 5_000_000);
        assert_eq!(plafon_lei("2026-12-31"), 5_000_000);
        assert_eq!(plafon_lei("2027-01-01"), 5_500_000);
        assert_eq!(plafon_lei("2028-06-30"), 5_500_000);
    }

    #[test]
    fn plafon_breach_detects_first_crossing_month() {
        let months = vec![
            ("2026-01".to_string(), 2_000_000),
            ("2026-02".to_string(), 2_000_000),
            ("2026-03".to_string(), 1_500_000), // cumulative 5.5M > 5M here
            ("2026-04".to_string(), 1_000_000),
        ];
        assert_eq!(
            plafon_breach_month(&months, 5_000_000),
            Some("2026-03".to_string())
        );
    }

    #[test]
    fn plafon_breach_none_when_under() {
        let months = vec![
            ("2026-01".to_string(), 2_000_000),
            ("2026-02".to_string(), 2_000_000),
        ];
        assert_eq!(plafon_breach_month(&months, 5_000_000), None);
        // Exactly at the plafon is NOT a breach (strictly greater).
        let exact = vec![("2026-01".to_string(), 5_000_000)];
        assert_eq!(plafon_breach_month(&exact, 5_000_000), None);
    }

    #[test]
    fn allocate_two_buckets_and_true_up() {
        // 21% (10000/2100) + 11% (5000/550). Gross 17650. Pay 8825 then 8825.
        let bk = [
            RateBucket {
                rate_key: 2100,
                base_bani: 10000,
                vat_bani: 2100,
            },
            RateBucket {
                rate_key: 1100,
                base_bani: 5000,
                vat_bani: 550,
            },
        ];
        let p1 = allocate_collection(17650, &bk, 0, 8825);
        let p2 = allocate_collection(17650, &bk, 8825, 8825);
        // Each bucket's base and VAT sum across the two receipts to the invoice totals.
        assert_eq!(p1[0].vat_bani + p2[0].vat_bani, 2100);
        assert_eq!(p1[1].vat_bani + p2[1].vat_bani, 550);
        assert_eq!(p1[0].base_bani + p2[0].base_bani, 10000);
        assert_eq!(p1[1].base_bani + p2[1].base_bani, 5000);
    }
}
