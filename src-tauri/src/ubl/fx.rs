//! FX helpers — shared currency conversion used by the UBL generator and
//! future report modules.

use rust_decimal::Decimal;

/// Convert an amount in `currency` to RON using `rate` (RON per 1 unit of
/// `currency`).
///
/// - If `currency` is "RON" (case-insensitive) the amount is returned unchanged.
/// - If `rate` is `None` the amount is returned unchanged (caller must ensure
///   the rate is validated before generation; see BR-RO-028).
/// - Otherwise returns `amount * rate`, rounded to 2 decimals with COMMERCIAL
///   rounding (MidpointAwayFromZero) — the convention all RON money paths use;
///   `round_dp` (banker's) would diverge at .xx5 midpoints.
pub fn amount_to_ron(amount: Decimal, currency: &str, rate: Option<Decimal>) -> Decimal {
    if currency.eq_ignore_ascii_case("RON") {
        return amount;
    }
    match rate {
        None => amount,
        Some(r) => (amount * r)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero),
    }
}

/// Convert the stored `f64` exchange rate field to `Decimal`.
///
/// Returns `None` when the field is absent or the value is `<= 0`
/// (a non-positive rate is nonsensical and must not be used for conversion).
pub fn parse_rate(rate: Option<f64>) -> Option<Decimal> {
    let r = rate?;
    let d = Decimal::try_from(r).ok()?;
    if d <= Decimal::ZERO {
        None
    } else {
        Some(d)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ron_currency_returns_amount_unchanged() {
        let amt = Decimal::new(10050, 2); // 100.50
        let result = amount_to_ron(amt, "RON", Some(Decimal::new(5, 0)));
        assert_eq!(result, amt, "RON amount must not be converted");
    }

    #[test]
    fn ron_currency_case_insensitive() {
        let amt = Decimal::new(20000, 2); // 200.00
        assert_eq!(amount_to_ron(amt, "ron", Some(Decimal::new(5, 0))), amt);
        assert_eq!(amount_to_ron(amt, "Ron", Some(Decimal::new(5, 0))), amt);
    }

    #[test]
    fn none_rate_returns_amount_unchanged() {
        let amt = Decimal::new(19000, 2); // 190.00
        let result = amount_to_ron(amt, "EUR", None);
        assert_eq!(result, amt, "None rate must not convert the amount");
    }

    #[test]
    fn eur_times_rate_converts_correctly() {
        // 190.00 EUR * 5.0 RON/EUR = 950.00 RON
        let amt = Decimal::new(19000, 2); // 190.00
        let rate = Decimal::new(5, 0); // 5.0
        let result = amount_to_ron(amt, "EUR", Some(rate));
        assert_eq!(
            result,
            Decimal::new(95000, 2),
            "190.00 * 5 must equal 950.00"
        );
    }

    #[test]
    fn rounding_applied_to_two_decimal_places() {
        // 1.005 EUR * 2.0 = 2.010 → rounds to 2.01
        let amt = Decimal::new(1005, 3); // 1.005
        let rate = Decimal::new(2, 0);
        let result = amount_to_ron(amt, "USD", Some(rate));
        assert_eq!(result, Decimal::new(201, 2)); // 2.01
    }

    #[test]
    fn parse_rate_none_input_returns_none() {
        assert!(parse_rate(None).is_none());
    }

    #[test]
    fn parse_rate_zero_returns_none() {
        assert!(parse_rate(Some(0.0)).is_none(), "rate=0 must be rejected");
    }

    #[test]
    fn parse_rate_negative_returns_none() {
        assert!(
            parse_rate(Some(-1.5)).is_none(),
            "negative rate must be rejected"
        );
    }

    #[test]
    fn parse_rate_valid_positive_returns_decimal() {
        let d = parse_rate(Some(5.0)).expect("5.0 is a valid rate");
        assert_eq!(d, Decimal::new(5, 0));
    }

    // ── Property-based invariants (proptest) for the FX money path ─────────────
    // Fiscal money is the highest-stakes surface; example tests only cover cases someone thought of.
    use proptest::prelude::*;

    /// A money amount built from whole bani (2dp), bounded so `amount * rate` never overflows Decimal.
    fn money() -> impl Strategy<Value = Decimal> {
        (-1_000_000_000i64..1_000_000_000i64).prop_map(|bani| Decimal::new(bani, 2))
    }
    /// A strictly-positive exchange rate in 0.0001 .. 100.0000.
    fn pos_rate() -> impl Strategy<Value = Decimal> {
        (1i64..1_000_000i64).prop_map(|r| Decimal::new(r, 4))
    }

    proptest! {
        /// A converted (non-RON) amount always has at most 2 decimal places.
        #[test]
        fn ron_result_has_at_most_two_decimals(amt in money(), r in pos_rate()) {
            let out = amount_to_ron(amt, "EUR", Some(r));
            prop_assert!(out.scale() <= 2, "result {} must have <= 2 dp", out);
        }

        /// The rounded RON value is within half a ban of the exact product (commercial rounding bound).
        #[test]
        fn ron_rounding_within_half_ban(amt in money(), r in pos_rate()) {
            let out = amount_to_ron(amt, "EUR", Some(r));
            let diff = (out - amt * r).abs();
            prop_assert!(diff <= Decimal::new(5, 3), "rounding diff {} must be <= 0.005", diff);
        }

        /// RON currency is always a passthrough, with or without a rate.
        #[test]
        fn ron_currency_is_always_passthrough(amt in money(), r in pos_rate()) {
            prop_assert_eq!(amount_to_ron(amt, "RON", Some(r)), amt);
            prop_assert_eq!(amount_to_ron(amt, "ron", None), amt);
        }

        /// A non-negative amount at a positive rate yields a non-negative RON value (no sign flips).
        #[test]
        fn non_negative_in_non_negative_out(bani in 0i64..1_000_000_000i64, r in pos_rate()) {
            prop_assert!(amount_to_ron(Decimal::new(bani, 2), "EUR", Some(r)) >= Decimal::ZERO);
        }

        /// parse_rate accepts every clearly-positive rate and rejects every non-positive one.
        #[test]
        fn parse_rate_accepts_positive_only(r in 0.001f64..1_000_000.0f64) {
            prop_assert!(parse_rate(Some(r)).is_some());
            prop_assert!(parse_rate(Some(-r)).is_none());
            prop_assert!(parse_rate(Some(0.0)).is_none());
        }
    }
}
