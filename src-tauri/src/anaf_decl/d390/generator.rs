//! D390 XML generator — emits `<declaratie390>` with `<rezumat>` + `<operatie>` rows.
//!
//! Uses `quick_xml::Writer` + `BytesStart::push_attribute` (raw, pre-escaped via `xml_attr`),
//! the same pattern as the D394 generator.

use std::io::Cursor;

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};
use quick_xml::Writer;

use super::{D390Doc, D390Submission};
use crate::db::companies::Company;
use crate::error::{AppError, AppResult};

// Namespace `:v3` CONFIRMED correct per the official ANAF structura D390 (OPANAF 705/11.03.2020,
// `structura_D390_2020_180320.pdf`): `declaratie:v3` din perioada 02/2020 (`:v2`=2017, `:v1`=<2017),
// iar `bazaR` (regim agricultori) face parte din structura 2020. NOTĂ: `d390.xsd` de la calea publică
// `.../AplicatiiDec/d390.xsd` e schema VECHE `:v1` (fără `bazaR`) — de aceea nu există o poartă
// xmllint pentru D390 (XSD-ul `:v3` nu e publicat standalone, e în pachetul Soft A/DUK al ANAF; D390
// nu are nici validator DUK dedicat). Verificarea = testul structural de mai jos.
const NAMESPACE: &str = "mfp:anaf:dgti:d390:declaratie:v3";

fn map_err(e: quick_xml::Error) -> AppError {
    AppError::Other(format!("XML write error: {e}"))
}

/// Strip the "RO" prefix from a CUI (digits-only form required by the cui field).
fn strip_ro(cui: &str) -> String {
    let s = cui.trim();
    let s = if s.to_uppercase().starts_with("RO") {
        &s[2..]
    } else {
        s
    };
    s.trim().to_string()
}

/// Generate the D390 XML string from the aggregated document, submission metadata + company.
pub fn generate_d390_xml(
    doc: &D390Doc,
    submission: &D390Submission,
    company: &Company,
) -> AppResult<String> {
    let mut w = Writer::new_with_indent(Cursor::new(Vec::<u8>::new()), b' ', 2);
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(map_err)?;

    // ── Per-code totals + control sum ────────────────────────────────────────
    let total_for = |code: &str| -> i64 {
        doc.operations
            .iter()
            .filter(|o| o.tip == code)
            .map(|o| o.baza)
            .sum()
    };
    let baza_l = total_for("L");
    let baza_t = total_for("T");
    let baza_a = total_for("A");
    let baza_p = total_for("P");
    let baza_s = total_for("S");
    let baza_r = total_for("R");
    let total_baza: i64 = doc.operations.iter().map(|o| o.baza).sum();
    let nr_opi = doc.operations.len() as i64;
    // totalPlata_A = nrOPI + Σ all per-code bases (sumă de control).
    let total_plata_a = nr_opi + baza_l + baza_t + baza_a + baza_p + baza_s + baza_r;

    // ── Root <declaratie390 …> ───────────────────────────────────────────────
    let mut root = BytesStart::new("declaratie390");
    root.push_attribute(("xmlns", NAMESPACE));
    root.push_attribute(("luna", doc.luna.to_string().as_str()));
    root.push_attribute(("an", doc.an.to_string().as_str()));
    root.push_attribute(("d_rec", if submission.d_rec { "1" } else { "0" }));
    root.push_attribute((
        "nume_declar",
        field_or(&submission.nume_declar, &company.legal_name, 75).as_str(),
    ));
    root.push_attribute((
        "prenume_declar",
        // Mandatory (DA) — default to "-" for a legal-entity declarant with no separate first
        // name, so the field is never emitted empty (ANAF rejects an empty prenume_declar).
        field_or(&submission.prenume_declar, "-", 75).as_str(),
    ));
    root.push_attribute((
        "functie_declar",
        field_or(&submission.functie_declar, "Administrator", 50).as_str(),
    ));
    root.push_attribute(("cui", strip_ro(&company.cui).as_str()));
    root.push_attribute(("den", take(&company.legal_name, 200).as_str()));
    let adresa = {
        let parts: Vec<&str> = [
            company.address.as_str(),
            company.city.as_str(),
            company.county.as_str(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
        take(&parts.join(", "), 1000)
    };
    root.push_attribute(("adresa", adresa.as_str()));
    if let Some(ref phone) = company.phone {
        root.push_attribute(("telefon", take(phone, 15).as_str()));
    }
    if let Some(ref email) = company.email {
        root.push_attribute(("mail", take(email, 200).as_str()));
    }
    root.push_attribute(("totalPlata_A", total_plata_a.to_string().as_str()));
    w.write_event(Event::Start(root)).map_err(map_err)?;

    // ── <rezumat …/> ─────────────────────────────────────────────────────────
    let mut rez = BytesStart::new("rezumat");
    rez.push_attribute(("nr_pag", "1"));
    rez.push_attribute(("nrOPI", nr_opi.to_string().as_str()));
    rez.push_attribute(("bazaL", baza_l.to_string().as_str()));
    rez.push_attribute(("bazaT", baza_t.to_string().as_str()));
    rez.push_attribute(("bazaA", baza_a.to_string().as_str()));
    rez.push_attribute(("bazaP", baza_p.to_string().as_str()));
    rez.push_attribute(("bazaS", baza_s.to_string().as_str()));
    rez.push_attribute(("bazaR", baza_r.to_string().as_str()));
    rez.push_attribute(("total_baza", total_baza.to_string().as_str()));
    w.write_event(Event::Empty(rez)).map_err(map_err)?;

    // ── <operatie …/> rows ───────────────────────────────────────────────────
    for op in &doc.operations {
        let mut e = BytesStart::new("operatie");
        e.push_attribute(("tip", op.tip.as_str()));
        e.push_attribute(("tara", op.tara.as_str()));
        e.push_attribute(("codO", op.cod_o.as_str()));
        e.push_attribute(("denO", take(&op.den_o, 200).as_str()));
        e.push_attribute(("baza", op.baza.to_string().as_str()));
        w.write_event(Event::Empty(e)).map_err(map_err)?;
    }

    w.write_event(Event::End(BytesEnd::new("declaratie390")))
        .map_err(map_err)?;

    let bytes = w.into_inner().into_inner();
    String::from_utf8(bytes).map_err(|e| AppError::Other(format!("UTF-8: {e}")))
}

/// First non-empty of `value`/`fallback`, truncated to `max` chars.
fn field_or(value: &str, fallback: &str, max: usize) -> String {
    let v = if value.trim().is_empty() {
        fallback
    } else {
        value
    };
    take(v, max)
}

/// Truncate to `max` chars and drop XML-1.0-forbidden control characters (quick-xml escapes
/// &<>'" but writes control bytes raw, which would make the document non-well-formed).
fn take(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anaf_decl::d390::D390Op;

    fn company() -> Company {
        Company {
            id: "c1".into(),
            cui: "RO12345678".into(),
            legal_name: "Test SRL".into(),
            trade_name: None,
            registry_number: None,
            vat_payer: true,
            cash_vat: false,
            address: "Str. Exemplu 1".into(),
            city: "București".into(),
            county: "Sector 1".into(),
            postal_code: None,
            country: "RO".into(),
            email: Some("a@b.ro".into()),
            phone: Some("0712345678".into()),
            iban: None,
            bank_name: None,
            is_active: true,
            spv_enabled: false,
            tax_regime: "micro".into(),
            invoice_series: "FAC".into(),
            last_invoice_number: 1,
            logo_path: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn doc() -> D390Doc {
        D390Doc {
            luna: 3,
            an: 2026,
            operations: vec![
                D390Op {
                    tip: "L".into(),
                    tara: "DE".into(),
                    cod_o: "123456789".into(),
                    den_o: "Kunde GmbH".into(),
                    baza: 10000,
                },
                D390Op {
                    tip: "A".into(),
                    tara: "FR".into(),
                    cod_o: "55512345".into(),
                    den_o: "Fournisseur SARL".into(),
                    baza: 5000,
                },
            ],
            dropped: 0,
        }
    }

    #[test]
    fn emits_root_rezumat_and_operations() {
        let xml = generate_d390_xml(&doc(), &D390Submission::default(), &company()).unwrap();
        assert!(xml.contains("<declaratie390 "));
        assert!(xml.contains(&format!("xmlns=\"{NAMESPACE}\"")));
        // Namespace MUST be the OPANAF 705/2020 `:v3` (NOT the pre-2017 `:v1` of the stale public XSD),
        // and `bazaR` (regim agricultori) must be present — both confirmed by structura_D390_2020.
        assert!(
            xml.contains("d390:declaratie:v3\""),
            "D390 namespace must be :v3 (OPANAF 705/2020)"
        );
        assert!(
            !xml.contains("d390:declaratie:v1"),
            "must NOT emit the old :v1 namespace"
        );
        assert!(
            xml.contains("bazaR="),
            "rezumat must carry the R-code (agricultori) total"
        );
        assert!(xml.contains("luna=\"3\""));
        assert!(xml.contains("an=\"2026\""));
        assert!(xml.contains("cui=\"12345678\""), "RO prefix stripped");
        assert!(xml.contains("d_rec=\"0\""));
        // rezumat totals.
        assert!(xml.contains("nrOPI=\"2\""));
        assert!(xml.contains("bazaL=\"10000\""));
        assert!(xml.contains("bazaA=\"5000\""));
        assert!(xml.contains("total_baza=\"15000\""));
        // totalPlata_A = nrOPI(2) + bazaL(10000) + bazaA(5000) = 15002.
        assert!(xml.contains("totalPlata_A=\"15002\""));
        // operation rows.
        assert!(xml.contains("tip=\"L\""));
        assert!(xml.contains("tara=\"DE\""));
        assert!(xml.contains("codO=\"123456789\""));
        assert!(xml.contains("tip=\"A\""));
        assert!(xml.contains("</declaratie390>"));
    }

    #[test]
    fn escapes_partner_name() {
        let mut d = doc();
        d.operations[0].den_o = "A & B <Ltd>".into();
        let xml = generate_d390_xml(&d, &D390Submission::default(), &company()).unwrap();
        assert!(xml.contains("A &amp; B &lt;Ltd&gt;"));
        assert!(!xml.contains("A & B <Ltd>"));
    }
}
