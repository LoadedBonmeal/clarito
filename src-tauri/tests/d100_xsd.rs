//! XSD structural test for D100 — declarație privind obligațiile de plată la bugetul de stat.
//!
//! Validates the D100 XML emitter against the official ANAF XSD
//! (`tools/anaf/d100_24022022.xsd`, targetNamespace `mfp:anaf:dgti:d100:declaratie:v2`,
//! version 1.02) via `xmllint`. Skips gracefully when the XSD or xmllint are absent.
//!
//! ## Official validators
//! - **XSD structural gate** (this test): `xmllint --schema tools/anaf/d100_24022022.xsd <xml>`
//! - **Business-rule gate**: `D100Validator.jar` via DUKIntegrator
//!   (`java -jar DUKIntegrator.jar -v D100 <xml> <result>`)
//!
//! ## DUK rules implemented in the emitter
//! - R11b: totalPlata_A = Σ(suma_dat + suma_ded + suma_plata + suma_rest) (ALL sum fields)
//! - R16: nr_evid must be 23 digits (auto-computed when nr_evid=0 via D710 algorithm)
//! - Rcota + R17: cota=1 required when cod_oblig=121 (impozit micro), auto-filled

use std::path::Path;

use efactura_desktop_lib::anaf_decl::d100_xml::{build_d100_xml, D100Header, D100Obligatie};
use efactura_desktop_lib::anaf_decl::validation::{validate_with_xsd, xmllint_available};
use rust_decimal::Decimal;
use std::str::FromStr;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn test_header() -> D100Header {
    D100Header {
        luna: 3,
        an: 2026,
        d_anulare: 0,
        cui: "12345674".into(),
        den: "Test SRL".into(),
        adresa: "Str. Exemplu nr. 1, Bucuresti".into(),
        telefon: None,
        fax: None,
        email: None,
        nume_declar: "Popescu".into(),
        prenume_declar: "Ion".into(),
        functie_declar: "Administrator".into(),
        obligatii: vec![D100Obligatie {
            // cod_oblig 121 = impozit pe veniturile microîntreprinderilor (DUK-confirmed nomenclator).
            // DUK rules: cota=1 required (Rcota + R17), nr_evid must be 23 chars (R16).
            cod_oblig: 121,
            cod_bugetar: "20470101".into(),
            scadenta: "25.04.2026".into(),
            nr_evid: 0, // auto-computed to 23-char via D710 algorithm
            suma_dat: Some(d("1000")),
            suma_ded: None,
            suma_plata: Some(d("1000")),
            suma_rest: None,
            cota: None, // auto-filled to 1 for cod_oblig=121 (DUK Rcota)
            suma_redu: None,
        }],
    }
}

#[test]
fn d100_validates_against_official_xsd() {
    let xsd_path = Path::new("tools/anaf/d100_24022022.xsd");
    if !xsd_path.exists() {
        eprintln!("SKIP d100_xsd: XSD not vendored at {xsd_path:?}");
        return;
    }
    if !xmllint_available() {
        eprintln!("SKIP d100_xsd: xmllint not available");
        return;
    }
    let xml = build_d100_xml(&test_header()).expect("build_d100_xml");
    eprintln!("D100 XML ({} bytes):\n{xml}", xml.len());
    let tmp = std::env::temp_dir().join("d100_xsd_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp D100 XML");
    let result = validate_with_xsd(xsd_path, &tmp).expect("validate_with_xsd");
    let _ = std::fs::remove_file(&tmp);
    assert!(
        result.passed,
        "D100 XSD validation failed:\n{}",
        result.errors.join("\n")
    );
}
