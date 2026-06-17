//! Integration test: generate a D207 XML (non-resident dividends) and validate it against the official
//! ANAF XSD (`d207.xsd`, targetNamespace `mfp:anaf:dgti:d207:declaratie:v2`, version 1.02) via `xmllint`.
//!
//! Skips gracefully when the XSD or xmllint are absent so the standard `cargo test` gate stays green
//! everywhere. On a machine that has both (the XSD is fetched by scripts/fetch-validators.sh), this is
//! the proof that the generated D207 declaration is structurally conformant with the official schema —
//! D207 has NO DUKIntegrator validator, so the XSD round-trip is the authoritative check.

use std::path::Path;

use efactura_desktop_lib::anaf_decl::d207_xml::{build_d207_xml, D207Benef, D207Header};
use efactura_desktop_lib::anaf_decl::validation::{validate_with_xsd, xmllint_available};

fn header() -> D207Header {
    D207Header {
        cui: "RO12345678".into(),
        den: "Plătitor SRL".into(),
        adresa: "Str. Exemplu nr. 1, București, Sector 1".into(),
        an: 2025,
        d_rec: 0,
        nume_declar: "Popescu".into(),
        prenume_declar: "Ion".into(),
        functie_declar: "Administrator".into(),
    }
}

/// A mix of taxable (01 + treaty 22) and exempt (14) non-resident dividend rows — exercises both the
/// Tbaza/Timp path and the Tscutit/zero-tax path, plus the optional cifR/cifS attributes.
fn beneficiaries() -> Vec<D207Benef> {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let d = |s: &str| Decimal::from_str(s).unwrap();
    vec![
        D207Benef {
            tip_venit: "01".into(),
            name: "Müller GmbH".into(),
            stat_r: "DE".into(),
            cif_r: None,
            cif_s: Some("DE811234567".into()),
            baza: d("10000.00"),
            impozit: d("1000.00"),
            impozit_suportat: d("0"),
            act_n: 1,
        },
        D207Benef {
            tip_venit: "22".into(),
            name: "Dupont SA".into(),
            stat_r: "FR".into(),
            cif_r: Some("RO99887766".into()),
            cif_s: Some("FR12345678901".into()),
            baza: d("5000.00"),
            impozit: d("250.00"),
            impozit_suportat: d("0"),
            act_n: 2,
        },
        D207Benef {
            tip_venit: "14".into(), // exempt (art. 229) — EU parent-subsidiary
            name: "EU Parent BV".into(),
            stat_r: "NL".into(),
            cif_r: None,
            cif_s: None,
            baza: d("20000.00"),
            impozit: d("0"),
            impozit_suportat: d("0"),
            act_n: 1,
        },
    ]
}

#[test]
fn d207_validates_against_official_xsd() {
    let xsd_path = Path::new("tools/anaf/d207.xsd");
    if !xsd_path.exists() {
        eprintln!("SKIP d207_xsd: official XSD not vendored at {xsd_path:?}");
        return;
    }
    if !xmllint_available() {
        eprintln!("SKIP d207_xsd: xmllint not available");
        return;
    }

    let xml = build_d207_xml(&header(), &beneficiaries()).expect("build_d207_xml");
    eprintln!("Generated D207 XML ({} bytes):\n{xml}", xml.len());

    let tmp = std::env::temp_dir().join("d207_xsd_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");
    let result = validate_with_xsd(xsd_path, &tmp).expect("validate_with_xsd (xmllint)");
    let _ = std::fs::remove_file(&tmp);

    assert!(
        result.passed,
        "D207 XML failed official XSD validation. Errors:\n{}",
        result.errors.join("\n")
    );
}
