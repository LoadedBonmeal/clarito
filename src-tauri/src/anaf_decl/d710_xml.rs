//! D710 — Declarație rectificativă (OPANAF 587/2016, modificat prin OPANAF 779/2024).
//!
//! **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK / XSD.**
//! Namespace-ul și versiunea schemei (`D710_NAMESPACE`) sunt marcate ca TODO-verify:
//! acestea TREBUIE verificate față de pachetul oficial Soft J (DUKIntegrator) și față de
//! XSD-ul oficial ANAF înainte de depunerea electronică prin SPV.
//!
//! ## Ce corectează D710 și ce NU?
//! D710 rectifică EXCLUSIV obligațiile din formularul D100 (autoimpunere și reținere la sursă):
//! impozit pe profit, impozit nerezidenți, impozit dividende, accize, impozit pe construcții,
//! contribuții ale angajatorilor din vectorul D100. NU rectifică D112 (are D112 propriu), NU
//! rectifică D300 (are D300 propriu).
//!
//! ## Reguli structurale
//! - **Sumele sunt SUMELE CORECTE** (totalul corect, nu diferența față de declarația anterioară).
//! - Mai multe obligații **pentru aceeași perioadă** → mai multe `<tabel>` în același D710.
//! - Obligații cu **perioade diferite** → formulare D710 separate (câte un fișier XML per perioadă).
//! - Codul obligației (`cod_oblig`) provine din Nomenclatorul D100 (Anexa formularului D100).
//!
//! ## Nomenclator D100 (coduri frecvente — completați după Nomenclatorul oficial)
//! - `2` = Impozit pe profit (plăți anticipate, persoane juridice române)
//! - `5` = Impozit pe veniturile microîntreprinderilor
//! - `17` = Impozit pe dividende (reținere la sursă, rezidenți)
//! - `22` = Impozit pe veniturile nerezidenților (reținere la sursă)
//! - `37` = Impozit pe construcții
//!   (consultați Anexa formularului D100 publicat de ANAF pentru lista completă)
//!
//! ## IMPORTANT — Validare obligatorie înainte de depunere
//! Înainte de depunerea la ANAF, XML-ul generat TREBUIE validat cu DUKIntegrator împotriva
//! XSD-ului oficial. Obțineți XSD-ul din pachetul Soft J de pe site-ul ANAF (declaratii.anaf.ro).
//! Namespace-ul D710 poate fi partajat cu D100 (TODO-verify).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::anaf_decl::round_lei;
use crate::anaf_decl::xml::{
    end_elem, finish, new_writer, pretty_print, start_elem, start_elem_attrs, write_text_elem,
};
use crate::error::{AppError, AppResult};

// ── Schema version — TODO: verify against official ANAF XSD + DUKIntegrator ──

/// Namespace D710. Poate fi partajat cu D100 (ambele vizează obligațiile D100 vector).
/// **TODO-verify**: Confirmați versiunea exactă (vN) față de XSD-ul oficial din pachetul
/// Soft J publicat pe declaratii.anaf.ro (OPANAF 587/2016 + 779/2024).
pub const D710_NAMESPACE: &str = "mfp:anaf:dgti:d710:declaratie:v1";

/// Elementul rădăcină al documentului D710.
pub const D710_ROOT: &str = "declaratie710";

// ── Model date ────────────────────────────────────────────────────────────────

/// Antetul declarației D710.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D710Header {
    /// CUI-ul declarantului (fără „RO", doar cifre).
    pub cui: String,
    /// Denumirea persoanei impozabile.
    pub den: String,
    /// Adresa completă.
    pub adresa: String,
    /// Trimestrul de raportare rectificat (1-4). D710 rectifică O SINGURĂ perioadă per formular.
    pub quarter: u32,
    /// Anul de raportare rectificat.
    pub year: i32,
    /// 0 = declarație inițială de rectificare, 1 = re-rectificativă.
    pub d_rec: u8,
    /// Numele declarantului.
    pub nume_declar: String,
    /// Prenumele declarantului.
    pub prenume_declar: String,
    /// Funcția declarantului.
    pub functie_declar: String,
}

/// O obligație rectificată (un rând `<tabel>` în D710).
/// Suma este CORECTUL integral (nu diferența).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D710Obligation {
    /// Codul obligației din Nomenclatorul D100 (Anexa formularului D100, ex. "2", "5", "17").
    pub cod_oblig: String,
    /// Denumirea scurtă a obligației (pentru claritate, nu intră în XML ca element separat —
    /// XML-ul D710 identifică obligația prin `cod_oblig` conform nomenclatorului oficial).
    pub den_oblig: String,
    /// Suma corectă (totală), în lei. Se rotunjește la lei întregi la emitere.
    pub suma_corecta: Decimal,
}

/// Datele complete ale declarației D710 pentru O perioadă.
/// Perioade diferite → obiecte D710Input separate → fișiere XML separate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D710Input {
    /// Antet cu datele declarantului și perioada rectificată.
    pub header: D710Header,
    /// Lista obligațiilor rectificate (minimum una). Obligații cu cod diferit → rânduri separate.
    pub obligations: Vec<D710Obligation>,
}

// ── Emitorul XML ──────────────────────────────────────────────────────────────

/// Construiește XML-ul D710 (declarație rectificativă obligații D100) pentru perioada dată.
///
/// **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK/XSD.**
/// Verificați namespace-ul (`D710_NAMESPACE`) și structura față de XSD-ul oficial ANAF
/// (OPANAF 587/2016 + 779/2024) înainte de depunerea electronică prin SPV.
///
/// Fiecare obligație din `input.obligations` devine un element `<tabel>` separat în XML,
/// cu `cod_oblig` și `suma_corecta` (sumele corecte TOTALE, nu diferențele).
///
/// # Erori
/// Returnează eroare dacă lista de obligații e goală sau trimestrul e invalid (1-4).
pub fn build_d710_xml(input: &D710Input) -> AppResult<String> {
    if input.obligations.is_empty() {
        return Err(AppError::Validation(
            "D710: lista de obligații rectificate este goală. \
             Adăugați cel puțin o obligație (cod_oblig + suma_corecta)."
                .into(),
        ));
    }
    let hdr = &input.header;
    if hdr.quarter == 0 || hdr.quarter > 4 {
        return Err(AppError::Validation(format!(
            "D710: trimestrul {} este invalid — trebuie să fie 1-4.",
            hdr.quarter
        )));
    }

    // Scadența din tabel / perioadă: 25 a lunii următoare trimestrului.
    let luna_scadenta = match hdr.quarter {
        1 => "04",
        2 => "07",
        3 => "10",
        _ => "01",
    };
    let an_scadenta = if hdr.quarter == 4 {
        hdr.year + 1
    } else {
        hdr.year
    };
    let scadenta = format!("25.{luna_scadenta}.{an_scadenta}");

    // Luna de raportare = ultima lună a trimestrului (Q1→3, Q2→6, Q3→9, Q4→12).
    let luna_raportare = (hdr.quarter * 3).to_string();
    let an_s = hdr.year.to_string();
    let d_rec_s = hdr.d_rec.to_string();

    let den = crate::anaf_decl::xml::trunc(hdr.den.trim(), 200);
    let adresa = crate::anaf_decl::xml::trunc(hdr.adresa.trim(), 200);
    let nume = crate::anaf_decl::xml::trunc(hdr.nume_declar.trim(), 75);
    let prenume = crate::anaf_decl::xml::trunc(hdr.prenume_declar.trim(), 75);
    let functie = crate::anaf_decl::xml::trunc(hdr.functie_declar.trim(), 75);

    let mut w = new_writer()?;

    start_elem_attrs(
        &mut w,
        D710_ROOT,
        &[
            ("xmlns", D710_NAMESPACE),
            ("luna", &luna_raportare),
            ("an", &an_s),
            ("d_rec", &d_rec_s),
            ("cui", hdr.cui.trim()),
            ("den", &den),
            ("adresa", &adresa),
            ("nume_declar", &nume),
            ("prenume_declar", &prenume),
            ("functie_declar", &functie),
        ],
    )?;

    // Un `<tabel>` per obligație rectificată — mai multe obligații aceeași perioadă =
    // mai multe `<tabel>` siblings în același D710 (per specificație OPANAF 587/2016).
    for oblig in &input.obligations {
        let suma = round_lei(oblig.suma_corecta).to_string();
        start_elem(&mut w, "tabel")?;
        write_text_elem(&mut w, "cod_oblig", oblig.cod_oblig.trim())?;
        write_text_elem(&mut w, "suma_corecta", &suma)?;
        write_text_elem(&mut w, "scadenta", &scadenta)?;
        end_elem(&mut w, "tabel")?;
    }

    end_elem(&mut w, D710_ROOT)?;
    Ok(pretty_print(&finish(w)?))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn header(quarter: u32, year: i32) -> D710Header {
        D710Header {
            cui: "12345674".into(),
            den: "Test SRL".into(),
            adresa: "Str. Test 1, București".into(),
            quarter,
            year,
            d_rec: 0,
            nume_declar: "Popescu".into(),
            prenume_declar: "Ion".into(),
            functie_declar: "Administrator".into(),
        }
    }

    fn oblig(cod: &str, suma: &str, den: &str) -> D710Obligation {
        D710Obligation {
            cod_oblig: cod.into(),
            den_oblig: den.into(),
            suma_corecta: d(suma),
        }
    }

    /// Structural tests — NOT DUK/XSD validation (no official XSD bundled).
    /// Verifies: well-formed XML, correct namespace, one <tabel> per obligation,
    /// correct replacement amounts (not differences), scadenta derived from quarter.

    #[test]
    fn empty_obligations_returns_error() {
        let input = D710Input {
            header: header(1, 2026),
            obligations: vec![],
        };
        assert!(
            build_d710_xml(&input).is_err(),
            "empty obligations should fail"
        );
    }

    #[test]
    fn invalid_quarter_returns_error() {
        let input = D710Input {
            header: header(5, 2026), // invalid: 5 > 4
            obligations: vec![oblig("2", "10000", "Impozit profit")],
        };
        assert!(build_d710_xml(&input).is_err(), "quarter=5 should fail");
    }

    #[test]
    fn two_obligations_same_period_produce_two_tabele() {
        // D710 per spec: mai multe obligații aceeași perioadă → mai multe <tabel> siblings.
        let input = D710Input {
            header: header(2, 2026),
            obligations: vec![
                oblig("5", "2000", "Impozit micro"),
                oblig("17", "1600", "Impozit dividende"),
            ],
        };
        let xml = build_d710_xml(&input).unwrap();

        // Root + namespace
        assert!(
            xml.contains(&format!(r#"xmlns="{D710_NAMESPACE}""#)),
            "namespace missing: {xml}"
        );
        assert!(
            xml.contains(&format!("<{D710_ROOT}")),
            "root missing: {xml}"
        );
        assert!(xml.contains(r#"cui="12345674""#), "cui: {xml}");
        // Trimestrul 2 → luna 6 (ultima lună a trimestrului)
        assert!(xml.contains(r#"luna="6""#), "luna pentru Q2: {xml}");
        assert!(xml.contains(r#"an="2026""#), "an: {xml}");

        // Două elemente <tabel>
        assert_eq!(
            xml.matches("<tabel>").count(),
            2,
            "expected 2 <tabel> elements: {xml}"
        );
        assert_eq!(
            xml.matches("</tabel>").count(),
            2,
            "expected 2 </tabel> close tags: {xml}"
        );

        // Sumele corecte (REPLACEMENT, nu diferențe)
        assert!(
            xml.contains("<suma_corecta>2000</suma_corecta>"),
            "suma micro: {xml}"
        );
        assert!(
            xml.contains("<suma_corecta>1600</suma_corecta>"),
            "suma dividende: {xml}"
        );

        // Codurile obligațiilor
        assert!(xml.contains("<cod_oblig>5</cod_oblig>"), "cod micro: {xml}");
        assert!(
            xml.contains("<cod_oblig>17</cod_oblig>"),
            "cod dividende: {xml}"
        );

        // Scadența pentru Q2 → 25.07.2026
        assert!(
            xml.contains("<scadenta>25.07.2026</scadenta>"),
            "scadenta Q2: {xml}"
        );

        // XML bine-format
        assert!(
            xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
            "prolog: {xml}"
        );
        assert!(
            xml.contains(&format!("</{D710_ROOT}>")),
            "root close: {xml}"
        );
    }

    #[test]
    fn amounts_rounded_to_whole_lei() {
        // Sumele cu zecimale se rotunjesc la lei întregi (comercial: 0.5 → 1).
        let input = D710Input {
            header: header(1, 2026),
            obligations: vec![oblig("2", "9999.50", "Impozit profit")],
        };
        let xml = build_d710_xml(&input).unwrap();
        assert!(
            xml.contains("<suma_corecta>10000</suma_corecta>"),
            "rounding: {xml}"
        );
    }

    #[test]
    fn quarter4_scadenta_next_year() {
        // Q4 → luna 12, scadenta 25.01 anul următor.
        let input = D710Input {
            header: header(4, 2026),
            obligations: vec![oblig("5", "5000", "Impozit micro")],
        };
        let xml = build_d710_xml(&input).unwrap();
        assert!(xml.contains(r#"luna="12""#), "luna Q4: {xml}");
        assert!(
            xml.contains("<scadenta>25.01.2027</scadenta>"),
            "scadenta Q4: {xml}"
        );
    }

    #[test]
    fn rectificativa_flag_emitted() {
        let mut hdr = header(3, 2026);
        hdr.d_rec = 1;
        let input = D710Input {
            header: hdr,
            obligations: vec![oblig("22", "3000", "Impozit nerezidenți")],
        };
        let xml = build_d710_xml(&input).unwrap();
        assert!(xml.contains(r#"d_rec="1""#), "d_rec=1: {xml}");
    }
}
