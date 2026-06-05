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
}
