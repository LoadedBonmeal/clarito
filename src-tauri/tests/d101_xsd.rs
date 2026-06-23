//! XSD structural test for D101 — declarație privind impozitul pe profit.
//!
//! ## Namespace mismatch — why this test SKIPS
//!
//! The vendored XSD (`tools/anaf/d101_20250214.xsd`) declares
//! `targetNamespace="mfp:anaf:dgti:d101:declaratie:v3"`. However, DUKIntegrator (D101Validator.jar)
//! requires PERIOD-DEPENDENT namespaces (confirmed by live DUK testing in 2026-06):
//!   ≤2023 → mfp:anaf:dgti:d101:declaratie:v9
//!   ≥2024 → mfp:anaf:dgti:d101:declaratie:v10
//!
//! Neither the v9 nor v10 XSD files are publicly available from ANAF. Our emitter
//! (`d101_xml.rs`) uses `d101_namespace_for_year(an)` to emit the correct DUK-required namespace.
//!
//! Because xmllint validates namespace membership against the XSD's targetNamespace, running
//! xmllint against the v3 XSD with a v9/v10 document will always fail — not because our XML is
//! wrong, but because the XSD file we have doesn't match the DUK-required namespace.
//!
//! The DUK gate (D101Validator.jar) is the authoritative business-rule validator for D101.
//! This test file documents the mismatch and skips gracefully when the XSD namespace doesn't match.

use std::path::Path;

use efactura_desktop_lib::anaf_decl::d101_xml::{build_d101_xml, D101Header};
use efactura_desktop_lib::anaf_decl::validation::xmllint_available;

fn test_header() -> D101Header {
    D101Header {
        luna_i: 1,
        luna: 12,
        an: 2025,
        an_i: 2025,
        d_rec: 0, // overridden to 2 for an>=2024 by build_d101_xml (DUK R2a)
        d_anulare: 0,
        d_succ: 0,
        d_alte: 0,
        d_reglem: 0,
        data_i: "01.01.2025".into(),
        data_s: "31.12.2025".into(),
        // DUK v8: cod_obligatie must be one of "102","103","104","105".
        // "102" = impozit pe profit anual; trim_micro must be absent (R10.2).
        cod_obligatie: "102".into(),
        scadenta: "250625".into(),
        cod_bug: "20470101".into(),
        // nr_evid = 23-char computed: "10" + "102" + "01" + "1225" + "25" + "0625" + "0000" + ctrl
        nr_evid: "10102011225250625000035".into(),
        total_plata_a: 0,
        cif: "12345674".into(),
        caen: "6201".into(),
        denumire: "Test SRL".into(),
        adresa: "Str. Exemplu nr. 1, Bucuresti".into(),
        telefon: None,
        fax: None,
        email: None,
        nume_declar: "Popescu".into(),
        prenume_declar: "Ion".into(),
        functie_declar: "Administrator".into(),
        p1: None,
        p2: None,
        p3: None,
        p4: None,
        p5: None,
        p6: None,
        p7: None,
        p8: None,
        p9: None,
        p10: None,
        p11: None,
        p12: None,
        p13: None,
        p14: None,
        p15: None,
    }
}

#[test]
fn d101_emitter_produces_period_correct_namespace() {
    // Smoke-test: the emitter must produce the DUK-confirmed period-dependent namespace.
    // test_header() uses an=2025 → DUK requires v10 for ≥2024.
    // This is a purely in-memory check — no XSD, no xmllint.
    let xml = build_d101_xml(&test_header()).expect("build_d101_xml");
    assert!(
        xml.contains(r#"xmlns="mfp:anaf:dgti:d101:declaratie:v10""#),
        "D101 emitter must use v10 namespace for an=2025 (DUK-confirmed); got:\n{xml}"
    );
    assert!(
        xml.contains("<declaratie101 "),
        "D101 root element must be declaratie101"
    );
    // Verify 2023 → v9
    let mut hdr2023 = test_header();
    hdr2023.an = 2023;
    hdr2023.an_i = 2023;
    hdr2023.data_i = "01.01.2023".into();
    hdr2023.data_s = "31.12.2023".into();
    let xml2023 = build_d101_xml(&hdr2023).expect("build_d101_xml 2023");
    assert!(
        xml2023.contains(r#"xmlns="mfp:anaf:dgti:d101:declaratie:v9""#),
        "D101 emitter must use v9 namespace for an=2023 (DUK-confirmed); got:\n{xml2023}"
    );
}

#[test]
fn d101_xsd_structural_validation() {
    // This test skips because the vendored XSD (v3) does not match the DUK-required namespace (v2).
    // xmllint would report a namespace mismatch, not an actual structural error.
    // See the module-level comment for full explanation.
    let xsd_path = Path::new("tools/anaf/d101_20250214.xsd");
    if !xsd_path.exists() {
        eprintln!("SKIP d101_xsd: XSD not vendored at {xsd_path:?}");
        return;
    }
    if !xmllint_available() {
        eprintln!("SKIP d101_xsd: xmllint not available");
        return;
    }
    // Check if the XSD uses v3 namespace (expected — means we must skip).
    let xsd_content = std::fs::read_to_string(xsd_path).unwrap_or_default();
    if xsd_content.contains("declaratie:v3") {
        eprintln!(
            "SKIP d101_xsd: vendored XSD uses v3 namespace but DUK requires v9 (≤2023) or v10 \
             (≥2024). xmllint would fail due to namespace mismatch, not a structural error. \
             Use the DUK gate (D101Validator.jar) for authoritative validation."
        );
        return;
    }
    // If ANAF ever publishes v9/v10 XSD files, the test will run here automatically.
    eprintln!("INFO d101_xsd: XSD doesn't appear to be v3; attempting structural validation.");
    let xml = build_d101_xml(&test_header()).expect("build_d101_xml");
    let tmp = std::env::temp_dir().join("d101_xsd_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp D101 XML");
    let result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("validate_with_xsd");
    let _ = std::fs::remove_file(&tmp);
    assert!(
        result.passed,
        "D101 XSD validation failed:\n{}",
        result.errors.join("\n")
    );
}
