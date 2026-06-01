//! FX helpers — shared currency conversion used by the UBL generator and
//! future report modules.

use rust_decimal::Decimal;

/// Convert an amount in `currency` to RON using `rate` (RON per 1 unit of
/// `currency`).
///
/// - If `currency` is "RON" (case-insensitive) the amount is returned unchanged.
/// - If `rate` is `None` the amount is returned unchanged (caller must ensure
///   the rate is validated before generation; see BR-RO-028).
/// - Otherwise returns `amount * rate`, rounded to 2 decimal places.
pub fn amount_to_ron(amount: Decimal, currency: &str, rate: Option<Decimal>) -> Decimal {
    if currency.eq_ignore_ascii_case("RON") {
        return amount;
    }
    match rate {
        None => amount,
        Some(r) => (amount * r).round_dp(2),
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
}
