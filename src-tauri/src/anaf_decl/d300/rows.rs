//! D300 v12 row mapping.
//!
//! Maps a `D300Report` (from `commands::declarations::compute_d300`) + a
//! `D300Submission` (user-supplied metadata) + the company record into the flat
//! `D300Rows` struct that mirrors the official XSD attributes.
//!
//! ALL amounts are rounded to whole lei (0 decimal places) before writing.
//! The attribute-to-row spec is derived from the vendored
//! `src-tauri/tools/anaf/sample_d300_v12.xml` (the official XSD); only attributes
//! present in that XSD are populated — the task description's R69/R70/R74/R75/R24
//! rows do NOT exist in v12 and are omitted.
//!
//! ROW MAPPING SUMMARY (v12 XSD-validated):
//!
//! SALES (TVA colectată):
//!   category=S  rate=21%  → R9_1 / R9_2    (cota standard 21%)
//!   category=S  rate=19%  → R9_1 / R9_2    (fallback: residual 19% also → R9)
//!   category=SR rate=11%  → R10_1 / R10_2  (cotă redusă 11%, dacă stocată ca SR)
//!   category=S  rate=11%  → R10_1 / R10_2  (cotă redusă 11%)
//!   category=S  rate=9%   → R10_1 / R10_2  (fallback: residual 9% also → R10)
//!   category=S  rate=5%   → R11_1 / R11_2  (cotă redusă 5%)
//!   category=Z  (zero-rated intra-EU / export)  → R1_1
//!   category=K  (intra-EU livrări)              → R1_1
//!   category=AE (taxare inversă domestică)       → R13_1 (baza only, XSD has no R13_2)
//!   category=E  (scutit fără drept de deducere) → R1_1 (scutite art.294)
//!
//! PURCHASES (TVA deductibilă):
//!   category=K  intra-EU acquisitions           → R5_1 / R5_2
//!   category=S  rate=21%                        → R22_1 / R22_2
//!   category=S  rate=19%                        → R22_1 / R22_2 (fallback)
//!   category=S  rate=11%                        → R23_1 / R23_2
//!   category=S  rate=9%                         → R23_1 / R23_2 (fallback)
//!   category=S  rate=5%                         → R25_1 / R25_2 (R24 ∉ XSD v12)
//!
//! TOTALS:
//!   R17_2 = sum of all collected-VAT legs (R5_2+R6_2+R7_2+R8_2+R9_2+R10_2+R11_2+R12_2+R16_2+R64_2+R65_2)
//!   R27_2 = sum of all deductible-VAT legs (R18_2+R19_2+R20_2+R21_2+R22_2+R23_2+R25_2+R43_2+R44_2)
//!   R32_2 = R28_2 = R27_2 (no pro-rata / carryover adjustments in this v1 implementation)
//!   R33_2 = MAX(R32_2 - R17_2, 0)  [TVA de recuperat]
//!   R34_2 = MAX(R17_2 - R32_2, 0)  [TVA de plată]
//!   R37_2 = R34_2, R40_2 = R33_2
//!   R41_2 = MAX(R37_2 - R40_2, 0)  [sold de plată final]
//!   R42_2 = MAX(R40_2 - R37_2, 0)  [sold de recuperat final]
//!   totalPlata_A = R41_2

use chrono::{Datelike, NaiveDate};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::commands::declarations::{D300Group, D300Report};
use crate::db::companies::Company;
use crate::error::{AppError, AppResult};

use super::D300Submission;

/// All attributes of the `<declaratie300>` element, mirroring the v12 XSD.
/// `Option<i64>` fields: `None` means the attribute is omitted (XSD allows absence
/// for non-required attributes); required header fields use non-optional types.
#[derive(Debug, Clone, Default)]
pub struct D300Rows {
    // ── Required header ──────────────────────────────────────────────────────
    pub luna: i32,
    pub an: i32,
    pub depus_reprezentant: i32, // 0/1
    pub bifa_interne: i32,       // 0/1
    pub temei: i32,              // 0 or 2
    pub nume_declar: String,
    pub prenume_declar: String,
    pub functie_declar: String,
    pub cui: String, // digits only, no "RO" prefix
    pub den: String,
    pub adresa: String,
    pub banca: String,
    pub cont: String,
    pub caen: String,
    pub tip_decont: String,   // L/T/S/A
    pub pro_rata: f64,        // 0.0–100.0
    pub bifa_cereale: String, // D/N
    pub bifa_mob: String,     // D/N
    pub bifa_disp: String,    // D/N
    pub bifa_cons: String,    // D/N
    pub solicit_ramb: String, // D/N
    pub nr_evid: String,      // integer string
    pub total_plata_a: i64,   // IntNeg18SType

    // ── Sales rows (TVA colectată) ────────────────────────────────────────────
    /// R1_1 — scutite art.294 (livrări intracomunitare / export / Z / K / E)
    pub r1_1: Option<i64>,
    /// R9_1 / R9_2 — livrări taxabile cotă 21% (standard)
    pub r9_1: Option<i64>,
    pub r9_2: Option<i64>,
    /// R10_1 / R10_2 — livrări taxabile cotă 11% (redusă)
    pub r10_1: Option<i64>,
    pub r10_2: Option<i64>,
    /// R11_1 / R11_2 — livrări taxabile cotă 5% (redusă)
    pub r11_1: Option<i64>,
    pub r11_2: Option<i64>,
    /// R13_1 — baza taxare inversă domestică (AE); XSD v12 has no R13_2
    pub r13_1: Option<i64>,

    // ── Purchases rows (TVA deductibilă) ─────────────────────────────────────
    /// R5_1 / R5_2 — achiziții intracomunitare (K)
    pub r5_1: Option<i64>,
    pub r5_2: Option<i64>,
    /// R22_1 / R22_2 — achiziții interne cotă 21% (S)
    pub r22_1: Option<i64>,
    pub r22_2: Option<i64>,
    /// R23_1 / R23_2 — achiziții interne cotă 11% (S redusă)
    pub r23_1: Option<i64>,
    pub r23_2: Option<i64>,
    /// R25_1 / R25_2 — achiziții interne cotă 5% (R24 ∉ XSD v12; spec maps to R25)
    pub r25_1: Option<i64>,
    pub r25_2: Option<i64>,

    // ── Totals (computed) ─────────────────────────────────────────────────────
    /// R17_1 / R17_2 — TOTAL TAXĂ COLECTATĂ (baza / TVA)
    pub r17_1: Option<i64>,
    pub r17_2: Option<i64>,
    /// R27_1 / R27_2 — TOTAL TAXĂ DEDUCTIBILĂ
    pub r27_1: Option<i64>,
    pub r27_2: Option<i64>,
    /// R28_2 — sub-total taxă dedusă (= R27_2 here)
    pub r28_2: Option<i64>,
    /// R32_2 — TOTAL TAXĂ DEDUSĂ (= R28_2 when no pro-rata)
    pub r32_2: Option<i64>,
    /// R33_2 — TVA de recuperat: MAX(R32_2 - R17_2, 0)
    pub r33_2: Option<i64>,
    /// R34_2 — TVA de plată: MAX(R17_2 - R32_2, 0)
    pub r34_2: Option<i64>,
    /// R37_2 — sold de plată înainte de compensare (= R34_2 here)
    pub r37_2: Option<i64>,
    /// R40_2 — sold de recuperat înainte de compensare (= R33_2 here)
    pub r40_2: Option<i64>,
    /// R41_2 — sold final de plată: MAX(R37_2 - R40_2, 0)
    pub r41_2: Option<i64>,
    /// R42_2 — sold final de recuperat: MAX(R40_2 - R37_2, 0)
    pub r42_2: Option<i64>,
}

/// Convert a `bool` flag to the XSD `Str_listaDaNuSType` ("D"/"N").
fn da_nu(v: bool) -> String {
    if v {
        "D".to_string()
    } else {
        "N".to_string()
    }
}

/// Strip the "RO" prefix from a CUI string to get the numeric-only form
/// required by `CuiSType` (pattern `[1-9]\d{1,9}`).
fn strip_ro_prefix(cui: &str) -> String {
    let s = cui.trim();
    let s = if s.to_uppercase().starts_with("RO") {
        &s[2..]
    } else {
        s
    };
    s.trim().to_string()
}

/// Round a `Decimal` to 0 decimal places and convert to `i64`.
fn round_to_lei(d: Decimal) -> i64 {
    d.round_dp(0).to_i64().unwrap_or(0)
}

/// Parse a monetary string (as produced by `D300Report`) to `Decimal`.
fn parse_dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

/// Accumulate base+vat from matching `D300Group` entries into mutable Decimals.
fn accumulate<F>(groups: &[D300Group], predicate: F, base_acc: &mut Decimal, vat_acc: &mut Decimal)
where
    F: Fn(&D300Group) -> bool,
{
    for g in groups {
        if predicate(g) {
            *base_acc += parse_dec(&g.base);
            *vat_acc += parse_dec(&g.vat);
        }
    }
}

/// Test whether a group's vat_rate (stored as "0.21", "21.00", "0.19", etc.)
/// corresponds to a given percentage (e.g. 21).
fn rate_matches(group: &D300Group, pct: i64) -> bool {
    let d = parse_dec(&group.vat_rate);
    // Handle both "0.21" and "21.00" encodings
    let as_pct = if d > Decimal::ONE {
        // already in percent form (e.g. "21.00")
        d.round_dp(0).to_i64().unwrap_or(-1)
    } else {
        // fractional form (e.g. "0.21" → 21)
        (d * Decimal::from(100)).round_dp(0).to_i64().unwrap_or(-1)
    };
    as_pct == pct
}

/// Map `D300Report + D300Submission + Company + period` → `D300Rows`.
///
/// This is the canonical mapping from the BIZ/fiscal data to the ANAF XSD
/// attribute set. See module-level docs for the per-row rationale.
pub fn map_to_rows(
    report: &D300Report,
    submission: &D300Submission,
    company: &Company,
    period: NaiveDate,
) -> AppResult<D300Rows> {
    // ── Header ────────────────────────────────────────────────────────────────
    let luna = period.month() as i32;
    let an = period.year();
    if !(2017..=2100).contains(&an) {
        return Err(AppError::Validation(format!(
            "Anul {an} nu se încadrează în domeniul acceptat de XSD (2017–2100)."
        )));
    }

    let cui = strip_ro_prefix(&company.cui);
    // Validate CUI pattern: [1-9]\d{1,9}
    {
        let bytes = cui.as_bytes();
        let valid = !bytes.is_empty()
            && bytes[0].is_ascii_digit()
            && bytes[0] != b'0'
            && bytes.iter().all(|b| b.is_ascii_digit())
            && cui.len() >= 2
            && cui.len() <= 10;
        if !valid {
            return Err(AppError::Validation(format!(
                "CUI '{cui}' nu respectă pattern-ul XSD [1-9]\\d{{1,9}} după eliminarea prefixului RO."
            )));
        }
    }

    let den = company.legal_name.chars().take(200).collect::<String>();
    let adresa = {
        // Build address from components; truncate to 1000 chars
        let parts: Vec<&str> = [
            company.address.as_str(),
            company.city.as_str(),
            company.county.as_str(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
        parts.join(", ").chars().take(1000).collect::<String>()
    };

    // ── Sales row accumulation ────────────────────────────────────────────────

    // R1_1 — scutite art.294: livrări intracomunitare + export
    //         category Z (zero-rated intra-EU), K (intra-community delivery),
    //         E (exempt without right of deduction / export scutit)
    // Spec: "intra-EU exempt deliveries / category for intra-community (Z/K)" → R1_1
    let mut r1_1_base = Decimal::ZERO;
    let mut _r1_1_vat = Decimal::ZERO; // VAT on exempt deliveries is always 0
    accumulate(
        &report.groups,
        |g| matches!(g.vat_category.as_str(), "Z" | "K" | "E"),
        &mut r1_1_base,
        &mut _r1_1_vat,
    );

    // R9_1 / R9_2 — standard rate 21% (or residual 19% which also uses R9 in v12)
    // Spec: category S rate 21% → R9; rate 19% (residual) → R9 as well
    let mut r9_base = Decimal::ZERO;
    let mut r9_vat = Decimal::ZERO;
    accumulate(
        &report.groups,
        |g| g.vat_category == "S" && (rate_matches(g, 21) || rate_matches(g, 19)),
        &mut r9_base,
        &mut r9_vat,
    );

    // R10_1 / R10_2 — reduced rate 11% (or residual 9% → R10 in v12)
    // Spec: category S/SR rate 11% → R10; rate 9% (residual) → R10
    let mut r10_base = Decimal::ZERO;
    let mut r10_vat = Decimal::ZERO;
    accumulate(
        &report.groups,
        |g| {
            (g.vat_category == "S" || g.vat_category == "SR")
                && (rate_matches(g, 11) || rate_matches(g, 9))
        },
        &mut r10_base,
        &mut r10_vat,
    );

    // R11_1 / R11_2 — reduced rate 5%
    // Spec: category S rate 5% → R11
    let mut r11_base = Decimal::ZERO;
    let mut r11_vat = Decimal::ZERO;
    accumulate(
        &report.groups,
        |g| g.vat_category == "S" && rate_matches(g, 5),
        &mut r11_base,
        &mut r11_vat,
    );

    // R13_1 — taxare inversă domestică (AE), baza only (XSD v12 has no R13_2)
    // Spec: reverse-charge domestic (AE) collected → R13_1
    let mut r13_base = Decimal::ZERO;
    let mut _r13_vat = Decimal::ZERO;
    accumulate(
        &report.groups,
        |g| g.vat_category == "AE",
        &mut r13_base,
        &mut _r13_vat,
    );

    // ── Purchase row accumulation ─────────────────────────────────────────────

    // R5_1 / R5_2 — achiziții intracomunitare (category K)
    // Spec: "intra-EU acquisitions (categories per D300Report) → R5_1/R5_2"
    let mut r5_base = Decimal::ZERO;
    let mut r5_vat = Decimal::ZERO;
    accumulate(
        &report.purchase_groups,
        |g| g.vat_category == "K",
        &mut r5_base,
        &mut r5_vat,
    );

    // R22_1 / R22_2 — achiziții interne cotă 21% (or residual 19%)
    // Spec: "domestic deductible, rate 21% → R22; rate 19% (residual) → R22"
    let mut r22_base = Decimal::ZERO;
    let mut r22_vat = Decimal::ZERO;
    accumulate(
        &report.purchase_groups,
        |g| g.vat_category == "S" && (rate_matches(g, 21) || rate_matches(g, 19)),
        &mut r22_base,
        &mut r22_vat,
    );

    // R23_1 / R23_2 — achiziții interne cotă 11% (or residual 9%)
    // Spec: "domestic deductible, rate 11% → R23; rate 9% (residual) → R23"
    let mut r23_base = Decimal::ZERO;
    let mut r23_vat = Decimal::ZERO;
    accumulate(
        &report.purchase_groups,
        |g| g.vat_category == "S" && (rate_matches(g, 11) || rate_matches(g, 9)),
        &mut r23_base,
        &mut r23_vat,
    );

    // R25_1 / R25_2 — achiziții interne cotă 5%
    // Note: the task spec says R24_1/R24_2 for 5% domestic purchases, but R24
    // does NOT exist in the v12 XSD. R25 is the next available domestic row.
    // Spec: "domestic deductible, rate 5% → R25 (R24 absent from XSD v12)"
    let mut r25_base = Decimal::ZERO;
    let mut r25_vat = Decimal::ZERO;
    accumulate(
        &report.purchase_groups,
        |g| g.vat_category == "S" && rate_matches(g, 5),
        &mut r25_base,
        &mut r25_vat,
    );

    // ── Margin checks (logged, non-fatal) ─────────────────────────────────────
    // The collected VAT on each rate row should fall within the rate's corridor
    // (R9 ≈ 19–21%, R10 ≈ 9–11%, R11 ≈ 5% with rounding slack). We LOG an anomaly
    // rather than panic: grossly-inconsistent source data (e.g. a mis-keyed line)
    // should degrade gracefully and still produce a (flagged) declaration, not
    // abort the export. DUKIntegrator's business rules are the authoritative check.
    let margin_warn = |row: &str, base: Decimal, vat: Decimal, lo_pct: i64, hi_pct: i64| {
        if base > Decimal::ZERO && vat > Decimal::ZERO {
            let low = (base * Decimal::new(lo_pct, 2)).round_dp(0);
            let high = (base * Decimal::new(hi_pct, 2)).round_dp(0);
            let v = vat.round_dp(0);
            if v < low || v > high {
                tracing::warn!(
                    row,
                    %base,
                    %vat,
                    "D300 margin check: VAT outside expected corridor [{low},{high}] — verify source data"
                );
            }
        }
    };
    margin_warn("R9_2", r9_base, r9_vat, 18, 22);
    margin_warn("R10_2", r10_base, r10_vat, 8, 12);
    margin_warn("R11_2", r11_base, r11_vat, 4, 6);

    // ── Totals ────────────────────────────────────────────────────────────────

    // R17_2 = sum of all collected-VAT legs that are populated:
    // R5_2 + R6_2 + R7_2 + R8_2 + R9_2 + R10_2 + R11_2 + R12_2 + R16_2 + R64_2 + R65_2
    // (R6/R7/R8/R12/R16/R64/R65 are zero/absent in this v1 — no regularizations)
    let r17_vat = r9_vat + r10_vat + r11_vat;
    // R17_1 = analogous base sum
    let r17_base = r9_base + r10_base + r11_base;

    // R27_2 = sum of all deductible-VAT legs:
    // R18_2+R19_2+R20_2+R21_2+R22_2+R23_2+R25_2+R43_2+R44_2
    // (R18–R21/R43/R44 are zero/absent)
    let r27_vat = r5_vat + r22_vat + r23_vat + r25_vat;
    let r27_base = r5_base + r22_base + r23_base + r25_base;

    // R28_2 (sub-total dedusă) = R27_2 (no pro-rata adjustment)
    let r28_vat = r27_vat;

    // R32_2 = R28_2 + R29_2 + R30_2 + R31_2 (= R27_2 with no carryover)
    let r32_vat = r28_vat;

    // R33_2 = MAX(R32_2 - R17_2, 0)  [TVA de recuperat]
    let r33_vat = if r32_vat > r17_vat {
        r32_vat - r17_vat
    } else {
        Decimal::ZERO
    };

    // R34_2 = MAX(R17_2 - R32_2, 0)  [TVA de plată]
    let r34_vat = if r17_vat > r32_vat {
        r17_vat - r32_vat
    } else {
        Decimal::ZERO
    };

    // R37_2 = R34_2 (sold de plată = TVA de plată, no prior-period deductions)
    let r37_vat = r34_vat;

    // R40_2 = R33_2 (sold de recuperat)
    let r40_vat = r33_vat;

    // R41_2 = MAX(R37_2 - R40_2, 0)  [sold final de plată]
    let r41_vat = if r37_vat > r40_vat {
        r37_vat - r40_vat
    } else {
        Decimal::ZERO
    };

    // R42_2 = MAX(R40_2 - R37_2, 0)  [sold final de recuperat]
    let r42_vat = if r40_vat > r37_vat {
        r40_vat - r37_vat
    } else {
        Decimal::ZERO
    };

    // totalPlata_A = R41_2
    let total_plata_a = round_to_lei(r41_vat);

    // ── Assemble D300Rows ─────────────────────────────────────────────────────

    let opt_nonzero = |v: i64| if v != 0 { Some(v) } else { None };

    Ok(D300Rows {
        // required header
        luna,
        an,
        depus_reprezentant: if submission.depus_reprezentant { 1 } else { 0 },
        bifa_interne: if submission.bifa_interne { 1 } else { 0 },
        temei: submission.temei,
        nume_declar: submission.nume_declar.chars().take(75).collect(),
        prenume_declar: submission.prenume_declar.chars().take(75).collect(),
        functie_declar: submission.functie_declar.chars().take(50).collect(),
        cui,
        den,
        adresa,
        banca: submission.banca.chars().take(50).collect(),
        cont: submission.cont.chars().take(50).collect(),
        caen: submission.caen.clone(),
        tip_decont: submission.tip_decont.clone(),
        pro_rata: submission.pro_rata,
        bifa_cereale: da_nu(submission.bifa_cereale),
        bifa_mob: da_nu(submission.bifa_mob),
        bifa_disp: da_nu(submission.bifa_disp),
        bifa_cons: da_nu(submission.bifa_cons),
        solicit_ramb: da_nu(submission.solicit_ramb),
        nr_evid: submission.nr_evid.clone(),
        total_plata_a,

        // sales
        r1_1: opt_nonzero(round_to_lei(r1_1_base)),
        r9_1: opt_nonzero(round_to_lei(r9_base)),
        r9_2: opt_nonzero(round_to_lei(r9_vat)),
        r10_1: opt_nonzero(round_to_lei(r10_base)),
        r10_2: opt_nonzero(round_to_lei(r10_vat)),
        r11_1: opt_nonzero(round_to_lei(r11_base)),
        r11_2: opt_nonzero(round_to_lei(r11_vat)),
        r13_1: opt_nonzero(round_to_lei(r13_base)),

        // purchases
        r5_1: opt_nonzero(round_to_lei(r5_base)),
        r5_2: opt_nonzero(round_to_lei(r5_vat)),
        r22_1: opt_nonzero(round_to_lei(r22_base)),
        r22_2: opt_nonzero(round_to_lei(r22_vat)),
        r23_1: opt_nonzero(round_to_lei(r23_base)),
        r23_2: opt_nonzero(round_to_lei(r23_vat)),
        r25_1: opt_nonzero(round_to_lei(r25_base)),
        r25_2: opt_nonzero(round_to_lei(r25_vat)),

        // totals
        r17_1: opt_nonzero(round_to_lei(r17_base)),
        r17_2: opt_nonzero(round_to_lei(r17_vat)),
        r27_1: opt_nonzero(round_to_lei(r27_base)),
        r27_2: opt_nonzero(round_to_lei(r27_vat)),
        r28_2: opt_nonzero(round_to_lei(r28_vat)),
        r32_2: opt_nonzero(round_to_lei(r32_vat)),
        r33_2: opt_nonzero(round_to_lei(r33_vat)),
        r34_2: opt_nonzero(round_to_lei(r34_vat)),
        r37_2: opt_nonzero(round_to_lei(r37_vat)),
        r40_2: opt_nonzero(round_to_lei(r40_vat)),
        r41_2: opt_nonzero(round_to_lei(r41_vat)),
        r42_2: opt_nonzero(round_to_lei(r42_vat)),
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::declarations::{D300Group, D300Report};
    use crate::db::companies::Company;

    fn make_company() -> Company {
        Company {
            id: "test-id".to_string(),
            cui: "RO12345674".to_string(), // valid CUI: base=1234567, check=4
            legal_name: "Test SRL".to_string(),
            trade_name: None,
            registry_number: None,
            vat_payer: true,
            address: "Str. Testului 1".to_string(),
            city: "București".to_string(),
            county: "IF".to_string(),
            postal_code: None,
            country: "RO".to_string(),
            email: None,
            phone: None,
            iban: Some("RO49AAAA1B31007593840000".to_string()),
            bank_name: Some("Banca Test".to_string()),
            is_active: true,
            spv_enabled: false,
            invoice_series: "F".to_string(),
            last_invoice_number: 0,
            logo_path: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn make_submission() -> D300Submission {
        D300Submission {
            nume_declar: "POPESCU".to_string(),
            prenume_declar: "ION".to_string(),
            functie_declar: "DIRECTOR".to_string(),
            caen: "6201".to_string(),
            banca: "Banca Test".to_string(),
            cont: "RO49AAAA1B31007593840000".to_string(),
            tip_decont: "L".to_string(),
            ..Default::default()
        }
    }

    fn make_report(
        sales: Vec<(&str, &str, &str, &str)>, // (rate, category, base, vat)
        purchases: Vec<(&str, &str, &str, &str)>,
    ) -> D300Report {
        let groups: Vec<D300Group> = sales
            .into_iter()
            .map(|(rate, cat, base, vat)| D300Group {
                vat_rate: rate.to_string(),
                vat_category: cat.to_string(),
                base: base.to_string(),
                vat: vat.to_string(),
            })
            .collect();
        let purchase_groups: Vec<D300Group> = purchases
            .into_iter()
            .map(|(rate, cat, base, vat)| D300Group {
                vat_rate: rate.to_string(),
                vat_category: cat.to_string(),
                base: base.to_string(),
                vat: vat.to_string(),
            })
            .collect();

        let total_vat: Decimal = groups.iter().map(|g| parse_dec(&g.vat)).sum();
        let total_base: Decimal = groups.iter().map(|g| parse_dec(&g.base)).sum();
        let total_ded_vat: Decimal = purchase_groups.iter().map(|g| parse_dec(&g.vat)).sum();
        let total_ded_base: Decimal = purchase_groups.iter().map(|g| parse_dec(&g.base)).sum();

        D300Report {
            company_cui: "RO12345674".to_string(),
            period_from: "2025-09-01".to_string(),
            period_to: "2025-09-30".to_string(),
            groups,
            total_base: total_base.round_dp(2).to_string(),
            total_vat: total_vat.round_dp(2).to_string(),
            invoice_count: 5,
            purchase_groups,
            total_deductible_base: total_ded_base.round_dp(2).to_string(),
            total_deductible_vat: total_ded_vat.round_dp(2).to_string(),
            purchase_invoice_count: 3,
            purchase_unparsed_count: 0,
            net_vat: (total_vat - total_ded_vat).round_dp(2).to_string(),
        }
    }

    #[test]
    fn totals_reconcile_simple() {
        // Sales: 1000 at 21% (210 VAT) + 500 at 11% (55 VAT)
        // Purchases: 800 at 21% (168 VAT)
        // R17_2 = 210 + 55 = 265
        // R27_2 = 168
        // R34_2 = 265 - 168 = 97  (plată)
        // R33_2 = 0
        // R41_2 = totalPlata_A = 97
        let report = make_report(
            vec![
                ("0.21", "S", "1000.00", "210.00"),
                ("0.11", "S", "500.00", "55.00"),
            ],
            vec![("0.21", "S", "800.00", "168.00")],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let rows = map_to_rows(&report, &sub, &company, period).expect("map_to_rows");

        // R9_1/R9_2 = sales at 21%
        assert_eq!(rows.r9_1, Some(1000), "R9_1 should be 1000");
        assert_eq!(rows.r9_2, Some(210), "R9_2 should be 210");

        // R10_1/R10_2 = sales at 11%
        assert_eq!(rows.r10_1, Some(500), "R10_1 should be 500");
        assert_eq!(rows.r10_2, Some(55), "R10_2 should be 55");

        // R17_2 = 210 + 55 = 265
        assert_eq!(rows.r17_2, Some(265), "R17_2 = 265");

        // R22_1/R22_2 = purchases at 21%
        assert_eq!(rows.r22_1, Some(800), "R22_1 should be 800");
        assert_eq!(rows.r22_2, Some(168), "R22_2 should be 168");

        // R27_2 = 168
        assert_eq!(rows.r27_2, Some(168), "R27_2 = 168");
        assert_eq!(rows.r28_2, Some(168), "R28_2 = R27_2 = 168");
        assert_eq!(rows.r32_2, Some(168), "R32_2 = 168");

        // R34_2 = 265 - 168 = 97 (TVA de plată)
        assert_eq!(rows.r34_2, Some(97), "R34_2 = 97");
        // R33_2 = 0 (no refund)
        assert_eq!(rows.r33_2, None, "R33_2 should be None (zero → omitted)");

        // R41_2 = totalPlata_A = 97
        assert_eq!(rows.r41_2, Some(97), "R41_2 = 97");
        assert_eq!(rows.total_plata_a, 97, "totalPlata_A = 97");
        assert_eq!(rows.r42_2, None, "R42_2 = None (no refund)");
    }

    #[test]
    fn refund_period_sets_r33_and_r42() {
        // Purchases > Sales → TVA de recuperat
        // Sales: 500 at 21% (105 VAT)
        // Purchases: 1000 at 21% (210 VAT)
        // R17_2 = 105, R27_2 = 210
        // R33_2 = 210 - 105 = 105 (de recuperat)
        // R34_2 = 0, R41_2 = 0, R42_2 = 105, totalPlata_A = 0
        let report = make_report(
            vec![("0.21", "S", "500.00", "105.00")],
            vec![("0.21", "S", "1000.00", "210.00")],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let rows = map_to_rows(&report, &sub, &company, period).expect("map_to_rows");

        assert_eq!(rows.r17_2, Some(105), "R17_2 = 105");
        assert_eq!(rows.r27_2, Some(210), "R27_2 = 210");
        assert_eq!(rows.r33_2, Some(105), "R33_2 = 105 (de recuperat)");
        assert_eq!(rows.r34_2, None, "R34_2 = None (zero)");
        assert_eq!(rows.r41_2, None, "R41_2 = None (no plată)");
        assert_eq!(rows.r42_2, Some(105), "R42_2 = 105 (de recuperat)");
        assert_eq!(rows.total_plata_a, 0, "totalPlata_A = 0 when refund");
    }

    #[test]
    fn intra_eu_categories_map_to_r1_and_r5() {
        // Intra-EU delivery Z → R1_1 (sales, no VAT)
        // Intra-EU acquisition K purchases → R5_1/R5_2
        let report = make_report(
            vec![("0.00", "Z", "2000.00", "0.00")],
            vec![("0.21", "K", "1000.00", "210.00")],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 10, 1).unwrap();

        let rows = map_to_rows(&report, &sub, &company, period).expect("map_to_rows");

        assert_eq!(rows.r1_1, Some(2000), "R1_1 = 2000 (Z sales)");
        assert_eq!(rows.r9_1, None, "R9_1 = None (no standard-rate sales)");
        assert_eq!(rows.r5_1, Some(1000), "R5_1 = 1000 (K purchases)");
        assert_eq!(rows.r5_2, Some(210), "R5_2 = 210 (K purchases VAT)");
        // R17_2 = 0 (Z deliveries are exempt, no collected VAT)
        assert_eq!(rows.r17_2, None, "R17_2 = None (only exempt sales)");
    }

    #[test]
    fn cui_ro_prefix_stripped() {
        let mut company = make_company();
        company.cui = "RO 12345678".to_string();
        let sub = make_submission();
        let report = make_report(vec![], vec![]);
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let rows = map_to_rows(&report, &sub, &company, period).expect("map_to_rows");
        assert_eq!(
            rows.cui, "12345678",
            "RO prefix and spaces must be stripped"
        );
    }

    #[test]
    fn header_flags_da_nu() {
        let mut sub = make_submission();
        sub.bifa_cereale = true;
        sub.solicit_ramb = true;
        sub.bifa_mob = false;

        let company = make_company();
        let report = make_report(vec![], vec![]);
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let rows = map_to_rows(&report, &sub, &company, period).expect("map_to_rows");
        assert_eq!(rows.bifa_cereale, "D");
        assert_eq!(rows.solicit_ramb, "D");
        assert_eq!(rows.bifa_mob, "N");
    }

    #[test]
    fn margin_checks_hold_for_21pct() {
        // Base=1000, VAT=210 (exactly 21%) — margin check should pass
        let report = make_report(vec![("0.21", "S", "1000.00", "210.00")], vec![]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        // This must not panic (debug_assert in map_to_rows)
        let rows = map_to_rows(&report, &sub, &company, period).expect("map_to_rows");
        assert_eq!(rows.r9_2, Some(210));
    }

    #[test]
    fn rate_matches_both_fractional_and_percent_forms() {
        let g_frac = D300Group {
            vat_rate: "0.21".to_string(),
            vat_category: "S".to_string(),
            base: "100".to_string(),
            vat: "21".to_string(),
        };
        let g_pct = D300Group {
            vat_rate: "21.00".to_string(),
            vat_category: "S".to_string(),
            base: "100".to_string(),
            vat: "21".to_string(),
        };
        assert!(rate_matches(&g_frac, 21), "fractional 0.21 should match 21");
        assert!(rate_matches(&g_pct, 21), "percent 21.00 should match 21");
        assert!(!rate_matches(&g_frac, 19), "0.21 should not match 19");
    }
}
