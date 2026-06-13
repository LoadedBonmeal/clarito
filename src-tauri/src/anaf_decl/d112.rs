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
    /// Suma netaxabilă (art. III OUG 89/2025): 300 lei sem. I / 200 lei sem. II 2026, scutită de
    /// impozit ȘI de CAS/CASS/CAM. Se rezolvă cu [`suma_netaxabila`] (0 dacă nu se aplică). Scăzută
    /// din baza de calcul ÎNAINTE de toate cele patru prelevări.
    #[serde(default)]
    pub non_taxable: Decimal,
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
    /// Suma netaxabilă aplicată efectiv (300/200 lei sau 0).
    pub non_taxable: String,
}

fn pct(d: Decimal, (n, s): (i64, u32)) -> Decimal {
    // Contributions/tax rounded to whole lei with COMMERCIAL rounding (half away from zero), the
    // ANAF convention — e.g. 5.000 × 2,25% = 112,5 → 113 (banker's would give 112).
    (d * Decimal::new(n, s)).round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
}
fn fmt(d: Decimal) -> String {
    let d = d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero);
    let d = if d.is_zero() { Decimal::ZERO } else { d };
    format!("{:.2}", d)
}

/// True dacă salariatul e EXCEPTAT de la baza minimă CAS/CASS part-time conform art. 146 alin. (5^7)
/// Cod fiscal (pentru el baza rămâne venitul realizat). Categoriile (lit. a–e), OG 16/2022:
/// a) elevi/studenți până la 26 ani; b) ucenici până la 18 ani; c) persoane cu dizabilități / care
/// pot lucra < 8h/zi potrivit legii; d) pensionari (limită de vârstă) — flagul `pensionar`;
/// e) venit cumulat din mai multe contracte ≥ salariul minim (procedura OMF 1855/2022).
pub fn exempt_part_time_min_base(pensionar: bool, exceptie_cas_min: &str) -> bool {
    pensionar
        || matches!(
            exceptie_cas_min,
            "elev_student" | "ucenic" | "dizabilitate" | "contracte_multiple"
        )
}

/// Part-time (contract Pi) minimum CAS/CASS base override — art. 146 alin. (5^6)-(5^9) + art. 168
/// alin. (6^1) Cod fiscal (OG 16/2022), cu derogarea sumei netaxabile (OUG 156/2024). Baza CAS/CASS
/// nu poate fi sub salariul minim ÎNTREG (NU prorata cu fracția de normă orară). 2026: 4.050−300 =
/// 3.750 lei (sem. I) / 4.325−200 = 4.125 lei (de la 1 iulie, HG 146/2026). Diferența de contribuție
/// față de cea pe venitul realizat e suportată de ANGAJATOR. `exempt` (art. 146 (5^7), via
/// [`exempt_part_time_min_base`]) sare peste majorare — baza rămâne venitul realizat.
///
/// Limitare cunoscută: art. 146 (5^6) permite proratarea bazei minime la numărul zilelor lucrătoare
/// din lună în care contractul a fost ACTIV (angajare/încetare la mijlocul lunii). Aplicația nu
/// urmărește încă data încetării / zilele active, deci se folosește baza minimă ÎNTREAGĂ — o
/// supra-declarare conservatoare (protejează baza de pensie a salariatului). Proratarea pe zile e o
/// extensie ulterioară.
///
/// Returnează Some((baza_minimă, cas_diff_angajator, cass_diff_angajator)) când se aplică majorarea.
pub fn part_time_min_base(
    gross: Decimal,
    tip_contract: &str,
    exempt: bool,
    month: u32,
) -> Option<(Decimal, Decimal, Decimal)> {
    if tip_contract == "N" || exempt || gross <= Decimal::ZERO {
        return None;
    }
    // Baza minimă = salariul minim − suma netaxabilă (NU se prorata cu ore/normă).
    let base = if month <= 6 {
        Decimal::from(3750) // 4.050 − 300
    } else {
        Decimal::from(4125) // 4.325 − 200 (de la 1 iulie 2026, HG 146/2026)
    };
    if gross >= base {
        return None; // venitul realizat ≥ baza minimă → fără majorare.
    }
    let cas_diff = pct(base, CAS_PCT) - pct(gross, CAS_PCT);
    let cass_diff = pct(base, CASS_PCT) - pct(gross, CASS_PCT);
    Some((base, cas_diff, cass_diff))
}

/// Suma netaxabilă din salariul minim — art. III OUG 89/2025 (continuă OUG 156/2024 art. LXVI).
/// 300 lei/lună sem. I 2026 / 200 lei/lună sem. II 2026, scutită de impozit pe venit ȘI de
/// CAS/CASS/CAM (derogare art. 78/139(1)/140/157(1)/220^4(1) Cod fiscal).
///
/// Condiții CUMULATIVE: (a) salariat cu normă întreagă pe CIM (tip_contract "N"); (b) salariul de
/// bază contractual = salariul minim brut în vigoare (4.050 sem. I / 4.325 sem. II); (c) venitul brut
/// realizat (fără tichete/vouchere) ≤ 4.300 sem. I / 4.600 sem. II inclusiv; (d) angajatorul nu a
/// diminuat salariul de bază între 01.01.2026 și 31.12.2026.
///
/// `beneficiar` este ATESTAREA contabilului că (b)+(d) sunt îndeplinite (aplicația nu modelează
/// salariul de bază contractual separat de brut, nici istoricul diminuărilor). Aici aplicăm automat
/// (a) normă întreagă + (c) plafonul brut; restul țin de flag. Întoarce 0 dacă nu se aplică.
///
/// Limitare cunoscută: nu se prorata pe zile pentru luni parțiale (angajare/încetare la mijlocul
/// lunii) — se aplică suma întreagă (conservator), aliniat cu [`part_time_min_base`].
pub fn suma_netaxabila(
    beneficiar: bool,
    tip_contract: &str,
    gross: Decimal,
    month: u32,
) -> Decimal {
    if !beneficiar || tip_contract != "N" || gross <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    let (amount, ceiling) = if month <= 6 {
        (Decimal::from(300), Decimal::from(4300)) // sem. I 2026
    } else {
        (Decimal::from(200), Decimal::from(4600)) // sem. II 2026 (HG 146/2026)
    };
    if gross > ceiling {
        return Decimal::ZERO; // peste plafonul brut → întreaga sumă netaxabilă se pierde
    }
    amount.min(gross)
}

/// Compute one monthly salary state from the gross + personal deduction (2026 rates).
/// `input.non_taxable` (resolved by [`suma_netaxabila`]) is carved out of the base BEFORE CAS, CASS,
/// CAM and income tax (art. III OUG 89/2025).
pub fn compute_payroll(input: &PayrollInput) -> PayrollResult {
    let z = Decimal::ZERO;
    let gross = input.gross.max(z);
    let non_taxable = input.non_taxable.max(z).min(gross);
    // Contribution base = gross − suma netaxabilă; CAS/CASS/CAM all computed on it.
    let contrib_base = (gross - non_taxable).max(z);
    let cas = pct(contrib_base, CAS_PCT);
    let cass = pct(contrib_base, CASS_PCT);
    let after_contrib = gross - cas - cass;
    let deduction = input.personal_deduction.max(z).min(after_contrib.max(z));
    // Income-tax base = venit net − deducere personală − suma netaxabilă (Baza_impozit, FGO/Cod fiscal).
    let taxable_base = (after_contrib - deduction - non_taxable).max(z);
    let income_tax = pct(taxable_base, INCOME_TAX_PCT);
    let net = gross - cas - cass - income_tax;
    let cam = pct(contrib_base, CAM_PCT);
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
        non_taxable: fmt(non_taxable),
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
    fn part_time_min_base_full_minimum_not_prorated() {
        // Part-time P1, gross 3.000, H1 (month 3): baza = salariul minim ÎNTREG 3.750 (NU prorata).
        // cas_diff = 938 − 750 = 188 (pct(3750,25%)=937.5→938); cass_diff = 375 − 300 = 75.
        let r = part_time_min_base(d("3000"), "P1", false, 3);
        assert_eq!(r, Some((d("3750"), d("188"), d("75"))));
        // H2 (month 8): baza 4.125.
        assert_eq!(
            part_time_min_base(d("3000"), "P1", false, 8).unwrap().0,
            d("4125")
        );
        // Full-time N → fără majorare.
        assert_eq!(part_time_min_base(d("3000"), "N", false, 3), None);
        // Exceptat (art. 146 (5^7)) → baza rămâne venitul realizat.
        assert_eq!(part_time_min_base(d("3000"), "P1", true, 3), None);
        // Venit ≥ baza minimă → fără majorare.
        assert_eq!(part_time_min_base(d("4000"), "P1", false, 3), None);
    }

    #[test]
    fn art146_5_7_exemption_categories() {
        // Pensionar (lit. d) + cele 4 categorii cu cod → exceptat; restul → neexceptat.
        assert!(exempt_part_time_min_base(true, ""));
        assert!(exempt_part_time_min_base(false, "elev_student")); // lit. a
        assert!(exempt_part_time_min_base(false, "ucenic")); // lit. b
        assert!(exempt_part_time_min_base(false, "dizabilitate")); // lit. c
        assert!(exempt_part_time_min_base(false, "contracte_multiple")); // lit. e
        assert!(!exempt_part_time_min_base(false, ""));
        assert!(!exempt_part_time_min_base(false, "altceva"));
    }

    #[test]
    fn payroll_2026_rates_gross_to_net() {
        // Gross 5.000, no personal deduction.
        // CAS 25% = 1.250; CASS 10% = 500; base = 5.000 − 1.250 − 500 = 3.250; impozit 10% = 325.
        // Net = 5.000 − 1.250 − 500 − 325 = 2.925. CAM 2,25% = 113 (rounded). Cost = 5.113.
        let r = compute_payroll(&PayrollInput {
            gross: d("5000"),
            personal_deduction: d("0"),
            non_taxable: d("0"),
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
    fn suma_netaxabila_gating() {
        // Sem. I (≤6): 300 lei for a full-time beneficiary; sem. II (≥7): 200 lei.
        assert_eq!(suma_netaxabila(true, "N", d("4050"), 3), d("300"));
        assert_eq!(suma_netaxabila(true, "N", d("4325"), 8), d("200"));
        // Not a beneficiary → 0.
        assert_eq!(suma_netaxabila(false, "N", d("4050"), 3), d("0"));
        // Part-time (Pi) → 0 (measure is full-time only).
        assert_eq!(suma_netaxabila(true, "P1", d("4050"), 3), d("0"));
        // Over the gross ceiling (4.300 H1 / 4.600 H2) → whole benefit lost.
        assert_eq!(suma_netaxabila(true, "N", d("4301"), 3), d("0"));
        assert_eq!(suma_netaxabila(true, "N", d("4500"), 8), d("200")); // 4500 ≤ 4600 H2
        assert_eq!(suma_netaxabila(true, "N", d("4601"), 8), d("0"));
    }

    #[test]
    fn carveout_reduces_all_four_levies() {
        // Full-time min-wage beneficiary, H1: gross 4.050, carve-out 300 → base 3.750.
        // CAS 25%·3750 = 938 (937.5→938); CASS 10%·3750 = 375; CAM 2.25%·3750 = 84 (84.375→84).
        // venit net = 4050 − 938 − 375 = 2737; with deducere 807: base = 2737 − 807 − 300 = 1630;
        // impozit 10% = 163; net = 2737 − 163 = 2574.
        let r = compute_payroll(&PayrollInput {
            gross: d("4050"),
            personal_deduction: d("807"),
            non_taxable: d("300"),
        });
        assert_eq!(r.non_taxable, "300.00");
        assert_eq!(r.cas, "938.00");
        assert_eq!(r.cass, "375.00");
        assert_eq!(r.cam, "84.00"); // on the reduced base 3.750, NOT 4.050
        assert_eq!(r.taxable_base, "1630.00");
        assert_eq!(r.income_tax, "163.00");
        assert_eq!(r.net, "2574.00");
        // Same gross WITHOUT the carve-out over-declares: CAS on full 4.050 = 1013 (> 938).
        let no = compute_payroll(&PayrollInput {
            gross: d("4050"),
            personal_deduction: d("807"),
            non_taxable: d("0"),
        });
        // Without the carve-out CAS is on the full 4.050 (1013 > 938) and tax is higher (183 > 163) —
        // i.e. the missing carve-out OVER-declares. (Compare numerically, not as strings.)
        assert_eq!(no.cas, "1013.00");
        assert_eq!(no.income_tax, "183.00");
    }

    #[test]
    fn personal_deduction_reduces_the_income_tax_base() {
        // Gross 4.050 (min wage H1), deduction 700.
        // CAS 1.013 (4050×0.25=1012.5→1013); CASS 405; after = 4050−1013−405 = 2632.
        // base = 2632 − 700 = 1932; impozit 10% = 193. Net = 2632 − 193 = 2439.
        let r = compute_payroll(&PayrollInput {
            gross: d("4050"),
            personal_deduction: d("700"),
            non_taxable: d("0"),
        });
        assert_eq!(r.cas, "1013.00");
        assert_eq!(r.cass, "405.00");
        assert_eq!(r.taxable_base, "1932.00");
        assert_eq!(r.income_tax, "193.00");
        assert_eq!(r.net, "2439.00");
    }
}
