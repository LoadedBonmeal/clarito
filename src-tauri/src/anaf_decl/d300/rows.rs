//! D300 v12 row mapping.
//!
//! Maps a `D300Report` (from `commands::declarations::compute_d300`) + a
//! `D300Submission` (user-supplied metadata) + the company record into the flat
//! `D300Rows` struct that mirrors the official XSD attributes.
//!
//! ALL amounts are rounded to whole lei (0 decimal places) before writing.
//! The attribute-to-row spec is derived from the vendored
//! `src-tauri/tools/anaf/sample_d300_v12.xml` (the official XSD, namespace
//! `mfp:anaf:dgti:d300:declaratie:v12` version 1.02) together with
//! OPANAF 174/2026 and the DUKIntegrator business-rule validation.
//! Only attributes present in that XSD are populated — R69/R70/R71/R72/R74/R75
//! rows do NOT exist in v12 and are omitted.
//!
//! # ROW MAPPING SUMMARY (v12 XSD-validated, OPANAF 174/2026)
//!
//! ## SALES (TVA colectată)
//!
//! | Category | Rate     | Base row   | VAT row    | Notes                                      |
//! |----------|----------|------------|------------|--------------------------------------------|
//! | S        | 21%      | R9_1       | R9_2       | Cota standard, DUK margin 20–22%           |
//! | S        | 11%      | R10_1      | R10_2      | Cotă redusă 11%, DUK margin 8–10%         |
//! | S        | 9%       | R11_1      | R11_2      | Cotă redusă 9% (from 2026), DUK 8–10%     |
//! | S        | 19%/5%   | R16_1      | R16_2      | Regularizări cote vechi (Wave 8)           |
//! | Z/K/E    | 0%       | R1_1       | —          | Scutite art.294 (intra-EU / export)        |
//! | AE       | 21%      | R12_1_1    | R12_1_2    | Beneficiar taxare inversă 21%              |
//! | AE       | 11%      | R12_2_1    | R12_2_2    | Beneficiar taxare inversă 11%              |
//! | AE (Σ)   | —        | R12_1      | R12_2      | Sum of all AE sub-rows (parents)           |
//!
//! ## PURCHASES (TVA deductibilă)
//!
//! | Category | Kind     | Rate          | Base row   | VAT row    | Notes                               |
//! |----------|----------|---------------|------------|------------|-------------------------------------|
//! | K        | goods    | 21%           | R5_1 / R18_1 | R5_2 / R18_2 | Intra-EU bunuri; R18=R5        |
//! | K        | services | 21%           | R7_1 / R20_1 | R7_2 / R20_2 | Intra-EU servicii; R20=R7     |
//! | S        | —        | 21%           | R22_1      | R22_2      | Deductibil intern cotă standard     |
//! | S        | —        | 11%           | R23_1      | R23_2      | Deductibil intern cotă redusă 11%   |
//! | S        | —        | 19%/9%/5%     | R30_1      | R30_2      | Regularizări cote vechi (Wave 8)    |
//! | AE       | —        | 21%           | R25_1_1    | R25_1_2    | Mirror R12_1_1/R12_1_2              |
//! | AE       | —        | 11%           | R25_2_1    | R25_2_2    | Mirror R12_2_1/R12_2_2              |
//! | AE (Σ)   | —        | —             | R25_1      | R25_2      | = R12_1 / R12_2 (DUK enforced)     |
//!
//! ## DUK EQUALITY CONSTRAINTS (schema enforced, violations = E: errors)
//!
//! * R25_1 = R12_1  (V_19) — deductibil = colectat (base)
//! * R25_2 = R12_2  (V_20) — deductibil = colectat (VAT)
//! * R25_1_1 = R12_1_1 (V_21)
//! * R25_1_2 = R12_1_2 (V_22)
//! * R25_2_1 = R12_2_1 (V_23)
//! * R25_2_2 = R12_2_2 (V_24)
//! * R18_1 = R5_1  (V_7)  — intra-EU goods deductible = collected
//! * R18_2 = R5_2  (V_8)
//! * R20_1 = R7_1  (V_13) — intra-EU services deductible = collected
//! * R20_2 = R7_2  (V_14)
//! * R20_1_1 = R7_1_1 (V_15)
//!
//! ## TOTALS
//!
//! R17_2 = R5_2 + R7_2 + R9_2 + R10_2 + R11_2 + R12_2 + R16_2 + R64_2 + R65_2
//!   (R6/R8/R64/R65 absent; R7 added Wave 7; R16 added Wave 8)
//! R27_2 = R18_2 + R20_2 + R22_2 + R23_2 + R25_2 + R43_2 + R44_2
//!   (R19/R21/R43/R44 absent; R20 added Wave 7; R30 does NOT feed R27)
//! R28_2 = R27_2 (no pro-rata)
//! R32_2 = R28_2 + R30_2   (regularizări dedusă feeds R32 directly — DUK R108)
//!
//! ## REGULARIZĂRI (Wave 8 — OPANAF 174/2026)
//!
//! Per OPANAF 174/2026 the 2026 D300 has NO dedicated rows for old VAT rates.
//! Old-rate operations (sales 19%/5%, purchases 19%/9%/5%, category S) are
//! auto-included in the regularizări rows:
//!
//! - R16_1/R16_2 — regularizări taxă colectată (Rd.16 in printed form)
//! - R30_1/R30_2 — regularizări taxă dedusă (Rd.32/Rd.33 in printed form)
//!
//! Both rows are type IntNeg15SType (signed; no rate-margin DUK rule applies).
//! The values are auto-computed from the `D300Report.reg_*` fields and can be
//! overridden via `D300Submission.reg_*` (optional i64). The accountant is
//! advised to verify via the preflight warning `D300_COTE_VECHI`.
//!
//! NOTE: 9% purchases still do NOT go into R23 (the 11% row; DUK corridor 10–12%
//! rejects a 9% ratio). They flow into R30 as regularizări instead.
//!
//! * Intra-EU acquisitions of SERVICES (category K, intra_eu_kind="services"):
//!   Wave 7: mapped to R7/R20 (services rows). DUK V_13/V_14: R20=R7.
//!   Goods acquisitions (intra_eu_kind="goods" or default): R5/R18 (unchanged).

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
    /// R11_1 / R11_2 — livrări taxabile cotă 9% (from 2026; was 5% pre-2026)
    pub r11_1: Option<i64>,
    pub r11_2: Option<i64>,
    /// R12_1 / R12_2 — TOTAL taxare inversă domestică (AE) beneficiar
    ///   R12_1 = Σ base (R12_1_1 + R12_2_1)
    ///   R12_2 = Σ VAT  (R12_1_2 + R12_2_2)
    pub r12_1: Option<i64>,
    pub r12_2: Option<i64>,
    /// R12_1_1 / R12_1_2 — sub-rând 21% taxare inversă (baza / TVA)
    pub r12_1_1: Option<i64>,
    pub r12_1_2: Option<i64>,
    /// R12_2_1 / R12_2_2 — sub-rând 11% taxare inversă (baza / TVA)
    pub r12_2_1: Option<i64>,
    pub r12_2_2: Option<i64>,
    /// R13_1 — livrări taxare inversă outbound (vânzător); XSD v12 has no R13_2.
    ///   SELLER side of domestic reverse charge (art.331); base only, no VAT column.
    pub r13_1: Option<i64>,
    /// R16_1 / R16_2 — regularizări taxă colectată (Rd.16 in printed form).
    ///   Populated from old-rate S sales (19%/5%) — auto-computed or overridden.
    ///   Type IntNeg15SType (signed, no rate-margin DUK rule). Included in R17.
    pub r16_1: Option<i64>,
    pub r16_2: Option<i64>,

    // ── Purchase rows (TVA deductibilă) ─────────────────────────────────────
    /// R5_1 / R5_2 — achiziții intracomunitare de BUNURI (category K, intra_eu_kind=goods)
    pub r5_1: Option<i64>,
    pub r5_2: Option<i64>,
    /// R18_1 / R18_2 — deductibil corespunzător R5 (goods); DUK enforces R18=R5 (V_7/V_8).
    pub r18_1: Option<i64>,
    pub r18_2: Option<i64>,
    /// R7_1 / R7_2 — achiziții intracomunitare de SERVICII (category K, intra_eu_kind=services)
    ///   Collected leg of intra-EU services reverse charge. DUK V_13: R20_1=R7_1, V_14: R20_2=R7_2.
    pub r7_1: Option<i64>,
    pub r7_2: Option<i64>,
    /// R7_1_1 / R7_1_2 — sub-rows for R7 at rate 21% (mirrors R5_1_1/R5_1_2 structure).
    ///   DUK V_15: R20_1_1=R7_1_1. Populated when there are 21% K-services.
    pub r7_1_1: Option<i64>,
    pub r7_1_2: Option<i64>,
    /// R20_1 / R20_2 — deductibil corespunzător R7 (services); DUK enforces R20=R7 (V_13/V_14).
    pub r20_1: Option<i64>,
    pub r20_2: Option<i64>,
    /// R20_1_1 / R20_1_2 — mirror of R7_1_1/R7_1_2 (DUK V_15).
    pub r20_1_1: Option<i64>,
    pub r20_1_2: Option<i64>,
    /// R22_1 / R22_2 — achiziții interne cotă 21% (S)
    pub r22_1: Option<i64>,
    pub r22_2: Option<i64>,
    /// R23_1 / R23_2 — achiziții interne cotă 11% (S redusă)
    pub r23_1: Option<i64>,
    pub r23_2: Option<i64>,
    /// R25_1 / R25_2 — TOTAL deductibil taxare inversă domestică (AE)
    ///   DUK enforces R25_1 = R12_1, R25_2 = R12_2 (V_19/V_20).
    pub r25_1: Option<i64>,
    pub r25_2: Option<i64>,
    /// R25_1_1 / R25_1_2 — mirror of R12_1_1/R12_1_2 (DUK V_21/V_22)
    pub r25_1_1: Option<i64>,
    pub r25_1_2: Option<i64>,
    /// R25_2_1 / R25_2_2 — mirror of R12_2_1/R12_2_2 (DUK V_23/V_24)
    pub r25_2_1: Option<i64>,
    pub r25_2_2: Option<i64>,
    /// R30_1 / R30_2 — regularizări taxă dedusă (Rd.32/Rd.33 in printed form).
    ///   Populated from old-rate S purchases (19%/9%/5%) — auto-computed or overridden.
    ///   Type IntNeg15SType (signed, no rate-margin DUK rule). R30_2 feeds R32
    ///   (DUK R108), NOT R27 — see the TOTALS block above.
    pub r30_1: Option<i64>,
    pub r30_2: Option<i64>,

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

/// Generate a valid 23-character Număr de Evidență a Plății (NDP / nr_evid).
///
/// Structure (0-indexed positions, 1-based in spec):
/// - pos  0-1  : "10"
/// - pos  2-4  : obligation code — tip_decont L→"301", T→"302", S→"303", A→"304"
/// - pos  5-6  : "01"
/// - pos  7-8  : zero-padded luna (reporting month, 2 digits)
/// - pos  9-10 : last 2 digits of an (reporting year)
/// - pos 11-12 : "25" (day of payment)
/// - pos 13-14 : zero-padded luna+1 (month of due-date; rolls over to 01 if luna==12)
/// - pos 15-16 : last 2 digits of due-year (an+1 if luna==12, else an)
/// - pos 17-20 : "0000"
/// - pos 21-22 : check digits = zero-padded sum of digit-values of positions 0-20 mod 100
///
/// Validator composite check: chars[0..2] + chars[5..7] + chars[17..21] == "10010000"
pub fn generate_ndp(tip_decont: &str, luna: i32, an: i32) -> String {
    let obligation_code = match tip_decont {
        "T" => "302",
        "S" => "303",
        "A" => "304",
        _ => "301", // "L" and any unknown → monthly
    };

    let ll = luna % 100; // reporting month (1–12)
    let aa = an % 100; // last 2 digits of reporting year

    // Payment due date: 25th of the month following the reporting month
    let (due_month, due_year_2d) = if luna == 12 {
        (1i32, (an + 1) % 100)
    } else {
        (luna + 1, an % 100)
    };

    // Build positions 0-20 (21 chars) before the check digit
    let body = format!(
        "10{}01{:02}{:02}25{:02}{:02}0000",
        obligation_code, ll, aa, due_month, due_year_2d
    );
    // body is: "10" + obligation_code(3) + "01" + LL + AA + "25" + DM + DY + "0000"
    // = 2+3+2+2+2+2+2+2+4 = 21 chars
    debug_assert_eq!(
        body.len(),
        21,
        "NDP body must be 21 chars, got {}",
        body.len()
    );

    // Check digit = sum of all digit values in body, formatted as 2 digits (mod 100)
    let digit_sum: u32 = body.chars().map(|c| c.to_digit(10).unwrap_or(0)).sum();
    let check = digit_sum % 100;

    format!("{}{:02}", body, check)
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
///
/// # Wave 4 changes (OPANAF 174/2026)
///
/// * Rate fix: R9=21%, R10=11%, R11=9% (was 5% pre-2026).
/// * Reverse charge AE: collected R12 (sub-rows R12_1_1/R12_1_2 for 21%,
///   R12_2_1/R12_2_2 for 11%) + deductible mirror R25 (equal by DUK V_19–V_24).
/// * Intra-EU K purchases: R5 collected + R18 deductible (goods, equal by DUK V_7/V_8).
///
/// # Wave 8 changes (OPANAF 174/2026 regularizări)
///
/// * Old rates (S sales 19%/5%, S purchases 19%/9%/5%): auto-included in
///   regularizări rows R16 (collected) and R30 (deductible). Values may be
///   overridden via `submission.reg_colectata_*` / `submission.reg_dedusa_*`.
/// * R16_1/R16_2 added to R17 totals; R30_2 added to R32 (DUK R108), NOT R27
///   (R30_1 base feeds the control sum only).
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
    //   category Z (zero-rated intra-EU), K (intra-community delivery on sale side),
    //   E (exempt without right of deduction / export scutit)
    let mut r1_1_base = Decimal::ZERO;
    let mut _r1_1_vat = Decimal::ZERO; // VAT on exempt deliveries is always 0
    accumulate(
        &report.groups,
        |g| matches!(g.vat_category.as_str(), "Z" | "K" | "E"),
        &mut r1_1_base,
        &mut _r1_1_vat,
    );

    // R9_1 / R9_2 — standard rate 21%
    // Spec: category S rate 21% → R9 (DUK margin 20–22%)
    // OLD: 19% was also folded here — WRONG. 19% is excluded (old rate, residual).
    let mut r9_base = Decimal::ZERO;
    let mut r9_vat = Decimal::ZERO;
    accumulate(
        &report.groups,
        |g| g.vat_category == "S" && rate_matches(g, 21),
        &mut r9_base,
        &mut r9_vat,
    );

    // R10_1 / R10_2 — reduced rate 11%
    // Spec: category S/SR rate 11% → R10 (DUK margin 8–10%)
    // OLD: 9% was also folded here — WRONG. 9% now goes to R11 (from 2026).
    let mut r10_base = Decimal::ZERO;
    let mut r10_vat = Decimal::ZERO;
    accumulate(
        &report.groups,
        |g| (g.vat_category == "S" || g.vat_category == "SR") && rate_matches(g, 11),
        &mut r10_base,
        &mut r10_vat,
    );

    // R11_1 / R11_2 — reduced rate 9% (from 2026-01-01 per structura PDF / OPANAF 174/2026)
    // DUK margin for an>=2026 luna>=1: Round(8%*R11_1) <= R11_2 <= Round(10%*R11_1)
    // OLD: this was 5% — WRONG for 2026. 5% is excluded (old rate, residual).
    let mut r11_base = Decimal::ZERO;
    let mut r11_vat = Decimal::ZERO;
    accumulate(
        &report.groups,
        |g| g.vat_category == "S" && rate_matches(g, 9),
        &mut r11_base,
        &mut r11_vat,
    );

    // R12 — Taxare inversă domestică (AE) on BENEFICIAR (buyer) — COLLECTED leg
    //   Collected because under art.331 the buyer self-assesses the VAT both as
    //   collected (R12) AND as deductible (R25). These MUST be equal (DUK V_19–V_24).
    //
    //   Sub-row breakdown by rate:
    //     R12_1_1 / R12_1_2: 21% AE transactions (DUK margin 20–22%)
    //     R12_2_1 / R12_2_2: 11% AE transactions (DUK margin 10–12%)
    //   Parent rows:
    //     R12_1 = R12_1_1  (sum of bases; here only one sub-row)
    //     R12_2 = R12_1_2 + R12_2_2  (sum of VATs)
    //
    //   Data source: AE category can appear in BOTH `groups` (sales/collected) and
    //   `purchase_groups` (purchases/deductible). For the domestic reverse-charge
    //   model, the BUYER records:
    //     - Collected (R12): from report.groups where category=AE
    //     - Deductible (R25): from report.purchase_groups where category=AE
    //   However, DUK enforces R25 = R12 exactly, so the two legs MUST be equal.
    //   If the app puts AE only in purchase_groups (the more common ledger model),
    //   we use purchase_groups for both legs (self-assessment).

    // Accumulate AE from BOTH sources (groups + purchase_groups); the buyer
    // enters the AE invoice in both legs. Take whichever is non-zero; if both
    // exist, prefer groups (collected side) and trust the caller's data model.
    let mut ae21_base = Decimal::ZERO;
    let mut ae21_vat = Decimal::ZERO;
    let mut ae11_base = Decimal::ZERO;
    let mut ae11_vat = Decimal::ZERO;

    // Collect AE from sales groups first
    accumulate(
        &report.groups,
        |g| g.vat_category == "AE" && rate_matches(g, 21),
        &mut ae21_base,
        &mut ae21_vat,
    );
    accumulate(
        &report.groups,
        |g| g.vat_category == "AE" && rate_matches(g, 11),
        &mut ae11_base,
        &mut ae11_vat,
    );

    // If groups had no AE, fall back to purchase_groups (buyer ledger model)
    if ae21_base == Decimal::ZERO && ae11_base == Decimal::ZERO {
        accumulate(
            &report.purchase_groups,
            |g| g.vat_category == "AE" && rate_matches(g, 21),
            &mut ae21_base,
            &mut ae21_vat,
        );
        accumulate(
            &report.purchase_groups,
            |g| g.vat_category == "AE" && rate_matches(g, 11),
            &mut ae11_base,
            &mut ae11_vat,
        );
    }

    // R13_1 — livrări taxare inversă (VÂNZĂTOR / seller side), baza only
    // The SELLER in a domestic reverse-charge transaction reports the base in R13_1.
    // For the seller, AE appears in groups without VAT (buyer handles the VAT).
    // We only populate R13_1 when group VAT is zero (seller scenario).
    let mut r13_base = Decimal::ZERO;
    for g in &report.groups {
        if g.vat_category == "AE" && parse_dec(&g.vat) == Decimal::ZERO {
            r13_base += parse_dec(&g.base);
        }
    }

    // ── Purchase row accumulation ─────────────────────────────────────────────

    // R5_1 / R5_2 — achiziții intracomunitare de BUNURI (K, intra_eu_kind=goods or default)
    // Only K groups with intra_eu_kind != "services" land here.
    let mut r5_base = Decimal::ZERO;
    let mut r5_vat = Decimal::ZERO;
    accumulate(
        &report.purchase_groups,
        |g| g.vat_category == "K" && g.intra_eu_kind.as_deref() != Some("services"),
        &mut r5_base,
        &mut r5_vat,
    );

    // R7_1 / R7_2 — achiziții intracomunitare de SERVICII (K, intra_eu_kind=services)
    // Collected leg of intra-EU services self-assessment.
    // DUK V_13: R20_1=R7_1, V_14: R20_2=R7_2, V_15: R20_1_1=R7_1_1.
    let mut r7_base = Decimal::ZERO;
    let mut r7_vat = Decimal::ZERO;
    accumulate(
        &report.purchase_groups,
        |g| g.vat_category == "K" && g.intra_eu_kind.as_deref() == Some("services"),
        &mut r7_base,
        &mut r7_vat,
    );

    // R22_1 / R22_2 — achiziții interne cotă 21% (already correct, keep)
    // OLD: 19% was also folded here — WRONG. 19% is excluded (old rate, residual).
    let mut r22_base = Decimal::ZERO;
    let mut r22_vat = Decimal::ZERO;
    accumulate(
        &report.purchase_groups,
        |g| g.vat_category == "S" && rate_matches(g, 21),
        &mut r22_base,
        &mut r22_vat,
    );

    // R23_1 / R23_2 — achiziții interne cotă 11% ONLY.
    // R23's DUK corridor is 10–12% (rule R86: 10% ≤ R23_2/R23_1 ≤ 12%), so a 9%
    // purchase (vat = 9% of base) does NOT fit R23. Wave 8: 9% purchases flow into
    // R30 (regularizări) instead. Do NOT fold 9% into R23.
    let mut r23_base = Decimal::ZERO;
    let mut r23_vat = Decimal::ZERO;
    accumulate(
        &report.purchase_groups,
        |g| g.vat_category == "S" && rate_matches(g, 11),
        &mut r23_base,
        &mut r23_vat,
    );

    // ── Wave 8: R16 regularizări colectată + R30 regularizări dedusă ──────────
    // Override from submission if provided; otherwise use auto-computed prefill
    // values from `report.reg_colectata_*` / `report.reg_dedusa_*`.
    // Both rows are IntNeg15SType — no rate-margin DUK corridor applies.

    let r16_1_val: i64 = if let Some(ov) = submission.reg_colectata_baza {
        ov
    } else {
        parse_dec(&report.reg_colectata_baza)
            .round_dp(0)
            .to_i64()
            .unwrap_or(0)
    };
    let r16_2_val: i64 = if let Some(ov) = submission.reg_colectata_tva {
        ov
    } else {
        parse_dec(&report.reg_colectata_tva)
            .round_dp(0)
            .to_i64()
            .unwrap_or(0)
    };
    let r30_1_val: i64 = if let Some(ov) = submission.reg_dedusa_baza {
        ov
    } else {
        parse_dec(&report.reg_dedusa_baza)
            .round_dp(0)
            .to_i64()
            .unwrap_or(0)
    };
    let r30_2_val: i64 = if let Some(ov) = submission.reg_dedusa_tva {
        ov
    } else {
        parse_dec(&report.reg_dedusa_tva)
            .round_dp(0)
            .to_i64()
            .unwrap_or(0)
    };

    // ── Margin checks (logged, non-fatal) ─────────────────────────────────────
    // The collected VAT on each rate row should fall within the rate's corridor.
    // DUKIntegrator's business rules are the authoritative check.
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
    margin_warn("R9_2", r9_base, r9_vat, 20, 22); // 21% ± 1%
    margin_warn("R10_2", r10_base, r10_vat, 10, 12); // 11% ± 1%
    margin_warn("R11_2", r11_base, r11_vat, 8, 10); // 9% ± 1% (from 2026)
    margin_warn("R12_1_2", ae21_base, ae21_vat, 20, 22); // AE 21% ± 1%
    margin_warn("R12_2_2", ae11_base, ae11_vat, 10, 12); // AE 11% ± 1%

    // ── Totals ────────────────────────────────────────────────────────────────

    // R12 parent totals
    let r12_1_total = ae21_base + ae11_base;
    let r12_2_total = ae21_vat + ae11_vat;

    // R16 as Decimal (for totals arithmetic)
    let r16_1_dec = Decimal::from(r16_1_val);
    let r16_2_dec = Decimal::from(r16_2_val);

    // R17_2 = R5_2 + R7_2 + R9_2 + R10_2 + R11_2 + R12_2 + R16_2 + [R6_2 + R8_2 + R64_2 + R65_2]
    // R16_2 is the regularizări colectată for old rates (Wave 8).
    // R7_2 is the collected leg of intra-EU SERVICES (Wave 7)
    let r17_vat = r5_vat + r7_vat + r9_vat + r10_vat + r11_vat + r12_2_total + r16_2_dec;
    // R17_1 = R5_1 + R7_1 + R9_1 + R10_1 + R11_1 + R12_1 + R16_1 + ...
    let r17_base = r5_base + r7_base + r9_base + r10_base + r11_base + r12_1_total + r16_1_dec;

    // R25 = R12 (DUK V_19/V_20 enforced equality)
    let r25_1_total = r12_1_total;
    let r25_2_total = r12_2_total;

    // R18 = R5 (DUK V_7/V_8 enforced equality)
    let r18_base = r5_base;
    let r18_vat = r5_vat;

    // R20 = R7 (DUK V_13/V_14 enforced equality)
    let r20_base = r7_base;
    let r20_vat = r7_vat;

    // R30 as Decimal (for totals arithmetic)
    // r30_1 (base) feeds the control sum only; does not feed R27 or R32.
    let _r30_1_dec = Decimal::from(r30_1_val);
    let r30_2_dec = Decimal::from(r30_2_val);

    // R27_2 = R18_2 + R20_2 + R22_2 + R23_2 + R25_2 + [R19_2 + R21_2 + R43_2 + R44_2]
    // NOTE: R30 does NOT add into R27 — DUK rules R99/R100 verify R27 without R30.
    // R20_2 is the deductible leg of intra-EU SERVICES (Wave 7)
    let r27_vat = r18_vat + r20_vat + r22_vat + r23_vat + r25_2_total;
    let r27_base = r18_base + r20_base + r22_base + r23_base + r25_1_total;

    // R28_2 (rd.31, "SUB-TOTAL TAXĂ DEDUSĂ") — apply pro-rata de deducere (art. 300 Cod
    // fiscal; OPANAF 174/2026). The schema does NOT auto-apply pro-rata: the filer supplies
    // rd.31, constrained to rd.31 <= rd.30 (DUK control V_6). The app does not track each
    // purchase's deduction destination, so it scales the whole deductible VAT by the declared
    // pro_rata — exact for a fully mixed-use activity. A purely deductible payer files
    // pro_rata = 100 (the default) → rd.31 == rd.30 (behaviour unchanged for everyone else).
    let hundred = Decimal::from(100);
    let pro_rata_pct = Decimal::try_from(submission.pro_rata)
        .unwrap_or(hundred)
        .clamp(Decimal::ZERO, hundred);
    let r28_vat = if pro_rata_pct >= hundred {
        r27_vat
    } else {
        (r27_vat * pro_rata_pct / hundred).round_dp(0)
    };

    // R32_2 = R28_2 + R29_2 + R30_2 + R31_2
    // DUK rule R108: R32_2 = R28_2 + R30_2 (regularizări dedusă flows here, not into R27)
    let r32_vat = r28_vat + r30_2_dec;

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

    // ── Assemble row i64 values first so we can compute the control sum ─────────

    let opt_nonzero = |v: i64| if v != 0 { Some(v) } else { None };

    let r1_1_v = opt_nonzero(round_to_lei(r1_1_base));
    let r5_1_v = opt_nonzero(round_to_lei(r5_base));
    let r5_2_v = opt_nonzero(round_to_lei(r5_vat));
    // R7 — intra-EU services collected (Wave 7)
    let r7_1_v = opt_nonzero(round_to_lei(r7_base));
    let r7_2_v = opt_nonzero(round_to_lei(r7_vat));
    // R7 sub-rows: currently we accumulate all K-services into a single bucket
    // (no per-rate breakdown like R12 does for AE). The XSD has R7_1_1/R7_1_2
    // but DUK only mandates V_15: R20_1_1=R7_1_1, not that they must be present.
    // We omit the sub-rows here (None) — DUK does not require them to be present.
    let r7_1_1_v: Option<i64> = None;
    let r7_1_2_v: Option<i64> = None;
    let r9_1_v = opt_nonzero(round_to_lei(r9_base));
    let r9_2_v = opt_nonzero(round_to_lei(r9_vat));
    let r10_1_v = opt_nonzero(round_to_lei(r10_base));
    let r10_2_v = opt_nonzero(round_to_lei(r10_vat));
    let r11_1_v = opt_nonzero(round_to_lei(r11_base));
    let r11_2_v = opt_nonzero(round_to_lei(r11_vat));
    let r12_1_v = opt_nonzero(round_to_lei(r12_1_total));
    let r12_2_v = opt_nonzero(round_to_lei(r12_2_total));
    let r12_1_1_v = opt_nonzero(round_to_lei(ae21_base));
    let r12_1_2_v = opt_nonzero(round_to_lei(ae21_vat));
    let r12_2_1_v = opt_nonzero(round_to_lei(ae11_base));
    let r12_2_2_v = opt_nonzero(round_to_lei(ae11_vat));
    let r13_1_v = opt_nonzero(round_to_lei(r13_base));
    let r16_1_v = opt_nonzero(r16_1_val);
    let r16_2_v = opt_nonzero(r16_2_val);
    let r18_1_v = opt_nonzero(round_to_lei(r18_base));
    let r18_2_v = opt_nonzero(round_to_lei(r18_vat));
    let r17_1_v = opt_nonzero(round_to_lei(r17_base));
    let r17_2_v = opt_nonzero(round_to_lei(r17_vat));
    // R20 — intra-EU services deductible = R7 (DUK V_13/V_14 equality)
    let r20_1_v = opt_nonzero(round_to_lei(r20_base));
    let r20_2_v = opt_nonzero(round_to_lei(r20_vat));
    // R20 sub-rows mirror R7 sub-rows (DUK V_15: R20_1_1=R7_1_1). Since r7_1_1_v=None, None.
    let r20_1_1_v: Option<i64> = r7_1_1_v; // = None
    let r20_1_2_v: Option<i64> = r7_1_2_v; // = None
    let r22_1_v = opt_nonzero(round_to_lei(r22_base));
    let r22_2_v = opt_nonzero(round_to_lei(r22_vat));
    let r23_1_v = opt_nonzero(round_to_lei(r23_base));
    let r23_2_v = opt_nonzero(round_to_lei(r23_vat));
    let r25_1_v = opt_nonzero(round_to_lei(r25_1_total));
    let r25_2_v = opt_nonzero(round_to_lei(r25_2_total));
    let r25_1_1_v = opt_nonzero(round_to_lei(ae21_base)); // = R12_1_1 (DUK V_21)
    let r25_1_2_v = opt_nonzero(round_to_lei(ae21_vat)); // = R12_1_2 (DUK V_22)
    let r25_2_1_v = opt_nonzero(round_to_lei(ae11_base)); // = R12_2_1 (DUK V_23)
    let r25_2_2_v = opt_nonzero(round_to_lei(ae11_vat)); // = R12_2_2 (DUK V_24)
    let r30_1_v = opt_nonzero(r30_1_val);
    let r30_2_v = opt_nonzero(r30_2_val);
    let r27_1_v = opt_nonzero(round_to_lei(r27_base));
    let r27_2_v = opt_nonzero(round_to_lei(r27_vat));
    let r28_2_v = opt_nonzero(round_to_lei(r28_vat));
    let r32_2_v = opt_nonzero(round_to_lei(r32_vat));
    let r33_2_v = opt_nonzero(round_to_lei(r33_vat));
    let r34_2_v = opt_nonzero(round_to_lei(r34_vat));
    let r37_2_v = opt_nonzero(round_to_lei(r37_vat));
    let r40_2_v = opt_nonzero(round_to_lei(r40_vat));
    let r41_2_v = opt_nonzero(round_to_lei(r41_vat));
    let r42_2_v = opt_nonzero(round_to_lei(r42_vat));

    // totalPlata_A = CONTROL SUM of all populated R-row field values (DUK R26).
    // This is NOT the payable amount — it is a checksum that DUKIntegrator verifies
    // by independently summing every R-row attribute in the XML and comparing.
    // Absent (None) fields contribute 0.  Header-summary fields (nr_facturi, baza,
    // tva, nr_facturi_primite, baza_primite, tva_primite) are not present in
    // D300Rows so they contribute 0 as well.
    // Wave 7: include R7_* and R20_* in the control sum.
    // Wave 8: include R16_* and R30_* in the control sum.
    let total_plata_a: i64 = [
        r1_1_v, r5_1_v, r5_2_v, r7_1_v, r7_2_v, r7_1_1_v, r7_1_2_v, r9_1_v, r9_2_v, r10_1_v,
        r10_2_v, r11_1_v, r11_2_v, r12_1_v, r12_2_v, r12_1_1_v, r12_1_2_v, r12_2_1_v, r12_2_2_v,
        r13_1_v, r16_1_v, r16_2_v, r18_1_v, r18_2_v, r17_1_v, r17_2_v, r20_1_v, r20_2_v, r20_1_1_v,
        r20_1_2_v, r22_1_v, r22_2_v, r23_1_v, r23_2_v, r25_1_v, r25_2_v, r25_1_1_v, r25_1_2_v,
        r25_2_1_v, r25_2_2_v, r27_1_v, r27_2_v, r28_2_v, r30_1_v, r30_2_v, r32_2_v, r33_2_v,
        r34_2_v, r37_2_v, r40_2_v, r41_2_v, r42_2_v,
    ]
    .iter()
    .map(|o| o.unwrap_or(0))
    .sum();

    // ── Generate NDP (nr_evid) ────────────────────────────────────────────────
    // If the submission supplies a valid 23-char NDP, keep it; otherwise generate
    // the correct one from tip_decont + period.
    let nr_evid = {
        let s = submission.nr_evid.trim();
        if s.len() == 23 && s.chars().all(|c| c.is_ascii_digit()) {
            s.to_string()
        } else {
            generate_ndp(&submission.tip_decont, luna, an)
        }
    };

    // ── Assemble D300Rows ─────────────────────────────────────────────────────

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
        nr_evid,
        total_plata_a,

        // sales
        r1_1: r1_1_v,
        r9_1: r9_1_v,
        r9_2: r9_2_v,
        r10_1: r10_1_v,
        r10_2: r10_2_v,
        r11_1: r11_1_v,
        r11_2: r11_2_v,
        r12_1: r12_1_v,
        r12_2: r12_2_v,
        r12_1_1: r12_1_1_v,
        r12_1_2: r12_1_2_v,
        r12_2_1: r12_2_1_v,
        r12_2_2: r12_2_2_v,
        r13_1: r13_1_v,
        // R16 — regularizări colectată cote vechi (Wave 8)
        r16_1: r16_1_v,
        r16_2: r16_2_v,

        // purchases
        r5_1: r5_1_v,
        r5_2: r5_2_v,
        r18_1: r18_1_v,
        r18_2: r18_2_v,
        // R7/R20 — intra-EU services (Wave 7)
        r7_1: r7_1_v,
        r7_2: r7_2_v,
        r7_1_1: r7_1_1_v,
        r7_1_2: r7_1_2_v,
        r20_1: r20_1_v,
        r20_2: r20_2_v,
        r20_1_1: r20_1_1_v,
        r20_1_2: r20_1_2_v,
        r22_1: r22_1_v,
        r22_2: r22_2_v,
        r23_1: r23_1_v,
        r23_2: r23_2_v,
        r25_1: r25_1_v,
        r25_2: r25_2_v,
        r25_1_1: r25_1_1_v,
        r25_1_2: r25_1_2_v,
        r25_2_1: r25_2_1_v,
        r25_2_2: r25_2_2_v,
        // R30 — regularizări dedusă cote vechi (Wave 8)
        r30_1: r30_1_v,
        r30_2: r30_2_v,

        // totals
        r17_1: r17_1_v,
        r17_2: r17_2_v,
        r27_1: r27_1_v,
        r27_2: r27_2_v,
        r28_2: r28_2_v,
        r32_2: r32_2_v,
        r33_2: r33_2_v,
        r34_2: r34_2_v,
        r37_2: r37_2_v,
        r40_2: r40_2_v,
        r41_2: r41_2_v,
        r42_2: r42_2_v,
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
            cash_vat: false,
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
            tax_regime: "micro".into(),
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
                intra_eu_kind: None,
            })
            .collect();
        let purchase_groups: Vec<D300Group> = purchases
            .into_iter()
            .map(|(rate, cat, base, vat)| D300Group {
                vat_rate: rate.to_string(),
                vat_category: cat.to_string(),
                base: base.to_string(),
                vat: vat.to_string(),
                // K purchases in the test helper default to goods (None → accumulate_goods)
                intra_eu_kind: if cat == "K" {
                    Some("goods".to_string())
                } else {
                    None
                },
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
            // Wave 8: regularizări — tests override these explicitly when needed.
            reg_colectata_baza: "0.00".to_string(),
            reg_colectata_tva: "0.00".to_string(),
            reg_dedusa_baza: "0.00".to_string(),
            reg_dedusa_tva: "0.00".to_string(),
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
        // R41_2 = 97 (sold de plată final)
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

        // R41_2 = 97 (sold de plată final)
        assert_eq!(rows.r41_2, Some(97), "R41_2 = 97");
        assert_eq!(rows.r42_2, None, "R42_2 = None (no refund)");
    }

    #[test]
    fn refund_period_sets_r33_and_r42() {
        // Purchases > Sales → TVA de recuperat
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
    }

    #[test]
    fn rate_9pct_maps_to_r11_not_r10() {
        // Wave 4: 9% sales → R11 (not R10 as in old code)
        let report = make_report(vec![("0.09", "S", "200.00", "18.00")], vec![]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        let rows = map_to_rows(&report, &sub, &company, period).expect("map_to_rows");

        assert_eq!(rows.r11_1, Some(200), "R11_1 = 200 (9% sales → R11)");
        assert_eq!(rows.r11_2, Some(18), "R11_2 = 18 (9% VAT → R11)");
        assert_eq!(rows.r10_1, None, "R10_1 = None (9% must not go in R10)");
        assert_eq!(rows.r10_2, None, "R10_2 = None");
    }

    #[test]
    fn intra_eu_k_purchase_populates_r5_and_r18() {
        // Wave 4: K purchase → R5 + R18 (R18 = R5 enforced)
        let report = make_report(vec![], vec![("0.21", "K", "1000.00", "210.00")]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        let rows = map_to_rows(&report, &sub, &company, period).expect("map_to_rows");

        assert_eq!(rows.r5_1, Some(1000), "R5_1 = 1000");
        assert_eq!(rows.r5_2, Some(210), "R5_2 = 210");
        // R18 must equal R5 (DUK V_7/V_8)
        assert_eq!(rows.r18_1, Some(1000), "R18_1 = R5_1 = 1000");
        assert_eq!(rows.r18_2, Some(210), "R18_2 = R5_2 = 210");
        // R17_2 must include R5_2 (collected leg of intra-EU acquisition)
        assert_eq!(rows.r17_2, Some(210), "R17_2 includes R5_2");
        // R27_2 must include R18_2 (deductible leg)
        assert_eq!(rows.r27_2, Some(210), "R27_2 includes R18_2");
        // Net VAT = 0 (collected = deductible for pure K acquisition)
        assert_eq!(rows.r34_2, None, "R34_2 = None (no net payable)");
        assert_eq!(rows.r33_2, None, "R33_2 = None (no net refund)");
    }

    #[test]
    fn reverse_charge_ae_populates_r12_and_r25_equal() {
        // Wave 4: AE reverse charge → R12 collected + R25 deductible (must be equal)
        // Source: purchase_groups with AE (buyer self-assessment model)
        let report = make_report(vec![], vec![("0.21", "AE", "1000.00", "210.00")]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        let rows = map_to_rows(&report, &sub, &company, period).expect("map_to_rows");

        // R12 collected
        assert_eq!(rows.r12_1, Some(1000), "R12_1 = 1000 (AE collected base)");
        assert_eq!(rows.r12_2, Some(210), "R12_2 = 210 (AE collected VAT)");
        assert_eq!(rows.r12_1_1, Some(1000), "R12_1_1 = 1000 (21% sub-row)");
        assert_eq!(rows.r12_1_2, Some(210), "R12_1_2 = 210 (21% sub-row VAT)");
        assert_eq!(rows.r12_2_1, None, "R12_2_1 = None (no 11%)");
        assert_eq!(rows.r12_2_2, None, "R12_2_2 = None");

        // R25 = R12 (DUK equality enforced)
        assert_eq!(rows.r25_1, rows.r12_1, "R25_1 = R12_1 (DUK V_19)");
        assert_eq!(rows.r25_2, rows.r12_2, "R25_2 = R12_2 (DUK V_20)");
        assert_eq!(rows.r25_1_1, rows.r12_1_1, "R25_1_1 = R12_1_1 (DUK V_21)");
        assert_eq!(rows.r25_1_2, rows.r12_1_2, "R25_1_2 = R12_1_2 (DUK V_22)");

        // R17_2 must include R12_2; R27_2 must include R25_2
        assert_eq!(rows.r17_2, Some(210), "R17_2 includes R12_2");
        assert_eq!(rows.r27_2, Some(210), "R27_2 includes R25_2");

        // No old r13_1 for AE buyer model
        assert_eq!(
            rows.r13_1, None,
            "R13_1 = None (buyer model; no seller-side base)"
        );
    }

    #[test]
    fn reverse_charge_ae_11pct_uses_r12_2_sub_row() {
        // AE at 11% → R12_2_1/R12_2_2 sub-rows
        let report = make_report(vec![], vec![("0.11", "AE", "500.00", "55.00")]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        let rows = map_to_rows(&report, &sub, &company, period).expect("map_to_rows");

        assert_eq!(rows.r12_1, Some(500), "R12_1 = 500");
        assert_eq!(rows.r12_2, Some(55), "R12_2 = 55");
        assert_eq!(rows.r12_1_1, None, "R12_1_1 = None (no 21%)");
        assert_eq!(rows.r12_2_1, Some(500), "R12_2_1 = 500 (11% sub-row)");
        assert_eq!(rows.r12_2_2, Some(55), "R12_2_2 = 55 (11% VAT)");
        // R25 mirrors
        assert_eq!(rows.r25_1, rows.r12_1);
        assert_eq!(rows.r25_2, rows.r12_2);
        assert_eq!(rows.r25_2_1, rows.r12_2_1);
        assert_eq!(rows.r25_2_2, rows.r12_2_2);
    }

    #[test]
    fn intra_eu_categories_map_to_r1_and_r5() {
        // Intra-EU delivery Z → R1_1 (sales, no VAT)
        // Intra-EU acquisition K purchases → R5_1/R5_2 + R18 mirror
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
        assert_eq!(rows.r18_1, Some(1000), "R18_1 = R5_1 = 1000");
        assert_eq!(rows.r18_2, Some(210), "R18_2 = R5_2 = 210");
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
            intra_eu_kind: None,
        };
        let g_pct = D300Group {
            vat_rate: "21.00".to_string(),
            vat_category: "S".to_string(),
            base: "100".to_string(),
            vat: "21".to_string(),
            intra_eu_kind: None,
        };
        assert!(rate_matches(&g_frac, 21), "fractional 0.21 should match 21");
        assert!(rate_matches(&g_pct, 21), "percent 21.00 should match 21");
        assert!(!rate_matches(&g_frac, 19), "0.21 should not match 19");
    }

    // ── NDP (generate_ndp) tests ─────────────────────────────────────────────

    /// Validate the structural rules for a generated NDP:
    /// - exactly 23 ASCII digits
    /// - chars[0..2] + chars[5..7] + chars[17..21] == "10010000"
    /// - Σ(digit_values[0..21]) == integer formed by last 2 digits (check digits)
    fn ndp_is_valid(ndp: &str) -> bool {
        if ndp.len() != 23 || !ndp.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        // Composite literal check
        let composite = format!("{}{}{}", &ndp[0..2], &ndp[5..7], &ndp[17..21]);
        if composite != "10010000" {
            return false;
        }
        // Check digit: sum of first 21 digits == last 2 digits as integer
        let digit_sum: u32 = ndp[..21].chars().map(|c| c.to_digit(10).unwrap()).sum();
        let check_val = digit_sum % 100;
        let check_digits: u32 = ndp[21..23].parse().unwrap_or(999);
        check_val == check_digits
    }

    #[test]
    fn generate_ndp_is_23_chars_and_passes_structure_check() {
        // Test all tip_decont codes and various periods
        for (tip, _code) in &[("L", "301"), ("T", "302"), ("S", "303"), ("A", "304")] {
            for (luna, an) in &[(1, 2026), (6, 2025), (12, 2025), (3, 2024)] {
                let ndp = generate_ndp(tip, *luna, *an);
                assert_eq!(
                    ndp.len(),
                    23,
                    "NDP must be 23 chars: tip={tip} luna={luna} an={an} → {ndp}"
                );
                assert!(
                    ndp.chars().all(|c| c.is_ascii_digit()),
                    "NDP must be all digits: {ndp}"
                );
                assert!(
                    ndp_is_valid(&ndp),
                    "NDP failed structural validation: tip={tip} luna={luna} an={an} → {ndp}"
                );
            }
        }
    }

    #[test]
    fn generate_ndp_check_digit_correct() {
        // For tip_decont=L, luna=1, an=2026:
        // body = "10301" + "01" + "01" + "26" + "25" + "02" + "26" + "0000"
        //       = "103010101262502260000"  (21 chars)
        // digit_sum = 1+0+3+0+1+0+1+0+1+2+6+2+5+0+2+2+6+0+0+0+0 = 32
        // check = 32 % 100 = 32 → "32"
        // expected = "10301010126250226000032"
        let ndp = generate_ndp("L", 1, 2026);
        assert_eq!(ndp, "10301010126250226000032", "NDP for L/2026-01");
        assert!(ndp_is_valid(&ndp), "must pass structural check");
    }

    #[test]
    fn generate_ndp_december_rolls_over_to_next_year() {
        // luna=12, an=2025: due_month=1, due_year=2026
        let ndp = generate_ndp("L", 12, 2025);
        assert_eq!(ndp.len(), 23, "December rollover NDP must be 23 chars");
        assert!(ndp_is_valid(&ndp), "December rollover NDP must be valid");
        // Verify the due-month/year part (positions 11-16) shows 01 and 26
        assert_eq!(&ndp[11..13], "25", "due-day must be 25");
        assert_eq!(&ndp[13..15], "01", "due-month must be 01 (rollover)");
        assert_eq!(&ndp[15..17], "26", "due-year must be 26 (2026)");
    }

    #[test]
    fn generate_ndp_tip_t_uses_code_302() {
        let ndp = generate_ndp("T", 3, 2025);
        assert_eq!(&ndp[2..5], "302", "T tip_decont → obligation code 302");
        assert!(ndp_is_valid(&ndp), "T-type NDP must be valid");
    }

    #[test]
    fn map_to_rows_generates_ndp_when_submission_has_placeholder() {
        let report = make_report(vec![], vec![]);
        let sub = make_submission(); // nr_evid = default_nr_evid() = "0"
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        let rows = map_to_rows(&report, &sub, &company, period).expect("map_to_rows");
        assert_eq!(
            rows.nr_evid.len(),
            23,
            "nr_evid should be a generated 23-char NDP, got: {}",
            rows.nr_evid
        );
        assert!(
            ndp_is_valid(&rows.nr_evid),
            "generated nr_evid must pass NDP structural validation: {}",
            rows.nr_evid
        );
    }

    #[test]
    fn map_to_rows_keeps_valid_23_char_nr_evid() {
        let report = make_report(vec![], vec![]);
        let mut sub = make_submission();
        sub.nr_evid = "10301010126250226000032".to_string(); // valid NDP
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        let rows = map_to_rows(&report, &sub, &company, period).expect("map_to_rows");
        assert_eq!(
            rows.nr_evid, "10301010126250226000032",
            "valid 23-char nr_evid should be kept verbatim"
        );
    }
}
