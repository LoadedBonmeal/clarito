//! Integration test: generate a D710 XML (declarație rectificativă obligații D100) and
//! validate it against the official ANAF XSD (`tools/anaf/d710.xsd`, targetNamespace
//! `mfp:anaf:dgti:d710:declaratie:v1`, version 1.02) via `xmllint`.
//!
//! Skips gracefully when the XSD or xmllint are absent so the standard `cargo test` gate
//! stays green everywhere. On a machine that has both (the XSD is fetched by
//! `scripts/fetch-validators.sh`), this is the proof that the generated D710 declaration
//! is structurally conformant with the official schema.
//!
//! ## Official validators
//! - **XSD structural gate** (this test): `xmllint --schema tools/anaf/d710.xsd <xml>`
//!   NOTE: The vendored d710.xsd has an ANAF publishing typo (`xmlns=v2` vs
//!   `targetNamespace=v1`). The file is vendored with the typo corrected to `xmlns=v1`
//!   so that xmllint can compile the schema.
//! - **Business-rule gate**: `D710Validator.jar` from pachetul standalone
//!   `D710_20052026.zip` pe declaratii.anaf.ro (NU prin DUKIntegrator — D710 are
//!   validator separat). Rulați: `java -jar D710Validator.jar <xml>`

use std::path::Path;

use efactura_desktop_lib::anaf_decl::d710_xml::{
    build_d710_xml, D710Header, D710Input, D710Obligation,
};
use efactura_desktop_lib::anaf_decl::validation::{validate_with_xsd, xmllint_available};
use rust_decimal::Decimal;
use std::str::FromStr;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn header(luna: u32, an: i32) -> D710Header {
    D710Header {
        cui: "12345674".into(),
        den: "Test SRL".into(),
        adresa: "Str. Exemplu nr. 1, București, Sector 1".into(),
        luna,
        an,
        d_anulare: 0,
        rectificativa: false,
        temei: None,
        telefon: None,
        fax: None,
        mail: None,
        cif_r: None,
        den_r: None,
        adr_r: None,
        tel_r: None,
        fax_r: None,
        email_r: None,
        cif_s: None,
        d_succ: None,
        d_dizolv: None,
        d_energie: None,
        d_modif: None,
        nume_declar: "Popescu".into(),
        prenume_declar: "Ion".into(),
        functie_declar: "Administrator".into(),
    }
}

/// Two obligations for the same period: impozit profit + impozit micro.
/// Exercises: cod_oblig, cod_bugetar, scadenta, nr_evid, suma_plata_i/c, totalPlata_A sum.
fn two_obligations() -> Vec<D710Obligation> {
    vec![
        D710Obligation {
            cod_oblig: 2,
            cod_bugetar: "0105".into(),
            scadenta: "25.04.2026".into(),
            nr_evid: 0,
            suma_plata_i: Some(d("8000")),
            suma_plata_c: Some(d("10000")),
            ..Default::default()
        },
        D710Obligation {
            cod_oblig: 5,
            cod_bugetar: "0205".into(),
            scadenta: "25.04.2026".into(),
            nr_evid: 0,
            suma_plata_i: Some(d("1800")),
            suma_plata_c: Some(d("2000")),
            ..Default::default()
        },
    ]
}

#[test]
fn d710_validates_against_official_xsd() {
    let xsd_path = Path::new("tools/anaf/d710.xsd");
    if !xsd_path.exists() {
        eprintln!("SKIP d710_xsd: official XSD not vendored at {xsd_path:?}");
        return;
    }
    if !xmllint_available() {
        eprintln!("SKIP d710_xsd: xmllint not available");
        return;
    }

    let input = D710Input {
        header: header(3, 2026),
        obligations: two_obligations(),
    };
    let xml = build_d710_xml(&input).expect("build_d710_xml");
    eprintln!("Generated D710 XML ({} bytes):\n{xml}", xml.len());

    let tmp = std::env::temp_dir().join("d710_xsd_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");
    let result = validate_with_xsd(xsd_path, &tmp).expect("validate_with_xsd (xmllint)");
    let _ = std::fs::remove_file(&tmp);

    assert!(
        result.passed,
        "D710 XML failed official XSD validation. Errors:\n{}",
        result.errors.join("\n")
    );
}

/// Validate a rectificativă declaration (d_recN=1) and a declaration with all optional
/// sum fields populated.
#[test]
fn d710_rectificativa_with_all_sums_validates_against_official_xsd() {
    let xsd_path = Path::new("tools/anaf/d710.xsd");
    if !xsd_path.exists() {
        eprintln!("SKIP d710_xsd_rect: official XSD not vendored at {xsd_path:?}");
        return;
    }
    if !xmllint_available() {
        eprintln!("SKIP d710_xsd_rect: xmllint not available");
        return;
    }

    let mut hdr = header(5, 2026);
    hdr.rectificativa = true;
    hdr.temei = Some(2);

    let input = D710Input {
        header: hdr,
        obligations: vec![D710Obligation {
            cod_oblig: 22,
            cod_bugetar: "0405".into(),
            scadenta: "25.06.2026".into(),
            nr_evid: 12345,
            suma_dat_i: Some(d("5000")),
            suma_dat_c: Some(d("5500")),
            suma_ded_i: Some(d("500")),
            suma_ded_c: Some(d("550")),
            suma_plata_i: Some(d("4500")),
            suma_plata_c: Some(d("4950")),
            suma_rest_i: Some(d("200")),
            suma_rest_c: Some(d("220")),
            cota: Some(1),
            den_oblig: "Impozit nerezidenți".into(),
        }],
    };

    let xml = build_d710_xml(&input).expect("build_d710_xml rectificativa");
    eprintln!(
        "Generated D710 rectificativa XML ({} bytes):\n{xml}",
        xml.len()
    );

    let tmp = std::env::temp_dir().join("d710_xsd_rect_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");
    let result = validate_with_xsd(xsd_path, &tmp).expect("validate_with_xsd (xmllint)");
    let _ = std::fs::remove_file(&tmp);

    assert!(
        result.passed,
        "D710 rectificativă failed official XSD validation. Errors:\n{}",
        result.errors.join("\n")
    );
}
