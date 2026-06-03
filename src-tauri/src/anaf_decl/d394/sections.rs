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
//!   category=S   (standard taxed, rate>0)   → tip="A",  cota=rate, tva=group VAT
//!   category=AE  (taxare inversă domestică)  → tip="C",  cota=0,    no tva
//!   category=K   (achiziție intracomunitară) → tip="AI", cota=rate, tva=group VAT
//!   category=E   (scutit)                    → tip="AS", cota=0,    no tva
//!   category=Z   (zero/export)               → tip="AS", cota=0,    no tva
//!   category=O/G (outside scope)             → tip="AS", cota=0,    no tva
//!
//! ## tip_partener logic (XSD `Int_tipPartenerOp1SType`, values 1–4):
//!   1 = persoană înregistrată în scopuri de TVA (valid RO CUI, digits 2–10)
//!   2 = persoană juridică neînregistrată în scopuri de TVA
//!   3 = persoană fizică (CUI absent or empty)
//!   4 = nerezidenți (foreign partners — not implemented here, defaults to 2/3)
//!
//! We use: if partner CUI is non-empty and all-digits (2–10 chars) → tip_partener=1,
//!         else if CUI is empty → tip_partener=3 (person natural assumed),
//!         else → tip_partener=2 (juridical, not VAT registered).
//!
//! ## informatii computed fields:
//!   nrCui1 = distinct CUIs in op1 tip=L (tip_partener=1)
//!   nrCui2 = distinct CUIs in op1 tip=L (tip_partener=2)
//!   nrCui3 = distinct CUIs in op1 tip=L (tip_partener=3)
//!   nrCui4 = distinct CUIs in op1 tip=L (tip_partener=4)
//!   nr_BF_i1 / incasari_i1 / incasari_i2 = 0 (no cash-register data)
//!   nrFacturi_terti / nrFacturi_benef = 0 (no series data)
//!   nrFacturi = total distinct invoice count (sales + purchase)
//!   nrFacturiL_PF / nrFacturiLS_PF / val_LS_PF = 0 (no PF-specific data)
//!   tvaCol{rate} = Σ op1(tip=L, cota=rate).tva
//!   tvaDed{rate} = Σ op1(tip=A, cota=rate).tva
//!   tvaDedAI{rate} = Σ op1(tip=AI, cota=rate).tva  [ALL required → emit 0 if absent]
//!   solicit = from D394Submission
//!   efectuat = from D394Submission.op_efectuate (0/1)
//!
//! ALL amounts are rounded to whole lei (0 dp) before writing.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use chrono::Datelike;
use chrono::NaiveDate;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::commands::d394::{D394Partner, D394Report};
use crate::db::companies::Company;
use crate::error::AppResult;

use super::D394Submission;

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
    /// VAT amount (whole lei); XSD optional — omitted for cota=0 or when zero
    pub tva: Option<i64>,
}

/// A rezumat1 record (summary per (tip_partener, cota) across all op1 lines).
/// Mirrors `Rezumat1Type` in the XSD.
#[derive(Debug, Clone)]
pub struct Rezumat1 {
    pub tip_partener: i64,
    pub cota: i64,
    // tip=L fields
    pub facturi_l: Option<i64>,
    pub baza_l: Option<i64>,
    pub tva_l: Option<i64>,
    // tip=LS fields
    pub facturi_ls: Option<i64>,
    pub baza_ls: Option<i64>,
    // tip=A fields
    pub facturi_a: Option<i64>,
    pub baza_a: Option<i64>,
    pub tva_a: Option<i64>,
    // tip=AI fields
    pub facturi_ai: Option<i64>,
    pub baza_ai: Option<i64>,
    pub tva_ai: Option<i64>,
    // tip=AS fields
    pub facturi_as: Option<i64>,
    pub baza_as: Option<i64>,
    // tip=V fields
    pub facturi_v: Option<i64>,
    pub baza_v: Option<i64>,
    // tip=C fields
    pub facturi_c: Option<i64>,
    pub baza_c: Option<i64>,
    pub tva_c: Option<i64>,
}

/// The `<informatii>` block — grand summary.
/// ALL required attributes must be present (defaulting to 0).
#[derive(Debug, Clone)]
pub struct Informatii {
    // Partner counts per tip_partener in L/LS/V/C/A/AI/AS operations
    pub nr_cui1: i64, // tip_partener=1
    pub nr_cui2: i64, // tip_partener=2
    pub nr_cui3: i64, // tip_partener=3
    pub nr_cui4: i64, // tip_partener=4
    // Cash-register (all zero — no data)
    pub nr_bf_i1: i64,
    pub incasari_i1: i64,
    pub incasari_i2: i64,
    // Invoice counts
    pub nr_facturi_terti: i64,
    pub nr_facturi_benef: i64,
    pub nr_facturi: i64,
    pub nr_facturi_l_pf: i64,
    pub nr_facturi_ls_pf: i64,
    pub val_ls_pf: i64,
    // TVA colectată per rate (from L ops)
    pub tva_col24: Option<i64>,
    pub tva_col21: Option<i64>,
    pub tva_col11: Option<i64>,
    pub tva_col20: Option<i64>,
    pub tva_col19: Option<i64>,
    pub tva_col9: Option<i64>,
    pub tva_col5: Option<i64>,
    // TVA deductibilă per rate (from A ops) — optional in XSD
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
    pub rezumat1_list: Vec<Rezumat1>,
    pub op1_list: Vec<Op1>,
    /// Total plată A — control sum = nrCui1 + nrCui2 + nrCui3 + nrCui4
    /// (+ rezumat2 base sums, which we never emit so they are zero here).
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
    d.round_dp(0).to_i64().unwrap_or(0)
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

/// Determine the `tip_partener` code from a partner CUI string.
///
/// Rule (documented against XSD Int_tipPartenerOp1SType):
///   - stripped digits only, length 2–10  → 1 (VAT-registered)
///   - empty string                        → 3 (persoană fizică assumed)
///   - anything else                       → 2 (juridic, neînregistrat TVA)
fn tip_partener_from_cui(raw_cui: &str) -> i64 {
    let digits = strip_ro(raw_cui);
    if digits.is_empty() {
        return 3; // PF — no CUI
    }
    let all_digits = digits.chars().all(|c| c.is_ascii_digit());
    let len = digits.len();
    if all_digits && (2..=10).contains(&len) {
        1 // CUI valid RO → persoană juridică înregistrată TVA
    } else {
        2 // juridic dar neînregistrat (sau CUI non-numeric)
    }
}

/// Parse a normalized vat_rate string (integer-percent, e.g. "19") to a D394
/// `cota` integer in the enum {0, 5, 9, 11, 19, 20, 21, 24}.
///
/// If the parsed value is not in the enum, emits a warning and returns 0 so
/// that the XML remains schema-valid (does not panic).
fn parse_cota_from_rate(vat_rate: &str) -> i64 {
    let pct: i64 = vat_rate.trim().parse().unwrap_or(0);
    match pct {
        0 | 5 | 9 | 11 | 19 | 20 | 21 | 24 => pct,
        other => {
            tracing::warn!(
                "D394: vat_rate '{}' ({}) is not in the cota enum {{0,5,9,11,19,20,21,24}}; \
                 falling back to 0",
                vat_rate,
                other
            );
            0
        }
    }
}

/// Map a D394Partner (from livrări/vânzări) to (tip, cota, has_tva).
///
/// Returns (tip: &'static str, cota: i64, emit_tva: bool).
/// `cota` is taken directly from `partner.vat_rate` (normalized integer-percent
/// string), which is already per-rate thanks to the compute_d394 grouping fix.
fn map_sales_partner(partner: &D394Partner) -> (&'static str, i64 /* cota */, bool /* emit_tva */) {
    match partner.vat_category.as_str() {
        "S" | "SR" => {
            // Standard taxed — use the explicit per-line rate
            let cota = parse_cota_from_rate(&partner.vat_rate);
            ("L", cota, true)
        }
        "AE" => {
            // Taxare inversă livrare — tip=V, cota=0, no tva (per spec)
            ("V", 0, false)
        }
        // Scutite / zero-rate / intra-EU delivery / outside-scope
        "E" | "Z" | "K" | "O" | "G" => ("LS", 0, false),
        _ => ("LS", 0, false), // safe default: scutit
    }
}

/// Map a D394Partner (from achiziții/cumpărări) to (tip, cota, has_tva).
fn map_purchase_partner(
    partner: &D394Partner,
) -> (&'static str, i64 /* cota */, bool /* emit_tva */) {
    match partner.vat_category.as_str() {
        "S" | "SR" => {
            // Standard domestic purchase — tip=A
            let cota = parse_cota_from_rate(&partner.vat_rate);
            ("A", cota, true)
        }
        "AE" => {
            // Taxare inversă achiziție — tip=C, cota=0, no tva (liability on buyer)
            ("C", 0, false)
        }
        "K" => {
            // Achiziție intracomunitară — tip=AI, cota=rate, tva=deductible
            let cota = parse_cota_from_rate(&partner.vat_rate);
            ("AI", cota, true)
        }
        "E" | "Z" | "O" | "G" => ("AS", 0, false),
        _ => ("AS", 0, false),
    }
}

// ── Main builder ──────────────────────────────────────────────────────────────

/// Build the complete `D394Doc` from a `D394Report` + `D394Submission` + `Company`.
///
/// Returns `AppResult<D394Doc>` which the generator will serialize to XML.
pub fn build_sections(
    report: &D394Report,
    submission: &D394Submission,
    _company: &Company,
    period: NaiveDate,
) -> AppResult<D394Doc> {
    let luna = period.month() as i32;
    let an = period.year();

    // ── Build op1 list ────────────────────────────────────────────────────────

    let mut op1_list: Vec<Op1> = Vec::new();

    // Sales (livrări) → various tip types
    for partner in &report.partners {
        let (tip, cota, emit_tva) = map_sales_partner(partner);
        let cui_digits = strip_ro(&partner.partner_cui);
        let tip_p = tip_partener_from_cui(&partner.partner_cui);
        let baza = round_to_lei(parse_dec(&partner.base));
        let tva = if emit_tva {
            let v = round_to_lei(parse_dec(&partner.vat));
            opt_nonzero(v)
        } else {
            None
        };
        let den_p = partner.partner_name.chars().take(200).collect::<String>();
        let den_p = if den_p.trim().is_empty() {
            "NECUNOSCUT".to_string()
        } else {
            den_p
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
        });
    }

    // Purchases (achiziții) → A / C / AI / AS
    for partner in &report.purchase_partners {
        let (tip, cota, emit_tva) = map_purchase_partner(partner);
        let cui_digits = strip_ro(&partner.partner_cui);
        let tip_p = tip_partener_from_cui(&partner.partner_cui);
        let baza = round_to_lei(parse_dec(&partner.base));
        let tva = if emit_tva {
            let v = round_to_lei(parse_dec(&partner.vat));
            opt_nonzero(v)
        } else {
            None
        };
        let den_p = partner.partner_name.chars().take(200).collect::<String>();
        let den_p = if den_p.trim().is_empty() {
            "NECUNOSCUT".to_string()
        } else {
            den_p
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
        });
    }

    // ── Build rezumat1 list ───────────────────────────────────────────────────
    // One rezumat1 per distinct (tip_partener, cota) present in op1.

    // Accumulator keyed by (tip_partener, cota)
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

    let mut rezumat1_list: Vec<Rezumat1> = r1_map
        .into_iter()
        .map(|((tp, cota), acc)| Rezumat1 {
            tip_partener: tp,
            cota,
            facturi_l: opt_nonzero(acc.facturi_l),
            baza_l: opt_nonzero(acc.baza_l),
            tva_l: opt_nonzero(acc.tva_l),
            facturi_ls: opt_nonzero(acc.facturi_ls),
            baza_ls: opt_nonzero(acc.baza_ls),
            facturi_a: opt_nonzero(acc.facturi_a),
            baza_a: opt_nonzero(acc.baza_a),
            tva_a: opt_nonzero(acc.tva_a),
            facturi_ai: opt_nonzero(acc.facturi_ai),
            baza_ai: opt_nonzero(acc.baza_ai),
            tva_ai: opt_nonzero(acc.tva_ai),
            facturi_as: opt_nonzero(acc.facturi_as),
            baza_as: opt_nonzero(acc.baza_as),
            facturi_v: opt_nonzero(acc.facturi_v),
            baza_v: opt_nonzero(acc.baza_v),
            facturi_c: opt_nonzero(acc.facturi_c),
            baza_c: opt_nonzero(acc.baza_c),
            tva_c: opt_nonzero(acc.tva_c),
        })
        .collect();

    // Sort by (tip_partener, cota) for deterministic output
    rezumat1_list.sort_by_key(|r| (r.tip_partener, r.cota));

    // ── Build informatii ──────────────────────────────────────────────────────

    // nrCui counts: distinct partner CUIs by tip_partener in the SALES direction
    // (only L/LS/V ops contribute to nrCui per typical ANAF interpretation).
    // We count distinct cuiP per tip_partener across ALL op1 entries.
    let mut cuis_by_tp: BTreeMap<i64, BTreeSet<String>> = BTreeMap::new();
    for op in &op1_list {
        if !op.cui_p.is_empty() {
            cuis_by_tp
                .entry(op.tip_partener)
                .or_default()
                .insert(op.cui_p.clone());
        }
    }

    let nr_cui1 = cuis_by_tp.get(&1).map(|s| s.len() as i64).unwrap_or(0);
    let nr_cui2 = cuis_by_tp.get(&2).map(|s| s.len() as i64).unwrap_or(0);
    let nr_cui3 = cuis_by_tp.get(&3).map(|s| s.len() as i64).unwrap_or(0);
    let nr_cui4 = cuis_by_tp.get(&4).map(|s| s.len() as i64).unwrap_or(0);

    // nrFacturi = total invoices counted in this declaration
    // Use report's invoice_count (sales) as primary; purchase_invoice_count is separate
    let nr_facturi = report.invoice_count;

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

    // total_plata_a = nrCui1 + nrCui2 + nrCui3 + nrCui4  (control sum per spec).
    // This is NOT a money amount — it is a partner-count control sum.
    // The spec formula: totalPlata_A = nrCui1 + nrCui2 + nrCui3 + nrCui4
    //   + rezumat2(bazaL + bazaA + bazaAI) [all zero here — we emit no rezumat2].
    // We compute nr_cui* here directly (they are also stored in informatii below).
    let total_plata_a = nr_cui1 + nr_cui2 + nr_cui3 + nr_cui4;

    // DUK rule R135A.1/R143A.1: tvaCol*/tvaDed* attributes belong to the sistemTVA
    // (real-time VAT) regime. Emit them only when sistemTVA=true; omit entirely otherwise.
    let (tva_col24_v, tva_col21_v, tva_col11_v, tva_col20_v, tva_col19_v, tva_col9_v, tva_col5_v) =
        if submission.sistem_tva {
            (
                opt_nonzero(*tva_col.get(&24).unwrap_or(&0)),
                opt_nonzero(*tva_col.get(&21).unwrap_or(&0)),
                opt_nonzero(*tva_col.get(&11).unwrap_or(&0)),
                opt_nonzero(*tva_col.get(&20).unwrap_or(&0)),
                opt_nonzero(*tva_col.get(&19).unwrap_or(&0)),
                opt_nonzero(*tva_col.get(&9).unwrap_or(&0)),
                opt_nonzero(*tva_col.get(&5).unwrap_or(&0)),
            )
        } else {
            (None, None, None, None, None, None, None)
        };
    let (tva_ded24_v, tva_ded21_v, tva_ded11_v, tva_ded20_v, tva_ded19_v, tva_ded9_v, tva_ded5_v) =
        if submission.sistem_tva {
            (
                opt_nonzero(*tva_ded.get(&24).unwrap_or(&0)),
                opt_nonzero(*tva_ded.get(&21).unwrap_or(&0)),
                opt_nonzero(*tva_ded.get(&11).unwrap_or(&0)),
                opt_nonzero(*tva_ded.get(&20).unwrap_or(&0)),
                opt_nonzero(*tva_ded.get(&19).unwrap_or(&0)),
                opt_nonzero(*tva_ded.get(&9).unwrap_or(&0)),
                opt_nonzero(*tva_ded.get(&5).unwrap_or(&0)),
            )
        } else {
            (None, None, None, None, None, None, None)
        };

    let informatii = Informatii {
        nr_cui1,
        nr_cui2,
        nr_cui3,
        nr_cui4,
        nr_bf_i1: 0,
        incasari_i1: 0,
        incasari_i2: 0,
        nr_facturi_terti: 0,
        nr_facturi_benef: 0,
        nr_facturi,
        nr_facturi_l_pf: 0,
        nr_facturi_ls_pf: 0,
        val_ls_pf: 0,
        tva_col24: tva_col24_v,
        tva_col21: tva_col21_v,
        tva_col11: tva_col11_v,
        tva_col20: tva_col20_v,
        tva_col19: tva_col19_v,
        tva_col9: tva_col9_v,
        tva_col5: tva_col5_v,
        tva_ded24: tva_ded24_v,
        tva_ded21: tva_ded21_v,
        tva_ded11: tva_ded11_v,
        tva_ded20: tva_ded20_v,
        tva_ded19: tva_ded19_v,
        tva_ded9: tva_ded9_v,
        tva_ded5: tva_ded5_v,
        // ALL tvaDedAI* are REQUIRED → must be present, default 0
        tva_ded_ai24: *tva_ded_ai.get(&24).unwrap_or(&0),
        tva_ded_ai21: *tva_ded_ai.get(&21).unwrap_or(&0),
        tva_ded_ai11: *tva_ded_ai.get(&11).unwrap_or(&0),
        tva_ded_ai20: *tva_ded_ai.get(&20).unwrap_or(&0),
        tva_ded_ai19: *tva_ded_ai.get(&19).unwrap_or(&0),
        tva_ded_ai9: *tva_ded_ai.get(&9).unwrap_or(&0),
        tva_ded_ai5: *tva_ded_ai.get(&5).unwrap_or(&0),
        solicit: if submission.solicit { 1 } else { 0 },
        // DUK rule R191.1: efectuat must only be present when solicit=1.
        // Only emit efectuat when the submission is requesting a refund (solicit=true).
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
        rezumat1_list,
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
            cui: "RO12345678".to_string(),
            legal_name: "Test SRL".to_string(),
            trade_name: None,
            registry_number: None,
            vat_payer: true,
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
            ..Default::default()
        }
    }

    fn make_report(
        sales: Vec<(&str, &str, &str, i64, &str, &str)>, // (cui, category, vat_rate, count, base, vat)
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
            })
            .collect();

        D394Report {
            company_cui: "RO12345678".to_string(),
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

    #[test]
    fn sales_standard_maps_to_l_with_cota() {
        let report = make_report(
            vec![("12345678", "S", "19", 3, "1000.00", "190.00")],
            vec![],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert_eq!(doc.op1_list.len(), 1);
        let op = &doc.op1_list[0];
        assert_eq!(op.tip, "L");
        assert_eq!(op.cota, 19); // 190/1000 = 19%
        assert_eq!(op.baza, 1000);
        assert_eq!(op.tva, Some(190));
        assert_eq!(op.tip_partener, 1); // valid RO CUI
    }

    #[test]
    fn sales_ae_maps_to_v_cota0_no_tva() {
        let report = make_report(vec![("12345678", "AE", "0", 1, "500.00", "0.00")], vec![]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let op = &doc.op1_list[0];
        assert_eq!(op.tip, "V");
        assert_eq!(op.cota, 0);
        assert_eq!(op.tva, None);
    }

    #[test]
    fn sales_z_maps_to_ls_cota0() {
        let report = make_report(vec![("12345678", "Z", "0", 2, "2000.00", "0.00")], vec![]);
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
        let report = make_report(vec![], vec![("98765432", "S", "21", 2, "800.00", "168.00")]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert_eq!(doc.op1_list.len(), 1);
        let op = &doc.op1_list[0];
        assert_eq!(op.tip, "A");
        assert_eq!(op.cota, 21); // 168/800 = 21%
        assert_eq!(op.baza, 800);
        assert_eq!(op.tva, Some(168));
    }

    #[test]
    fn purchase_ae_maps_to_c_cota0() {
        let report = make_report(vec![], vec![("98765432", "AE", "0", 1, "300.00", "0.00")]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let op = &doc.op1_list[0];
        assert_eq!(op.tip, "C");
        assert_eq!(op.cota, 0);
        assert_eq!(op.tva, None);
    }

    #[test]
    fn purchase_k_maps_to_ai_with_cota() {
        let report = make_report(
            vec![],
            vec![("98765432", "K", "19", 1, "1000.00", "190.00")],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let op = &doc.op1_list[0];
        assert_eq!(op.tip, "AI");
        assert_eq!(op.cota, 19);
        assert_eq!(op.tva, Some(190));
    }

    #[test]
    fn rezumat1_sums_reconcile_op1() {
        // 2 sales partners with category=S at 19%, 1 purchase at 21%
        let report = make_report(
            vec![
                ("11111111", "S", "19", 2, "1000.00", "190.00"),
                ("22222222", "S", "19", 1, "500.00", "95.00"),
            ],
            vec![("33333333", "S", "21", 1, "800.00", "168.00")],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();

        // op1 should have 3 entries
        assert_eq!(doc.op1_list.len(), 3);

        // rezumat1 for (tip_partener=1, cota=19) should aggregate both L ops
        let r1 = doc
            .rezumat1_list
            .iter()
            .find(|r| r.tip_partener == 1 && r.cota == 19)
            .expect("should have rezumat1 for (1, 19)");
        assert_eq!(r1.facturi_l, Some(3)); // 2+1
        assert_eq!(r1.baza_l, Some(1500)); // 1000+500
        assert_eq!(r1.tva_l, Some(285)); // 190+95

        // rezumat1 for (tip_partener=1, cota=21) should aggregate A op
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
    fn informatii_tva_col_sums_l_operations() {
        let report = make_report(
            vec![
                ("11111111", "S", "19", 2, "1000.00", "190.00"), // 19%
                ("22222222", "S", "11", 1, "500.00", "55.00"),   // 11%
            ],
            vec![],
        );
        // sistemTVA=true so tvaCol*/tvaDed* are populated
        let mut sub = make_submission();
        sub.sistem_tva = true;
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert_eq!(doc.informatii.tva_col19, Some(190));
        assert_eq!(doc.informatii.tva_col11, Some(55));
        assert_eq!(doc.informatii.tva_col21, None); // no 21% sales
    }

    #[test]
    fn informatii_tva_ded_ai_always_present() {
        // Even with no AI ops, tvaDedAI* must be 0 (required attrs)
        let report = make_report(vec![], vec![] as Vec<(&str, &str, &str, i64, &str, &str)>);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        // These are i64 (required), not Option<i64>
        assert_eq!(doc.informatii.tva_ded_ai21, 0);
        assert_eq!(doc.informatii.tva_ded_ai19, 0);
        assert_eq!(doc.informatii.tva_ded_ai5, 0);
    }

    #[test]
    fn pf_partner_gets_tip_partener_3() {
        // Empty CUI → PF → tip_partener=3
        let report = make_report(vec![("", "S", "19", 1, "200.00", "38.00")], vec![]);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert_eq!(doc.op1_list[0].tip_partener, 3);
    }

    #[test]
    fn total_plata_a_is_count_sum() {
        // totalPlata_A = nrCui1 + nrCui2 + nrCui3 + nrCui4 (control sum per spec).
        // Two partners with valid RO CUIs (8 digits) → both tip_partener=1.
        // nrCui1=2 (distinct CUIs: "11111111" + "33333333"), nrCui2/3/4=0.
        // totalPlata_A = 2.
        let report = make_report(
            vec![("11111111", "S", "19", 2, "1000.00", "190.00")],
            vec![("33333333", "S", "19", 1, "800.00", "120.00")],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        // nrCui1=2, others=0 → totalPlata_A = 2
        assert_eq!(doc.informatii.nr_cui1, 2);
        assert_eq!(doc.total_plata_a, 2);
    }

    #[test]
    fn total_plata_a_zero_for_empty_report() {
        // Empty report: no partners → nrCui* = 0 → totalPlata_A = 0
        let report = make_report(vec![], vec![] as Vec<(&str, &str, &str, i64, &str, &str)>);
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert_eq!(doc.total_plata_a, 0);
    }

    /// Defect-1: mixed-rate partner (19% and 21% category-S sales) must produce
    /// TWO op1 lines with cota=19 and cota=21 — NOT one blended line with cota=20.
    #[test]
    fn sections_mixed_rate_partner_produces_two_op1_lines() {
        // Same CUI, same category "S", two different rates.
        let report = make_report(
            vec![
                ("11111111", "S", "19", 3, "1000.00", "190.00"), // 19% row
                ("11111111", "S", "21", 2, "500.00", "105.00"),  // 21% row — SAME partner
            ],
            vec![],
        );
        let sub = make_submission();
        let company = make_company();
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        assert_eq!(
            doc.op1_list.len(),
            2,
            "Two op1 lines expected (one per rate), not one blended line"
        );

        let cotas: Vec<i64> = doc.op1_list.iter().map(|op| op.cota).collect();
        assert!(cotas.contains(&19), "Must have cota=19 line");
        assert!(cotas.contains(&21), "Must have cota=21 line");
        assert!(!cotas.contains(&20), "Must NOT have blended cota=20 line");

        // Verify each op1 line has correct baza and tva
        let op19 = doc.op1_list.iter().find(|op| op.cota == 19).unwrap();
        assert_eq!(op19.baza, 1000);
        assert_eq!(op19.tva, Some(190));

        let op21 = doc.op1_list.iter().find(|op| op.cota == 21).unwrap();
        assert_eq!(op21.baza, 500);
        assert_eq!(op21.tva, Some(105));
    }
}
