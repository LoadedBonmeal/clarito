//! D100 — Declarația privind obligațiile de plată la bugetul de stat (OPANAF 587/2016, model
//! actualizat prin OPANAF 57/2026). Pentru un SME, rândul trimestrial relevant: micro → cod_oblig
//! 121 «Impozit pe veniturile microîntreprinderilor» (1% × venituri); profit → cod_oblig 103
//! «Impozit pe profit» (plata anticipată/regularizare, 16% × rezultat).
//! Suma de plată = suma datorată − plățile anticipate ale perioadelor anterioare; scadența = 25 a
//! lunii următoare trimestrului. Depunerea rămâne manuală (PDF inteligent + SPV).

use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct D100Input {
    /// Trimestrul (1-4).
    pub quarter: u32,
    /// Anul.
    pub year: i32,
    /// Venituri (baza micro) — din P&L.
    #[serde(default)]
    pub revenue: Decimal,
    /// Rezultat brut (baza profit) — din P&L.
    #[serde(default)]
    pub result: Decimal,
    /// Impozitul deja declarat/plătit prin D100 în trimestrele anterioare ale anului.
    #[serde(default)]
    pub prior_payments: Decimal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D100Result {
    pub cod_oblig: String,
    pub label: String,
    pub base: String,
    pub rate_pct: String,
    pub suma_datorata: String,
    pub prior_payments: String,
    pub suma_de_plata: String,
    pub scadenta: String,
}

fn fmt(d: Decimal) -> String {
    let d = d.round_dp(0); // D100 amounts are whole lei.
    let d = if d.is_zero() { Decimal::ZERO } else { d };
    format!("{:.0}", d)
}

/// Scadența: 25 a lunii următoare trimestrului. Q1→25.04, Q2→25.07, Q3→25.10, Q4→25.01 anul următor.
fn scadenta(quarter: u32, year: i32) -> String {
    match quarter {
        1 => format!("25.04.{year}"),
        2 => format!("25.07.{year}"),
        3 => format!("25.10.{year}"),
        _ => format!("25.01.{}", year + 1),
    }
}

/// Compute the D100 quarterly obligation row for the company's tax regime.
pub fn compute_d100(tax_regime: &str, input: &D100Input) -> D100Result {
    let z = Decimal::ZERO;
    let (cod_oblig, label, base, rate, suma_datorata) = if tax_regime == "micro" {
        let base = input.revenue.max(z);
        // Commercial rounding (MidpointAwayFromZero) — the ANAF convention (cf. d112.rs / bilant_xml).
        let s = (base * Decimal::new(1, 2))
            .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero); // 1%
        (
            "121",
            "Impozit pe veniturile microîntreprinderilor (1%)",
            base,
            Decimal::new(1, 2),
            s,
        )
    } else {
        let base = input.result.max(z);
        let s = (base * Decimal::new(16, 2))
            .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero); // 16%
        (
            "103",
            "Impozit pe profit (16%)",
            base,
            Decimal::new(16, 2),
            s,
        )
    };
    let suma_de_plata = (suma_datorata - input.prior_payments).max(z);
    D100Result {
        cod_oblig: cod_oblig.to_string(),
        label: label.to_string(),
        base: fmt(base),
        rate_pct: format!("{}", (rate * Decimal::from(100)).round_dp(0)),
        suma_datorata: fmt(suma_datorata),
        prior_payments: fmt(input.prior_payments),
        suma_de_plata: fmt(suma_de_plata),
        scadenta: scadenta(input.quarter, input.year),
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
    fn micro_quarter_1pct_minus_prior() {
        // Micro, venituri 200.000 → 1% = 2.000; prior 0 → de plată 2.000; scadență 25.04.
        let r = compute_d100(
            "micro",
            &D100Input {
                quarter: 1,
                year: 2026,
                revenue: d("200000"),
                result: d("0"),
                prior_payments: d("0"),
            },
        );
        assert_eq!(r.cod_oblig, "121");
        assert_eq!(r.suma_datorata, "2000");
        assert_eq!(r.suma_de_plata, "2000");
        assert_eq!(r.scadenta, "25.04.2026");
    }

    #[test]
    fn micro_uses_commercial_rounding_at_half() {
        // 250 × 1% = 2.50 → commercial rounding gives 3 (banker's would give 2).
        let r = compute_d100(
            "micro",
            &D100Input {
                quarter: 1,
                year: 2026,
                revenue: d("250"),
                result: d("0"),
                prior_payments: d("0"),
            },
        );
        assert_eq!(r.suma_datorata, "3");
    }

    #[test]
    fn profit_quarter_16pct_minus_prior_clamped() {
        // Profit, rezultat 50.000 → 16% = 8.000; prior 9.000 → de plată max(0, -1.000) = 0; Q4 → 25.01.2027.
        let r = compute_d100(
            "profit",
            &D100Input {
                quarter: 4,
                year: 2026,
                revenue: d("0"),
                result: d("50000"),
                prior_payments: d("9000"),
            },
        );
        assert_eq!(r.cod_oblig, "103");
        assert_eq!(r.suma_datorata, "8000");
        assert_eq!(r.suma_de_plata, "0");
        assert_eq!(r.scadenta, "25.01.2027");
    }
}
