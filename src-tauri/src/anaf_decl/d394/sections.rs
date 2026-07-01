//! D394 v5 section builder.
//!
//! Maps a `D394Report` (from `commands::d394::compute_d394`) + a `D394Submission`
//! + company record into the structural types used by `generator.rs`.
//!
//! ## Category → op1 tip mapping (XSD `Str_listaTipOperatieSType`):
//!
//! SALES (livrări), from `report.partners`:
//!   category=S   (standard taxed, rate>0)   → tip="L",  cota=rate, tva=group VAT
//!   category=AE  (taxare inversă domestică)  → tip="V",  cota=0,    no tva
//!   category=E   (scutit fără drept ded.)    → tip="LS", cota=0,    no tva
//!   category=Z   (zero-rated export)         → tip="LS", cota=0,    no tva
//!   category=K   (livrare intracomunitară)   → tip="LS", cota=0,    no tva
//!   category=O   (outside scope)             → tip="LS", cota=0,    no tva
//!   category=G   (other)                     → tip="LS", cota=0,    no tva
//!
//! PURCHASES (achiziții), from `report.purchase_partners`:
//!   category=S   (standard taxed, rate>0)   → tip="A",  cota=rate, tva present
//!   category=AE  (taxare inversă)            → tip="C",  cota=rate, tva present (self-assessed)
//!                                               NOTE: cota MUST be ≠0 for C (R217.2)
//!   category=K   (achiziție intracomunitară)
//!     tip_partener=1 (RO CUI)               → tip="AI", cota=rate, tva present
//!     tip_partener=3/4 (foreign)             → tip="C",  cota=rate, tva present
//!       (AI is only valid for tp=1; tp=3/4 allows {L,LS,C})
//!   category=E   (scutit)                    → tip="AS", cota=0,    no tva
//!   category=Z   (zero/export)               → tip="AS", cota=0,    no tva
//!   category=O/G (outside scope)             → tip="AS", cota=0,    no tva
//!
//! ## tip_partener logic (XSD `Int_tipPartenerOp1SType`, values 1–4):
//!   1 = persoană înregistrată în scopuri de TVA (valid RO CUI, digits 2–10)
//!   2 = persoană juridică neînregistrată (non-VAT, or partner not typed)
//!   3 = persoană fizică (CUI absent or empty)
//!   4 = nerezidenți (foreign partners — non-RO country code prefix)
//!
//! ## Partner-type constraints enforced by DUK validator:
//!   R215.1: tp=1 → tip ≠ N
//!   R215.2: tp=2 → tip ∈ {L, LS, N}
//!   R215.3: tp=3/4 → tip ∈ {L, LS, C}
//!
//!   R217.1: cota=0 → tip ∈ {LS, AS, N, V}
//!   R217.2: tip ∈ {LS, AS, N, V} → cota MUST be 0
//!
//!   R218.1: tp≠2 → cuiP required; tp=1 → cuiP must be valid CUI
//!
//!   R232.1: tip ∈ {A, L, C, AI, NA} → tva REQUIRED
//!   R232.2: tip ∉ {A, L, C, AI, NA} → tva MUST be absent
//!
//!   R233.5: tp=1 AND tip ∈ {C, V} → at least one op11 required
//!
//!   R84:    cota≠0 in op1 → rezumat2 for that cota must exist
//!
//! ## informatii computed fields:
//!   nrCui1 = count of distinct cuiP in op1 with tip_partener=1
//!   nrCui2 = count of op1 lines with tip_partener=2 (not distinct — see validator source)
//!   nrCui3 = count of distinct cuiP in op1 with tip_partener=3
//!   nrCui4 = count of distinct cuiP in op1 with tip_partener=4
//!   nrFacturi = 0 (we emit no serieFacturi, so nrFacturi must be 0 per R131)
//!   tvaCol{rate} = Σ op1(tip=L, cota=rate).tva
//!   tvaDed{rate} = Σ op1(tip=A, cota=rate).tva
//!   tvaDedAI{rate} = Σ op1(tip=AI, cota=rate).tva  [ALL required → emit 0 if absent]
//!
//! ## totalPlata_A formula (R17):
//!   totalPlata_A = nrCui1 + nrCui2 + nrCui3 + nrCui4
//!               + Σ rezumat2(bazaL + bazaA + bazaAI)
//!
//! ALL amounts are rounded to whole lei (0 dp) before writing.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use chrono::Datelike;
use chrono::NaiveDate;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::anaf_decl::valid_cui;
use crate::commands::d394::{D394Partner, D394Report};
use crate::db::companies::Company;
use crate::error::AppResult;

use super::{D394CashRow, D394Submission};

// ── Public output types ───────────────────────────────────────────────────────

/// An op1 record (one per partner × operation group).
/// Mirrors `Op1Type` in the XSD.
#[derive(Debug, Clone)]
pub struct Op1 {
    /// XSD `Str_listaTipOperatieSType`: A/L/C/V/LS/AS/AI/N
    pub tip: String,
    /// XSD `Int_tipPartenerOp1SType`: 1–4
    pub tip_partener: i64,
    /// XSD `Int_coteTVASType`: 0/5/9/11/19/20/21/24
    pub cota: i64,
    /// Partner CUI digits (may be empty if PF/unknown); XSD `Str50` optional
    pub cui_p: String,
    /// Partner name; XSD `Str200` required
    pub den_p: String,
    /// Number of invoices; XSD `IntPoz15SType` required
    pub nr_fact: i64,
    /// Base amount (whole lei); XSD `IntNeg15SType` required
    pub baza: i64,
    /// VAT amount (whole lei); XSD optional — omitted for cota=0 / non-taxed
    /// R232.1: REQUIRED for tip ∈ {A, L, C, AI, NA}
    /// R232.2: MUST be absent for tip ∈ {LS, AS, N, V}
    pub tva: Option<i64>,
    /// op11 sub-sections for this op1 (emitted as children).
    /// Required when tip_partener=1 AND tip ∈ {C, V} (R233.5).
    pub op11_list: Vec<Op11>,
}

/// An op11 sub-section (child of op1).
/// Required by R233.5: tip_partener=1 & tip∈{C,V}.
#[derive(Debug, Clone)]
pub struct Op11 {
    /// Invoice count for this product/service code.
    pub nr_fact_pr: i64,
    /// Product/service code (must be in allowed list; for tp=1, NOT 32/33/34/35).
    pub cod_pr: i64,
    /// Base amount = the op1 baza.
    pub baza_pr: i64,
    /// TVA amount: REQUIRED when tip=C & tp=1; ABSENT when tip=V & tp=1 (R237.1).
    pub tva_pr: Option<i64>,
}

/// A `<detaliu>` element — child of rezumat1.
/// Required for each distinct codPR referenced by op11 elements within this rezumat1's scope.
/// The `bun` attribute matches the codPR used in op11.
///
/// For tp=1: nrLivV/bazaLivV (V ops) and nrAchizC/bazaAchizC/tvaAchizC (C ops) REQUIRED.
/// For tp≠1: these fields must NOT be present.
#[derive(Debug, Clone)]
pub struct Detaliu {
    /// Product/service code (same as op11.codPR); `bun` attribute in XSD.
    pub bun: i64,
    // For tp=1, V ops: required (can be 0 if no V ops for this bun)
    pub nr_liv_v: Option<i64>,
    pub baza_liv_v: Option<i64>,
    // For tp=1, C ops: required (can be 0 if no C ops for this bun)
    pub nr_achiz_c: Option<i64>,
    pub baza_achiz_c: Option<i64>,
    pub tva_achiz_c: Option<i64>,
}

/// A rezumat1 record (summary per (tip_partener, cota) across all op1 lines).
/// Mirrors `Rezumat1Type` in the XSD.
///
/// Field presence rules (from DUK validator Rezumat1.aggregation, exactly):
/// - facturiL/bazaL/tvaL: REQUIRED when cota≠0 (ALL tp); FORBIDDEN when cota=0
/// - facturiLS/bazaLS: REQUIRED when cota=0 (ALL tp, excluding tp=2 special path); FORBIDDEN when cota≠0
/// - facturiA/bazaA/tvaA: REQUIRED when cota≠0 AND tp=1; FORBIDDEN otherwise
/// - facturiAI/bazaAI/tvaAI: REQUIRED when cota≠0 AND tp=1; FORBIDDEN otherwise
/// - facturiAS/bazaAS: REQUIRED when cota=0 AND tp=1; FORBIDDEN otherwise
/// - facturiV/bazaV: REQUIRED when cota=0 AND tp=1; FORBIDDEN when cota≠0 OR tp≠1
/// - facturiC/bazaC/tvaC: REQUIRED when cota≠0 AND tp∈{1,3,4}; FORBIDDEN otherwise
/// - facturiN/document_N/bazaN: ONLY for tp=2, cota=0 (not used in our data model)
///
/// Important: fields are emitted with value 0 when required but no ops of that type exist.
/// This is what the DUK validator expects — it checks PRESENCE, not value > 0.
#[derive(Debug, Clone)]
pub struct Rezumat1 {
    pub tip_partener: i64,
    pub cota: i64,
    // tip=L fields: required when cota≠0 (ALL tp)
    pub facturi_l: Option<i64>,
    pub baza_l: Option<i64>,
    pub tva_l: Option<i64>,
    // tip=LS fields: required when cota=0 (ALL tp, excluding tp=2 special)
    pub facturi_ls: Option<i64>,
    pub baza_ls: Option<i64>,
    // tip=A fields: required when cota≠0 AND tp=1
    pub facturi_a: Option<i64>,
    pub baza_a: Option<i64>,
    pub tva_a: Option<i64>,
    // tip=AI fields: required when cota≠0 AND tp=1
    pub facturi_ai: Option<i64>,
    pub baza_ai: Option<i64>,
    pub tva_ai: Option<i64>,
    // tip=AS fields: required when cota=0 AND tp=1
    pub facturi_as: Option<i64>,
    pub baza_as: Option<i64>,
    // tip=V fields: required when cota=0 AND tp=1
    pub facturi_v: Option<i64>,
    pub baza_v: Option<i64>,
    // tip=C fields: required when cota≠0 AND tp∈{1,3,4}
    pub facturi_c: Option<i64>,
    pub baza_c: Option<i64>,
    pub tva_c: Option<i64>,
    // tip=N fields: ONLY for tp=2, cota=0 (not emitted — no N-category ops)
    pub facturi_n: Option<i64>,
    pub document_n: Option<i64>,
    pub baza_n: Option<i64>,
    // Detaliu children: required for each codPR referenced by op11 elements
    pub detaliu_list: Vec<Detaliu>,
}

/// A rezumat2 record (one per distinct cota≠0 in op1).
/// Aggregates nrFacturiL/bazaL/tvaL (tip∈{L,V}), nrFacturiA/bazaA/tvaA (tip∈{A,C}),
/// nrFacturiAI/bazaAI/tvaAI (tip=AI) — all per that cota.
/// Required by R84 for every cota≠0 in op1.
///
/// R105/R106/R107/R108: when cota≠24, baza_incasari_i1/tva_incasari_i1 and
/// baza_incasari_i2/tva_incasari_i2 are REQUIRED (emit 0 when no i1/i2 data).
#[derive(Debug, Clone)]
pub struct Rezumat2 {
    pub cota: i64,
    // Aggregate of tip=L and tip=V op1 (validator buckets both as "L")
    pub nr_facturi_l: i64,
    pub baza_l: i64,
    pub tva_l: i64,
    // Aggregate of tip=A and tip=C op1 (validator buckets both as "A")
    pub nr_facturi_a: i64,
    pub baza_a: i64,
    pub tva_a: i64,
    // Aggregate of tip=AI op1
    pub nr_facturi_ai: i64,
    pub baza_ai: i64,
    pub tva_ai: i64,
    // Cash-register incasari (R105-R108): required when cota≠24; emit 0
    pub baza_incasari_i1: i64,
    pub tva_incasari_i1: i64,
    pub baza_incasari_i2: i64,
    pub tva_incasari_i2: i64,
    // Facturi simplificate (cartuș I) per cotă — emit 0 when no data.
    // FSL = livrări fără cod beneficiar, FSLcod = livrări cu cod; FSA/FSAI = achiziții (intracom);
    // BFAI = bonuri fiscale achiziții intracomunitare.
    pub baza_fsl: i64,
    pub tva_fsl: i64,
    pub baza_fsl_cod: i64,
    pub tva_fsl_cod: i64,
    pub baza_fsa: i64,
    pub tva_fsa: i64,
    pub baza_fsai: i64,
    pub tva_fsai: i64,
    pub baza_bfai: i64,
    pub tva_bfai: i64,
}

/// A serieFacturi record.
/// Minimal: tip=1 (opening series) + tip=2 (closing series) needed for nrFacturi > 0.
/// Op_efectuate=0 → no serieFacturi allowed (R112.3).
/// We emit serieFacturi tip=1+2 when there are L/LS/V ops (R112.1 requires nrFacturi>0).
#[derive(Debug, Clone)]
pub struct SerieFacturi {
    /// 1=opening, 2=closing — both required when nrFacturi>0 (R112.2)
    pub tip: i64,
    /// Starting invoice number (required)
    pub nr_i: String,
}

/// The `<informatii>` block — grand summary.
/// ALL required attributes must be present (defaulting to 0).
#[derive(Debug, Clone)]
pub struct Informatii {
    // Partner counts per tip_partener
    // nrCui1 = distinct cuiP values with tp=1
    pub nr_cui1: i64,
    // nrCui2 = COUNT of op1 lines with tp=2 (NOT distinct CUIs — validator increments per line)
    pub nr_cui2: i64,
    // nrCui3 = distinct cuiP values with tp=3
    pub nr_cui3: i64,
    // nrCui4 = distinct cuiP values with tp=4
    pub nr_cui4: i64,
    // Cash-register (all zero — no data)
    pub nr_bf_i1: i64,
    pub incasari_i1: i64,
    pub incasari_i2: i64,
    // Invoice counts
    // nrFacturi_terti / nrFacturi_benef: 0 (no series data)
    pub nr_facturi_terti: i64,
    pub nr_facturi_benef: i64,
    // nrFacturi: must be 0 when no serieFacturi tip=2 exists (R131)
    // OR must equal invoice count when serieFacturi tip=2 exists.
    // We emit serieFacturi when there are L/LS/V ops, so this equals total sales invoices.
    pub nr_facturi: i64,
    pub nr_facturi_l_pf: i64,
    pub nr_facturi_ls_pf: i64,
    pub val_ls_pf: i64,
    // TVA colectată per rate (from L ops) — only when sistemTVA=1
    pub tva_col24: Option<i64>,
    pub tva_col21: Option<i64>,
    pub tva_col11: Option<i64>,
    pub tva_col20: Option<i64>,
    pub tva_col19: Option<i64>,
    pub tva_col9: Option<i64>,
    pub tva_col5: Option<i64>,
    // TVA deductibilă per rate (from A ops) — only when sistemTVA=1
    pub tva_ded24: Option<i64>,
    pub tva_ded21: Option<i64>,
    pub tva_ded11: Option<i64>,
    pub tva_ded20: Option<i64>,
    pub tva_ded19: Option<i64>,
    pub tva_ded9: Option<i64>,
    pub tva_ded5: Option<i64>,
    // TVA deductibilă achiziții intracomunitare per rate — ALL REQUIRED → 0 if absent
    pub tva_ded_ai24: i64,
    pub tva_ded_ai21: i64,
    pub tva_ded_ai11: i64,
    pub tva_ded_ai20: i64,
    pub tva_ded_ai19: i64,
    pub tva_ded_ai9: i64,
    pub tva_ded_ai5: i64,
    // solicit (from submission)
    pub solicit: i64,
    // efectuat (from submission.op_efectuate)
    pub efectuat: Option<i64>,
}

/// The complete D394 document ready for the XML generator.
#[derive(Debug, Clone)]
pub struct D394Doc {
    pub luna: i32,
    pub an: i32,
    pub informatii: Informatii,
    pub serie_facturi: Vec<SerieFacturi>,
    pub rezumat1_list: Vec<Rezumat1>,
    pub rezumat2_list: Vec<Rezumat2>,
    pub op1_list: Vec<Op1>,
    /// totalPlata_A = nrCui1+nrCui2+nrCui3+nrCui4 + Σrezumat2(bazaL+bazaA+bazaAI)
    pub total_plata_a: i64,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Strip "RO" prefix from CUI and trim whitespace.
fn strip_ro(cui: &str) -> String {
    let s = cui.trim();
    let s = if s.to_uppercase().starts_with("RO") {
        &s[2..]
    } else {
        s
    };
    s.trim().to_string()
}

/// Round Decimal to 0 dp → i64.
fn round_to_lei(d: Decimal) -> i64 {
    d.round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        .to_i64()
        .unwrap_or(0)
}

/// Parse a monetary string to Decimal.
fn parse_dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

/// Return Some(v) if v != 0, else None (for optional XSD attrs we only emit when nonzero).
fn opt_nonzero(v: i64) -> Option<i64> {
    if v != 0 {
        Some(v)
    } else {
        None
    }
}

/// Resolve the D394 op11 `codPR` (art. 331 product category) for a partner.
///
/// Rules (Parameters_v7._listaCodPR + R235):
///   tip_partener=1: allowed = NC cereal codes ∪ {22..31, 36} — NOT in {32,33,34,35,37}
///   tip_partener=2: allowed = {22,23,32,33,34,35}
///
/// If the partner has an explicit `art331_code` that is valid for the resolved
/// `tip_partener`, use it. Otherwise fall back to 22 (deșeuri/scrap), which is
/// valid for both tp=1 and tp=2.
///
/// A warning is emitted when an explicit code is invalid so the user can see it
/// in the app logs.
fn resolve_cod_pr(art331_code: &Option<String>, tip_partener: i64) -> i64 {
    // NC cereal codes valid for tp=1 (from Parameters_v7)
    const CEREAL_NC: &[i64] = &[
        1001, 1002, 1003, 1004, 1005, 1201, 1205, 120600, 121291, 10086000, 120400,
    ];

    // R235.1: codPR values FORBIDDEN for tip_partener=1
    const FORBIDDEN_TP1: &[i64] = &[32, 33, 34, 35, 37];
    // R235.3: codPR values FORBIDDEN for tip_partener=2
    const ALLOWED_TP2: &[i64] = &[22, 23, 32, 33, 34, 35];

    if let Some(code_str) = art331_code {
        let code_str = code_str.trim();
        if !code_str.is_empty() {
            if let Ok(code) = code_str.parse::<i64>() {
                let valid = match tip_partener {
                    1 => {
                        // tp=1: valid if in cereal NC set OR in 22..31 or 36, AND not in forbidden list
                        let in_cereal = CEREAL_NC.contains(&code);
                        let in_cat_range = matches!(code, 22..=31 | 36);
                        let forbidden = FORBIDDEN_TP1.contains(&code);
                        (in_cereal || in_cat_range) && !forbidden
                    }
                    2 => ALLOWED_TP2.contains(&code),
                    _ => {
                        // tp=3/4: no op11 required — caller should not call this for those
                        false
                    }
                };
                if valid {
                    return code;
                } else {
                    tracing::warn!(
                        "D394 op11: art331_code '{}' is not valid for tip_partener={} \
                         (R235); falling back to codPR=22",
                        code,
                        tip_partener
                    );
                }
            } else {
                tracing::warn!(
                    "D394 op11: art331_code '{}' is not a valid integer; falling back to codPR=22",
                    code_str
                );
            }
        }
    }
    // Default: 22 = deșeuri/scrap (valid for both tp=1 and tp=2)
    22
}

/// Determine the `tip_partener` code from a partner CUI string.
///
/// Rules (from DUK validator source):
///   tp=1: stripped digits-only 2–10 chars, not starting '0' → RO VAT registered
///   tp=2: non-empty CUI but doesn't match any specific pattern (juridic neregistrat)
///   tp=3: empty string → persoană fizică (no CUI)
///   tp=4: non-RO foreign country prefix (e.g. DE123, FR55) → nerezident
///
/// Detection of foreign: if CUI has a 2-letter alphabetic prefix that is NOT "RO",
/// it's treated as a foreign EU VAT number → tp=4.
pub fn tip_partener_from_cui(raw_cui: &str) -> i64 {
    let full = raw_cui.trim();
    if full.is_empty() {
        return 3; // PF — no CUI
    }

    // Check for foreign country prefix (2 alpha chars not "RO")
    if full.len() >= 2 {
        let prefix = &full[..2];
        let is_alpha = prefix.chars().all(|c| c.is_ascii_alphabetic());
        if is_alpha && !prefix.to_uppercase().eq_ignore_ascii_case("ro") {
            return 4; // Non-resident foreign partner
        }
    }

    // Strip RO and check for valid RO CUI format
    let digits = strip_ro(raw_cui);
    if digits.is_empty() {
        return 3;
    }
    let all_digits = digits.chars().all(|c| c.is_ascii_digit());
    let len = digits.len();
    if all_digits && (2..=10).contains(&len) && !digits.starts_with('0') {
        1 // CUI valid RO → persoană juridică înregistrată TVA
    } else {
        2 // juridic dar neînregistrat (sau CUI non-standard)
    }
}

/// Return the in-force standard VAT rate for a given reporting period.
///
/// Romania changed the standard rate from 19% to 21% effective 2025-08-01
/// (OUG 138/2024 / Legea 296/2023 modificată). Historical periods retain 19%.
pub fn standard_cota_for(period: NaiveDate) -> i64 {
    let cutover = NaiveDate::from_ymd_opt(2025, 8, 1).expect("static date");
    if period >= cutover {
        21
    } else {
        19
    }
}

/// Parse a normalized vat_rate string (integer-percent, e.g. "21") to a D394
/// `cota` integer in the enum {0, 5, 9, 11, 19, 20, 21, 24}.
///
/// If the parsed value is not in the enum, emits a warning and returns the
/// in-force standard rate for `period` (21% from 2025-08-01, 19% for earlier
/// periods) so that the XML remains schema-valid with a correct default.
fn parse_cota_from_rate(vat_rate: &str, period: NaiveDate) -> i64 {
    let std_cota = standard_cota_for(period);
    let pct: i64 = vat_rate.trim().parse().unwrap_or(std_cota);
    match pct {
        0 | 5 | 9 | 11 | 19 | 20 | 21 | 24 => pct,
        other => {
            tracing::warn!(
                "D394: vat_rate '{}' ({}) is not in the cota enum {{0,5,9,11,19,20,21,24}}; \
                 falling back to {} (standard rate for period {})",
                vat_rate,
                other,
                std_cota,
                period
            );
            std_cota
        }
    }
}

/// Map a D394Partner (from livrări/vânzări) to (tip, cota, emit_tva).
///
/// Returns (tip: &'static str, cota: i64, emit_tva: bool).
///
/// Constraints satisfied:
///   R215.1: tp=1 → tip≠N (not emitting N)
///   R215.2: tp=2 → tip∈{L,LS,N} (not emitting other tips for tp=2)
///   R215.3: tp=3/4 → tip∈{L,LS,C}
///   R217.1: cota=0 → tip∈{LS,AS,N,V}
///   R217.2: tip∈{LS,AS,N,V} → cota=0
fn map_sales_partner(partner: &D394Partner, period: NaiveDate) -> (&'static str, i64, bool) {
    match partner.vat_category.as_str() {
        "S" | "SR" => {
            let cota = parse_cota_from_rate(&partner.vat_rate, period);
            // cota=0 for S would violate R217.2 for L — ensure non-zero
            let cota = if cota == 0 {
                standard_cota_for(period)
            } else {
                cota
            };
            ("L", cota, true)
        }
        "AE" => {
            // Taxare inversă livrare → tip=V, cota=0, no tva (R217.1 ok, R232.2 ok)
            ("V", 0, false)
        }
        // Scutite / zero-rate / intra-EU delivery / outside-scope
        //
        // KNOWN LIMITATION (final v0.7.3 audit; deferred, verify-first): routing K (intra-EU,
        // D390 territory) and Z/G (exports) into D394 as "LS" may DOUBLE-DECLARE operations that
        // the ANAF instructions scope to D390/vamă, not D394 (D394 covers operațiuni pe teritoriul
        // național). The safe fix — EXCLUDING K (and possibly Z/G with non-RO partners) from the
        // D394 partner rollup — must be validated against the official D394 instructions + a DUK
        // run on a mixed fixture before changing declared totals; do not blind-edit.
        "E" | "Z" | "K" | "O" | "G" => ("LS", 0, false),
        _ => ("LS", 0, false),
    }
}

/// Map a D394Partner (from achiziții/cumpărări) to `Some((tip, cota, emit_tva))`, given the
/// already-determined `tip_partener`. Returns `None` when the line must be DROPPED from D394 (the
/// caller logs a `warn!`).
///
/// VAT-01 (verified vs OPANAF 3769/2015 + 3281/2020 + structura D394): a TAXABLE purchase (S/SR/AE/K)
/// from a partner who is NOT a valid RO VAT payer (tip_partener 2/3/4) is anomalous — a deductible-VAT
/// invoice can only come from a VAT-registered (tp=1) supplier. The old code mislabeled these as
/// livrare `"L"`, which is a SALES code: downstream it contaminated the collected-VAT (`tvaCol`) and the
/// `rezumat2` livrări summaries, over-declaring collected VAT. The technically-correct purchase code is
/// `"N"` (achiziție de la persoană neînregistrată), but `"N"` carries cota 0 / zero deductible VAT and
/// needs the `tip_N`/`nrN` sub-fields (schema-modelled only for tip_partener 1/2) — out of scope here.
/// Since such a line means the supplier CUI is simply dirty (re-validate → tp=1 → `"A"`), we DROP it
/// (+ warn). D394 is informative (no tax liability), so excluding an unreportable line is safe.
///
/// Rules: R215.2 tp=2 ⊆ {L/LS/N}; R215.3 tp=3/4 ⊆ {L/LS/C}; R217.2 C requires cota≠0; R232.1 C/A/AI need tva.
fn map_purchase_partner(
    partner: &D394Partner,
    tip_partener: i64,
    period: NaiveDate,
) -> Option<(&'static str, i64, bool)> {
    Some(match partner.vat_category.as_str() {
        "S" | "SR" => {
            let cota = parse_cota_from_rate(&partner.vat_rate, period);
            let cota = if cota == 0 {
                standard_cota_for(period)
            } else {
                cota
            };
            match tip_partener {
                1 => ("A", cota, true),
                // VAT-01: a deductible taxable purchase requires a registered (tp=1) supplier — drop.
                _ => return None,
            }
        }
        "AE" => {
            // Taxare inversă — C for all; cota MUST be ≠0 (R217.2)
            let cota = parse_cota_from_rate(&partner.vat_rate, period);
            let cota = if cota == 0 {
                standard_cota_for(period)
            } else {
                cota
            }; // must be non-zero
            match tip_partener {
                1 => ("C", cota, true), // self-assessed, needs op11
                // tp=3/4 → C allowed (R215.3)
                3 | 4 => ("C", cota, true),
                // VAT-01: tp=2 reverse-charge purchase with invalid CUI is anomalous (was "L") — drop.
                _ => return None,
            }
        }
        "K" => {
            // Intra-EU acquisition
            let cota = parse_cota_from_rate(&partner.vat_rate, period);
            let cota = if cota == 0 {
                standard_cota_for(period)
            } else {
                cota
            };
            match tip_partener {
                1 => ("AI", cota, true), // AI only valid for tp=1
                // tp=3/4 → use C (reverse-charge allowed for tp=3/4 per R215.3)
                3 | 4 => ("C", cota, true),
                // VAT-01: tp=2 intra-EU acquisition with invalid CUI is anomalous (was "L") — drop.
                _ => return None,
            }
        }
        "E" | "Z" => {
            // Scutit / zero-rate → AS (tp=1) or LS (tp=2/3/4)
            match tip_partener {
                1 => ("AS", 0, false),
                _ => ("LS", 0, false),
            }
        }
        "O" | "G" => match tip_partener {
            1 => ("AS", 0, false),
            _ => ("LS", 0, false),
        },
        _ => match tip_partener {
            1 => ("AS", 0, false),
            _ => ("LS", 0, false),
        },
    })
}

// ── Main builder ──────────────────────────────────────────────────────────────

/// Atributul `luna` din D394 după periodicitatea declarantului (OPANAF 3769/2015, modif. OPANAF
/// 2194/2025): lunar (`L`) = luna calendaristică; trimestrial (`T`) = ULTIMA lună a trimestrului
/// (3/6/9/12); semestrial (`S`) = 6 sau 12; anual (`A`) = 12. Înainte se emitea mereu luna
/// calendaristică a începutului perioadei, inconsistent cu `tip_D394="T"` pentru un plătitor trimestrial.
fn period_attr_luna(tip_d394: &str, month: u32) -> i32 {
    match tip_d394 {
        "T" => (((month - 1) / 3) * 3 + 3) as i32, // 1-3→3, 4-6→6, 7-9→9, 10-12→12
        "S" => {
            if month <= 6 {
                6
            } else {
                12
            }
        }
        "A" => 12,
        _ => month as i32, // "L" lunar (și orice necunoscut → luna calendaristică)
    }
}

/// Build the complete `D394Doc` from a `D394Report` + `D394Submission` + `Company`.
///
/// Returns `AppResult<D394Doc>` which the generator will serialize to XML.
pub fn build_sections(
    report: &D394Report,
    submission: &D394Submission,
    _company: &Company,
    period: NaiveDate,
) -> AppResult<D394Doc> {
    let luna = period_attr_luna(&submission.tip_d394, period.month());
    let an = period.year();

    // ── Build op1 list ────────────────────────────────────────────────────────

    let mut op1_list: Vec<Op1> = Vec::new();

    // Sales (livrări) → various tip types
    for partner in &report.partners {
        let tip_p = tip_partener_from_cui(&partner.partner_cui);
        let (tip, cota, emit_tva) = map_sales_partner(partner, period);
        let cui_digits = strip_ro(&partner.partner_cui);
        let baza = round_to_lei(parse_dec(&partner.base));
        let tva_val = round_to_lei(parse_dec(&partner.vat));
        let tva = if emit_tva { opt_nonzero(tva_val) } else { None };
        let den_p = partner.partner_name.chars().take(200).collect::<String>();
        let den_p = if den_p.trim().is_empty() {
            "NECUNOSCUT".to_string()
        } else {
            den_p
        };

        // Build op11 for tp=1 & tip∈{C,V} (R233.5)
        let op11_list = if tip_p == 1 && (tip == "C" || tip == "V") {
            // R235: use art331_code from partner if valid, else default 22.
            let cod_pr = resolve_cod_pr(&partner.art331_code, tip_p);
            let tva_pr = if tip == "C" {
                // C: tvaPR required (R237.1)
                Some(tva.unwrap_or(0))
            } else {
                // V: tvaPR must be ABSENT (R237.1)
                None
            };
            vec![Op11 {
                nr_fact_pr: partner.invoice_count,
                cod_pr,
                baza_pr: baza,
                tva_pr,
            }]
        } else {
            vec![]
        };

        op1_list.push(Op1 {
            tip: tip.to_string(),
            tip_partener: tip_p,
            cota,
            cui_p: cui_digits.chars().take(50).collect(),
            den_p,
            nr_fact: partner.invoice_count,
            baza,
            tva,
            op11_list,
        });
    }

    // Purchases (achiziții) → A / C / AI / AS / LS  (taxable purchases from non-tp=1 partners are dropped)
    for partner in &report.purchase_partners {
        let tip_p = tip_partener_from_cui(&partner.partner_cui);
        // VAT-01: a taxable purchase from a partner that isn't a valid RO VAT payer (tip_partener 2/3/4)
        // returns None — exclude it (it was wrongly emitted as livrare "L", inflating collected VAT).
        let Some((tip, cota, emit_tva)) = map_purchase_partner(partner, tip_p, period) else {
            tracing::warn!(
                tip_partener = tip_p,
                cui = %partner.partner_cui,
                categorie = %partner.vat_category,
                "D394: achiziție impozabilă de la partener neînregistrat/invalid în scopuri de TVA — \
                 exclusă din D394; re-validați CUI-ul furnizorului (o achiziție cu TVA deductibil \
                 necesită un furnizor înregistrat în scopuri de TVA)."
            );
            continue;
        };
        let cui_digits = strip_ro(&partner.partner_cui);
        let baza = round_to_lei(parse_dec(&partner.base));
        let tva_val = round_to_lei(parse_dec(&partner.vat));
        let tva = if emit_tva { opt_nonzero(tva_val) } else { None };
        let den_p = partner.partner_name.chars().take(200).collect::<String>();
        let den_p = if den_p.trim().is_empty() {
            "NECUNOSCUT".to_string()
        } else {
            den_p
        };

        // Build op11 for tp=1 & tip∈{C,V} (R233.5)
        let op11_list = if tip_p == 1 && (tip == "C" || tip == "V") {
            // R235: use art331_code from partner if valid, else default 22.
            let cod_pr = resolve_cod_pr(&partner.art331_code, tip_p);
            let tva_pr = if tip == "C" {
                Some(tva.unwrap_or(0))
            } else {
                None
            };
            vec![Op11 {
                nr_fact_pr: partner.invoice_count,
                cod_pr,
                baza_pr: baza,
                tva_pr,
            }]
        } else {
            vec![]
        };

        op1_list.push(Op1 {
            tip: tip.to_string(),
            tip_partener: tip_p,
            cota,
            cui_p: cui_digits.chars().take(50).collect(),
            den_p,
            nr_fact: partner.invoice_count,
            baza,
            tva,
            op11_list,
        });
    }

    // ── Validate / fix cuiP for tp=1 (R218.2 requires valid CUI) ─────────────
    // If tp=1 but CUI fails the checksum, demote to tp=2 to avoid R218.2.
    // (Real data should have valid CUIs; this is a defensive fallback.)
    for op in op1_list.iter_mut() {
        if op.tip_partener == 1 && !op.cui_p.is_empty() && !valid_cui(&op.cui_p) {
            tracing::warn!(
                "D394: op1 cuiP '{}' (partner '{}') fails CUI checksum; demoting tp=1→2",
                op.cui_p,
                op.den_p
            );
            op.tip_partener = 2;
            // Re-evaluate tip for tp=2 constraints (R215.2: only L/LS/N)
            // If the tip is not in {L, LS, N}, change to the closest safe value.
            if !["L", "LS", "N"].contains(&op.tip.as_str()) {
                // For purchases (A, AI, C → LS is safe; for sales V → LS)
                op.tip = "LS".to_string();
                op.cota = 0;
                op.tva = None;
                op.op11_list.clear();
            }
        }
    }

    // ── Build rezumat1 list ───────────────────────────────────────────────────
    // One rezumat1 per distinct (tip_partener, cota) present in op1.
    // Field presence follows DUK rules R38-R62.

    #[derive(Default)]
    struct R1Acc {
        facturi_l: i64,
        baza_l: i64,
        tva_l: i64,
        facturi_ls: i64,
        baza_ls: i64,
        facturi_a: i64,
        baza_a: i64,
        tva_a: i64,
        facturi_ai: i64,
        baza_ai: i64,
        tva_ai: i64,
        facturi_as: i64,
        baza_as: i64,
        facturi_v: i64,
        baza_v: i64,
        facturi_c: i64,
        baza_c: i64,
        tva_c: i64,
    }

    let mut r1_map: BTreeMap<(i64, i64), R1Acc> = BTreeMap::new();

    for op in &op1_list {
        let acc = r1_map.entry((op.tip_partener, op.cota)).or_default();
        let tva = op.tva.unwrap_or(0);
        match op.tip.as_str() {
            "L" => {
                acc.facturi_l += op.nr_fact;
                acc.baza_l += op.baza;
                acc.tva_l += tva;
            }
            "LS" => {
                acc.facturi_ls += op.nr_fact;
                acc.baza_ls += op.baza;
            }
            "A" => {
                acc.facturi_a += op.nr_fact;
                acc.baza_a += op.baza;
                acc.tva_a += tva;
            }
            "AI" => {
                acc.facturi_ai += op.nr_fact;
                acc.baza_ai += op.baza;
                acc.tva_ai += tva;
            }
            "AS" => {
                acc.facturi_as += op.nr_fact;
                acc.baza_as += op.baza;
            }
            "V" => {
                acc.facturi_v += op.nr_fact;
                acc.baza_v += op.baza;
            }
            "C" => {
                acc.facturi_c += op.nr_fact;
                acc.baza_c += op.baza;
                acc.tva_c += tva;
            }
            _ => {}
        }
    }

    // ── Build Detaliu accumulators per (tip_partener, cota, bun) ─────────────
    // Detaliu is needed for each codPR referenced by op11 elements.
    // Key: (tip_partener, cota) → BTreeMap<codPR → (nr_v, baza_v, nr_c, baza_c, tva_c)>
    // We also need (nr_fact for V, baza for V) and (nr_fact for C, baza for C, tva for C).
    #[derive(Default, Clone)]
    struct DetAcc {
        nr_v: i64,
        baza_v: i64,
        nr_c: i64,
        baza_c: i64,
        tva_c: i64,
    }
    let mut det_map: BTreeMap<(i64, i64), BTreeMap<i64, DetAcc>> = BTreeMap::new();

    for op in &op1_list {
        for op11 in &op.op11_list {
            // `detaliu/@bun` is an Int_nomenclatorBunuri code (21..=36). op11/@codPR also allows
            // other values (e.g. cereal NC codes 1001/1201… valid for art. 331), which must NOT
            // become a <detaliu> — they'd fail the XSD enum. Skip them here (op11 still emits them).
            if !(21..=36).contains(&op11.cod_pr) {
                continue;
            }
            let entry = det_map
                .entry((op.tip_partener, op.cota))
                .or_default()
                .entry(op11.cod_pr)
                .or_default();
            match op.tip.as_str() {
                "V" => {
                    entry.nr_v += op11.nr_fact_pr;
                    entry.baza_v += op11.baza_pr;
                }
                "C" => {
                    entry.nr_c += op11.nr_fact_pr;
                    entry.baza_c += op11.baza_pr;
                    entry.tva_c += op11.tva_pr.unwrap_or(0);
                }
                _ => {}
            }
        }
    }

    // Convert accumulators to Rezumat1, applying EXACT field-presence rules from DUK validator.
    //
    // KEY INSIGHT (from Rezumat1.aggregation validFBT analysis):
    // The validator requires PRESENCE of fields based on (tp, cota) combination,
    // regardless of whether there are actual ops. "Required" means emit with value 0 if no ops.
    //
    //   facturiL/bazaL/tvaL:       REQUIRED when cota≠0, nArray=null (all tp)
    //   facturiLS/bazaLS:           REQUIRED when cota=0,  nArray=null (all tp) [tp=2 has special path, skip]
    //   facturiA/bazaA/tvaA:        REQUIRED when cota≠0 AND tp=1
    //   facturiAI/bazaAI/tvaAI:     REQUIRED when cota≠0 AND tp=1
    //   facturiAS/bazaAS:            REQUIRED when cota=0  AND tp=1
    //   facturiV/bazaV:              REQUIRED when cota=0  AND tp=1 (nArray={1})
    //   facturiC/bazaC/tvaC:         REQUIRED when cota≠0 AND tp∈{1,3,4}
    //
    // We skip tp=2 special handling (N fields) as we have no N-category data.
    let mut rezumat1_list: Vec<Rezumat1> = r1_map
        .into_iter()
        .map(|((tp, cota), acc)| {
            // Required field groups based on (tp, cota):
            let req_l = cota != 0; // all tp
            let req_ls = cota == 0 && tp != 2; // all tp except tp=2 special
            let req_a = cota != 0 && tp == 1;
            let req_ai = cota != 0 && tp == 1;
            let req_as = cota == 0 && tp == 1;
            let req_v = cota == 0 && tp == 1;
            let req_c = cota != 0 && (tp == 1 || tp == 3 || tp == 4);

            // Build Detaliu list for this (tp, cota) from the det_map.
            //
            // Detaliu field constraints (from Detaliu.validFBT):
            //   nrLivV/bazaLivV: REQUIRED for tp=1, cota=0; FORBIDDEN otherwise
            //   nrAchizC/bazaAchizC/tvaAchizC: REQUIRED for tp=1, cota≠0; FORBIDDEN otherwise
            let detaliu_list: Vec<Detaliu> = det_map
                .get(&(tp, cota))
                .map(|bun_map| {
                    bun_map
                        .iter()
                        .map(|(&bun, da)| Detaliu {
                            bun,
                            // nrLivV/bazaLivV: only for tp=1, cota=0 (V ops)
                            nr_liv_v: if tp == 1 && cota == 0 {
                                Some(da.nr_v)
                            } else {
                                None
                            },
                            baza_liv_v: if tp == 1 && cota == 0 {
                                Some(da.baza_v)
                            } else {
                                None
                            },
                            // nrAchizC/bazaAchizC/tvaAchizC: only for tp=1, cota≠0 (C ops)
                            nr_achiz_c: if tp == 1 && cota != 0 {
                                Some(da.nr_c)
                            } else {
                                None
                            },
                            baza_achiz_c: if tp == 1 && cota != 0 {
                                Some(da.baza_c)
                            } else {
                                None
                            },
                            tva_achiz_c: if tp == 1 && cota != 0 {
                                Some(da.tva_c)
                            } else {
                                None
                            },
                        })
                        .collect()
                })
                .unwrap_or_default();

            Rezumat1 {
                tip_partener: tp,
                cota,
                // Emit required fields; use actual value (may be 0)
                facturi_l: if req_l { Some(acc.facturi_l) } else { None },
                baza_l: if req_l { Some(acc.baza_l) } else { None },
                tva_l: if req_l { Some(acc.tva_l) } else { None },
                facturi_ls: if req_ls { Some(acc.facturi_ls) } else { None },
                baza_ls: if req_ls { Some(acc.baza_ls) } else { None },
                facturi_a: if req_a { Some(acc.facturi_a) } else { None },
                baza_a: if req_a { Some(acc.baza_a) } else { None },
                tva_a: if req_a { Some(acc.tva_a) } else { None },
                facturi_ai: if req_ai { Some(acc.facturi_ai) } else { None },
                baza_ai: if req_ai { Some(acc.baza_ai) } else { None },
                tva_ai: if req_ai { Some(acc.tva_ai) } else { None },
                facturi_as: if req_as { Some(acc.facturi_as) } else { None },
                baza_as: if req_as { Some(acc.baza_as) } else { None },
                facturi_v: if req_v { Some(acc.facturi_v) } else { None },
                baza_v: if req_v { Some(acc.baza_v) } else { None },
                facturi_c: if req_c { Some(acc.facturi_c) } else { None },
                baza_c: if req_c { Some(acc.baza_c) } else { None },
                tva_c: if req_c { Some(acc.tva_c) } else { None },
                // N fields: not emitted (no N-category ops)
                facturi_n: None,
                document_n: None,
                baza_n: None,
                detaliu_list,
            }
        })
        .collect();

    rezumat1_list.sort_by_key(|r| (r.tip_partener, r.cota));

    // ── Build rezumat2 list ───────────────────────────────────────────────────
    // One rezumat2 per distinct cota≠0 in op1 (R84).
    // Aggregates:
    //   nrFacturiL/bazaL/tvaL = Σop1(tip∈{L,V}, cota)  — validator buckets V with L
    //   nrFacturiA/bazaA/tvaA = Σop1(tip∈{A,C}, cota)  — validator buckets C with A
    //   nrFacturiAI/bazaAI/tvaAI = Σop1(tip=AI, cota)

    #[derive(Default)]
    struct R2Acc {
        nr_l: i64,
        baza_l: i64,
        tva_l: i64,
        nr_a: i64,
        baza_a: i64,
        tva_a: i64,
        nr_ai: i64,
        baza_ai: i64,
        tva_ai: i64,
    }

    let mut r2_map: BTreeMap<i64, R2Acc> = BTreeMap::new();

    for op in &op1_list {
        if op.cota == 0 {
            continue;
        }
        let acc = r2_map.entry(op.cota).or_default();
        let tva = op.tva.unwrap_or(0);
        match op.tip.as_str() {
            "L" | "V" => {
                acc.nr_l += op.nr_fact;
                acc.baza_l += op.baza;
                acc.tva_l += tva;
            }
            "A" | "C" => {
                acc.nr_a += op.nr_fact;
                acc.baza_a += op.baza;
                acc.tva_a += tva;
            }
            "AI" => {
                acc.nr_ai += op.nr_fact;
                acc.baza_ai += op.baza;
                acc.tva_ai += tva;
            }
            _ => {}
        }
    }

    // Încasări numerar + facturi simplificate (cartuș G/I), introduse manual pe cotă. O cotă care are
    // DOAR date numerar (fără facturi în op1) trebuie totuși să primească un rezumat2 — altfel sumele
    // ei dispar. Ignorăm cota 0 (i1/i2 se emit doar pentru cota≠24, iar cota 0 nu are sens aici).
    // Agregăm pe cotă SUMÂND rândurile duplicate (mai multe D394CashRow cu aceeași cotă) — altfel un
    // BTreeMap colectat ar păstra doar ultimul rând și ar pierde tăcut datele celorlalte.
    let mut cash_map: BTreeMap<i64, D394CashRow> = BTreeMap::new();
    for c in submission.cash_rows.iter().filter(|c| c.cota != 0) {
        let e = cash_map.entry(c.cota).or_insert_with(|| D394CashRow {
            cota: c.cota,
            ..Default::default()
        });
        e.baza_i1 += c.baza_i1;
        e.tva_i1 += c.tva_i1;
        e.baza_i2 += c.baza_i2;
        e.tva_i2 += c.tva_i2;
        e.baza_fsl += c.baza_fsl;
        e.tva_fsl += c.tva_fsl;
        e.baza_fsl_cod += c.baza_fsl_cod;
        e.tva_fsl_cod += c.tva_fsl_cod;
        e.baza_fsa += c.baza_fsa;
        e.tva_fsa += c.tva_fsa;
        e.baza_fsai += c.baza_fsai;
        e.tva_fsai += c.tva_fsai;
        e.baza_bfai += c.baza_bfai;
        e.tva_bfai += c.tva_bfai;
    }
    for (&cota, c) in &cash_map {
        let has_data = c.baza_i1 != 0
            || c.tva_i1 != 0
            || c.baza_i2 != 0
            || c.tva_i2 != 0
            || c.baza_fsl != 0
            || c.tva_fsl != 0
            || c.baza_fsl_cod != 0
            || c.tva_fsl_cod != 0
            || c.baza_fsa != 0
            || c.tva_fsa != 0
            || c.baza_fsai != 0
            || c.tva_fsai != 0
            || c.baza_bfai != 0
            || c.tva_bfai != 0;
        if has_data {
            r2_map.entry(cota).or_default();
        }
    }

    let rezumat2_list: Vec<Rezumat2> = r2_map
        .into_iter()
        .map(|(cota, acc)| {
            let c = cash_map.get(&cota);
            let g = |f: fn(&D394CashRow) -> i64| c.map(f).unwrap_or(0);
            Rezumat2 {
                cota,
                nr_facturi_l: acc.nr_l,
                baza_l: acc.baza_l,
                tva_l: acc.tva_l,
                nr_facturi_a: acc.nr_a,
                baza_a: acc.baza_a,
                tva_a: acc.tva_a,
                nr_facturi_ai: acc.nr_ai,
                baza_ai: acc.baza_ai,
                tva_ai: acc.tva_ai,
                // R105-R108: required when cota≠24; din rândurile numerar (0 când lipsesc).
                baza_incasari_i1: g(|c| c.baza_i1),
                tva_incasari_i1: g(|c| c.tva_i1),
                baza_incasari_i2: g(|c| c.baza_i2),
                tva_incasari_i2: g(|c| c.tva_i2),
                // Facturi simplificate (cartuș I) per cotă.
                baza_fsl: g(|c| c.baza_fsl),
                tva_fsl: g(|c| c.tva_fsl),
                baza_fsl_cod: g(|c| c.baza_fsl_cod),
                tva_fsl_cod: g(|c| c.tva_fsl_cod),
                baza_fsa: g(|c| c.baza_fsa),
                tva_fsa: g(|c| c.tva_fsa),
                baza_fsai: g(|c| c.baza_fsai),
                tva_fsai: g(|c| c.tva_fsai),
                baza_bfai: g(|c| c.baza_bfai),
                tva_bfai: g(|c| c.tva_bfai),
            }
        })
        .collect();

    // ── Determine serieFacturi and nrFacturi ─────────────────────────────────
    // R112.1: if L/LS/V ops exist → nrFacturi OR nrFacturi_benef OR nrFacturi_terti > 0
    // R131: nrFacturi > 0 ↔ serieFacturi tip=2 exists
    // R112.2: if serieFacturi tip=2 exists → serieFacturi tip=1 must also exist
    // R112.3: op_efectuate=0 → no serieFacturi
    //
    // Strategy: when L/LS/V ops exist and op_efectuate is active,
    // emit serieFacturi tip=1 + tip=2, set nrFacturi = total sales invoices.
    let has_l_ls_v = op1_list
        .iter()
        .any(|op| op.tip == "L" || op.tip == "LS" || op.tip == "V");

    let effective_op_efectuate = submission.op_efectuate || !op1_list.is_empty();

    let (serie_facturi, nr_facturi) = if has_l_ls_v && effective_op_efectuate {
        // Emit tip=1 (series start) and tip=2 (invoice batch) per R112.1 + R112.2 + R131
        let sales_invoices = report.invoice_count;
        (
            vec![
                SerieFacturi {
                    tip: 1,
                    nr_i: "1".to_string(),
                },
                SerieFacturi {
                    tip: 2,
                    nr_i: "1".to_string(),
                },
            ],
            sales_invoices,
        )
    } else {
        (vec![], 0)
    };

    // ── Build informatii ──────────────────────────────────────────────────────

    // nrCui1: distinct cuiP for tp=1
    // nrCui2: count of op1 lines with tp=2 (validator increments per op1 line)
    // nrCui3: distinct cuiP for tp=3
    // nrCui4: distinct cuiP for tp=4
    let mut cuis_tp1: BTreeSet<String> = BTreeSet::new();
    let mut count_tp2: i64 = 0;
    let mut cuis_tp3: BTreeSet<String> = BTreeSet::new();
    let mut cuis_tp4: BTreeSet<String> = BTreeSet::new();

    for op in &op1_list {
        match op.tip_partener {
            1 => {
                cuis_tp1.insert(op.cui_p.clone());
            }
            2 => {
                count_tp2 += 1;
            }
            3 if !op.cui_p.is_empty() => {
                cuis_tp3.insert(op.cui_p.clone());
            }
            4 if !op.cui_p.is_empty() => {
                cuis_tp4.insert(op.cui_p.clone());
            }
            _ => {}
        }
    }

    let nr_cui1 = cuis_tp1.len() as i64;
    let nr_cui2 = count_tp2;
    let nr_cui3 = cuis_tp3.len() as i64;
    let nr_cui4 = cuis_tp4.len() as i64;

    // tvaCol per rate = Σ op1(tip=L, cota=rate).tva
    let mut tva_col: BTreeMap<i64, i64> = BTreeMap::new();
    for op in op1_list.iter().filter(|o| o.tip == "L") {
        if let Some(tva) = op.tva {
            *tva_col.entry(op.cota).or_insert(0) += tva;
        }
    }

    // tvaDed per rate = Σ op1(tip=A, cota=rate).tva
    let mut tva_ded: BTreeMap<i64, i64> = BTreeMap::new();
    for op in op1_list.iter().filter(|o| o.tip == "A") {
        if let Some(tva) = op.tva {
            *tva_ded.entry(op.cota).or_insert(0) += tva;
        }
    }

    // tvaDedAI per rate = Σ op1(tip=AI, cota=rate).tva [all required → 0 if absent]
    let mut tva_ded_ai: BTreeMap<i64, i64> = BTreeMap::new();
    for op in op1_list.iter().filter(|o| o.tip == "AI") {
        if let Some(tva) = op.tva {
            *tva_ded_ai.entry(op.cota).or_insert(0) += tva;
        }
    }

    // totalPlata_A (R17) = nrCui1+nrCui2+nrCui3+nrCui4 + Σrezumat2(bazaL+bazaA+bazaAI)
    let rezumat2_base_sum: i64 = rezumat2_list
        .iter()
        .map(|r| r.baza_l + r.baza_a + r.baza_ai)
        .sum();
    let total_plata_a = nr_cui1 + nr_cui2 + nr_cui3 + nr_cui4 + rezumat2_base_sum;

    // tvaCol/tvaDed: only emitted when sistemTVA=1
    let mk_opt = |map: &BTreeMap<i64, i64>, rate: i64| -> Option<i64> {
        if submission.sistem_tva {
            opt_nonzero(*map.get(&rate).unwrap_or(&0))
        } else {
            None
        }
    };

    // Sumele-total cartuș G se CALCULEAZĂ din rândurile pe cotă (regula DUK
    // incasari_iN = Σ_cote (baza_iN + tva_iN)) — astfel reconcilierea e garantată prin construcție.
    let incasari_i1: i64 = cash_map.values().map(|c| c.baza_i1 + c.tva_i1).sum();
    let incasari_i2: i64 = cash_map.values().map(|c| c.baza_i2 + c.tva_i2).sum();

    let informatii = Informatii {
        nr_cui1,
        nr_cui2,
        nr_cui3,
        nr_cui4,
        nr_bf_i1: submission.nr_bf_i1,
        incasari_i1,
        incasari_i2,
        nr_facturi_terti: 0,
        nr_facturi_benef: 0,
        nr_facturi,
        nr_facturi_l_pf: 0,
        nr_facturi_ls_pf: 0,
        val_ls_pf: 0,
        tva_col24: mk_opt(&tva_col, 24),
        tva_col21: mk_opt(&tva_col, 21),
        tva_col11: mk_opt(&tva_col, 11),
        tva_col20: mk_opt(&tva_col, 20),
        tva_col19: mk_opt(&tva_col, 19),
        tva_col9: mk_opt(&tva_col, 9),
        tva_col5: mk_opt(&tva_col, 5),
        tva_ded24: mk_opt(&tva_ded, 24),
        tva_ded21: mk_opt(&tva_ded, 21),
        tva_ded11: mk_opt(&tva_ded, 11),
        tva_ded20: mk_opt(&tva_ded, 20),
        tva_ded19: mk_opt(&tva_ded, 19),
        tva_ded9: mk_opt(&tva_ded, 9),
        tva_ded5: mk_opt(&tva_ded, 5),
        // ALL tvaDedAI* are REQUIRED → must be present, default 0
        tva_ded_ai24: *tva_ded_ai.get(&24).unwrap_or(&0),
        tva_ded_ai21: *tva_ded_ai.get(&21).unwrap_or(&0),
        tva_ded_ai11: *tva_ded_ai.get(&11).unwrap_or(&0),
        tva_ded_ai20: *tva_ded_ai.get(&20).unwrap_or(&0),
        tva_ded_ai19: *tva_ded_ai.get(&19).unwrap_or(&0),
        tva_ded_ai9: *tva_ded_ai.get(&9).unwrap_or(&0),
        tva_ded_ai5: *tva_ded_ai.get(&5).unwrap_or(&0),
        solicit: if submission.solicit { 1 } else { 0 },
        // R191.1: efectuat only when solicit=1
        efectuat: if submission.solicit {
            opt_nonzero(if submission.op_efectuate { 1 } else { 0 })
        } else {
            None
        },
    };

    Ok(D394Doc {
        luna,
        an,
        informatii,
        serie_facturi,
        rezumat1_list,
        rezumat2_list,
        op1_list,
        total_plata_a,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::d394::{D394Partner, D394Report};
    use crate::db::companies::Company;

    fn make_company() -> Company {
        Company {
            id: "test-id".to_string(),
            // Use a valid CUI checksum: 12345674
            cui: "RO12345674".to_string(),
            legal_name: "Test SRL".to_string(),
            trade_name: None,
            registry_number: None,
            vat_payer: true,
            cash_vat: false,
            address: "Str. Testului 1".to_string(),
            city: "Bucuresti".to_string(),
            county: "IF".to_string(),
            postal_code: None,
            country: "RO".to_string(),
            email: None,
            phone: None,
            iban: None,
            bank_name: None,
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

    fn make_submission() -> D394Submission {
        D394Submission {
            tip_d394: "L".to_string(),
            caen: "6201".to_string(),
            telefon: "0721000000".to_string(),
            den_r: "POPESCU ION".to_string(),
            functie_reprez: "DIRECTOR".to_string(),
            adresa_r: "Str. Test 1, Bucuresti".to_string(),
            den_intocmit: "POPESCU ION".to_string(),
            // Use valid CUI: 12345674
            cif_intocmit: 12345674,
            ..Default::default()
        }
    }

    fn make_report(
        sales: Vec<(&str, &str, &str, i64, &str, &str)>,
        purchases: Vec<(&str, &str, &str, i64, &str, &str)>,
    ) -> D394Report {
        let partners = sales
            .into_iter()
            .map(|(cui, cat, rate, count, base, vat)| D394Partner {
                partner_cui: cui.to_string(),
                partner_name: format!("Partner {}", cui),
                vat_category: cat.to_string(),
                vat_rate: rate.to_string(),
                invoice_count: count,
                base: base.to_string(),
                vat: vat.to_string(),
                art331_code: None,
            })
            .collect();

        let purchase_partners = purchases
            .into_iter()
            .map(|(cui, cat, rate, count, base, vat)| D394Partner {
                partner_cui: cui.to_string(),
                partner_name: format!("Supplier {}", cui),
                vat_category: cat.to_string(),
                vat_rate: rate.to_string(),
                invoice_count: count,
                base: base.to_string(),
                vat: vat.to_string(),
                art331_code: None,
            })
            .collect();

        D394Report {
            company_cui: "RO12345674".to_string(),
            period_from: "2025-09-01".to_string(),
            period_to: "2025-09-30".to_string(),
            partners,
            total_base: "0.00".to_string(),
            total_vat: "0.00".to_string(),
            invoice_count: 5,
            purchase_partners,
            total_purchase_base: "0.00".to_string(),
            total_purchase_vat: "0.00".to_string(),
            purchase_invoice_count: 3,
            purchase_unparsed_count: 0,
        }
    }

    /// VAT-01: a TAXABLE purchase from a partner that is NOT a valid RO VAT payer (tip_partener 2/3/4)
    /// must be DROPPED (returns None) — it was wrongly emitted as livrare "L", contaminating the
    /// collected-VAT (tvaCol) + rezumat2 livrări summaries. A valid registered (tp=1) supplier still maps
    /// to the correct purchase code ("A"/"C"/"AI").
    #[test]
    fn vat01_taxable_purchase_from_nonvat_partner_is_dropped() {
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let mk = |cat: &str, rate: &str, cui: &str| D394Partner {
            partner_cui: cui.to_string(),
            partner_name: "Supplier".to_string(),
            vat_category: cat.to_string(),
            vat_rate: rate.to_string(),
            invoice_count: 1,
            base: "100.00".to_string(),
            vat: "21.00".to_string(),
            art331_code: None,
        };
        // Valid RO VAT payer (tp=1) → correct purchase codes.
        assert_eq!(
            map_purchase_partner(&mk("S", "21", "RO12345674"), 1, period),
            Some(("A", 21, true))
        );
        assert_eq!(
            map_purchase_partner(&mk("AE", "21", "RO12345674"), 1, period).map(|t| t.0),
            Some("C")
        );
        // tip_partener 2/3/4 taxable purchase → DROPPED (None), no longer mislabeled "L".
        for tp in [2, 3, 4] {
            assert_eq!(
                map_purchase_partner(&mk("S", "21", "INVALID"), tp, period),
                None,
                "S taxable purchase from tp={tp} must be dropped, not emitted as L"
            );
        }
        assert_eq!(map_purchase_partner(&mk("AE", "21", "X"), 2, period), None);
        assert_eq!(map_purchase_partner(&mk("K", "21", "X"), 2, period), None);
        // AE/K from tp=3/4 stay reverse-charge "C" (not contaminating) — unchanged.
        assert_eq!(
            map_purchase_partner(&mk("AE", "21", "X"), 4, period).map(|t| t.0),
            Some("C")
        );
        assert_eq!(
            map_purchase_partner(&mk("K", "21", "X"), 3, period).map(|t| t.0),
            Some("C")
        );
        // Exempt purchases (cota 0) are unaffected — they don't contaminate the VAT summaries.
        assert_eq!(
            map_purchase_partner(&mk("E", "0", "X"), 2, period).map(|t| t.0),
            Some("LS")
        );
    }

    // ── FIX 3: D394 standard-cota fallback is period-aware ────────────────────

    /// standard_cota_for: 19% before 2025-08-01, 21% from 2025-08-01 onward.
    #[test]
    fn standard_cota_for_period_aware() {
        // Cutover date: exactly 2025-08-01 → 21
        assert_eq!(
            standard_cota_for(NaiveDate::from_ymd_opt(2025, 8, 1).unwrap()),
            21,
            "Standard rate from 2025-08-01 must be 21"
        );
        // Day before cutover → 19
        assert_eq!(
            standard_cota_for(NaiveDate::from_ymd_opt(2025, 7, 31).unwrap()),
            19,
            "Standard rate before 2025-08-01 must be 19"
        );
        // 2026 period → 21
        assert_eq!(
            standard_cota_for(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
            21
        );
        // Historical 2024 → 19
        assert_eq!(
            standard_cota_for(NaiveDate::from_ymd_opt(2024, 12, 31).unwrap()),
            19
        );
    }

    /// A malformed standard line (unmapped rate) in a 2026 period must fall back to 21, not 19.
    #[test]
    fn d394_standard_fallback_21_for_2026_period() {
        // Use an unmapped rate (e.g. "15") that is NOT in {0,5,9,11,19,20,21,24}
        // so that parse_cota_from_rate triggers the fallback branch.
        // Valid CUI: 98765438
        let period = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let report = make_report(
            vec![("98765438", "S", "15", 1, "1000.00", "150.00")],
            vec![],
        );
        let sub = make_submission();
        let company = make_company();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let op = &doc.op1_list[0];
        assert_eq!(
            op.cota, 21,
            "Malformed standard cota in 2026 period must fall back to 21 (not 19)"
        );
    }

    /// A malformed standard line in a pre-2025-08-01 period must still fall back to 19.
    #[test]
    fn d394_standard_fallback_19_for_historical_period() {
        let period = NaiveDate::from_ymd_opt(2025, 7, 1).unwrap();
        let report = make_report(
            vec![("98765438", "S", "15", 1, "1000.00", "150.00")],
            vec![],
        );
        let sub = make_submission();
        let company = make_company();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let op = &doc.op1_list[0];
        assert_eq!(
            op.cota, 19,
            "Malformed standard cota in pre-Aug-2025 period must fall back to 19"
        );
    }

    #[test]
    fn period_attr_luna_by_periodicity() {
        // Lunar: luna calendaristică.
        assert_eq!(period_attr_luna("L", 6), 6);
        // Trimestrial: ultima lună a trimestrului, indiferent de luna aleasă în trimestru.
        assert_eq!(period_attr_luna("T", 1), 3); // Q1
        assert_eq!(period_attr_luna("T", 2), 3);
        assert_eq!(period_attr_luna("T", 4), 6); // Q2
        assert_eq!(period_attr_luna("T", 9), 9); // Q3
        assert_eq!(period_attr_luna("T", 11), 12); // Q4
                                                   // Semestrial + anual.
        assert_eq!(period_attr_luna("S", 3), 6);
        assert_eq!(period_attr_luna("S", 8), 12);
        assert_eq!(period_attr_luna("A", 5), 12);
    }

    #[test]
    fn sales_standard_maps_to_l_with_cota() {
        // Valid CUI: 98765438
        let report = make_report(
            vec![("98765438", "S", "19", 3, "1000.00", "190.00")],
            vec![],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert_eq!(doc.op1_list.len(), 1);
        let op = &doc.op1_list[0];
        assert_eq!(op.tip, "L");
        assert_eq!(op.cota, 19);
        assert_eq!(op.baza, 1000);
        assert_eq!(op.tva, Some(190));
        assert_eq!(op.tip_partener, 1); // valid RO CUI
    }

    #[test]
    fn sales_ae_maps_to_v_cota0_no_tva() {
        // Valid CUI: 98765438
        let report = make_report(vec![("98765438", "AE", "0", 1, "500.00", "0.00")], vec![]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let op = &doc.op1_list[0];
        assert_eq!(op.tip, "V");
        assert_eq!(op.cota, 0);
        assert_eq!(op.tva, None);
        // op11 required for tp=1 & V
        assert!(!op.op11_list.is_empty(), "V with tp=1 needs op11");
        assert_eq!(op.op11_list[0].tva_pr, None, "V op11: no tvaPR");
    }

    #[test]
    fn sales_z_maps_to_ls_cota0() {
        // Valid CUI: 98765438
        let report = make_report(vec![("98765438", "Z", "0", 2, "2000.00", "0.00")], vec![]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let op = &doc.op1_list[0];
        assert_eq!(op.tip, "LS");
        assert_eq!(op.cota, 0);
        assert_eq!(op.tva, None);
    }

    #[test]
    fn purchase_standard_maps_to_a_with_cota() {
        // Valid CUI: 98765438
        let report = make_report(vec![], vec![("98765438", "S", "21", 2, "800.00", "168.00")]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert_eq!(doc.op1_list.len(), 1);
        let op = &doc.op1_list[0];
        assert_eq!(op.tip, "A");
        assert_eq!(op.cota, 21);
        assert_eq!(op.baza, 800);
        assert_eq!(op.tva, Some(168));
    }

    #[test]
    fn purchase_ae_maps_to_c_nonzero_cota() {
        // Valid CUI: 98765438 — AE purchase must have cota≠0 (R217.2)
        let report = make_report(vec![], vec![("98765438", "AE", "19", 1, "300.00", "57.00")]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let op = &doc.op1_list[0];
        assert_eq!(op.tip, "C");
        assert_ne!(op.cota, 0, "C must have non-zero cota (R217.2)");
        assert!(op.tva.is_some(), "C requires tva (R232.1)");
        // tp=1 with C needs op11
        assert!(!op.op11_list.is_empty(), "C with tp=1 needs op11");
        assert!(op.op11_list[0].tva_pr.is_some(), "C op11 needs tvaPR");
    }

    #[test]
    fn purchase_k_ro_maps_to_ai_with_cota() {
        // Valid CUI: 98765438 — K from RO partner → AI
        let report = make_report(
            vec![],
            vec![("98765438", "K", "19", 1, "1000.00", "190.00")],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let op = &doc.op1_list[0];
        assert_eq!(op.tip_partener, 1);
        assert_eq!(op.tip, "AI");
        assert_eq!(op.cota, 19);
        assert_eq!(op.tva, Some(190));
    }

    #[test]
    fn purchase_k_foreign_maps_to_c() {
        // Foreign EU partner → tp=4 → K purchase → tip=C (not AI)
        let report = make_report(
            vec![],
            vec![("FR55512345", "K", "19", 1, "1000.00", "190.00")],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let op = &doc.op1_list[0];
        assert_eq!(op.tip_partener, 4);
        assert_eq!(op.tip, "C", "Foreign K purchase → C (AI only for tp=1)");
        assert_ne!(op.cota, 0);
    }

    #[test]
    fn foreign_partner_gets_tp4() {
        // DE prefix → tp=4
        let report = make_report(
            vec![("DE123456789", "K", "0", 1, "2000.00", "0.00")],
            vec![],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let op = &doc.op1_list[0];
        assert_eq!(op.tip_partener, 4);
        assert_eq!(op.tip, "LS"); // K sale → LS
    }

    #[test]
    fn rezumat1_sums_reconcile_op1() {
        // Valid CUIs: 11111110, 22222229
        let report = make_report(
            vec![
                ("11111110", "S", "19", 2, "1000.00", "190.00"),
                ("22222229", "S", "19", 1, "500.00", "95.00"),
            ],
            vec![("98765438", "S", "21", 1, "800.00", "168.00")],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();

        assert_eq!(doc.op1_list.len(), 3);

        let r1 = doc
            .rezumat1_list
            .iter()
            .find(|r| r.tip_partener == 1 && r.cota == 19)
            .expect("should have rezumat1 for (1, 19)");
        assert_eq!(r1.facturi_l, Some(3)); // 2+1
        assert_eq!(r1.baza_l, Some(1500));
        assert_eq!(r1.tva_l, Some(285));

        let r2 = doc
            .rezumat1_list
            .iter()
            .find(|r| r.tip_partener == 1 && r.cota == 21)
            .expect("should have rezumat1 for (1, 21)");
        assert_eq!(r2.facturi_a, Some(1));
        assert_eq!(r2.baza_a, Some(800));
        assert_eq!(r2.tva_a, Some(168));
    }

    #[test]
    fn rezumat2_built_for_nonzero_cota() {
        // Valid CUI: 98765438
        let report = make_report(
            vec![("98765438", "S", "19", 3, "1000.00", "190.00")],
            vec![],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert!(
            !doc.rezumat2_list.is_empty(),
            "rezumat2 required for cota=19"
        );
        let r2 = &doc.rezumat2_list[0];
        assert_eq!(r2.cota, 19);
        assert_eq!(r2.nr_facturi_l, 3);
        assert_eq!(r2.baza_l, 1000);
    }

    #[test]
    fn cash_rows_feed_rezumat2_and_compute_incasari_totals() {
        // Vânzare pe cotă 21 (op1) + rând numerar pe cotă 21; PLUS un rând numerar pe cotă 11 FĂRĂ
        // facturi (cotă numai-numerar) — trebuie să primească totuși un rezumat2, altfel sumele dispar.
        let report = make_report(
            vec![("98765438", "S", "21", 2, "1000.00", "210.00")],
            vec![],
        );
        let mut sub = make_submission();
        sub.nr_bf_i1 = 37;
        sub.cash_rows = vec![
            D394CashRow {
                cota: 21,
                baza_i1: 5000,
                tva_i1: 1050,
                baza_i2: 200,
                tva_i2: 42,
                baza_fsl: 300,
                tva_fsl: 63,
                baza_fsl_cod: 100,
                tva_fsl_cod: 21,
                ..Default::default()
            },
            D394CashRow {
                cota: 11,
                baza_i1: 1000,
                tva_i1: 110,
                ..Default::default()
            },
        ];
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();

        let r21 = doc
            .rezumat2_list
            .iter()
            .find(|r| r.cota == 21)
            .expect("rezumat2 cotă 21");
        assert_eq!(r21.baza_incasari_i1, 5000);
        assert_eq!(r21.tva_incasari_i1, 1050);
        assert_eq!(r21.baza_incasari_i2, 200);
        assert_eq!(r21.baza_fsl, 300);
        assert_eq!(r21.baza_fsl_cod, 100);
        // Cotă 11 e numai-numerar (fără facturi) → rezumat2 generat din rândul numerar.
        let r11 = doc
            .rezumat2_list
            .iter()
            .find(|r| r.cota == 11)
            .expect("rezumat2 cotă 11 (numai numerar)");
        assert_eq!(r11.baza_incasari_i1, 1000);
        assert_eq!(r11.nr_facturi_l, 0);
        // Sumele-total cartuș G se CALCULEAZĂ din rânduri (regula DUK incasari = Σ(bază+TVA)).
        assert_eq!(doc.informatii.nr_bf_i1, 37);
        assert_eq!(doc.informatii.incasari_i1, 7160); // (5000+1050)+(1000+110)
        assert_eq!(doc.informatii.incasari_i2, 242); // (200+42)+0
    }

    #[test]
    fn duplicate_cota_cash_rows_are_summed_not_dropped() {
        // Două rânduri numerar pe ACEEAȘI cotă (21) trebuie SUMATE, nu păstrat doar ultimul (altfel
        // datele primului rând s-ar pierde tăcut — DUK D394-002).
        let report = make_report(
            vec![("98765438", "S", "21", 1, "1000.00", "210.00")],
            vec![],
        );
        let mut sub = make_submission();
        sub.cash_rows = vec![
            D394CashRow {
                cota: 21,
                baza_i1: 3000,
                tva_i1: 630,
                ..Default::default()
            },
            D394CashRow {
                cota: 21,
                baza_i1: 2000,
                tva_i1: 420,
                baza_fsl: 100,
                tva_fsl: 21,
                ..Default::default()
            },
        ];
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let r21 = doc
            .rezumat2_list
            .iter()
            .find(|r| r.cota == 21)
            .expect("rezumat2 cotă 21");
        assert_eq!(r21.baza_incasari_i1, 5000); // 3000 + 2000 (sumate)
        assert_eq!(r21.tva_incasari_i1, 1050); // 630 + 420
        assert_eq!(r21.baza_fsl, 100); // doar al 2-lea rând
        assert_eq!(doc.informatii.incasari_i1, 6050); // Σ(bază+TVA) = 5000 + 1050
    }

    #[test]
    fn informatii_tva_col_sums_l_operations() {
        let report = make_report(
            vec![
                ("11111110", "S", "19", 2, "1000.00", "190.00"),
                ("22222229", "S", "11", 1, "500.00", "55.00"),
            ],
            vec![],
        );
        let mut sub = make_submission();
        sub.sistem_tva = true;
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert_eq!(doc.informatii.tva_col19, Some(190));
        assert_eq!(doc.informatii.tva_col11, Some(55));
        assert_eq!(doc.informatii.tva_col21, None);
    }

    #[test]
    fn informatii_tva_ded_ai_always_present() {
        let report = make_report(vec![], vec![] as Vec<(&str, &str, &str, i64, &str, &str)>);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert_eq!(doc.informatii.tva_ded_ai21, 0);
        assert_eq!(doc.informatii.tva_ded_ai19, 0);
        assert_eq!(doc.informatii.tva_ded_ai5, 0);
    }

    #[test]
    fn pf_partner_gets_tip_partener_3() {
        let report = make_report(vec![("", "S", "19", 1, "200.00", "38.00")], vec![]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        // Empty CUI → tp=3; but R215.3 says tp=3 can only have L/LS/C.
        // S→L is in the allowed set for tp=3.
        assert_eq!(doc.op1_list[0].tip_partener, 3);
        assert_eq!(doc.op1_list[0].tip, "L");
    }

    #[test]
    fn nrcui2_counts_lines_not_distinct() {
        // tp=2 line count (not distinct CUIs)
        let report = make_report(
            vec![
                ("DE123456789", "K", "0", 1, "1000.00", "0.00"),
                ("DE987654321", "K", "0", 1, "500.00", "0.00"),
            ],
            vec![],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        // Both are foreign → tp=4 (not tp=2); nrCui4=2
        // nrCui2 = 0 since both are tp=4
        assert_eq!(doc.informatii.nr_cui4, 2);
        assert_eq!(doc.informatii.nr_cui2, 0);
    }

    #[test]
    fn serie_facturi_emitted_when_l_ls_v_ops_exist() {
        let report = make_report(
            vec![("98765438", "S", "19", 3, "1000.00", "190.00")],
            vec![],
        );
        let mut sub = make_submission();
        sub.op_efectuate = true;
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert!(
            !doc.serie_facturi.is_empty(),
            "serieFacturi required when L ops exist"
        );
        assert_eq!(doc.informatii.nr_facturi, 5); // report.invoice_count
    }

    #[test]
    fn total_plata_a_includes_rezumat2_bases() {
        // nrCui1=1, nrCui2=0, nrCui3=0, nrCui4=0
        // rezumat2(19).bazaL=1000
        // totalPlata_A = 1 + 1000 = 1001
        let report = make_report(
            vec![("98765438", "S", "19", 3, "1000.00", "190.00")],
            vec![],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let expected = doc.informatii.nr_cui1
            + doc.informatii.nr_cui2
            + doc.informatii.nr_cui3
            + doc.informatii.nr_cui4
            + doc
                .rezumat2_list
                .iter()
                .map(|r| r.baza_l + r.baza_a + r.baza_ai)
                .sum::<i64>();
        assert_eq!(doc.total_plata_a, expected);
    }

    #[test]
    fn sections_mixed_rate_partner_produces_two_op1_lines() {
        let report = make_report(
            vec![
                ("11111110", "S", "19", 3, "1000.00", "190.00"),
                ("11111110", "S", "21", 2, "500.00", "105.00"),
            ],
            vec![],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert_eq!(doc.op1_list.len(), 2);

        let cotas: Vec<i64> = doc.op1_list.iter().map(|op| op.cota).collect();
        assert!(cotas.contains(&19));
        assert!(cotas.contains(&21));

        let op19 = doc.op1_list.iter().find(|op| op.cota == 19).unwrap();
        assert_eq!(op19.baza, 1000);
        assert_eq!(op19.tva, Some(190));

        let op21 = doc.op1_list.iter().find(|op| op.cota == 21).unwrap();
        assert_eq!(op21.baza, 500);
        assert_eq!(op21.tva, Some(105));
    }

    // ── Art. 331 codPR tests ──────────────────────────────────────────────────

    /// resolve_cod_pr: art331_code="29" for tp=1 → codPR=29 (telefoane)
    #[test]
    fn resolve_cod_pr_uses_explicit_code_for_tp1() {
        let code = resolve_cod_pr(&Some("29".to_string()), 1);
        assert_eq!(code, 29, "valid art331_code=29 for tp=1 must be used as-is");
    }

    /// resolve_cod_pr: None → 22 (default)
    #[test]
    fn resolve_cod_pr_defaults_to_22_when_none() {
        let code = resolve_cod_pr(&None, 1);
        assert_eq!(code, 22, "missing art331_code must default to 22");
    }

    /// resolve_cod_pr: tp=1, code=32 (FORBIDDEN for tp=1) → fallback 22
    #[test]
    fn resolve_cod_pr_rejects_forbidden_tp1_code() {
        let code = resolve_cod_pr(&Some("32".to_string()), 1);
        assert_eq!(
            code, 22,
            "code 32 is forbidden for tp=1 (R235.1); must fall back to 22"
        );
    }

    /// resolve_cod_pr: tp=2, code=22 → 22 (allowed for tp=2)
    #[test]
    fn resolve_cod_pr_tp2_allows_22() {
        let code = resolve_cod_pr(&Some("22".to_string()), 2);
        assert_eq!(code, 22);
    }

    /// resolve_cod_pr: tp=2, code=29 (NOT in allowed tp=2 set) → fallback 22
    #[test]
    fn resolve_cod_pr_tp2_rejects_tp1_only_code() {
        let code = resolve_cod_pr(&Some("29".to_string()), 2);
        assert_eq!(
            code, 22,
            "code 29 is not in the tp=2 allowed set; must fall back to 22"
        );
    }

    /// resolve_cod_pr: cereal NC code 1001 valid for tp=1
    #[test]
    fn resolve_cod_pr_cereal_nc_code_valid_for_tp1() {
        let code = resolve_cod_pr(&Some("1001".to_string()), 1);
        assert_eq!(code, 1001, "NC cereal code 1001 must be accepted for tp=1");
    }

    /// AE sale with art331_code="29" → op11.cod_pr=29 (not 22)
    #[test]
    fn ae_sale_with_art331_code_emits_correct_cod_pr() {
        // Valid CUI: 76543210
        let mut report = make_report(vec![("76543210", "AE", "0", 1, "500.00", "0.00")], vec![]);
        // Set art331_code on the AE partner
        if let Some(p) = report.partners.first_mut() {
            p.art331_code = Some("29".to_string());
        }
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let ae_op = doc.op1_list.iter().find(|op| op.tip == "V").unwrap();
        assert!(
            !ae_op.op11_list.is_empty(),
            "AE sale (tp=1, V) must have op11"
        );
        assert_eq!(
            ae_op.op11_list[0].cod_pr, 29,
            "op11.codPR must be 29 (telefoane) when art331_code=29"
        );
    }

    /// AE sale without art331_code → op11.cod_pr=22 (default)
    #[test]
    fn ae_sale_without_art331_code_defaults_to_22() {
        // Valid CUI: 76543210
        let report = make_report(vec![("76543210", "AE", "0", 1, "500.00", "0.00")], vec![]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let ae_op = doc.op1_list.iter().find(|op| op.tip == "V").unwrap();
        assert!(
            !ae_op.op11_list.is_empty(),
            "AE sale (tp=1, V) must have op11"
        );
        assert_eq!(
            ae_op.op11_list[0].cod_pr, 22,
            "op11.codPR must default to 22 when art331_code is absent"
        );
    }
}
