//! Integration test: generate a D300 v12 XML and validate it against the official
//! vendored XSD via `xmllint --schema`.
//!
//! Skips gracefully when the XSD or xmllint are absent, so the standard
//! `cargo test` gate stays green everywhere. Both of the following must be
//! present for the XSD check to run:
//!   - XSD file at `src-tauri/tools/anaf/sample_d300_v12.xml`
//!   - `xmllint` CLI available on PATH
//!
//! On a machine/CI that has both, this is the proof that the generated XML is
//! structurally conformant with the official ANAF schema.

use std::path::Path;

use efactura_desktop_lib::anaf_decl::d300::generator::generate_d300_xml;
use efactura_desktop_lib::anaf_decl::d300::rows::map_to_rows;
use efactura_desktop_lib::anaf_decl::d300::D300Submission;
use efactura_desktop_lib::anaf_decl::validation::{validate_with_xsd, xmllint_available};
use efactura_desktop_lib::anaf_decl::version::resolve;
use efactura_desktop_lib::anaf_decl::DeclKind;
use efactura_desktop_lib::commands::declarations::{D300Group, D300Report};
use efactura_desktop_lib::db::companies::Company;

// ── Helpers ────────────────────────────────────────────────────────────────────

fn test_company() -> Company {
    Company {
        id: "test-co-id".to_string(),
        cui: "RO12345678".to_string(),
        legal_name: "CLARITO TEST SRL".to_string(),
        trade_name: None,
        registry_number: Some("J40/1234/2020".to_string()),
        vat_payer: true,
        address: "Calea Victoriei 155".to_string(),
        city: "Bucuresti".to_string(),
        county: "IF".to_string(),
        postal_code: Some("010073".to_string()),
        country: "RO".to_string(),
        email: Some("test@clarito.ro".to_string()),
        phone: Some("0721000000".to_string()),
        iban: Some("RO49AAAA1B31007593840000".to_string()),
        bank_name: Some("Banca Transilvania".to_string()),
        is_active: true,
        spv_enabled: false,
        invoice_series: "F".to_string(),
        last_invoice_number: 10,
        logo_path: None,
        created_at: 0,
        updated_at: 0,
    }
}

fn test_submission() -> D300Submission {
    D300Submission {
        nume_declar: "POPESCU".to_string(),
        prenume_declar: "ION".to_string(),
        functie_declar: "DIRECTOR".to_string(),
        caen: "6201".to_string(),
        banca: "Banca Transilvania".to_string(),
        cont: "RO49AAAA1B31007593840000".to_string(),
        tip_decont: "L".to_string(),
        ..Default::default()
    }
}

fn test_report() -> D300Report {
    // Synthetic fiscal data: sales at 21% + 11%, purchases at 21%
    D300Report {
        company_cui: "RO12345678".to_string(),
        period_from: "2025-09-01".to_string(),
        period_to: "2025-09-30".to_string(),
        groups: vec![
            D300Group {
                vat_rate: "0.21".to_string(),
                vat_category: "S".to_string(),
                base: "10000.00".to_string(),
                vat: "2100.00".to_string(),
            },
            D300Group {
                vat_rate: "0.11".to_string(),
                vat_category: "S".to_string(),
                base: "5000.00".to_string(),
                vat: "550.00".to_string(),
            },
        ],
        total_base: "15000.00".to_string(),
        total_vat: "2650.00".to_string(),
        invoice_count: 12,
        purchase_groups: vec![D300Group {
            vat_rate: "0.21".to_string(),
            vat_category: "S".to_string(),
            base: "8000.00".to_string(),
            vat: "1680.00".to_string(),
        }],
        total_deductible_base: "8000.00".to_string(),
        total_deductible_vat: "1680.00".to_string(),
        purchase_invoice_count: 7,
        purchase_unparsed_count: 0,
        net_vat: "970.00".to_string(),
    }
}

// ── Test ───────────────────────────────────────────────────────────────────────

#[test]
fn d300_validates_against_official_xsd() {
    // Locate the vendored XSD (relative to the `src-tauri/` crate root).
    // `cargo test --test d300_xsd` runs with cwd = src-tauri/.
    let xsd_path = Path::new("tools/anaf/sample_d300_v12.xml");

    if !xsd_path.exists() {
        eprintln!(
            "SKIP d300_xsd: XSD not found at {xsd_path:?} — vendor it at \
             src-tauri/tools/anaf/sample_d300_v12.xml to enable this gate."
        );
        return;
    }

    if !xmllint_available() {
        eprintln!(
            "SKIP d300_xsd: xmllint not available — install libxml2-utils (Linux) \
             or it ships with macOS Xcode CLT."
        );
        return;
    }

    // Build a fully-valid D300 from synthetic data.
    let period = chrono::NaiveDate::from_ymd_opt(2025, 9, 1).expect("test date");
    let ver = resolve(DeclKind::D300, period).expect("schema version");

    let report = test_report();
    let submission = test_submission();
    let company = test_company();

    let rows = map_to_rows(&report, &submission, &company, period)
        .expect("map_to_rows must not fail on valid input");

    let xml = generate_d300_xml(&rows, &ver).expect("generate_d300_xml must not fail");

    eprintln!("Generated D300 XML ({} bytes):", xml.len());
    eprintln!("{xml}");

    // Write to temp file for xmllint.
    let tmp = std::env::temp_dir().join("d300_xsd_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");

    let result = validate_with_xsd(xsd_path, &tmp)
        .expect("validate_with_xsd must not fail (xmllint must be available)");

    // Print all errors for diagnosis.
    if !result.passed {
        eprintln!("XSD VALIDATION FAILED:");
        for e in &result.errors {
            eprintln!("  {e}");
        }
    } else {
        eprintln!("XSD VALIDATION PASSED");
    }

    let _ = std::fs::remove_file(&tmp);

    assert!(
        result.passed,
        "D300 XML failed XSD validation. Errors:\n{}",
        result.errors.join("\n")
    );
}

// ── Additional unit-level checks ───────────────────────────────────────────────

/// Verify that map_to_rows + generate both complete without error on a zero
/// report (no sales, no purchases — edge case for empty period).
#[test]
fn d300_empty_period_generates_valid_xml() {
    let empty_report = D300Report {
        company_cui: "RO12345678".to_string(),
        period_from: "2025-09-01".to_string(),
        period_to: "2025-09-30".to_string(),
        groups: vec![],
        total_base: "0.00".to_string(),
        total_vat: "0.00".to_string(),
        invoice_count: 0,
        purchase_groups: vec![],
        total_deductible_base: "0.00".to_string(),
        total_deductible_vat: "0.00".to_string(),
        purchase_invoice_count: 0,
        purchase_unparsed_count: 0,
        net_vat: "0.00".to_string(),
    };

    let period = chrono::NaiveDate::from_ymd_opt(2025, 9, 1).expect("test date");
    let ver = resolve(DeclKind::D300, period).expect("schema version");
    let sub = test_submission();
    let co = test_company();

    let rows = map_to_rows(&empty_report, &sub, &co, period).expect("map_to_rows empty");
    assert_eq!(rows.total_plata_a, 0, "empty period: totalPlata_A = 0");
    assert_eq!(rows.r9_1, None, "empty period: R9_1 = None");

    let xml = generate_d300_xml(&rows, &ver).expect("generate_d300_xml empty");
    assert!(xml.contains("declaratie300"), "must contain root element");
    assert!(
        xml.contains("totalPlata_A=\"0\""),
        "must have totalPlata_A=0"
    );
}

/// Verify the explicit totals reconciliation stated in the spec.
#[test]
fn d300_totals_reconciliation() {
    // Sales: 10000@21% + 5000@11%
    // R17_2 = 2100 + 550 = 2650
    // Purchases: 8000@21%
    // R27_2 = 1680
    // R34_2 = 2650 - 1680 = 970 (TVA de plată)
    // R33_2 = 0
    // R41_2 = 970, totalPlata_A = 970
    let period = chrono::NaiveDate::from_ymd_opt(2025, 9, 1).expect("test date");
    let ver = resolve(DeclKind::D300, period).expect("schema version");
    let report = test_report();
    let sub = test_submission();
    let co = test_company();

    let rows = map_to_rows(&report, &sub, &co, period).expect("map_to_rows");

    assert_eq!(rows.r17_2, Some(2650), "R17_2");
    assert_eq!(rows.r27_2, Some(1680), "R27_2");
    assert_eq!(rows.r28_2, Some(1680), "R28_2 = R27_2");
    assert_eq!(rows.r32_2, Some(1680), "R32_2 = R27_2");
    assert_eq!(rows.r34_2, Some(970), "R34_2 = R17_2 - R32_2");
    assert_eq!(rows.r33_2, None, "R33_2 = None (zero, omitted)");
    assert_eq!(rows.r37_2, Some(970), "R37_2 = R34_2");
    assert_eq!(rows.r41_2, Some(970), "R41_2 = sold de plată");
    assert_eq!(rows.r42_2, None, "R42_2 = None (no refund)");
    assert_eq!(rows.total_plata_a, 970, "totalPlata_A = R41_2");

    // Also verify the XML string contains key computed values
    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    assert!(xml.contains("R17_2=\"2650\""), "XML R17_2");
    assert!(xml.contains("R34_2=\"970\""), "XML R34_2");
    assert!(xml.contains("totalPlata_A=\"970\""), "XML totalPlata_A");
}
