//! D301 — Decont special de TVA (OPANAF 592/2016, model actualizat).
//!
//! **XSD-VALIDAT via `xmllint --schema tools/anaf/d301.xsd`** (official ANAF XSD,
//! targetNamespace `mfp:anaf:dgti:d301:declaratie:v1`, version 1.02).
//! Structura, atributele obligatorii, enumerările și tipurile sunt exacte față de XSD.
//! Validarea completă a regulilor de business necesită rularea
//! `D301Validator.jar` (din pachetul `D301_20201022.zip` de pe declaratii.anaf.ro,
//! prin DUKIntegrator) înainte de depunerea electronică prin SPV.
//!
//! ## Cine depune D301 și de ce diferă de D300?
//! D301 e depus de persoanele **NEÎNREGISTRATE** în scopuri de TVA conform art.316 Cod
//! fiscal (firmele înregistrate normal depun D300, nu D301). Categoriile de operațiuni
//! (`tip_operatie`):
//! - **1**: Achiziții intracomunitare (AIC) de bunuri taxabile, ALTELE decât mijloace
//!   de transport noi sau produse accizabile (art.268 alin.(3) lit.c). Categorie UBL: K
//!   cu `intra_eu_kind = "goods"`.
//! - **2**: AIC de mijloace de transport noi (art.268 alin.(3) lit.b). Necesită flag
//!   explicit `new_vehicle = true` — modelul curent nu capturează acest flag; rândurile
//!   de tip 2 pot fi adăugate manual.
//! - **3**: AIC de produse accizabile (art.268 alin.(3) lit.d). Necesită flag explicit
//!   `excisable = true` — rândurile de tip 3 pot fi adăugate manual.
//! - **4**: Servicii intracomunitare primite (beneficiar obligat la plata TVA, art.307
//!   alin.(2)) — prestator UE. Categorie UBL: K cu `intra_eu_kind = "services"`.
//! - **5**: Alte operațiuni (taxare inversă art.307 alin.(3),(5),(6), prestatoare non-UE
//!   / nerezidenți). Categorie UBL: AE.
//!
//! ## Structura XML (per d301.xsd v1.02)
//! ```text
//!   <declaratie301 xmlns="mfp:anaf:dgti:d301:declaratie:v1"
//!                  luna="N" an="AAAA" d_rec="0|1" temei="1|2"
//!                  mijl_trans="0|1"   ← 1 dacă există sectiune tip_operatie=2
//!                  cif="…" denumire="…" adresa="…"
//!                  telefon="…" fax="…" email="…" banca="…" cont="…"
//!                  pers_inreg="1|2"   ← 1=neînregistrat art.316; 2=înregistrat art.317
//!                  nr_evid="N"        ← INTEGER ≥ 0 (IntStr23SType)
//!                  baza1="…" tva1="…" baza2="…" tva2="…"
//!                  baza3="…" tva3="…" baza4="…" tva4="…" baza5="…" tva5="…"
//!                  totalPlata_A="N"   ← suma TVA totale (întreg lei)
//!                  nume_declarant="…" prenume_declarant="…" functia_declarant="…">
//!     <sectiune tip_operatie="1|2|3|4|5"
//!               nr_doc="…(max 20 chr)" data_doc="ZZ.LL.AAAA"
//!               val_valuta="N15.2" tip_valuta="RON|EUR|…"
//!               curs_valutar="N15.4" baza="N15.2" tva="N15.2"/>
//!     …
//!   </declaratie301>
//! ```

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::anaf_decl::xml::{
    empty_elem_attrs, end_elem, finish, new_writer, pretty_print, start_elem_attrs, trunc,
};
use crate::error::{AppError, AppResult};

// ── Schema constants ──────────────────────────────────────────────────────────

/// Namespace D301 — versiunea oficială v1 (per d301.xsd, targetNamespace).
pub const D301_NAMESPACE: &str = "mfp:anaf:dgti:d301:declaratie:v1";

/// Elementul rădăcină al documentului D301.
pub const D301_ROOT: &str = "declaratie301";

// ── Model date ────────────────────────────────────────────────────────────────

/// Antetul declarației D301 (datele declarantului + perioada).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D301Header {
    /// CUI-ul declarantului (fără „RO", doar cifre). Atribut `cif` în XML.
    pub cif: String,
    /// Denumirea persoanei impozabile. Atribut `denumire` în XML (max 200 chr).
    pub denumire: String,
    /// Adresa completă (opțional, max 1000 chr).
    pub adresa: String,
    /// Telefon (opțional, max 15 chr).
    pub telefon: String,
    /// Fax (opțional, max 15 chr).
    pub fax: String,
    /// E-mail (opțional, max 200 chr).
    pub email: String,
    /// Banca declarantului (max 50 chr).
    pub banca: String,
    /// Contul bancar (IBAN) al declarantului (max 50 chr).
    pub cont: String,
    /// Statutul TVA: 1 = neînregistrat art.316 (tipic D301), 2 = înregistrat art.317.
    pub pers_inreg: u8,
    /// Numărul de evidență în ROI (IntStr23SType — integer ≥ 0; 0 dacă lipsește).
    pub nr_evid: u64,
    /// Luna perioadei de raportare (1-12).
    pub luna: u32,
    /// Anul perioadei de raportare (≥ 2013).
    pub an: i32,
    /// 0 = declarație inițială, 1 = rectificativă.
    pub d_rec: u8,
    /// Temeiul legal: 1 = declarație normală, 2 = corectivă (IntInt1_2SType).
    pub temei: u8,
    /// Numele declarantului (semnatar, max 75 chr).
    pub nume_declarant: String,
    /// Prenumele declarantului (max 75 chr).
    pub prenume_declarant: String,
    /// Funcția declarantului (max 50 chr).
    pub functia_declarant: String,
}

/// Un rând din D301 (`<sectiune>`), corespunzând unui document sursă.
///
/// Sumele sunt `Decimal` cu 2 zecimale (N15.2); se formatează cu 2 zecimale la emitere.
/// Cursul valutar are 4 zecimale (N15.4); pentru RON nativ se folosește `1.0000`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct D301Sectiune {
    /// tip_operatie ∈ {1, 2, 3, 4, 5} — vezi doc modul.
    pub tip_operatie: u8,
    /// Numărul documentului (max 20 chr conform Str20 din XSD).
    pub nr_doc: String,
    /// Data documentului (ZZ.LL.AAAA — formatul ANAF).
    pub data_doc: String,
    /// Valoarea în valuta originală a tranzacției (N15.2).
    pub val_valuta: Decimal,
    /// Codul ISO 4217 al valutei (3 litere, ex. "RON", "EUR").
    pub tip_valuta: String,
    /// Cursul de schimb față de RON (N15.4). 1.0000 pentru RON nativ.
    pub curs_valutar: Decimal,
    /// Baza impozabilă în lei (N15.2).
    pub baza: Decimal,
    /// TVA datorată în lei (N15.2).
    pub tva: Decimal,
}

/// Datele complete D301 pentru o perioadă: lista de rânduri `<sectiune>`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct D301Data {
    /// Rândurile de operațiuni (0..n `<sectiune>`).
    pub sectiuni: Vec<D301Sectiune>,
}

impl D301Data {
    /// Returnează `true` dacă există cel puțin un rând cu date de raportat.
    pub fn has_any_data(&self) -> bool {
        !self.sectiuni.is_empty()
    }
}

// ── Helpers de formatare ──────────────────────────────────────────────────────

/// Formatează un `Decimal` ca N15.2 (2 zecimale fixe, rotunjire comercială).
fn fmt_n15_2(d: Decimal) -> String {
    format!(
        "{:.2}",
        d.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
    )
}

/// Formatează un `Decimal` ca N15.4 (4 zecimale fixe, pentru curs_valutar).
fn fmt_n15_4(d: Decimal) -> String {
    format!(
        "{:.4}",
        d.round_dp_with_strategy(4, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
    )
}

/// Agregat (baza, tva) pentru un tip_operatie, din lista de sectiuni.
fn totals_for(sectiuni: &[D301Sectiune], tip: u8) -> (Decimal, Decimal) {
    sectiuni
        .iter()
        .filter(|s| s.tip_operatie == tip)
        .fold((Decimal::ZERO, Decimal::ZERO), |(b, t), s| {
            (b + s.baza, t + s.tva)
        })
}

// ── Emitorul XML ──────────────────────────────────────────────────────────────

/// Construiește XML-ul D301 (decont special de TVA) pentru perioada dată.
///
/// Structura este **XSD-validată** față de `tools/anaf/d301.xsd` (ANAF oficial, v1.02).
/// Atributele obligatorii per XSD: `luna`, `an`, `d_rec`, `temei`, `mijl_trans`, `cif`,
/// `denumire`, `banca`, `cont`, `pers_inreg`, `nr_evid`, `baza1..5`, `tva1..5`,
/// `totalPlata_A`, `nume_declarant`, `prenume_declarant`, `functia_declarant`.
/// `<sectiune>` necesită și `curs_valutar` (DblPoz15_4SType, required per XSD).
///
/// Validarea completă a regulilor de business necesită `D301Validator.jar` din pachetul
/// `D301_20201022.zip` de pe declaratii.anaf.ro, rulat prin DUKIntegrator.
///
/// # Erori
/// Returnează eroare dacă nu există niciun rând de raportat.
pub fn build_d301_xml(header: &D301Header, data: &D301Data) -> AppResult<String> {
    if !data.has_any_data() {
        return Err(AppError::Validation(
            "D301: nu există operațiuni de raportat în nicio secțiune pentru perioada selectată."
                .into(),
        ));
    }

    // ── Totalizatoare per tip_operatie (baza1..5 / tva1..5) ──────────────────
    let (baza1, tva1) = totals_for(&data.sectiuni, 1);
    let (baza2, tva2) = totals_for(&data.sectiuni, 2);
    let (baza3, tva3) = totals_for(&data.sectiuni, 3);
    let (baza4, tva4) = totals_for(&data.sectiuni, 4);
    let (baza5, tva5) = totals_for(&data.sectiuni, 5);

    // totalPlata_A = suma TVA totale din toate secțiunile (IntNeg17SType — întreg lei).
    let total_tva = tva1 + tva2 + tva3 + tva4 + tva5;
    let total_plata_a = total_tva
        .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        .to_string();

    // mijl_trans = 1 dacă există rânduri cu tip_operatie=2 (mijloace transport noi).
    let mijl_trans: u8 = if data.sectiuni.iter().any(|s| s.tip_operatie == 2) {
        1
    } else {
        0
    };

    let luna_s = header.luna.to_string();
    let an_s = header.an.to_string();
    let d_rec_s = header.d_rec.to_string();
    let mijl_s = mijl_trans.to_string();
    let pers_s = header.pers_inreg.to_string();
    let temei_s = header.temei.to_string();
    let nr_evid_s = header.nr_evid.to_string();

    // Truncate per XSD field lengths.
    let denumire = trunc(header.denumire.trim(), 200);
    let adresa = trunc(header.adresa.trim(), 1000);
    let telefon = trunc(header.telefon.trim(), 15);
    let fax = trunc(header.fax.trim(), 15);
    let email = trunc(header.email.trim(), 200);
    let banca = trunc(header.banca.trim(), 50);
    let cont = trunc(header.cont.trim(), 50);
    let nume = trunc(header.nume_declarant.trim(), 75);
    let prenume = trunc(header.prenume_declarant.trim(), 75);
    let functia = trunc(header.functia_declarant.trim(), 50);

    // N15.2 strings for totals.
    let baza1_s = fmt_n15_2(baza1);
    let tva1_s = fmt_n15_2(tva1);
    let baza2_s = fmt_n15_2(baza2);
    let tva2_s = fmt_n15_2(tva2);
    let baza3_s = fmt_n15_2(baza3);
    let tva3_s = fmt_n15_2(tva3);
    let baza4_s = fmt_n15_2(baza4);
    let tva4_s = fmt_n15_2(tva4);
    let baza5_s = fmt_n15_2(baza5);
    let tva5_s = fmt_n15_2(tva5);

    let mut w = new_writer()?;

    start_elem_attrs(
        &mut w,
        D301_ROOT,
        &[
            ("xmlns", D301_NAMESPACE),
            ("luna", &luna_s),
            ("an", &an_s),
            ("d_rec", &d_rec_s),
            ("mijl_trans", &mijl_s),
            ("temei", &temei_s),
            ("cif", header.cif.trim()),
            ("denumire", &denumire),
            ("adresa", &adresa),
            ("telefon", &telefon),
            ("fax", &fax),
            ("email", &email),
            ("banca", &banca),
            ("cont", &cont),
            ("pers_inreg", &pers_s),
            ("nr_evid", &nr_evid_s),
            ("baza1", &baza1_s),
            ("tva1", &tva1_s),
            ("baza2", &baza2_s),
            ("tva2", &tva2_s),
            ("baza3", &baza3_s),
            ("tva3", &tva3_s),
            ("baza4", &baza4_s),
            ("tva4", &tva4_s),
            ("baza5", &baza5_s),
            ("tva5", &tva5_s),
            ("totalPlata_A", &total_plata_a),
            ("nume_declarant", &nume),
            ("prenume_declarant", &prenume),
            ("functia_declarant", &functia),
        ],
    )?;

    // Emit rows.
    for s in &data.sectiuni {
        let tip_s = s.tip_operatie.to_string();
        let val_s = fmt_n15_2(s.val_valuta);
        let baza_s = fmt_n15_2(s.baza);
        let tva_s = fmt_n15_2(s.tva);
        let curs_s = fmt_n15_4(s.curs_valutar);
        let tip_val = trunc(s.tip_valuta.trim().to_uppercase().as_str(), 3);
        let nr_doc = trunc(s.nr_doc.trim(), 20);
        empty_elem_attrs(
            &mut w,
            "sectiune",
            &[
                ("tip_operatie", &tip_s),
                ("nr_doc", &nr_doc),
                ("data_doc", s.data_doc.trim()),
                ("val_valuta", &val_s),
                ("tip_valuta", &tip_val),
                ("curs_valutar", &curs_s),
                ("baza", &baza_s),
                ("tva", &tva_s),
            ],
        )?;
    }

    end_elem(&mut w, D301_ROOT)?;
    Ok(pretty_print(&finish(w)?))
}

// ── Auto-agregare din date contabile ─────────────────────────────────────────

/// Un rând brut extras din `received_invoice_vat_lines` JOIN `received_invoices`
/// (folosit intern de `aggregate_d301`).
#[derive(Debug)]
struct RawVatLine {
    /// Nr. document (numărul facturii furnizorului).
    nr_doc: String,
    /// Data documentului ISO (YYYY-MM-DD) — convertit în ZZ.LL.AAAA la emitere.
    data_doc_iso: String,
    /// Baza impozabilă din linia TVA (TEXT în DB).
    base_amount: String,
    /// TVA din linia TVA (TEXT în DB).
    vat_amount: String,
    /// Valuta facturii.
    currency: String,
    /// Cursul de schimb față de RON (None = RON nativ → 1.0000).
    exchange_rate: Option<f64>,
    /// Categoria TVA UBL: "K" sau "AE".
    vat_category: String,
    /// Tipul achiziției intra-UE: "goods" sau "services" (relevant pentru K).
    intra_eu_kind: String,
}

/// Convertește o dată ISO (YYYY-MM-DD) în formatul ANAF (ZZ.LL.AAAA).
fn iso_to_anaf_date(iso: &str) -> String {
    let parts: Vec<&str> = iso.split('-').collect();
    if parts.len() == 3 {
        format!("{}.{}.{}", parts[2], parts[1], parts[0])
    } else {
        iso.to_string()
    }
}

/// Parsează un șir Decimal sau returnează zero.
fn parse_dec(s: &str) -> Decimal {
    s.trim().parse::<Decimal>().unwrap_or(Decimal::ZERO)
}

/// Convertește o sumă în valuta dată la RON (cu cursul de schimb).
fn to_ron(amount: Decimal, currency: &str, fx: Option<f64>) -> Decimal {
    if currency.eq_ignore_ascii_case("RON") {
        return amount;
    }
    let rate = fx
        .and_then(Decimal::from_f64_retain)
        .unwrap_or(Decimal::ONE);
    if rate.is_zero() {
        amount
    } else {
        amount * rate
    }
}

/// Clasifică un rând TVA în `tip_operatie` D301:
/// - K + goods → 1 (AIC bunuri)
/// - K + services → 4 (servicii intracomunitare art.307(2), beneficiar obligat la TVA)
/// - AE → 5 (alte operațiuni — taxare inversă art.307 alin.(3),(5),(6))
/// - Altele → None (nu intră în D301 — sare)
fn classify_tip(vat_category: &str, intra_eu_kind: &str) -> Option<u8> {
    match vat_category {
        "K" => {
            if intra_eu_kind == "services" {
                Some(4) // servicii intracomunitare (beneficiar obligat, art.150/307(2))
            } else {
                Some(1) // AIC bunuri (goods sau default)
            }
        }
        "AE" => Some(5), // alte operațiuni reverse-charge (non-UE / art.307(3)(5)(6))
        _ => None,
    }
}

/// Agregă rânduri D301 din `received_invoice_vat_lines` pentru o companie și perioadă.
///
/// Condiții pentru includere:
/// - `ri.company_id = company_id`
/// - `ri.issue_date` în `[period_from, period_to]` (format YYYY-MM-DD)
/// - `ri.status != 'REJECTED'`
/// - `vl.vat_category IN ('K', 'AE')`
///
/// Clasificare (conformă cu d301.xsd enumeration {1,2,3,4,5}):
/// - `K` + `intra_eu_kind = "goods"` → tip_operatie 1 (AIC bunuri)
/// - `K` + `intra_eu_kind = "services"` → tip_operatie 4 (servicii intracomunitare
///   beneficiar obligat la TVA, art.150/307(2))
/// - `AE` → tip_operatie 5 (alte operațiuni taxare inversă)
///
/// **Limitare**: tipurile 2 (mijloace transport noi) și 3 (produse accizabile) necesită
/// un flag explicit absent din modelul curent — rândurile respective trebuie adăugate manual.
///
/// Dacă `vat_payer = true` (art.316), compania depune D300, nu D301 → returnează eroare.
pub async fn aggregate_d301(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
    vat_payer: bool,
) -> AppResult<Vec<D301Sectiune>> {
    if vat_payer {
        return Err(AppError::Validation(
            "D301: compania este înregistrată în scopuri de TVA (art.316) și depune D300, nu D301."
                .into(),
        ));
    }

    let rows = sqlx::query(
        "SELECT ri.number   AS nr_doc, \
                ri.issue_date AS data_doc_iso, \
                vl.base_amount, \
                vl.vat_amount, \
                COALESCE(ri.currency, 'RON')           AS currency, \
                ri.exchange_rate, \
                vl.vat_category, \
                COALESCE(ri.intra_eu_kind, 'goods')    AS intra_eu_kind \
         FROM received_invoice_vat_lines vl \
         JOIN received_invoices ri ON ri.id = vl.received_invoice_id \
         WHERE ri.company_id  = ?1 \
           AND ri.issue_date >= ?2 \
           AND ri.issue_date <= ?3 \
           AND ri.status     != 'REJECTED' \
           AND vl.vat_category IN ('K', 'AE') \
         ORDER BY ri.issue_date, ri.number",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    use sqlx::Row;
    let raw: Vec<RawVatLine> = rows
        .iter()
        .map(|r| RawVatLine {
            nr_doc: r
                .try_get::<Option<String>, _>("nr_doc")
                .unwrap_or(None)
                .unwrap_or_default(),
            data_doc_iso: r.try_get("data_doc_iso").unwrap_or_default(),
            base_amount: r.try_get("base_amount").unwrap_or_default(),
            vat_amount: r.try_get("vat_amount").unwrap_or_default(),
            currency: r.try_get("currency").unwrap_or_else(|_| "RON".into()),
            exchange_rate: r.try_get("exchange_rate").unwrap_or(None),
            vat_category: r.try_get("vat_category").unwrap_or_default(),
            intra_eu_kind: r
                .try_get("intra_eu_kind")
                .unwrap_or_else(|_| "goods".into()),
        })
        .collect();

    let mut sectiuni = Vec::with_capacity(raw.len());
    for line in raw {
        let Some(tip) = classify_tip(&line.vat_category, &line.intra_eu_kind) else {
            continue;
        };
        let base_dec = parse_dec(&line.base_amount);
        let vat_dec = parse_dec(&line.vat_amount);
        let baza_ron = to_ron(base_dec, &line.currency, line.exchange_rate);
        let tva_ron = to_ron(vat_dec, &line.currency, line.exchange_rate);

        // val_valuta = valoarea în valuta originală (baza + tva în valuta documentului).
        let val_valuta = base_dec + vat_dec;

        // curs_valutar: cursul de schimb față de RON (1.0000 pentru RON nativ).
        let curs_valutar = if line.currency.eq_ignore_ascii_case("RON") {
            Decimal::ONE
        } else {
            line.exchange_rate
                .and_then(Decimal::from_f64_retain)
                .unwrap_or(Decimal::ONE)
        };

        sectiuni.push(D301Sectiune {
            tip_operatie: tip,
            nr_doc: line.nr_doc,
            data_doc: iso_to_anaf_date(&line.data_doc_iso),
            val_valuta,
            tip_valuta: if line.currency.is_empty() {
                "RON".into()
            } else {
                line.currency.to_uppercase()
            },
            curs_valutar,
            baza: baza_ron,
            tva: tva_ron,
        });
    }
    Ok(sectiuni)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn header() -> D301Header {
        D301Header {
            cif: "12345674".into(),
            denumire: "Test SRL".into(),
            adresa: "Str. Test 1, București".into(),
            telefon: "0721000000".into(),
            fax: "".into(),
            email: "test@test.ro".into(),
            banca: "Banca Test".into(),
            cont: "RO49AAAA1B31007593840000".into(),
            pers_inreg: 1,
            nr_evid: 0,
            luna: 5,
            an: 2026,
            d_rec: 0,
            temei: 1,
            nume_declarant: "Popescu".into(),
            prenume_declarant: "Ion".into(),
            functia_declarant: "Administrator".into(),
        }
    }

    #[test]
    fn empty_data_returns_error() {
        let result = build_d301_xml(&header(), &D301Data::default());
        assert!(result.is_err(), "empty D301Data should return an error");
    }

    #[test]
    fn namespace_is_v1_and_root_is_declaratie301() {
        let data = D301Data {
            sectiuni: vec![D301Sectiune {
                tip_operatie: 1,
                nr_doc: "FAC001".into(),
                data_doc: "15.05.2026".into(),
                val_valuta: d("1190.00"),
                tip_valuta: "EUR".into(),
                curs_valutar: d("5.0200"),
                baza: d("1000.00"),
                tva: d("190.00"),
            }],
        };
        let xml = build_d301_xml(&header(), &data).unwrap();
        assert!(
            xml.contains(&format!(r#"xmlns="{D301_NAMESPACE}""#)),
            "namespace must be v1: {xml}"
        );
        assert!(
            xml.contains(&format!("<{D301_ROOT} ")),
            "root must be declaratie301: {xml}"
        );
    }

    #[test]
    fn root_attributes_present() {
        let data = D301Data {
            sectiuni: vec![D301Sectiune {
                tip_operatie: 1,
                nr_doc: "FAC001".into(),
                data_doc: "15.05.2026".into(),
                val_valuta: d("1000.00"),
                tip_valuta: "RON".into(),
                curs_valutar: d("1.0000"),
                baza: d("1000.00"),
                tva: d("190.00"),
            }],
        };
        let xml = build_d301_xml(&header(), &data).unwrap();

        // Required root attributes per d301.xsd v1.02.
        assert!(xml.contains(r#"luna="5""#), "luna missing: {xml}");
        assert!(xml.contains(r#"an="2026""#), "an missing: {xml}");
        assert!(xml.contains(r#"d_rec="0""#), "d_rec missing: {xml}");
        assert!(xml.contains(r#"temei="1""#), "temei missing: {xml}");
        assert!(xml.contains(r#"cif="12345674""#), "cif missing: {xml}");
        assert!(
            xml.contains(r#"pers_inreg="1""#),
            "pers_inreg missing: {xml}"
        );
        assert!(
            xml.contains(r#"mijl_trans="0""#),
            "mijl_trans missing: {xml}"
        );
        assert!(
            xml.contains(r#"denumire="Test SRL""#),
            "denumire missing: {xml}"
        );
        assert!(xml.contains(r#"nr_evid="0""#), "nr_evid missing: {xml}");
        assert!(xml.contains(r#"baza1="1000.00""#), "baza1 missing: {xml}");
        assert!(xml.contains(r#"tva1="190.00""#), "tva1 missing: {xml}");
        assert!(
            xml.contains(r#"baza5="0.00""#),
            "baza5 (zero) missing: {xml}"
        );
        assert!(xml.contains(r#"tva5="0.00""#), "tva5 (zero) missing: {xml}");
        assert!(
            xml.contains(r#"totalPlata_A="190""#),
            "totalPlata_A missing: {xml}"
        );
        assert!(
            xml.contains(r#"nume_declarant="Popescu""#),
            "nume_declarant missing: {xml}"
        );
        assert!(
            xml.contains(r#"prenume_declarant="Ion""#),
            "prenume_declarant missing: {xml}"
        );
        assert!(
            xml.contains(r#"functia_declarant="Administrator""#),
            "functia_declarant missing: {xml}"
        );
    }

    #[test]
    fn sectiune_row_attributes_correct() {
        // tip 1 (AIC goods) + tip 4 (EU intra-community service → art.307(2))
        let data = D301Data {
            sectiuni: vec![
                D301Sectiune {
                    tip_operatie: 1,
                    nr_doc: "FAC001".into(),
                    data_doc: "15.05.2026".into(),
                    val_valuta: d("1000.00"),
                    tip_valuta: "RON".into(),
                    curs_valutar: d("1.0000"),
                    baza: d("1000.00"),
                    tva: d("0.00"),
                },
                D301Sectiune {
                    tip_operatie: 4,
                    nr_doc: "SRV001".into(),
                    data_doc: "20.05.2026".into(),
                    val_valuta: d("500.00"),
                    tip_valuta: "EUR".into(),
                    curs_valutar: d("5.0100"),
                    baza: d("500.00"),
                    tva: d("95.00"),
                },
            ],
        };
        let xml = build_d301_xml(&header(), &data).unwrap();

        // Rows.
        assert!(
            xml.contains(r#"tip_operatie="1""#),
            "tip_operatie=1 missing: {xml}"
        );
        assert!(
            xml.contains(r#"tip_operatie="4""#),
            "tip_operatie=4 missing: {xml}"
        );
        assert!(xml.contains(r#"nr_doc="FAC001""#), "nr_doc missing: {xml}");
        assert!(
            xml.contains(r#"data_doc="15.05.2026""#),
            "data_doc missing: {xml}"
        );
        assert!(
            xml.contains(r#"tip_valuta="RON""#),
            "tip_valuta RON missing: {xml}"
        );
        assert!(
            xml.contains(r#"tip_valuta="EUR""#),
            "tip_valuta EUR missing: {xml}"
        );
        assert!(
            xml.contains(r#"curs_valutar="1.0000""#),
            "curs_valutar RON missing: {xml}"
        );
        assert!(
            xml.contains(r#"curs_valutar="5.0100""#),
            "curs_valutar EUR missing: {xml}"
        );

        // Totals: baza1=1000.00 tva1=0.00 ; baza4=500.00 tva4=95.00.
        assert!(
            xml.contains(r#"baza1="1000.00""#),
            "baza1 total wrong: {xml}"
        );
        assert!(xml.contains(r#"tva1="0.00""#), "tva1 total wrong: {xml}");
        assert!(
            xml.contains(r#"baza4="500.00""#),
            "baza4 total wrong: {xml}"
        );
        assert!(xml.contains(r#"tva4="95.00""#), "tva4 total wrong: {xml}");

        // mijl_trans=0 (no tip 2 row).
        assert!(
            xml.contains(r#"mijl_trans="0""#),
            "mijl_trans should be 0: {xml}"
        );
    }

    #[test]
    fn mijl_trans_set_when_tip2_present() {
        let data = D301Data {
            sectiuni: vec![D301Sectiune {
                tip_operatie: 2,
                nr_doc: "MT001".into(),
                data_doc: "01.05.2026".into(),
                val_valuta: d("50000.00"),
                tip_valuta: "EUR".into(),
                curs_valutar: d("5.0200"),
                baza: d("50000.00"),
                tva: d("9500.00"),
            }],
        };
        let xml = build_d301_xml(&header(), &data).unwrap();
        assert!(
            xml.contains(r#"mijl_trans="1""#),
            "mijl_trans must be 1 when tip_operatie=2 exists: {xml}"
        );
        assert!(
            xml.contains(r#"baza2="50000.00""#),
            "baza2 total wrong: {xml}"
        );
        assert!(xml.contains(r#"tva2="9500.00""#), "tva2 total wrong: {xml}");
        assert!(
            xml.contains(r#"totalPlata_A="9500""#),
            "totalPlata_A: {xml}"
        );
    }

    #[test]
    fn amounts_n15_2_format() {
        // Amounts must be formatted as N15.2 (2 decimal places), not whole lei.
        let data = D301Data {
            sectiuni: vec![D301Sectiune {
                tip_operatie: 1,
                nr_doc: "FAC999".into(),
                data_doc: "01.05.2026".into(),
                val_valuta: d("999.505"), // → 999.51 (round half-up)
                tip_valuta: "RON".into(),
                curs_valutar: d("1.0000"),
                baza: d("999.505"),
                tva: d("199.491"), // → 199.49
            }],
        };
        let xml = build_d301_xml(&header(), &data).unwrap();
        // Row-level amounts (sectiune attrs).
        assert!(
            xml.contains(r#"baza="999.51""#),
            "N15.2 rounding baza: {xml}"
        );
        assert!(xml.contains(r#"tva="199.49""#), "N15.2 rounding tva: {xml}");
        // Total in root attrs.
        assert!(
            xml.contains(r#"baza1="999.51""#),
            "N15.2 baza1 total: {xml}"
        );
        assert!(xml.contains(r#"tva1="199.49""#), "N15.2 tva1 total: {xml}");
    }

    #[test]
    fn rectificativa_flag_emitted_correctly() {
        let data = D301Data {
            sectiuni: vec![D301Sectiune {
                tip_operatie: 1,
                nr_doc: "FAC001".into(),
                data_doc: "01.05.2026".into(),
                val_valuta: d("1000.00"),
                tip_valuta: "RON".into(),
                curs_valutar: d("1.0000"),
                baza: d("1000.00"),
                tva: d("0.00"),
            }],
        };
        let mut hdr = header();
        hdr.d_rec = 1;
        let xml = build_d301_xml(&hdr, &data).unwrap();
        assert!(xml.contains(r#"d_rec="1""#), "d_rec rectificativă: {xml}");
    }

    #[test]
    fn multiple_sections_totals_aggregated_correctly() {
        // 2 × tip 1 rows (should sum into baza1/tva1) + 1 × tip 4.
        let data = D301Data {
            sectiuni: vec![
                D301Sectiune {
                    tip_operatie: 1,
                    nr_doc: "F1".into(),
                    data_doc: "01.05.2026".into(),
                    val_valuta: d("200.00"),
                    tip_valuta: "RON".into(),
                    curs_valutar: d("1.0000"),
                    baza: d("200.00"),
                    tva: d("0.00"),
                },
                D301Sectiune {
                    tip_operatie: 1,
                    nr_doc: "F2".into(),
                    data_doc: "10.05.2026".into(),
                    val_valuta: d("300.00"),
                    tip_valuta: "RON".into(),
                    curs_valutar: d("1.0000"),
                    baza: d("300.00"),
                    tva: d("0.00"),
                },
                D301Sectiune {
                    tip_operatie: 4,
                    nr_doc: "S1".into(),
                    data_doc: "15.05.2026".into(),
                    val_valuta: d("100.00"),
                    tip_valuta: "EUR".into(),
                    curs_valutar: d("5.0000"),
                    baza: d("100.00"),
                    tva: d("19.00"),
                },
            ],
        };
        let xml = build_d301_xml(&header(), &data).unwrap();
        // baza1 = 200 + 300 = 500.
        assert!(xml.contains(r#"baza1="500.00""#), "baza1 aggregate: {xml}");
        assert!(xml.contains(r#"tva1="0.00""#), "tva1 aggregate: {xml}");
        // baza4 = 100, tva4 = 19.
        assert!(xml.contains(r#"baza4="100.00""#), "baza4 aggregate: {xml}");
        assert!(xml.contains(r#"tva4="19.00""#), "tva4 aggregate: {xml}");
        // totalPlata_A = 0 + 19 = 19 lei.
        assert!(xml.contains(r#"totalPlata_A="19""#), "totalPlata_A: {xml}");
    }

    // ── Auto-agregare (DB) ──────────────────────────────────────────────────────

    async fn test_pool() -> sqlx::SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        // Company: not VAT payer (vat_payer = 0).
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co','RO12345674','TestSRL','Str 1','Buc','IF','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_received_with_vat(
        pool: &sqlx::SqlitePool,
        company_id: &str,
        inv_id: &str,
        number: &str,
        issue_date: &str,
        currency: &str,
        intra_eu_kind: &str,
        vat_category: &str,
        base_amount: &str,
        vat_amount: &str,
    ) {
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, \
              total_amount, currency, issue_date, xml_path, status, intra_eu_kind, \
              number, downloaded_at, created_at) \
             VALUES (?1, ?2, ?3, 'RO999', 'Furnizor EU', '1000.00', ?4, ?5, '/x.xml', \
                     'NEW', ?6, ?7, 1, 1)",
        )
        .bind(inv_id)
        .bind(company_id)
        .bind(inv_id) // anaf_download_id = inv_id for uniqueness
        .bind(currency)
        .bind(issue_date)
        .bind(intra_eu_kind)
        .bind(number)
        .execute(pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO received_invoice_vat_lines \
             (id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount) \
             VALUES (?1, ?2, '19', ?3, ?4, ?5)",
        )
        .bind(format!("{inv_id}-line"))
        .bind(inv_id)
        .bind(vat_category)
        .bind(base_amount)
        .bind(vat_amount)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn aggregate_art316_company_returns_error() {
        let pool = test_pool().await;
        let err = aggregate_d301(&pool, "co", "2026-05-01", "2026-05-31", true).await;
        assert!(err.is_err(), "art.316 company should return error for D301");
        assert!(
            err.unwrap_err().to_string().contains("D300"),
            "error should mention D300"
        );
    }

    #[tokio::test]
    async fn aggregate_ic_goods_maps_to_tip1_and_eu_service_maps_to_tip4() {
        let pool = test_pool().await;

        // Invoice 1: IC goods (K + goods) → tip 1, baza 1000.
        seed_received_with_vat(
            &pool,
            "co",
            "inv1",
            "FAC001",
            "2026-05-10",
            "RON",
            "goods",
            "K",
            "1000.00",
            "0.00",
        )
        .await;

        // Invoice 2: EU intra-community service (K + services) → tip 4 (art.150/307(2)).
        seed_received_with_vat(
            &pool,
            "co",
            "inv2",
            "SRV001",
            "2026-05-15",
            "RON",
            "services",
            "K",
            "500.00",
            "95.00",
        )
        .await;

        let rows = aggregate_d301(&pool, "co", "2026-05-01", "2026-05-31", false)
            .await
            .unwrap();

        assert_eq!(rows.len(), 2, "expected 2 rows, got {}", rows.len());
        let tip1: Vec<_> = rows.iter().filter(|r| r.tip_operatie == 1).collect();
        let tip4: Vec<_> = rows.iter().filter(|r| r.tip_operatie == 4).collect();

        assert_eq!(tip1.len(), 1, "expected 1 tip-1 row");
        assert_eq!(
            tip4.len(),
            1,
            "expected 1 tip-4 row (EU intra-community service)"
        );

        let baza1_total: Decimal = tip1.iter().map(|r| r.baza).sum();
        let baza4_total: Decimal = tip4.iter().map(|r| r.baza).sum();
        assert_eq!(baza1_total, d("1000.00"), "tip-1 baza total");
        assert_eq!(baza4_total, d("500.00"), "tip-4 baza total");
    }

    #[tokio::test]
    async fn aggregate_ae_category_maps_to_tip5() {
        let pool = test_pool().await;

        // Invoice: AE reverse-charge (non-EU / art.307(3)(5)(6)) → tip 5.
        seed_received_with_vat(
            &pool,
            "co",
            "inv3",
            "SERV-NON-EU",
            "2026-05-20",
            "USD",
            "goods",
            "AE",
            "800.00",
            "152.00",
        )
        .await;

        let rows = aggregate_d301(&pool, "co", "2026-05-01", "2026-05-31", false)
            .await
            .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].tip_operatie, 5,
            "AE must map to tip_operatie=5 (alte operațiuni)"
        );
        assert_eq!(rows[0].baza, d("800.00"), "baza for AE row");
        assert_eq!(rows[0].tva, d("152.00"), "tva for AE row");
    }

    #[tokio::test]
    async fn aggregate_rejected_invoice_excluded() {
        let pool = test_pool().await;

        // Insert a REJECTED invoice — should NOT appear in D301.
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, \
              total_amount, currency, issue_date, xml_path, status, intra_eu_kind, \
              number, downloaded_at, created_at) \
             VALUES ('inv-rej', 'co', 'inv-rej', 'RO1', 'X', '100.00', 'RON', '2026-05-01', \
                     '/x.xml', 'REJECTED', 'goods', 'FAC-REJ', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO received_invoice_vat_lines \
             (id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount) \
             VALUES ('inv-rej-line', 'inv-rej', '19', 'K', '100.00', '0.00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let rows = aggregate_d301(&pool, "co", "2026-05-01", "2026-05-31", false)
            .await
            .unwrap();

        assert!(rows.is_empty(), "REJECTED invoice must not appear in D301");
    }

    #[tokio::test]
    async fn aggregate_outside_period_excluded() {
        let pool = test_pool().await;

        // Invoice outside the requested period.
        seed_received_with_vat(
            &pool,
            "co",
            "inv-old",
            "FAC-OLD",
            "2026-04-30",
            "RON",
            "goods",
            "K",
            "2000.00",
            "0.00",
        )
        .await;

        let rows = aggregate_d301(&pool, "co", "2026-05-01", "2026-05-31", false)
            .await
            .unwrap();

        assert!(
            rows.is_empty(),
            "invoice outside period must be excluded: {rows:?}"
        );
    }

    #[test]
    fn iso_to_anaf_date_conversion() {
        assert_eq!(iso_to_anaf_date("2026-05-15"), "15.05.2026");
        assert_eq!(iso_to_anaf_date("2026-01-01"), "01.01.2026");
        // Edge: malformed → returned as-is.
        assert_eq!(iso_to_anaf_date("bad"), "bad");
    }

    #[test]
    fn classify_tip_all_variants() {
        assert_eq!(classify_tip("K", "goods"), Some(1));
        assert_eq!(classify_tip("K", ""), Some(1)); // default goods
        assert_eq!(classify_tip("K", "services"), Some(4)); // intra-community service → tip 4
        assert_eq!(classify_tip("AE", "goods"), Some(5)); // alte operațiuni → tip 5
        assert_eq!(classify_tip("AE", "services"), Some(5));
        assert_eq!(classify_tip("S", "goods"), None);
        assert_eq!(classify_tip("Z", "goods"), None);
    }
}
