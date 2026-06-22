//! D301 — Decont special de TVA (OPANAF 592/2016, model actualizat).
//!
//! **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK / XSD.**
//! Namespace-ul și versiunea schemei (`D301_SCHEMA_VERSION` / `D301_NAMESPACE`) sunt
//! marcate ca TODO-verify: acestea TREBUIE verificate față de pachetul oficial Soft J
//! (DUKIntegrator) și față de XSD-ul oficial ANAF înainte de depunerea electronică.
//! Fără XSD-ul oficial LOCAL nu se poate rula DUKIntegrator — testele de mai jos sunt
//! STRUCTURALE (XML bine-format, secțiuni + atribute prezente, sume corecte).
//!
//! ## Cine depune D301 și de ce diferă de D300?
//! D301 e depus de persoanele **NEÎNREGISTRATE** în scopuri de TVA conform art.316 Cod fiscal
//! (deci firmele înregistrate normal depun D300, nu D301). Categoriile de operațiuni:
//! - **Secțiunea 1**: Achiziții intracomunitare (AIC) de bunuri taxabile, ALTELE decât mijloace
//!   de transport noi sau produse accizabile (art.268 alin.(3) lit.c).
//! - **Secțiunea 2**: AIC de mijloace de transport noi (art.268 alin.(3) lit.b).
//! - **Secțiunea 3**: AIC de produse accizabile (art.268 alin.(3) lit.d).
//! - **Secțiunea 4**: Operațiuni cu taxare inversă pentru servicii primite de la nerezidenți
//!   (art.307 alin.(2)-(6)).
//!
//! Fiecare secțiune conține câte un rând `<rand>` cu `baza_impozabila` + `tva_datorata`.
//! Secțiunile fără operațiuni în perioadă NU se emit.
//! Termen: lunar, 25 a lunii următoare perioadei de raportare.
//!
//! ## IMPORTANT — Validare obligatorie înainte de depunere
//! Înainte de depunerea la ANAF, XML-ul generat TREBUIE validat cu DUKIntegrator împotriva
//! XSD-ului oficial. Obțineți XSD-ul din pachetul Soft J de pe site-ul ANAF (declaratii.anaf.ro).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::anaf_decl::round_lei;
use crate::anaf_decl::xml::{
    empty_elem_attrs, end_elem, finish, new_writer, pretty_print, start_elem_attrs, trunc,
};
use crate::error::{AppError, AppResult};

// ── Schema version — TODO: verify against official ANAF XSD + DUKIntegrator ──

/// Namespace D301.
/// **TODO-verify**: Confirmați versiunea exactă (vN) față de XSD-ul oficial din pachetul
/// Soft J publicat pe declaratii.anaf.ro. Versiunea v4 este o estimare structurală.
pub const D301_NAMESPACE: &str = "mfp:anaf:dgti:d301:declaratie:v4";

/// Elementul rădăcină al documentului D301.
pub const D301_ROOT: &str = "declaratie301";

/// Versiunea schemei D301, ca etichetă (pentru SchemaVersion). TODO-verify.
pub const D301_SCHEMA_VERSION: &str = "v4 (TODO-verify vs XSD oficial)";

// ── Model date ────────────────────────────────────────────────────────────────

/// Antetul declarației D301 (datele declarantului + perioada).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D301Header {
    /// CUI-ul declarantului (fără „RO", doar cifre).
    pub cui: String,
    /// Denumirea persoanei impozabile.
    pub den: String,
    /// Adresa completă.
    pub adresa: String,
    /// Luna perioadei de raportare (1-12).
    pub luna: u32,
    /// Anul perioadei de raportare.
    pub an: i32,
    /// 0 = declarație inițială, 1 = rectificativă.
    pub d_rec: u8,
    /// Numele declarantului.
    pub nume_declar: String,
    /// Prenumele declarantului.
    pub prenume_declar: String,
    /// Funcția declarantului.
    pub functie_declar: String,
}

/// Un rând de operațiuni D301 — baza impozabilă + TVA datorată.
/// Sumele sunt `Decimal` (2 zecimale); se rotunjesc la lei întregi la emitere.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct D301Row {
    /// Baza impozabilă (lei).
    pub baza_impozabila: Decimal,
    /// TVA datorată (lei).
    pub tva_datorata: Decimal,
}

impl D301Row {
    /// Returnează `true` dacă rândul are operațiuni de raportat (baza sau TVA > 0).
    pub fn has_data(&self) -> bool {
        !self.baza_impozabila.is_zero() || !self.tva_datorata.is_zero()
    }
}

/// Datele complete D301 pentru o perioadă.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct D301Data {
    /// Secțiunea 1: AIC bunuri taxabile (altele decât noi mijloace de transport + accizabile).
    pub sectiune1: Option<D301Row>,
    /// Secțiunea 2: AIC mijloace de transport noi.
    pub sectiune2: Option<D301Row>,
    /// Secțiunea 3: AIC produse accizabile.
    pub sectiune3: Option<D301Row>,
    /// Secțiunea 4: Servicii primite de la nerezidenți cu taxare inversă (art.307 alin.(2)-(6)).
    pub sectiune4: Option<D301Row>,
}

impl D301Data {
    /// Returnează `true` dacă există cel puțin o secțiune cu date de raportat.
    pub fn has_any_data(&self) -> bool {
        [
            &self.sectiune1,
            &self.sectiune2,
            &self.sectiune3,
            &self.sectiune4,
        ]
        .iter()
        .any(|s| s.as_ref().map(|r| r.has_data()).unwrap_or(false))
    }
}

// ── Emitorul XML ──────────────────────────────────────────────────────────────

/// Construiește XML-ul D301 (decont special de TVA) pentru perioada dată.
///
/// **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK/XSD.**
/// Verificați namespace-ul (`D301_NAMESPACE`) și structura față de XSD-ul oficial ANAF
/// înainte de depunerea electronică prin SPV.
///
/// # Erori
/// Returnează eroare dacă nu există nicio secțiune cu date de raportat.
pub fn build_d301_xml(header: &D301Header, data: &D301Data) -> AppResult<String> {
    if !data.has_any_data() {
        return Err(AppError::Validation(
            "D301: nu există operațiuni de raportat în nicio secțiune pentru perioada selectată."
                .into(),
        ));
    }

    // TVA totală de plată = suma TVA din toate secțiunile cu date.
    let total_tva: i64 = [
        &data.sectiune1,
        &data.sectiune2,
        &data.sectiune3,
        &data.sectiune4,
    ]
    .iter()
    .filter_map(|s| s.as_ref())
    .filter(|r| r.has_data())
    .map(|r| round_lei(r.tva_datorata))
    .sum();

    let luna_s = header.luna.to_string();
    let an_s = header.an.to_string();
    let d_rec_s = header.d_rec.to_string();
    let total_s = total_tva.to_string();
    let den = trunc(header.den.trim(), 200);
    let adresa = trunc(header.adresa.trim(), 200);
    let nume = trunc(header.nume_declar.trim(), 75);
    let prenume = trunc(header.prenume_declar.trim(), 75);
    let functie = trunc(header.functie_declar.trim(), 75);

    let mut w = new_writer()?;

    start_elem_attrs(
        &mut w,
        D301_ROOT,
        &[
            ("xmlns", D301_NAMESPACE),
            ("luna", &luna_s),
            ("an", &an_s),
            ("d_rec", &d_rec_s),
            ("cui", header.cui.trim()),
            ("den", &den),
            ("adresa", &adresa),
            ("nume_declar", &nume),
            ("prenume_declar", &prenume),
            ("functie_declar", &functie),
            ("totalPlata_A", &total_s),
        ],
    )?;

    // Emit only sections that have data (per spec: sections without operations are omitted).
    emit_section(&mut w, "sectiune1", &data.sectiune1)?;
    emit_section(&mut w, "sectiune2", &data.sectiune2)?;
    emit_section(&mut w, "sectiune3", &data.sectiune3)?;
    emit_section(&mut w, "sectiune4", &data.sectiune4)?;

    end_elem(&mut w, D301_ROOT)?;
    Ok(pretty_print(&finish(w)?))
}

/// Emite o secțiune `<sectiuneN baza_impozabila="…" tva_datorata="…"/>` dacă există date.
fn emit_section(
    w: &mut crate::anaf_decl::xml::XmlWriter,
    elem: &str,
    row: &Option<D301Row>,
) -> AppResult<()> {
    if let Some(r) = row {
        if r.has_data() {
            let baza = round_lei(r.baza_impozabila).to_string();
            let tva = round_lei(r.tva_datorata).to_string();
            empty_elem_attrs(
                w,
                elem,
                &[("baza_impozabila", &baza), ("tva_datorata", &tva)],
            )?;
        }
    }
    Ok(())
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
            cui: "12345674".into(),
            den: "Test SRL".into(),
            adresa: "Str. Test 1, București".into(),
            luna: 5,
            an: 2026,
            d_rec: 0,
            nume_declar: "Popescu".into(),
            prenume_declar: "Ion".into(),
            functie_declar: "Administrator".into(),
        }
    }

    /// Structural tests — NOT DUK/XSD validation (no official XSD bundled).
    /// These verify: well-formed XML, correct namespace, sections present when data is non-zero,
    /// amounts formatted as whole lei. DUK validation requires the official XSD from ANAF.

    #[test]
    fn empty_data_returns_error() {
        let result = build_d301_xml(&header(), &D301Data::default());
        assert!(result.is_err(), "empty D301Data should return an error");
    }

    #[test]
    fn sectiune1_and_sectiune4_present_sectiune2_3_absent() {
        // D301 per spec: only sections with data are emitted.
        let data = D301Data {
            sectiune1: Some(D301Row {
                baza_impozabila: d("50000"),
                tva_datorata: d("10000"),
            }),
            sectiune2: None,
            sectiune3: None,
            sectiune4: Some(D301Row {
                baza_impozabila: d("20000"),
                tva_datorata: d("4000"),
            }),
        };
        let xml = build_d301_xml(&header(), &data).unwrap();

        // Root + namespace
        assert!(
            xml.contains(&format!(r#"xmlns="{D301_NAMESPACE}""#)),
            "namespace mismatch: {xml}"
        );
        assert!(
            xml.contains(&format!("<{D301_ROOT}")),
            "root element missing: {xml}"
        );
        assert!(
            xml.contains(r#"luna="5""#) && xml.contains(r#"an="2026""#),
            "period attributes missing: {xml}"
        );
        assert!(xml.contains(r#"cui="12345674""#), "cui missing: {xml}");
        assert!(xml.contains(r#"d_rec="0""#), "d_rec missing: {xml}");

        // Secțiunea 1: baza=50000, tva=10000
        assert!(
            xml.contains("<sectiune1 ") || xml.contains("<sectiune1\n"),
            "sectiune1 missing: {xml}"
        );
        assert!(
            xml.contains(r#"baza_impozabila="50000""#),
            "baza s1 wrong: {xml}"
        );

        // Secțiunea 4: present (reverse charge services)
        assert!(
            xml.contains("<sectiune4 ") || xml.contains("<sectiune4\n"),
            "sectiune4 missing: {xml}"
        );
        assert!(
            xml.contains(r#"baza_impozabila="20000""#),
            "baza s4 wrong: {xml}"
        );

        // Secțiunile 2 și 3: absente (nu au date)
        assert!(
            !xml.contains("<sectiune2"),
            "sectiune2 should be absent: {xml}"
        );
        assert!(
            !xml.contains("<sectiune3"),
            "sectiune3 should be absent: {xml}"
        );

        // totalPlata_A = 10000 + 4000 = 14000
        assert!(
            xml.contains(r#"totalPlata_A="14000""#),
            "totalPlata_A wrong: {xml}"
        );

        // XML bine-format: are declaratie XML + rădăcină deschisă + rădăcină închisă
        assert!(
            xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
            "XML prolog missing: {xml}"
        );
        assert!(
            xml.contains(&format!("</{D301_ROOT}>")),
            "root close tag missing: {xml}"
        );
    }

    #[test]
    fn amounts_rounded_to_whole_lei() {
        // Sume cu zecimale — se rotunjesc la lei întregi (comercial).
        let data = D301Data {
            sectiune1: Some(D301Row {
                baza_impozabila: d("999.50"), // → 1000 (commercial round)
                tva_datorata: d("199.49"),    // → 199
            }),
            ..D301Data::default()
        };
        let xml = build_d301_xml(&header(), &data).unwrap();
        assert!(
            xml.contains(r#"baza_impozabila="1000""#),
            "rounding baza: {xml}"
        );
        assert!(xml.contains(r#"tva_datorata="199""#), "rounding tva: {xml}");
        assert!(xml.contains(r#"totalPlata_A="199""#), "totalPlata_A: {xml}");
    }

    #[test]
    fn all_four_sections_emitted_when_all_have_data() {
        let data = D301Data {
            sectiune1: Some(D301Row {
                baza_impozabila: d("1000"),
                tva_datorata: d("190"),
            }),
            sectiune2: Some(D301Row {
                baza_impozabila: d("2000"),
                tva_datorata: d("380"),
            }),
            sectiune3: Some(D301Row {
                baza_impozabila: d("3000"),
                tva_datorata: d("570"),
            }),
            sectiune4: Some(D301Row {
                baza_impozabila: d("4000"),
                tva_datorata: d("760"),
            }),
        };
        let xml = build_d301_xml(&header(), &data).unwrap();
        assert!(xml.contains("<sectiune1"), "s1 missing: {xml}");
        assert!(xml.contains("<sectiune2"), "s2 missing: {xml}");
        assert!(xml.contains("<sectiune3"), "s3 missing: {xml}");
        assert!(xml.contains("<sectiune4"), "s4 missing: {xml}");
        // totalPlata_A = 190+380+570+760 = 1900
        assert!(xml.contains(r#"totalPlata_A="1900""#), "total wrong: {xml}");
    }

    #[test]
    fn rectificativa_flag_emitted_correctly() {
        let data = D301Data {
            sectiune1: Some(D301Row {
                baza_impozabila: d("1000"),
                tva_datorata: d("190"),
            }),
            ..D301Data::default()
        };
        let mut hdr = header();
        hdr.d_rec = 1;
        let xml = build_d301_xml(&hdr, &data).unwrap();
        assert!(xml.contains(r#"d_rec="1""#), "d_rec rectificativă: {xml}");
    }
}
