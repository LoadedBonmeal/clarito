//! Integration test: generate a D394 v5 XML and validate it against the official
//! vendored XSD via `xmllint --schema`.
//!
//! Skips gracefully when the XSD or xmllint are absent, so the standard
//! `cargo test` gate stays green everywhere. Both of the following must be
//! present for the XSD check to run:
//!   - XSD file at `src-tauri/tools/anaf/sample_d394.xml`
//!   - `xmllint` CLI available on PATH
//!
//! On a machine/CI that has both, this is the proof that the generated XML is
//! structurally conformant with the official ANAF D394 v5 schema.
//!
//! All partner CUIs use valid checksums (CUI mod-11):
//!   12345674 — company / cif_intocmit
//!   98765438 — sales partner 1
//!   87654329 — sales partner 2
//!   76543210 — sales reverse-charge partner (AE → V)
//!   11111110 — purchase standard partner
//!   22222229 — purchase reverse-charge partner (AE → C, cota=19)
//! Foreign partners use EU VAT prefix (→ tip_partener=4):
//!   DE123456789 — intra-EU sale (K → LS, tp=4)
//!   FR55512345  — intra-EU purchase (K → C, tp=4)

use std::path::Path;

use efactura_desktop_lib::anaf_decl::d394::generator::generate_d394_xml;
use efactura_desktop_lib::anaf_decl::d394::sections::build_sections;
use efactura_desktop_lib::anaf_decl::d394::D394Submission;
use efactura_desktop_lib::anaf_decl::validation::{validate_with_xsd, xmllint_available};
use efactura_desktop_lib::anaf_decl::version::resolve;
use efactura_desktop_lib::anaf_decl::DeclKind;
use efactura_desktop_lib::commands::d394::{D394Partner, D394Report};
use efactura_desktop_lib::db::companies::Company;

// ── Helpers ────────────────────────────────────────────────────────────────────

fn test_company() -> Company {
    Company {
        id: "test-co-id".to_string(),
        // Valid CUI checksum: 12345674
        cui: "RO12345674".to_string(),
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

fn test_submission() -> D394Submission {
    D394Submission {
        tip_d394: "L".to_string(),
        sistem_tva: false,
        // op_efectuate=true: test report has partners
        op_efectuate: true,
        caen: "6201".to_string(),
        telefon: "0721000000".to_string(),
        den_r: "POPESCU ION".to_string(),
        functie_reprez: "DIRECTOR".to_string(),
        adresa_r: "Calea Victoriei 155, Bucuresti, IF".to_string(),
        tip_intocmit: 0,
        den_intocmit: "POPESCU ION".to_string(),
        // Valid CUI checksum: 12345674
        cif_intocmit: 12345674,
        calitate_intocmit: Some("Reprezentant".to_string()),
        optiune: false,
        prs_afiliat: false,
        solicit: false,
    }
}

/// A realistic D394Report with multiple partner categories.
/// All RO CUIs use valid mod-11 checksums.
fn test_report() -> D394Report {
    D394Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2025-09-01".to_string(),
        period_to: "2025-09-30".to_string(),
        partners: vec![
            // Standard 19% sales to a VAT-registered RO company (valid CUI: 98765438)
            D394Partner {
                partner_cui: "RO98765438".to_string(),
                partner_name: "SC CLIENT MARE SRL".to_string(),
                vat_category: "S".to_string(),
                vat_rate: "19".to_string(),
                invoice_count: 5,
                base: "10000.00".to_string(),
                vat: "1900.00".to_string(),
                art331_code: None,
            },
            // Standard 21% sales (valid CUI: 87654329)
            D394Partner {
                partner_cui: "RO87654329".to_string(),
                partner_name: "SC CLIENT MIC SRL".to_string(),
                vat_category: "S".to_string(),
                vat_rate: "21".to_string(),
                invoice_count: 2,
                base: "5000.00".to_string(),
                vat: "1050.00".to_string(),
                art331_code: None,
            },
            // Reverse-charge domestic (AE) delivery → tip=V (valid CUI: 76543210)
            D394Partner {
                partner_cui: "RO76543210".to_string(),
                partner_name: "SC CONSTRUCTII SRL".to_string(),
                vat_category: "AE".to_string(),
                vat_rate: "0".to_string(),
                invoice_count: 1,
                base: "3000.00".to_string(),
                vat: "0.00".to_string(),
                art331_code: None, // default → codPR=22 (deșeuri/scrap)
            },
            // Intra-EU delivery (K) → tip=LS, tp=4 (foreign DE partner)
            D394Partner {
                partner_cui: "DE123456789".to_string(),
                partner_name: "GERMAN GMBH".to_string(),
                vat_category: "K".to_string(),
                vat_rate: "0".to_string(),
                invoice_count: 1,
                base: "2000.00".to_string(),
                vat: "0.00".to_string(),
                art331_code: None,
            },
        ],
        total_base: "20000.00".to_string(),
        total_vat: "2950.00".to_string(),
        // invoice_count = total sales invoices reported in informatii.nrFacturi
        invoice_count: 9,
        purchase_partners: vec![
            // Standard 19% purchases (valid CUI: 11111110)
            D394Partner {
                partner_cui: "RO11111110".to_string(),
                partner_name: "SC FURNIZOR MARE SRL".to_string(),
                vat_category: "S".to_string(),
                vat_rate: "19".to_string(),
                invoice_count: 3,
                base: "8000.00".to_string(),
                vat: "1520.00".to_string(),
                art331_code: None,
            },
            // Reverse-charge domestic purchase (AE) → tip=C cota=19 (valid CUI: 22222229)
            D394Partner {
                partner_cui: "RO22222229".to_string(),
                partner_name: "SC CONSTRUCTORI SRL".to_string(),
                vat_category: "AE".to_string(),
                vat_rate: "19".to_string(),
                invoice_count: 1,
                base: "2000.00".to_string(),
                vat: "380.00".to_string(),
                art331_code: None, // default → codPR=22
            },
            // Intra-EU acquisition (K) from foreign EU partner → tp=4 → tip=C cota=19
            D394Partner {
                partner_cui: "FR55512345".to_string(),
                partner_name: "FRANCE SARL".to_string(),
                vat_category: "K".to_string(),
                vat_rate: "19".to_string(),
                invoice_count: 1,
                base: "1000.00".to_string(),
                vat: "190.00".to_string(),
                art331_code: None,
            },
        ],
        total_purchase_base: "11000.00".to_string(),
        total_purchase_vat: "2090.00".to_string(),
        purchase_invoice_count: 5,
        purchase_unparsed_count: 0,
    }
}

// ── Main XSD validation test ───────────────────────────────────────────────────

#[test]
fn d394_validates_against_official_xsd() {
    let xsd_path = Path::new("tools/anaf/sample_d394.xml");

    if !xsd_path.exists() {
        eprintln!(
            "SKIP d394_xsd: XSD not found at {xsd_path:?} — vendor it at \
             src-tauri/tools/anaf/sample_d394.xml to enable this gate."
        );
        return;
    }

    if !xmllint_available() {
        eprintln!(
            "SKIP d394_xsd: xmllint not available — install libxml2-utils (Linux) \
             or it ships with macOS Xcode CLT."
        );
        return;
    }

    let period = chrono::NaiveDate::from_ymd_opt(2025, 9, 1).expect("test date");
    let ver = resolve(DeclKind::D394, period).expect("schema version");

    let report = test_report();
    let submission = test_submission();
    let company = test_company();

    let doc = build_sections(&report, &submission, &company, period)
        .expect("build_sections must not fail");

    let xml = generate_d394_xml(&doc, &submission, &company, &ver)
        .expect("generate_d394_xml must not fail");

    eprintln!("Generated D394 XML ({} bytes):", xml.len());
    eprintln!("{xml}");

    // TEMPORARY: dump XML for DUK validation if EFACTURA_DUMP_DIR is set
    if let Ok(dump_dir) = std::env::var("EFACTURA_DUMP_DIR") {
        let dump_path = std::path::Path::new(&dump_dir).join("d394.xml");
        std::fs::write(&dump_path, xml.as_bytes()).expect("write dump XML");
        eprintln!("DUMP: wrote D394 XML to {:?}", dump_path);
    }

    // Write to temp file for xmllint
    let tmp = std::env::temp_dir().join("d394_xsd_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");

    let result = validate_with_xsd(xsd_path, &tmp)
        .expect("validate_with_xsd must not fail (xmllint must be available)");

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
        "D394 XML failed XSD validation. Errors:\n{}",
        result.errors.join("\n")
    );
}

// ── Additional structural tests ────────────────────────────────────────────────

/// Empty period (no partners) must still produce valid XML with <informatii>.
#[test]
fn d394_empty_period_generates_valid_xml() {
    let empty_report = D394Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2025-09-01".to_string(),
        period_to: "2025-09-30".to_string(),
        partners: vec![],
        total_base: "0.00".to_string(),
        total_vat: "0.00".to_string(),
        invoice_count: 0,
        purchase_partners: vec![],
        total_purchase_base: "0.00".to_string(),
        total_purchase_vat: "0.00".to_string(),
        purchase_invoice_count: 0,
        purchase_unparsed_count: 0,
    };

    let period = chrono::NaiveDate::from_ymd_opt(2025, 9, 1).expect("test date");
    let ver = resolve(DeclKind::D394, period).expect("schema version");
    let sub = test_submission();
    let co = test_company();

    let doc = build_sections(&empty_report, &sub, &co, period).expect("build_sections empty");
    assert_eq!(doc.total_plata_a, 0, "empty period: totalPlata_A = 0");
    assert!(doc.op1_list.is_empty(), "empty period: no op1");
    assert!(doc.rezumat1_list.is_empty(), "empty period: no rezumat1");
    assert!(doc.rezumat2_list.is_empty(), "empty period: no rezumat2");

    let xml = generate_d394_xml(&doc, &sub, &co, &ver).expect("generate empty");
    assert!(xml.contains("declaratie394"), "must contain root element");
    assert!(
        xml.contains("totalPlata_A=\"0\""),
        "must have totalPlata_A=0"
    );
    assert!(xml.contains("<informatii "), "must have informatii");
    assert!(
        !xml.contains("<op1 ") && !xml.contains("<op1\n"),
        "empty: no op1"
    );
}

/// Validate that the empty-period XML also passes the XSD (if available).
#[test]
fn d394_empty_period_xsd_valid() {
    let xsd_path = Path::new("tools/anaf/sample_d394.xml");
    if !xsd_path.exists() || !xmllint_available() {
        return;
    }

    let empty_report = D394Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2025-09-01".to_string(),
        period_to: "2025-09-30".to_string(),
        partners: vec![],
        total_base: "0.00".to_string(),
        total_vat: "0.00".to_string(),
        invoice_count: 0,
        purchase_partners: vec![],
        total_purchase_base: "0.00".to_string(),
        total_purchase_vat: "0.00".to_string(),
        purchase_invoice_count: 0,
        purchase_unparsed_count: 0,
    };

    let period = chrono::NaiveDate::from_ymd_opt(2025, 9, 1).expect("test date");
    let ver = resolve(DeclKind::D394, period).expect("schema version");
    let sub = test_submission();
    let co = test_company();

    let doc = build_sections(&empty_report, &sub, &co, period).unwrap();
    let xml = generate_d394_xml(&doc, &sub, &co, &ver).unwrap();

    let tmp = std::env::temp_dir().join("d394_empty_xsd_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");
    let result = validate_with_xsd(xsd_path, &tmp).unwrap();
    let _ = std::fs::remove_file(&tmp);

    if !result.passed {
        eprintln!("Empty-period D394 XSD FAILED:");
        for e in &result.errors {
            eprintln!("  {e}");
        }
    }
    assert!(
        result.passed,
        "Empty D394 must pass XSD. Errors:\n{}",
        result.errors.join("\n")
    );
}

/// Totals reconciliation: totalPlata_A = nrCui1+nrCui2+nrCui3+nrCui4 + Σrezumat2.bases
#[test]
fn d394_totals_reconciliation() {
    let period = chrono::NaiveDate::from_ymd_opt(2025, 9, 1).expect("test date");
    let ver = resolve(DeclKind::D394, period).expect("schema version");

    let report = D394Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2025-09-01".to_string(),
        period_to: "2025-09-30".to_string(),
        partners: vec![D394Partner {
            // Valid CUI: 98765438
            partner_cui: "RO98765438".to_string(),
            partner_name: "SC CLIENT SRL".to_string(),
            vat_category: "S".to_string(),
            vat_rate: "19".to_string(),
            invoice_count: 3,
            base: "10000.00".to_string(),
            vat: "1900.00".to_string(),
            art331_code: None,
        }],
        total_base: "10000.00".to_string(),
        total_vat: "1900.00".to_string(),
        invoice_count: 3,
        purchase_partners: vec![D394Partner {
            // Valid CUI: 11111110
            partner_cui: "RO11111110".to_string(),
            partner_name: "SC FURNIZOR SRL".to_string(),
            vat_category: "S".to_string(),
            vat_rate: "19".to_string(),
            invoice_count: 2,
            base: "8000.00".to_string(),
            vat: "1520.00".to_string(),
            art331_code: None,
        }],
        total_purchase_base: "8000.00".to_string(),
        total_purchase_vat: "1520.00".to_string(),
        purchase_invoice_count: 2,
        purchase_unparsed_count: 0,
    };

    let sub = test_submission();
    let co = test_company();
    let doc = build_sections(&report, &sub, &co, period).unwrap();

    // nrCui1 = 2 (98765438 + 11111110)
    assert_eq!(
        doc.informatii.nr_cui1, 2,
        "nrCui1 = 2 distinct VAT-registered CUIs"
    );

    // totalPlata_A = nrCui1+nrCui2+nrCui3+nrCui4 + Σrezumat2(bazaL+bazaA+bazaAI)
    let rezumat2_sum: i64 = doc
        .rezumat2_list
        .iter()
        .map(|r| r.baza_l + r.baza_a + r.baza_ai)
        .sum();
    let expected = doc.informatii.nr_cui1
        + doc.informatii.nr_cui2
        + doc.informatii.nr_cui3
        + doc.informatii.nr_cui4
        + rezumat2_sum;
    assert_eq!(doc.total_plata_a, expected, "totalPlata_A formula");

    let xml = generate_d394_xml(&doc, &sub, &co, &ver).unwrap();
    let tp_str = format!("totalPlata_A=\"{}\"", doc.total_plata_a);
    assert!(
        xml.contains(&tp_str),
        "XML must reflect correct totalPlata_A"
    );
}

// ── Art. 331 codPR=29 fixture — XSD + DUK gate ────────────────────────────────

/// Generate a D394 with an AE sale using art331_code="29" (telefoane).
/// The XML must contain codPR=29 (not 22) AND must pass the official XSD.
/// When EFACTURA_DUMP_DIR is set, also dumps for external DUK validation.
///
/// This test is the primary acceptance gate for the art331_code feature.
#[test]
fn d394_codpr29_validates_against_xsd() {
    let xsd_path = std::path::Path::new("tools/anaf/sample_d394.xml");

    let period = chrono::NaiveDate::from_ymd_opt(2025, 9, 1).expect("test date");
    let ver = resolve(DeclKind::D394, period).expect("schema version");
    let sub = test_submission();
    let co = test_company();

    // Report with AE sale coded as telefoane (29), valid CUI: 76543210
    let report = D394Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2025-09-01".to_string(),
        period_to: "2025-09-30".to_string(),
        partners: vec![
            // Normal 19% sale
            D394Partner {
                partner_cui: "RO98765438".to_string(),
                partner_name: "SC CLIENT MARE SRL".to_string(),
                vat_category: "S".to_string(),
                vat_rate: "19".to_string(),
                invoice_count: 3,
                base: "5000.00".to_string(),
                vat: "950.00".to_string(),
                art331_code: None,
            },
            // AE reverse-charge with art331_code=29 (telefoane)
            D394Partner {
                partner_cui: "RO76543210".to_string(),
                partner_name: "SC TELEFONIE SRL".to_string(),
                vat_category: "AE".to_string(),
                vat_rate: "0".to_string(),
                invoice_count: 2,
                base: "3000.00".to_string(),
                vat: "0.00".to_string(),
                art331_code: Some("29".to_string()),
            },
        ],
        total_base: "8000.00".to_string(),
        total_vat: "950.00".to_string(),
        invoice_count: 5,
        purchase_partners: vec![],
        total_purchase_base: "0.00".to_string(),
        total_purchase_vat: "0.00".to_string(),
        purchase_invoice_count: 0,
        purchase_unparsed_count: 0,
    };

    let doc = build_sections(&report, &sub, &co, period).expect("build_sections");
    let xml = generate_d394_xml(&doc, &sub, &co, &ver).expect("generate_d394_xml");

    // Verify codPR=29 appears in the XML (not 22)
    assert!(
        xml.contains("codPR=\"29\""),
        "D394 XML must contain codPR=\"29\" for the telefoane AE partner\n\nXML:\n{xml}"
    );

    eprintln!("Generated D394 (codPR=29) XML ({} bytes):", xml.len());
    eprintln!("{xml}");

    // Dump for external DUK validation when EFACTURA_DUMP_DIR is set
    if let Ok(dump_dir) = std::env::var("EFACTURA_DUMP_DIR") {
        let dump_path = std::path::Path::new(&dump_dir).join("d394_codpr29.xml");
        std::fs::write(&dump_path, xml.as_bytes()).expect("write dump XML");
        eprintln!("DUMP: wrote D394 (codPR=29) XML to {:?}", dump_path);
    }

    // XSD validation (skip gracefully when XSD absent)
    if !xsd_path.exists() {
        eprintln!("SKIP XSD gate: XSD not found");
        return;
    }
    if !efactura_desktop_lib::anaf_decl::validation::xmllint_available() {
        eprintln!("SKIP XSD gate: xmllint not available");
        return;
    }

    let tmp = std::env::temp_dir().join("d394_codpr29_xsd_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");

    let result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("validate_with_xsd");

    if !result.passed {
        eprintln!("XSD VALIDATION FAILED (codPR=29):");
        for e in &result.errors {
            eprintln!("  {e}");
        }
    } else {
        eprintln!("XSD VALIDATION PASSED (codPR=29)");
    }

    let _ = std::fs::remove_file(&tmp);

    assert!(
        result.passed,
        "D394 with codPR=29 failed XSD validation. Errors:\n{}",
        result.errors.join("\n")
    );
}
