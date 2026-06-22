//! Integration test: generate a D301 XML (decont special de TVA) and validate it against
//! the official ANAF XSD (`tools/anaf/d301.xsd`, targetNamespace
//! `mfp:anaf:dgti:d301:declaratie:v1`, version 1.02) via `xmllint`.
//!
//! Skips gracefully when the XSD or xmllint are absent so the standard `cargo test` gate
//! stays green everywhere. On a machine that has both (the XSD is fetched by
//! `scripts/fetch-validators.sh`), this is the proof that the generated D301 declaration
//! is structurally conformant with the official schema.
//!
//! ## Official validators
//! - **XSD structural gate** (this test): `xmllint --schema tools/anaf/d301.xsd <xml>`
//! - **Business-rule gate**: `D301Validator.jar` from pachetul `D301_20201022.zip` pe
//!   declaratii.anaf.ro, rulat prin DUKIntegrator
//!   (`java -jar DUKIntegrator.jar -v D301 <xml> <result>`)

use std::path::Path;

use efactura_desktop_lib::anaf_decl::d301_xml::{
    build_d301_xml, D301Data, D301Header, D301Sectiune,
};
use efactura_desktop_lib::anaf_decl::validation::{validate_with_xsd, xmllint_available};
use rust_decimal::Decimal;
use std::str::FromStr;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn header() -> D301Header {
    D301Header {
        cif: "12345674".into(),
        denumire: "Test SRL".into(),
        adresa: "Str. Exemplu nr. 1, București, Sector 1".into(),
        telefon: "0721000000".into(),
        fax: "".into(),
        email: "test@test.ro".into(),
        banca: "Banca Comerciala Romana".into(),
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

/// Exercises all five tip_operatie values + a rectificativă flag, covering:
/// - tip 1: AIC bunuri taxabile (RON, curs 1.0000)
/// - tip 2: AIC mijloace transport noi (EUR, curs 5.0200) → triggers mijl_trans=1
/// - tip 3: AIC produse accizabile (EUR, curs 5.0200)
/// - tip 4: Servicii intracomunitare (beneficiar obligat, art.307(2)) (EUR)
/// - tip 5: Alte operațiuni taxare inversă (USD, curs 4.6300)
fn all_sections() -> D301Data {
    D301Data {
        sectiuni: vec![
            D301Sectiune {
                tip_operatie: 1,
                nr_doc: "FAC-001".into(),
                data_doc: "10.05.2026".into(),
                val_valuta: d("5000.00"),
                tip_valuta: "RON".into(),
                curs_valutar: d("1.0000"),
                baza: d("5000.00"),
                tva: d("950.00"),
            },
            D301Sectiune {
                tip_operatie: 2,
                nr_doc: "MT-001".into(),
                data_doc: "15.05.2026".into(),
                val_valuta: d("10000.00"),
                tip_valuta: "EUR".into(),
                curs_valutar: d("5.0200"),
                baza: d("50200.00"),
                tva: d("9538.00"),
            },
            D301Sectiune {
                tip_operatie: 3,
                nr_doc: "ACC-001".into(),
                data_doc: "18.05.2026".into(),
                val_valuta: d("2000.00"),
                tip_valuta: "EUR".into(),
                curs_valutar: d("5.0200"),
                baza: d("10040.00"),
                tva: d("1907.60"),
            },
            D301Sectiune {
                tip_operatie: 4,
                nr_doc: "SRV-EU-001".into(),
                data_doc: "20.05.2026".into(),
                val_valuta: d("3000.00"),
                tip_valuta: "EUR".into(),
                curs_valutar: d("5.0200"),
                baza: d("15060.00"),
                tva: d("2861.40"),
            },
            D301Sectiune {
                tip_operatie: 5,
                nr_doc: "SRV-NEU-001".into(),
                data_doc: "22.05.2026".into(),
                val_valuta: d("1500.00"),
                tip_valuta: "USD".into(),
                curs_valutar: d("4.6300"),
                baza: d("6945.00"),
                tva: d("1319.55"),
            },
        ],
    }
}

#[test]
fn d301_validates_against_official_xsd() {
    let xsd_path = Path::new("tools/anaf/d301.xsd");
    if !xsd_path.exists() {
        eprintln!("SKIP d301_xsd: official XSD not vendored at {xsd_path:?}");
        return;
    }
    if !xmllint_available() {
        eprintln!("SKIP d301_xsd: xmllint not available");
        return;
    }

    let xml = build_d301_xml(&header(), &all_sections()).expect("build_d301_xml");
    eprintln!("Generated D301 XML ({} bytes):\n{xml}", xml.len());

    let tmp = std::env::temp_dir().join("d301_xsd_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");
    let result = validate_with_xsd(xsd_path, &tmp).expect("validate_with_xsd (xmllint)");
    let _ = std::fs::remove_file(&tmp);

    assert!(
        result.passed,
        "D301 XML failed official XSD validation. Errors:\n{}",
        result.errors.join("\n")
    );
}

/// Also validate a rectificativă (d_rec=1) declaration to exercise that path.
#[test]
fn d301_rectificativa_validates_against_official_xsd() {
    let xsd_path = Path::new("tools/anaf/d301.xsd");
    if !xsd_path.exists() {
        eprintln!("SKIP d301_xsd_rect: official XSD not vendored at {xsd_path:?}");
        return;
    }
    if !xmllint_available() {
        eprintln!("SKIP d301_xsd_rect: xmllint not available");
        return;
    }

    let mut hdr = header();
    hdr.d_rec = 1;

    let data = D301Data {
        sectiuni: vec![D301Sectiune {
            tip_operatie: 1,
            nr_doc: "FAC-RECT".into(),
            data_doc: "01.05.2026".into(),
            val_valuta: d("1000.00"),
            tip_valuta: "RON".into(),
            curs_valutar: d("1.0000"),
            baza: d("1000.00"),
            tva: d("190.00"),
        }],
    };

    let xml = build_d301_xml(&hdr, &data).expect("build_d301_xml rectificativa");
    eprintln!(
        "Generated D301 rectificativa XML ({} bytes):\n{xml}",
        xml.len()
    );

    let tmp = std::env::temp_dir().join("d301_xsd_rect_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");
    let result = validate_with_xsd(xsd_path, &tmp).expect("validate_with_xsd (xmllint)");
    let _ = std::fs::remove_file(&tmp);

    assert!(
        result.passed,
        "D301 rectificativă failed official XSD validation. Errors:\n{}",
        result.errors.join("\n")
    );
}
