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

#[allow(unused_imports)]
use std::path::PathBuf;

// ── Helpers ────────────────────────────────────────────────────────────────────

fn test_company() -> Company {
    Company {
        id: "test-co-id".to_string(),
        cui: "RO12345674".to_string(), // valid CUI: base=1234567, check=4
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
        // nr_evid = "0" → map_to_rows will generate a valid 23-char NDP automatically
        // (DUK R25 requires a structurally valid NDP, not just any 23-char string).
        nr_evid: "0".to_string(),
        ..Default::default()
    }
}

fn test_report() -> D300Report {
    // Synthetic fiscal data: sales at 21% + 11%, purchases at 21%
    D300Report {
        company_cui: "RO12345674".to_string(),
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
    // Use a 2026 period so version.rs resolves v12 (matching the vendored sample_d300_v12.xml XSD).
    let period = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("test date");
    let ver = resolve(DeclKind::D300, period).expect("schema version");

    let report = test_report();
    let submission = test_submission();
    let company = test_company();

    let rows = map_to_rows(&report, &submission, &company, period)
        .expect("map_to_rows must not fail on valid input");

    let xml = generate_d300_xml(&rows, &ver).expect("generate_d300_xml must not fail");

    eprintln!("Generated D300 XML ({} bytes):", xml.len());
    eprintln!("{xml}");

    // TEMPORARY: dump XML for DUK validation if EFACTURA_DUMP_DIR is set
    if let Ok(dump_dir) = std::env::var("EFACTURA_DUMP_DIR") {
        let dump_path = std::path::Path::new(&dump_dir).join("d300.xml");
        std::fs::write(&dump_path, xml.as_bytes()).expect("write dump XML");
        eprintln!("DUMP: wrote D300 XML to {:?}", dump_path);
    }

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
        company_cui: "RO12345674".to_string(),
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

    // Use 2026 period to resolve v12 (vendored XSD)
    let period = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("test date");
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

// ── Wave 4 integration tests — XSD + DUK validation ──────────────────────────

/// Helper: run DUK validation on an XML string.
/// Returns Ok(true) if "Validare fara erori", Ok(false) if errors/warnings,
/// Err if the DUK jar is not available.
fn run_duk(xml: &str, label: &str) -> Result<bool, String> {
    let jar = std::path::Path::new("/tmp/dukrun/DUKIntegrator.jar");
    if !jar.exists() {
        return Err(format!("DUK jar not found at {jar:?}"));
    }
    let java = "/opt/homebrew/opt/openjdk@17/bin/java";
    if !std::path::Path::new(java).exists() {
        return Err(format!("Java not found at {java}"));
    }

    let tmp_dir = std::env::temp_dir().join(format!("d300_duk_{label}"));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let xml_path = tmp_dir.join("d300.xml");
    let result_path = tmp_dir.join("result.txt");
    std::fs::write(&xml_path, xml.as_bytes()).map_err(|e| e.to_string())?;

    let output = std::process::Command::new(java)
        .args([
            "-jar",
            jar.to_str().unwrap(),
            "-v",
            "D300",
            xml_path.to_str().unwrap(),
            result_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| e.to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result_content = std::fs::read_to_string(&result_path).unwrap_or_default();

    eprintln!("DUK [{label}] stdout: {stdout}");
    eprintln!("DUK [{label}] result: {result_content}");

    // DUK success: stdout contains "Validare fara erori" AND result file = "ok"
    let passed = stdout.contains("Validare fara erori") && result_content.trim() == "ok";
    Ok(passed)
}

/// WAVE 4 SCENARIO A: Domestic reverse charge (AE) — buyer model.
///
/// AE invoice at 21%: base=1000, VAT=210.
/// Expected: R12 collected + R25 deductible (equal), R17/R27 include them.
/// Must pass XSD AND DUK ("Validare fara erori").
#[test]
fn d300_wave4_scenario_a_reverse_charge_ae() {
    let xsd_path = std::path::Path::new("tools/anaf/sample_d300_v12.xml");
    if !xsd_path.exists() {
        eprintln!("SKIP: XSD not found");
        return;
    }
    if !efactura_desktop_lib::anaf_decl::validation::xmllint_available() {
        eprintln!("SKIP: xmllint not available");
        return;
    }

    let period = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("date");
    let ver = resolve(DeclKind::D300, period).expect("version");

    // AE invoice in purchase_groups (buyer model: AE goes in purchase side)
    let report = D300Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2026-01-01".to_string(),
        period_to: "2026-01-31".to_string(),
        groups: vec![],
        total_base: "0.00".to_string(),
        total_vat: "0.00".to_string(),
        invoice_count: 0,
        purchase_groups: vec![D300Group {
            vat_rate: "0.21".to_string(),
            vat_category: "AE".to_string(),
            base: "1000.00".to_string(),
            vat: "210.00".to_string(),
        }],
        total_deductible_base: "1000.00".to_string(),
        total_deductible_vat: "210.00".to_string(),
        purchase_invoice_count: 1,
        purchase_unparsed_count: 0,
        net_vat: "0.00".to_string(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period).expect("map_to_rows");

    // Verify AE mapping
    assert_eq!(rows.r12_1, Some(1000), "Scenario A: R12_1=1000");
    assert_eq!(rows.r12_2, Some(210), "Scenario A: R12_2=210");
    assert_eq!(
        rows.r12_1_1,
        Some(1000),
        "Scenario A: R12_1_1=1000 (21% sub)"
    );
    assert_eq!(rows.r12_1_2, Some(210), "Scenario A: R12_1_2=210 (21% VAT)");
    assert_eq!(rows.r25_1, rows.r12_1, "Scenario A: R25_1=R12_1 (DUK V_19)");
    assert_eq!(rows.r25_2, rows.r12_2, "Scenario A: R25_2=R12_2 (DUK V_20)");
    assert_eq!(
        rows.r25_1_1, rows.r12_1_1,
        "Scenario A: R25_1_1=R12_1_1 (DUK V_21)"
    );
    assert_eq!(
        rows.r25_1_2, rows.r12_1_2,
        "Scenario A: R25_1_2=R12_1_2 (DUK V_22)"
    );
    assert_eq!(rows.r17_2, Some(210), "Scenario A: R17_2 includes R12_2");
    assert_eq!(rows.r27_2, Some(210), "Scenario A: R27_2 includes R25_2");
    // Net VAT = 0 (collected = deductible)
    assert_eq!(rows.r34_2, None, "Scenario A: no net payable");
    assert_eq!(
        rows.r13_1, None,
        "Scenario A: no seller-side R13_1 (buyer model)"
    );

    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    eprintln!("Scenario A XML:\n{xml}");

    // DUMP for DUK
    if let Ok(dump_dir) = std::env::var("EFACTURA_DUMP_DIR") {
        let path = std::path::Path::new(&dump_dir).join("d300_scenario_a.xml");
        std::fs::write(&path, xml.as_bytes()).expect("write dump");
        eprintln!("DUMP: scenario A → {:?}", path);
    }

    // XSD validation
    let tmp = std::env::temp_dir().join("d300_wave4_a.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write tmp");
    let xsd_result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("xmllint");
    if !xsd_result.passed {
        for e in &xsd_result.errors {
            eprintln!("XSD error: {e}");
        }
    }
    assert!(xsd_result.passed, "Scenario A must pass XSD validation");
    let _ = std::fs::remove_file(&tmp);

    // DUK validation
    match run_duk(&xml, "scenario_a") {
        Ok(passed) => assert!(passed, "Scenario A: DUK must say 'Validare fara erori'"),
        Err(e) => eprintln!("SKIP DUK Scenario A: {e}"),
    }
}

/// WAVE 4 SCENARIO B: Intra-EU acquisition (category K, goods).
///
/// K purchase at 21%: base=2000, VAT=420.
/// Expected: R5=R18 (goods, equal), included in R17 and R27 totals.
/// Must pass XSD AND DUK ("Validare fara erori").
#[test]
fn d300_wave4_scenario_b_intra_eu_k_purchase() {
    let xsd_path = std::path::Path::new("tools/anaf/sample_d300_v12.xml");
    if !xsd_path.exists() {
        eprintln!("SKIP: XSD not found");
        return;
    }
    if !efactura_desktop_lib::anaf_decl::validation::xmllint_available() {
        eprintln!("SKIP: xmllint not available");
        return;
    }

    let period = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("date");
    let ver = resolve(DeclKind::D300, period).expect("version");

    let report = D300Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2026-01-01".to_string(),
        period_to: "2026-01-31".to_string(),
        groups: vec![],
        total_base: "0.00".to_string(),
        total_vat: "0.00".to_string(),
        invoice_count: 0,
        purchase_groups: vec![D300Group {
            vat_rate: "0.21".to_string(),
            vat_category: "K".to_string(),
            base: "2000.00".to_string(),
            vat: "420.00".to_string(),
        }],
        total_deductible_base: "2000.00".to_string(),
        total_deductible_vat: "420.00".to_string(),
        purchase_invoice_count: 1,
        purchase_unparsed_count: 0,
        net_vat: "0.00".to_string(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period).expect("map_to_rows");

    assert_eq!(rows.r5_1, Some(2000), "Scenario B: R5_1=2000");
    assert_eq!(rows.r5_2, Some(420), "Scenario B: R5_2=420");
    assert_eq!(rows.r18_1, Some(2000), "Scenario B: R18_1=R5_1 (DUK V_7)");
    assert_eq!(rows.r18_2, Some(420), "Scenario B: R18_2=R5_2 (DUK V_8)");
    assert_eq!(rows.r17_2, Some(420), "Scenario B: R17_2 includes R5_2");
    assert_eq!(rows.r27_2, Some(420), "Scenario B: R27_2 includes R18_2");
    assert_eq!(rows.r34_2, None, "Scenario B: no net payable");
    assert_eq!(rows.r33_2, None, "Scenario B: no net refund");

    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    eprintln!("Scenario B XML:\n{xml}");

    if let Ok(dump_dir) = std::env::var("EFACTURA_DUMP_DIR") {
        let path = std::path::Path::new(&dump_dir).join("d300_scenario_b.xml");
        std::fs::write(&path, xml.as_bytes()).expect("write dump");
        eprintln!("DUMP: scenario B → {:?}", path);
    }

    let tmp = std::env::temp_dir().join("d300_wave4_b.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write tmp");
    let xsd_result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("xmllint");
    assert!(
        xsd_result.passed,
        "Scenario B must pass XSD: {:?}",
        xsd_result.errors
    );
    let _ = std::fs::remove_file(&tmp);

    match run_duk(&xml, "scenario_b") {
        Ok(passed) => assert!(passed, "Scenario B: DUK must say 'Validare fara erori'"),
        Err(e) => eprintln!("SKIP DUK Scenario B: {e}"),
    }
}

/// WAVE 4 SCENARIO C: Multi-rate sales 21% + 11% + 9%.
///
/// Sales: 1000@21% (VAT 210) + 500@11% (VAT 55) + 200@9% (VAT 18).
/// Expected: R9/R10/R11 correctly populated; DUK margin checks pass.
/// Must pass XSD AND DUK ("Validare fara erori").
#[test]
fn d300_wave4_scenario_c_multirate_sales() {
    let xsd_path = std::path::Path::new("tools/anaf/sample_d300_v12.xml");
    if !xsd_path.exists() {
        eprintln!("SKIP: XSD not found");
        return;
    }
    if !efactura_desktop_lib::anaf_decl::validation::xmllint_available() {
        eprintln!("SKIP: xmllint not available");
        return;
    }

    let period = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("date");
    let ver = resolve(DeclKind::D300, period).expect("version");

    let report = D300Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2026-01-01".to_string(),
        period_to: "2026-01-31".to_string(),
        groups: vec![
            D300Group {
                vat_rate: "0.21".to_string(),
                vat_category: "S".to_string(),
                base: "1000.00".to_string(),
                vat: "210.00".to_string(),
            },
            D300Group {
                vat_rate: "0.11".to_string(),
                vat_category: "S".to_string(),
                base: "500.00".to_string(),
                vat: "55.00".to_string(),
            },
            D300Group {
                vat_rate: "0.09".to_string(),
                vat_category: "S".to_string(),
                base: "200.00".to_string(),
                vat: "18.00".to_string(),
            },
        ],
        total_base: "1700.00".to_string(),
        total_vat: "283.00".to_string(),
        invoice_count: 3,
        purchase_groups: vec![],
        total_deductible_base: "0.00".to_string(),
        total_deductible_vat: "0.00".to_string(),
        purchase_invoice_count: 0,
        purchase_unparsed_count: 0,
        net_vat: "283.00".to_string(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period).expect("map_to_rows");

    assert_eq!(rows.r9_1, Some(1000), "Scenario C: R9_1=1000 (21%)");
    assert_eq!(rows.r9_2, Some(210), "Scenario C: R9_2=210 (21% VAT)");
    assert_eq!(rows.r10_1, Some(500), "Scenario C: R10_1=500 (11%)");
    assert_eq!(rows.r10_2, Some(55), "Scenario C: R10_2=55 (11% VAT)");
    assert_eq!(rows.r11_1, Some(200), "Scenario C: R11_1=200 (9%)");
    assert_eq!(rows.r11_2, Some(18), "Scenario C: R11_2=18 (9% VAT)");
    assert_eq!(rows.r17_2, Some(283), "Scenario C: R17_2=283");
    assert_eq!(rows.r34_2, Some(283), "Scenario C: R34_2=283 (de plată)");

    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    eprintln!("Scenario C XML:\n{xml}");

    if let Ok(dump_dir) = std::env::var("EFACTURA_DUMP_DIR") {
        let path = std::path::Path::new(&dump_dir).join("d300_scenario_c.xml");
        std::fs::write(&path, xml.as_bytes()).expect("write dump");
        eprintln!("DUMP: scenario C → {:?}", path);
    }

    let tmp = std::env::temp_dir().join("d300_wave4_c.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write tmp");
    let xsd_result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("xmllint");
    assert!(
        xsd_result.passed,
        "Scenario C must pass XSD: {:?}",
        xsd_result.errors
    );
    let _ = std::fs::remove_file(&tmp);

    match run_duk(&xml, "scenario_c") {
        Ok(passed) => assert!(passed, "Scenario C: DUK must say 'Validare fara erori'"),
        Err(e) => eprintln!("SKIP DUK Scenario C: {e}"),
    }
}

/// Scenario D — a 9% DEDUCTIBLE (purchase) operation must be EXCLUDED from R23.
/// R23 is the 11% deductible row; its DUK corridor (rule R86) is 10–12%, so a 9%
/// purchase placed there fails validation AND misclassifies the rate. There is no
/// 9%-deductible row in XSD v1.02, so the generator excludes it (+ preflight warns).
/// This test locks that: R23 stays empty and the XML still DUK-validates.
#[test]
fn d300_wave4_scenario_d_9pct_purchase_excluded() {
    let xsd_path = std::path::Path::new("tools/anaf/sample_d300_v12.xml");
    if !xsd_path.exists() {
        eprintln!("SKIP: XSD not found");
        return;
    }
    if !efactura_desktop_lib::anaf_decl::validation::xmllint_available() {
        eprintln!("SKIP: xmllint not available");
        return;
    }

    let period = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("date");
    let ver = resolve(DeclKind::D300, period).expect("version");

    let report = D300Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2026-01-01".to_string(),
        period_to: "2026-01-31".to_string(),
        // One ordinary 21% sale so the period isn't empty.
        groups: vec![D300Group {
            vat_rate: "0.21".to_string(),
            vat_category: "S".to_string(),
            base: "1000.00".to_string(),
            vat: "210.00".to_string(),
        }],
        total_base: "1000.00".to_string(),
        total_vat: "210.00".to_string(),
        invoice_count: 1,
        // A 9% domestic PURCHASE — has no valid v12 deductible row.
        purchase_groups: vec![D300Group {
            vat_rate: "0.09".to_string(),
            vat_category: "S".to_string(),
            base: "1000.00".to_string(),
            vat: "90.00".to_string(),
        }],
        total_deductible_base: "1000.00".to_string(),
        total_deductible_vat: "90.00".to_string(),
        purchase_invoice_count: 1,
        purchase_unparsed_count: 0,
        net_vat: "120.00".to_string(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period).expect("map_to_rows");

    // The 9% purchase must NOT land in R23 (the 11% row) — it is excluded.
    assert_eq!(
        rows.r23_1, None,
        "Scenario D: 9% purchase must NOT populate R23_1"
    );
    assert_eq!(
        rows.r23_2, None,
        "Scenario D: 9% purchase must NOT populate R23_2"
    );

    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    let tmp = std::env::temp_dir().join("d300_wave4_d.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write tmp");
    let xsd_result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("xmllint");
    assert!(
        xsd_result.passed,
        "Scenario D must pass XSD: {:?}",
        xsd_result.errors
    );
    let _ = std::fs::remove_file(&tmp);

    // DUK must stay clean precisely because the 9% purchase was excluded (not in R23).
    match run_duk(&xml, "scenario_d") {
        Ok(passed) => assert!(
            passed,
            "Scenario D: DUK must say 'Validare fara erori' (9% purchase excluded, not in R23)"
        ),
        Err(e) => eprintln!("SKIP DUK Scenario D: {e}"),
    }
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
    // Use 2026 period so version.rs resolves v12 (matching the vendored XSD)
    let period = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("test date");
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
    // totalPlata_A = control sum of all populated R-rows (DUK R26 — NOT R41_2 alone)
    // Populated: r9_1=10000, r9_2=2100, r10_1=5000, r10_2=550,
    //   r17_1=15000, r17_2=2650, r22_1=8000, r22_2=1680,
    //   r27_1=8000, r27_2=1680, r28_2=1680, r32_2=1680, r34_2=970, r37_2=970, r41_2=970
    // sum = 60930
    assert_eq!(
        rows.total_plata_a, 60930,
        "totalPlata_A = control sum 60930"
    );

    // Also verify the XML string contains key computed values
    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    assert!(xml.contains("R17_2=\"2650\""), "XML R17_2");
    assert!(xml.contains("R34_2=\"970\""), "XML R34_2");
    assert!(xml.contains("totalPlata_A=\"60930\""), "XML totalPlata_A");
}
