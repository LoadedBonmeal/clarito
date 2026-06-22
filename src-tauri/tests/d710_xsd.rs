//! Integration test: generate a D710 XML (declarație rectificativă obligații D100) and
//! validate it against the official ANAF XSD (`tools/anaf/d710.xsd`, patched to
//! `mfp:anaf:dgti:d710:declaratie:v2`, version 1.02) via `xmllint`.
//!
//! Skips gracefully when the XSD or xmllint are absent so the standard `cargo test` gate
//! stays green everywhere. On a machine that has both (the XSD is fetched by
//! `scripts/fetch-validators.sh`), this is the proof that the generated D710 declaration
//! is structurally conformant with the patched schema.
//!
//! ## Official validators
//! - **XSD structural gate** (this test): `xmllint --schema tools/anaf/d710.xsd <xml>`
//!   NOTE: The vendored d710.xsd is patched by `fetch-validators.sh` to use `v2` namespace
//!   (both `xmlns=v2` and `targetNamespace=v2`) so that xmllint validates v2 documents.
//!   The ANAF published XSD has `targetNamespace=v1` but DUKIntegrator requires `v2`.
//! - **Business-rule gate**: `D710Validator.jar` prin DUKIntegrator (același mecanism ca D301/D700):
//!   `java -jar DUKIntegrator.jar -v D710 <xml> <result>` (NU validator standalone!)

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

/// Two obligations for the same period: impozit profit (cod_oblig=103) + impozit micro (cod_oblig=121).
/// Exercises: cod_oblig, cod_bugetar, scadenta, nr_evid, suma_plata_i/c, totalPlata_A sum.
///
/// DUK nomenclator (Parameters_v33, valabil 2026-03):
/// - cod_oblig=103 (impozit profit, model 8#): cod_bugetar="20470101"
///   Model 8# amounts: suma_dat_I/C, suma_plata_I/C (suma_plata_C = suma_dat_C as simplification)
/// - cod_oblig=121 (impozit micro, model 8#): cod_bugetar="20470101"
///
/// DUK R11b: totalPlata_A = Σ(ALL non-null amount fields across ALL obligations).
fn two_obligations() -> Vec<D710Obligation> {
    vec![
        D710Obligation {
            cod_oblig: 103,
            cod_bugetar: "20470101".into(),
            scadenta: "25.04.2026".into(),
            nr_evid: 0, // → auto-computed per compute_nr_evid_d710
            suma_dat_I: Some(d("8000")),
            suma_dat_C: Some(d("10000")),
            suma_plata_I: Some(d("8000")),
            suma_plata_C: Some(d("10000")),
            ..Default::default()
        },
        D710Obligation {
            cod_oblig: 121,
            cod_bugetar: "20470101".into(),
            scadenta: "25.04.2026".into(),
            nr_evid: 0, // → auto-computed per compute_nr_evid_d710
            suma_dat_I: Some(d("1800")),
            suma_dat_C: Some(d("2000")),
            suma_plata_I: Some(d("1800")),
            suma_plata_C: Some(d("2000")),
            cota: Some(1), // DUK R17: cod_oblig=121 (micro) requires cota impozitare
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
            suma_dat_I: Some(d("5000")), // uppercase I/C per DUK v2 protocol
            suma_dat_C: Some(d("5500")),
            suma_ded_I: Some(d("500")),
            suma_ded_C: Some(d("550")),
            suma_plata_I: Some(d("4500")),
            suma_plata_C: Some(d("4950")),
            suma_rest_I: Some(d("200")),
            suma_rest_C: Some(d("220")),
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
