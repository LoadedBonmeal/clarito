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
use efactura_desktop_lib::commands::declarations::{CashVatMemo, D300Group, D300Report};
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
        cash_vat: false,
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
        tax_regime: "micro".into(),
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
                intra_eu_kind: None,
            },
            D300Group {
                vat_rate: "0.11".to_string(),
                vat_category: "S".to_string(),
                base: "5000.00".to_string(),
                vat: "550.00".to_string(),
                intra_eu_kind: None,
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
            intra_eu_kind: None,
        }],
        total_deductible_base: "8000.00".to_string(),
        total_deductible_vat: "1680.00".to_string(),
        purchase_invoice_count: 7,
        purchase_unparsed_count: 0,
        net_vat: "970.00".to_string(),
        reg_colectata_baza: "0.00".to_string(),
        reg_colectata_tva: "0.00".to_string(),
        reg_dedusa_baza: "0.00".to_string(),
        reg_dedusa_tva: "0.00".to_string(),
        // Non-zero informational TVA-neexigibilă memo rows A/A1/B/B1 — exercises their XSD emission.
        cash_vat_memo: CashVatMemo {
            a_base: 10000,
            a_vat: 2100,
            a1_base: 10000,
            a1_vat: 2100,
            b_base: 5000,
            b_vat: 1050,
            b1_base: 5000,
            b1_vat: 1050,
        },
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

    // Non-zero art. 305 capital-goods adjustment → exercises R31_2 emission + R32 inclusion under XSD.
    let rows = map_to_rows(&report, &submission, &company, period, -500)
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
        reg_colectata_baza: "0.00".to_string(),
        reg_colectata_tva: "0.00".to_string(),
        reg_dedusa_baza: "0.00".to_string(),
        reg_dedusa_tva: "0.00".to_string(),
        cash_vat_memo: Default::default(),
    };

    // Use 2026 period to resolve v12 (vendored XSD)
    let period = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("test date");
    let ver = resolve(DeclKind::D300, period).expect("schema version");
    let sub = test_submission();
    let co = test_company();

    let rows = map_to_rows(&empty_report, &sub, &co, period, 0).expect("map_to_rows empty");
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
            intra_eu_kind: None,
        }],
        total_deductible_base: "1000.00".to_string(),
        total_deductible_vat: "210.00".to_string(),
        purchase_invoice_count: 1,
        purchase_unparsed_count: 0,
        net_vat: "0.00".to_string(),
        reg_colectata_baza: "0.00".to_string(),
        reg_colectata_tva: "0.00".to_string(),
        reg_dedusa_baza: "0.00".to_string(),
        reg_dedusa_tva: "0.00".to_string(),
        cash_vat_memo: Default::default(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period, 0).expect("map_to_rows");

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

/// WAVE 4 SCENARIO E: company is BOTH a reverse-charge buyer AND seller on the same cota (21%).
/// AE purchase (self-assessed VAT) → R12+R25; AE sale (VAT 0) → R13. They must NOT collide:
/// R12 = the purchase only (1000/210), R13 = the seller base (500), R17_1 = 1500 (no double-count).
#[test]
fn d300_wave4_scenario_e_reverse_charge_buyer_and_seller() {
    let xsd_path = std::path::Path::new("tools/anaf/sample_d300_v12.xml");
    if !xsd_path.exists() || !efactura_desktop_lib::anaf_decl::validation::xmllint_available() {
        eprintln!("SKIP scenario E: XSD or xmllint absent");
        return;
    }
    let period = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("date");
    let ver = resolve(DeclKind::D300, period).expect("version");

    let report = D300Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2026-01-01".to_string(),
        period_to: "2026-01-31".to_string(),
        groups: vec![D300Group {
            vat_rate: "0.21".to_string(),
            vat_category: "AE".to_string(),
            base: "500.00".to_string(),
            vat: "0.00".to_string(), // seller leg: VAT 0 → R13
            intra_eu_kind: None,
        }],
        total_base: "500.00".to_string(),
        total_vat: "0.00".to_string(),
        invoice_count: 1,
        purchase_groups: vec![D300Group {
            vat_rate: "0.21".to_string(),
            vat_category: "AE".to_string(),
            base: "1000.00".to_string(),
            vat: "210.00".to_string(), // buyer leg: self-assessed → R12+R25
            intra_eu_kind: None,
        }],
        total_deductible_base: "1000.00".to_string(),
        total_deductible_vat: "210.00".to_string(),
        purchase_invoice_count: 1,
        purchase_unparsed_count: 0,
        net_vat: "0.00".to_string(),
        reg_colectata_baza: "0.00".to_string(),
        reg_colectata_tva: "0.00".to_string(),
        reg_dedusa_baza: "0.00".to_string(),
        reg_dedusa_tva: "0.00".to_string(),
        cash_vat_memo: Default::default(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period, 0).expect("map_to_rows");
    assert_eq!(rows.r12_1, Some(1000), "E: R12_1 = purchase only");
    assert_eq!(rows.r12_2, Some(210), "E: R12_2 = self-assessed VAT");
    assert_eq!(rows.r13_1, Some(500), "E: R13_1 = seller base");
    assert_eq!(
        rows.r17_1,
        Some(1500),
        "E: R17_1 = R12_1 + R13_1, no double-count"
    );

    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    let tmp = std::env::temp_dir().join("d300_wave4_e.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write tmp");
    let xsd_result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("xmllint");
    let _ = std::fs::remove_file(&tmp);
    assert!(
        xsd_result.passed,
        "Scenario E must pass XSD: {}",
        xsd_result.errors.join("; ")
    );
    match run_duk(&xml, "scenario_e") {
        Ok(passed) => assert!(passed, "Scenario E: DUK must validate"),
        Err(e) => eprintln!("SKIP DUK Scenario E: {e}"),
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
            intra_eu_kind: Some("goods".to_string()),
        }],
        total_deductible_base: "2000.00".to_string(),
        total_deductible_vat: "420.00".to_string(),
        purchase_invoice_count: 1,
        purchase_unparsed_count: 0,
        net_vat: "0.00".to_string(),
        reg_colectata_baza: "0.00".to_string(),
        reg_colectata_tva: "0.00".to_string(),
        reg_dedusa_baza: "0.00".to_string(),
        reg_dedusa_tva: "0.00".to_string(),
        cash_vat_memo: Default::default(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period, 0).expect("map_to_rows");

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
                intra_eu_kind: None,
            },
            D300Group {
                vat_rate: "0.11".to_string(),
                vat_category: "S".to_string(),
                base: "500.00".to_string(),
                vat: "55.00".to_string(),
                intra_eu_kind: None,
            },
            D300Group {
                vat_rate: "0.09".to_string(),
                vat_category: "S".to_string(),
                base: "200.00".to_string(),
                vat: "18.00".to_string(),
                intra_eu_kind: None,
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
        reg_colectata_baza: "0.00".to_string(),
        reg_colectata_tva: "0.00".to_string(),
        reg_dedusa_baza: "0.00".to_string(),
        reg_dedusa_tva: "0.00".to_string(),
        cash_vat_memo: Default::default(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period, 0).expect("map_to_rows");

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
            intra_eu_kind: None,
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
            intra_eu_kind: None,
        }],
        total_deductible_base: "1000.00".to_string(),
        total_deductible_vat: "90.00".to_string(),
        purchase_invoice_count: 1,
        purchase_unparsed_count: 0,
        net_vat: "120.00".to_string(),
        // Wave 8: 9% S purchase auto-included in regularizări R30.
        reg_colectata_baza: "0.00".to_string(),
        reg_colectata_tva: "0.00".to_string(),
        reg_dedusa_baza: "1000.00".to_string(),
        reg_dedusa_tva: "90.00".to_string(),
        cash_vat_memo: Default::default(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period, 0).expect("map_to_rows");

    // The 9% purchase must NOT land in R23 (the 11% row) — it goes to R30 instead (Wave 8).
    assert_eq!(
        rows.r23_1, None,
        "Scenario D: 9% purchase must NOT populate R23_1"
    );
    assert_eq!(
        rows.r23_2, None,
        "Scenario D: 9% purchase must NOT populate R23_2"
    );
    // Wave 8: 9% purchase flows into R30 (regularizări dedusă) instead.
    assert_eq!(
        rows.r30_1,
        Some(1000),
        "Scenario D (Wave 8): 9% purchase must populate R30_1=1000"
    );
    assert_eq!(
        rows.r30_2,
        Some(90),
        "Scenario D (Wave 8): 9% purchase must populate R30_2=90"
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

    // DUK must stay clean: 9% purchase is in R30 (regularizări, no margin corridor).
    match run_duk(&xml, "scenario_d") {
        Ok(passed) => assert!(
            passed,
            "Scenario D: DUK must say 'Validare fara erori' (9% purchase in R30, not R23)"
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

    let rows = map_to_rows(&report, &sub, &co, period, 0).expect("map_to_rows");

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

/// WAVE 7 SCENARIO: Intra-EU acquisition of SERVICES (category K, intra_eu_kind=services).
///
/// K purchase with intra_eu_kind=services: base=3000, VAT=630 (21%).
/// Expected:
///   - R7_1=3000, R7_2=630 (collected leg of services reverse charge)
///   - R20_1=3000, R20_2=630 (deductible leg; DUK V_13/V_14: R20=R7)
///   - R5_1/R5_2 must be None (goods row empty — this is a services acquisition)
///   - R18_1/R18_2 must be None (goods deductible row empty)
///   - R17_2=630 (includes R7_2), R27_2=630 (includes R20_2)
///   - Net VAT = 0 (collected = deductible for pure K services acquisition)
///   - Must pass XSD AND DUK ("Validare fara erori").
#[test]
fn d300_wave7_intra_eu_services() {
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

    // K purchase with intra_eu_kind=services (e.g. SaaS from EU vendor)
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
            base: "3000.00".to_string(),
            vat: "630.00".to_string(),
            intra_eu_kind: Some("services".to_string()),
        }],
        total_deductible_base: "3000.00".to_string(),
        total_deductible_vat: "630.00".to_string(),
        purchase_invoice_count: 1,
        purchase_unparsed_count: 0,
        net_vat: "0.00".to_string(),
        reg_colectata_baza: "0.00".to_string(),
        reg_colectata_tva: "0.00".to_string(),
        reg_dedusa_baza: "0.00".to_string(),
        reg_dedusa_tva: "0.00".to_string(),
        cash_vat_memo: Default::default(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period, 0).expect("map_to_rows");

    // R7/R20 populated for services
    assert_eq!(
        rows.r7_1,
        Some(3000),
        "Wave7: R7_1=3000 (services collected base)"
    );
    assert_eq!(
        rows.r7_2,
        Some(630),
        "Wave7: R7_2=630 (services collected VAT)"
    );
    assert_eq!(rows.r20_1, Some(3000), "Wave7: R20_1=R7_1=3000 (DUK V_13)");
    assert_eq!(rows.r20_2, Some(630), "Wave7: R20_2=R7_2=630 (DUK V_14)");

    // R5/R18 must be empty (this is a services acquisition, not goods)
    assert_eq!(rows.r5_1, None, "Wave7: R5_1=None (no goods)");
    assert_eq!(rows.r5_2, None, "Wave7: R5_2=None (no goods)");
    assert_eq!(
        rows.r18_1, None,
        "Wave7: R18_1=None (goods deductible empty)"
    );
    assert_eq!(
        rows.r18_2, None,
        "Wave7: R18_2=None (goods deductible empty)"
    );

    // Totals include R7/R20
    assert_eq!(rows.r17_2, Some(630), "Wave7: R17_2 includes R7_2");
    assert_eq!(rows.r27_2, Some(630), "Wave7: R27_2 includes R20_2");

    // Net VAT = 0 (collected == deductible for K services)
    assert_eq!(rows.r34_2, None, "Wave7: no net payable");
    assert_eq!(rows.r33_2, None, "Wave7: no net refund");

    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    eprintln!("Wave7 Services XML:\n{xml}");

    // DUMP for DUK
    if let Ok(dump_dir) = std::env::var("EFACTURA_DUMP_DIR") {
        let path = std::path::Path::new(&dump_dir).join("d300_wave7_services.xml");
        std::fs::write(&path, xml.as_bytes()).expect("write dump");
        eprintln!("DUMP: wave7 services → {:?}", path);
    }

    // XML must contain R7 and R20, must NOT contain R5 or R18
    assert!(xml.contains("R7_1=\"3000\""), "XML must have R7_1=3000");
    assert!(xml.contains("R7_2=\"630\""), "XML must have R7_2=630");
    assert!(xml.contains("R20_1=\"3000\""), "XML must have R20_1=3000");
    assert!(xml.contains("R20_2=\"630\""), "XML must have R20_2=630");
    assert!(
        !xml.contains("R5_1="),
        "XML must NOT have R5 (services, not goods)"
    );
    assert!(
        !xml.contains("R18_1="),
        "XML must NOT have R18 (goods deductible empty)"
    );

    // XSD validation
    let tmp = std::env::temp_dir().join("d300_wave7_services.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write tmp");
    let xsd_result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("xmllint");
    if !xsd_result.passed {
        for e in &xsd_result.errors {
            eprintln!("XSD error: {e}");
        }
    }
    assert!(
        xsd_result.passed,
        "Wave7 Services: must pass XSD validation: {:?}",
        xsd_result.errors
    );
    let _ = std::fs::remove_file(&tmp);

    // DUK validation (mandatory gate)
    match run_duk(&xml, "wave7_services") {
        Ok(passed) => assert!(passed, "Wave7 Services: DUK must say 'Validare fara erori'"),
        Err(e) => eprintln!("SKIP DUK Wave7 Services: {e}"),
    }
}

// ── Wave 3 (audit) scenario — FIX 1: Z/K/G/E sales routing ─────────────────────

/// WAVE 3 AUDIT FIX 1: sales with categories Z, K, G, E in the same period.
///
/// Prior bug: Z/K/E were all dumped into R1_1, and G was accumulated NOWHERE
/// (silently dropped from the whole declaration — a P1 finding). This scenario
/// exercises the corrected routing end-to-end (XSD + DUK):
/// - K (2000, intra-EU): R1_1 = 2000 (rd.1 = ONLY art. 294(2)(a)/(d))
/// - Z (1000) + G (3000): R14_1 = 4000 (rd.14 scutite CU drept de deducere — export
///   art. 294(1) + zero-rated; G previously vanished)
/// - E (4000, exempt): R15_1 = 4000 (rd.15 fără drept; previously wrongly in R1_1)
/// - a 21% S sale (1000/210) so the totals aren't degenerate
///
/// Must pass XSD AND DUK ("Validare fara erori").
#[test]
fn d300_wave3_audit_fix1_zkge_sales_routing() {
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
                vat_rate: "0.00".to_string(),
                vat_category: "Z".to_string(),
                base: "1000.00".to_string(),
                vat: "0.00".to_string(),
                intra_eu_kind: None,
            },
            D300Group {
                vat_rate: "0.00".to_string(),
                vat_category: "K".to_string(),
                base: "2000.00".to_string(),
                vat: "0.00".to_string(),
                intra_eu_kind: None,
            },
            D300Group {
                vat_rate: "0.00".to_string(),
                vat_category: "G".to_string(),
                base: "3000.00".to_string(),
                vat: "0.00".to_string(),
                intra_eu_kind: None,
            },
            D300Group {
                vat_rate: "0.00".to_string(),
                vat_category: "E".to_string(),
                base: "4000.00".to_string(),
                vat: "0.00".to_string(),
                intra_eu_kind: None,
            },
            D300Group {
                vat_rate: "0.21".to_string(),
                vat_category: "S".to_string(),
                base: "1000.00".to_string(),
                vat: "210.00".to_string(),
                intra_eu_kind: None,
            },
        ],
        total_base: "11000.00".to_string(),
        total_vat: "210.00".to_string(),
        invoice_count: 5,
        purchase_groups: vec![],
        total_deductible_base: "0.00".to_string(),
        total_deductible_vat: "0.00".to_string(),
        purchase_invoice_count: 0,
        purchase_unparsed_count: 0,
        net_vat: "210.00".to_string(),
        reg_colectata_baza: "0.00".to_string(),
        reg_colectata_tva: "0.00".to_string(),
        reg_dedusa_baza: "0.00".to_string(),
        reg_dedusa_tva: "0.00".to_string(),
        cash_vat_memo: Default::default(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period, 0).expect("map_to_rows");

    assert_eq!(rows.r1_1, Some(2000), "Wave3 FIX1: R1_1 = K(2000) only");
    assert_eq!(
        rows.r14_1,
        Some(4000),
        "Wave3 FIX1: R14_1 = Z(1000)+G(3000), G no longer dropped"
    );
    assert_eq!(rows.r15_1, Some(4000), "Wave3 FIX1: R15_1 = E(4000)");
    assert_eq!(rows.r9_1, Some(1000), "Wave3 FIX1: R9_1 = S(1000)");
    assert_eq!(
        rows.r3_1, None,
        "Wave3 FIX1: R3_1 unpopulated (pending sales-side K goods/services flag)"
    );
    assert_eq!(
        rows.r17_1,
        Some(11000),
        "Wave3 FIX1: R17_1 = R1_1+R14_1+R15_1+R9_1 = 2000+4000+4000+1000 (nothing dropped)"
    );
    assert_eq!(
        rows.r17_2,
        Some(210),
        "Wave3 FIX1: R17_2 = 210 (only S has VAT)"
    );

    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    eprintln!("Wave3 FIX1 ZKGE XML:\n{xml}");

    if let Ok(dump_dir) = std::env::var("EFACTURA_DUMP_DIR") {
        let path = std::path::Path::new(&dump_dir).join("d300_wave3_fix1_zkge.xml");
        std::fs::write(&path, xml.as_bytes()).expect("write dump");
        eprintln!("DUMP: wave3 fix1 zkge → {:?}", path);
    }

    assert!(xml.contains("R1_1=\"2000\""), "XML must have R1_1=2000");
    assert!(xml.contains("R14_1=\"4000\""), "XML must have R14_1=4000");
    assert!(xml.contains("R15_1=\"4000\""), "XML must have R15_1=4000");
    assert!(
        !xml.contains("R3_1="),
        "XML must NOT have R3_1 (unpopulated pending K-services flag)"
    );

    let tmp = std::env::temp_dir().join("d300_wave3_fix1_zkge.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write tmp");
    let xsd_result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("xmllint");
    if !xsd_result.passed {
        for e in &xsd_result.errors {
            eprintln!("XSD error: {e}");
        }
    }
    assert!(
        xsd_result.passed,
        "Wave3 FIX1 ZKGE must pass XSD: {:?}",
        xsd_result.errors
    );
    let _ = std::fs::remove_file(&tmp);

    match run_duk(&xml, "wave3_fix1_zkge") {
        Ok(passed) => assert!(
            passed,
            "Wave3 FIX1 ZKGE: DUK must say 'Validare fara erori'"
        ),
        Err(e) => eprintln!("SKIP DUK Wave3 FIX1 ZKGE: {e}"),
    }
}

// ── Wave 8 scenarios — old-rate regularizări (R16/R30) ─────────────────────────

/// WAVE 8 SCENARIO: 19% S SALE → R16_1/R16_2 (regularizări colectată).
///
/// A sale at 19% (old rate) base=1000, VAT=190.
/// Expected: R16_1=1000, R16_2=190; R9/R10/R11 empty; R17_2=190.
/// Must pass XSD AND DUK ("Validare fara erori").
#[test]
fn d300_wave8_old_rate_sales_to_r16() {
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

    // Old-rate 19% sale — reg_colectata_* pre-computed as if by compute_d300
    let report = D300Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2026-01-01".to_string(),
        period_to: "2026-01-31".to_string(),
        groups: vec![D300Group {
            vat_rate: "0.19".to_string(),
            vat_category: "S".to_string(),
            base: "1000.00".to_string(),
            vat: "190.00".to_string(),
            intra_eu_kind: None,
        }],
        total_base: "1000.00".to_string(),
        total_vat: "190.00".to_string(),
        invoice_count: 1,
        purchase_groups: vec![],
        total_deductible_base: "0.00".to_string(),
        total_deductible_vat: "0.00".to_string(),
        purchase_invoice_count: 0,
        purchase_unparsed_count: 0,
        net_vat: "190.00".to_string(),
        // Wave 8: auto-computed by compute_d300 from old-rate groups
        reg_colectata_baza: "1000.00".to_string(),
        reg_colectata_tva: "190.00".to_string(),
        reg_dedusa_baza: "0.00".to_string(),
        reg_dedusa_tva: "0.00".to_string(),
        cash_vat_memo: Default::default(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period, 0).expect("map_to_rows");

    // R16 populated for old-rate sales
    assert_eq!(
        rows.r16_1,
        Some(1000),
        "Wave8 sales R16_1=1000 (old 19% base)"
    );
    assert_eq!(rows.r16_2, Some(190), "Wave8 sales R16_2=190 (old 19% VAT)");

    // Current-rate rows must be empty (no 21%/11%/9% sales)
    assert_eq!(rows.r9_1, None, "Wave8: R9_1 must be None (no 21% sales)");
    assert_eq!(rows.r10_1, None, "Wave8: R10_1 must be None (no 11% sales)");
    assert_eq!(rows.r11_1, None, "Wave8: R11_1 must be None (no 9% sales)");

    // R17_2 must include R16_2
    assert_eq!(rows.r17_2, Some(190), "Wave8: R17_2 includes R16_2=190");
    assert_eq!(
        rows.r34_2,
        Some(190),
        "Wave8: R34_2=190 (TVA de plată = R17_2)"
    );

    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    eprintln!("Wave8 old-rate sales XML:\n{xml}");

    if let Ok(dump_dir) = std::env::var("EFACTURA_DUMP_DIR") {
        let path = std::path::Path::new(&dump_dir).join("d300_wave8_old_rate_sales.xml");
        std::fs::write(&path, xml.as_bytes()).expect("write dump");
        eprintln!("DUMP: wave8 old rate sales → {:?}", path);
    }

    assert!(xml.contains("R16_1=\"1000\""), "XML must have R16_1=1000");
    assert!(xml.contains("R16_2=\"190\""), "XML must have R16_2=190");
    assert!(
        !xml.contains("R9_1="),
        "XML must NOT have R9_1 (no 21% sales)"
    );

    let tmp = std::env::temp_dir().join("d300_wave8_old_sales.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write tmp");
    let xsd_result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("xmllint");
    if !xsd_result.passed {
        for e in &xsd_result.errors {
            eprintln!("XSD error: {e}");
        }
    }
    assert!(
        xsd_result.passed,
        "Wave8 old-rate sales must pass XSD: {:?}",
        xsd_result.errors
    );
    let _ = std::fs::remove_file(&tmp);

    match run_duk(&xml, "wave8_old_rate_sales") {
        Ok(passed) => assert!(
            passed,
            "Wave8 old-rate sales: DUK must say 'Validare fara erori'"
        ),
        Err(e) => eprintln!("SKIP DUK Wave8 old-rate sales: {e}"),
    }
}

/// WAVE 8 SCENARIO: 9% S PURCHASE → R30_1/R30_2 (regularizări dedusă).
///
/// A purchase at 9% (old rate) base=1000, VAT=90.
/// Expected: R30_1=1000, R30_2=90; R23 empty; R27_2=90.
/// Must pass XSD AND DUK ("Validare fara erori").
#[test]
fn d300_wave8_old_rate_purchase_to_r30() {
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

    // A 21% sale to give some collected VAT; and a 9% purchase for R30.
    let report = D300Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2026-01-01".to_string(),
        period_to: "2026-01-31".to_string(),
        groups: vec![D300Group {
            vat_rate: "0.21".to_string(),
            vat_category: "S".to_string(),
            base: "1000.00".to_string(),
            vat: "210.00".to_string(),
            intra_eu_kind: None,
        }],
        total_base: "1000.00".to_string(),
        total_vat: "210.00".to_string(),
        invoice_count: 1,
        purchase_groups: vec![D300Group {
            vat_rate: "0.09".to_string(),
            vat_category: "S".to_string(),
            base: "1000.00".to_string(),
            vat: "90.00".to_string(),
            intra_eu_kind: None,
        }],
        total_deductible_base: "1000.00".to_string(),
        total_deductible_vat: "90.00".to_string(),
        purchase_invoice_count: 1,
        purchase_unparsed_count: 0,
        net_vat: "120.00".to_string(),
        // Wave 8: 9% S purchase → R30
        reg_colectata_baza: "0.00".to_string(),
        reg_colectata_tva: "0.00".to_string(),
        reg_dedusa_baza: "1000.00".to_string(),
        reg_dedusa_tva: "90.00".to_string(),
        cash_vat_memo: Default::default(),
    };

    let rows =
        map_to_rows(&report, &test_submission(), &test_company(), period, 0).expect("map_to_rows");

    // R30 populated for 9% purchases
    assert_eq!(
        rows.r30_1,
        Some(1000),
        "Wave8 purchase R30_1=1000 (9% base)"
    );
    assert_eq!(rows.r30_2, Some(90), "Wave8 purchase R30_2=90 (9% VAT)");

    // R23 must be empty (9% purchase must NOT go into R23 — DUK corridor 10–12%)
    assert_eq!(
        rows.r23_1, None,
        "Wave8: R23_1 must be None (9% must not go in R23)"
    );
    assert_eq!(rows.r23_2, None, "Wave8: R23_2 must be None");

    // R27_2 does NOT include R30_2 (DUK rule R99/R100 verifies R27 without R30).
    // R30_2 flows directly into R32_2 (DUK rule R108: R32_2 = R28_2 + R30_2).
    assert_eq!(
        rows.r27_2, None,
        "Wave8: R27_2=None (R30 does not feed R27)"
    );
    assert_eq!(
        rows.r32_2,
        Some(90),
        "Wave8: R32_2=R28_2(0)+R30_2(90)=90 (DUK R108)"
    );
    // R17_2 = R9_2 = 210
    assert_eq!(rows.r17_2, Some(210), "Wave8: R17_2=210");
    // R34_2 = MAX(R17_2 - R32_2, 0) = MAX(210 - 90, 0) = 120
    assert_eq!(rows.r34_2, Some(120), "Wave8: R34_2=120 (TVA de plată)");

    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    eprintln!("Wave8 old-rate purchase XML:\n{xml}");

    if let Ok(dump_dir) = std::env::var("EFACTURA_DUMP_DIR") {
        let path = std::path::Path::new(&dump_dir).join("d300_wave8_old_rate_purchase.xml");
        std::fs::write(&path, xml.as_bytes()).expect("write dump");
        eprintln!("DUMP: wave8 old rate purchase → {:?}", path);
    }

    assert!(xml.contains("R30_1=\"1000\""), "XML must have R30_1=1000");
    assert!(xml.contains("R30_2=\"90\""), "XML must have R30_2=90");
    assert!(
        !xml.contains("R23_1="),
        "XML must NOT have R23_1 (9% not in R23)"
    );

    let tmp = std::env::temp_dir().join("d300_wave8_old_purchase.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write tmp");
    let xsd_result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("xmllint");
    if !xsd_result.passed {
        for e in &xsd_result.errors {
            eprintln!("XSD error: {e}");
        }
    }
    assert!(
        xsd_result.passed,
        "Wave8 old-rate purchase must pass XSD: {:?}",
        xsd_result.errors
    );
    let _ = std::fs::remove_file(&tmp);

    match run_duk(&xml, "wave8_old_rate_purchase") {
        Ok(passed) => assert!(
            passed,
            "Wave8 old-rate purchase: DUK must say 'Validare fara erori'"
        ),
        Err(e) => eprintln!("SKIP DUK Wave8 old-rate purchase: {e}"),
    }
}

/// WAVE 8 SCENARIO: submission override for reg_colectata_tva → R16_2.
///
/// Auto-computed = 190; user overrides reg_colectata_tva = Some(180).
/// Expected: R16_2 = 180 (not 190); DUK clean.
#[test]
fn d300_wave8_override() {
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
        groups: vec![D300Group {
            vat_rate: "0.19".to_string(),
            vat_category: "S".to_string(),
            base: "1000.00".to_string(),
            vat: "190.00".to_string(),
            intra_eu_kind: None,
        }],
        total_base: "1000.00".to_string(),
        total_vat: "190.00".to_string(),
        invoice_count: 1,
        purchase_groups: vec![],
        total_deductible_base: "0.00".to_string(),
        total_deductible_vat: "0.00".to_string(),
        purchase_invoice_count: 0,
        purchase_unparsed_count: 0,
        net_vat: "190.00".to_string(),
        reg_colectata_baza: "1000.00".to_string(),
        reg_colectata_tva: "190.00".to_string(), // auto-computed
        reg_dedusa_baza: "0.00".to_string(),
        reg_dedusa_tva: "0.00".to_string(),
        cash_vat_memo: Default::default(),
    };

    // User overrides reg_colectata_tva to 180 (e.g. after reviewing rounding).
    let mut submission = test_submission();
    submission.reg_colectata_tva = Some(180);

    let rows = map_to_rows(&report, &submission, &test_company(), period, 0).expect("map_to_rows");

    // Override must take effect: R16_2 = 180, not 190
    assert_eq!(
        rows.r16_2,
        Some(180),
        "Wave8 override: R16_2 must be 180 (not 190)"
    );
    // Base was not overridden, so auto-computed value used: R16_1 = 1000
    assert_eq!(rows.r16_1, Some(1000), "Wave8 override: R16_1=1000 (auto)");

    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    eprintln!("Wave8 override XML:\n{xml}");

    if let Ok(dump_dir) = std::env::var("EFACTURA_DUMP_DIR") {
        let path = std::path::Path::new(&dump_dir).join("d300_wave8_override.xml");
        std::fs::write(&path, xml.as_bytes()).expect("write dump");
        eprintln!("DUMP: wave8 override → {:?}", path);
    }

    assert!(
        xml.contains("R16_2=\"180\""),
        "XML must have overridden R16_2=180"
    );
    assert!(
        !xml.contains("R16_2=\"190\""),
        "XML must NOT have auto-computed R16_2=190"
    );

    let tmp = std::env::temp_dir().join("d300_wave8_override.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write tmp");
    let xsd_result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("xmllint");
    if !xsd_result.passed {
        for e in &xsd_result.errors {
            eprintln!("XSD error: {e}");
        }
    }
    assert!(
        xsd_result.passed,
        "Wave8 override must pass XSD: {:?}",
        xsd_result.errors
    );
    let _ = std::fs::remove_file(&tmp);

    match run_duk(&xml, "wave8_override") {
        Ok(passed) => assert!(passed, "Wave8 override: DUK must say 'Validare fara erori'"),
        Err(e) => eprintln!("SKIP DUK Wave8 override: {e}"),
    }
}

/// WAVE 5 FIX 1 SCENARIO: QUARTERLY decont (tip_decont=T) over a full calendar
/// quarter of mixed data.
///
/// VERIFY-FIRST (structura_D300_v12.pdf câmp 18 + câmp 2): for tip_decont=T the
/// `luna` attribute must carry the LAST month of the quarter (standard set
/// 03/06/09/12; DUK: "tip_decont=T si luna # (02,03,05,06,08,09,11,12)" = error).
/// Period = 2026-04-01..2026-06-30 (Q2) → luna="6", an="2026".
///
/// Also exercises Wave 5 FIX 2 in the same document: an exempt purchase (E) lands
/// on R26_1 (rd.28, base-only, in the control sum, NOT in R27).
/// Must pass XSD AND DUK ("Validare fara erori").
#[test]
fn d300_wave5_quarterly_decont_luna_quarter_end() {
    let xsd_path = std::path::Path::new("tools/anaf/sample_d300_v12.xml");
    if !xsd_path.exists() {
        eprintln!("SKIP: XSD not found");
        return;
    }
    if !efactura_desktop_lib::anaf_decl::validation::xmllint_available() {
        eprintln!("SKIP: xmllint not available");
        return;
    }

    // period = period_from of the WIDENED range the UI sends for T (first day of Q2).
    let period = chrono::NaiveDate::from_ymd_opt(2026, 4, 1).expect("date");
    let ver = resolve(DeclKind::D300, period).expect("version");

    // Mixed data across the 3 months of the quarter (compute_d300 aggregates the whole
    // range into per-(rate, category) groups — reuse the same group shapes):
    //   April:  sales S 21% 10000/2100
    //   May:    sales S 11% 5000/550
    //   June:   purchases S 21% 8000/1680 + exempt purchase E 1200 (→ R26_1)
    let report = D300Report {
        company_cui: "RO12345674".to_string(),
        period_from: "2026-04-01".to_string(),
        period_to: "2026-06-30".to_string(),
        groups: vec![
            D300Group {
                vat_rate: "0.21".to_string(),
                vat_category: "S".to_string(),
                base: "10000.00".to_string(),
                vat: "2100.00".to_string(),
                intra_eu_kind: None,
            },
            D300Group {
                vat_rate: "0.11".to_string(),
                vat_category: "S".to_string(),
                base: "5000.00".to_string(),
                vat: "550.00".to_string(),
                intra_eu_kind: None,
            },
        ],
        total_base: "15000.00".to_string(),
        total_vat: "2650.00".to_string(),
        invoice_count: 9,
        purchase_groups: vec![
            D300Group {
                vat_rate: "0.21".to_string(),
                vat_category: "S".to_string(),
                base: "8000.00".to_string(),
                vat: "1680.00".to_string(),
                intra_eu_kind: None,
            },
            D300Group {
                vat_rate: "0.00".to_string(),
                vat_category: "E".to_string(),
                base: "1200.00".to_string(),
                vat: "0.00".to_string(),
                intra_eu_kind: None,
            },
        ],
        total_deductible_base: "9200.00".to_string(),
        total_deductible_vat: "1680.00".to_string(),
        purchase_invoice_count: 5,
        purchase_unparsed_count: 0,
        net_vat: "970.00".to_string(),
        reg_colectata_baza: "0.00".to_string(),
        reg_colectata_tva: "0.00".to_string(),
        reg_dedusa_baza: "0.00".to_string(),
        reg_dedusa_tva: "0.00".to_string(),
        cash_vat_memo: Default::default(),
    };

    let mut submission = test_submission();
    submission.tip_decont = "T".to_string();

    let rows = map_to_rows(&report, &submission, &test_company(), period, 0).expect("map_to_rows");

    // FIX 1: luna = quarter-end month (6), NOT the range's first month (4).
    assert_eq!(rows.luna, 6, "T over Q2 → luna = 6 (quarter-end month)");
    assert_eq!(rows.an, 2026);
    assert_eq!(rows.tip_decont, "T");
    // The auto-generated NDP embeds obligation code 302 (trimestrial) + luna 06.
    assert!(
        rows.nr_evid.starts_with("1030201" /* 10 + 302 + 01 */),
        "NDP must use the quarterly obligation code 302, got {}",
        rows.nr_evid
    );
    assert_eq!(&rows.nr_evid[7..9], "06", "NDP reporting month = 06");

    // FIX 2: exempt purchase E → R26_1 (base-only), excluded from R27.
    assert_eq!(rows.r26_1, Some(1200), "E purchase 1200 → R26_1");
    assert_eq!(rows.r27_1, Some(8000), "R27_1 excludes R26_1");
    assert_eq!(rows.r27_2, Some(1680), "R27_2 excludes exempt purchases");

    let xml = generate_d300_xml(&rows, &ver).expect("generate");
    eprintln!("Wave5 quarterly XML:\n{xml}");

    assert!(xml.contains("luna=\"6\""), "XML luna must be 6");
    assert!(xml.contains("tip_decont=\"T\""), "XML tip_decont must be T");
    assert!(xml.contains("R26_1=\"1200\""), "XML must carry R26_1=1200");

    if let Ok(dump_dir) = std::env::var("EFACTURA_DUMP_DIR") {
        let path = std::path::Path::new(&dump_dir).join("d300_wave5_quarterly.xml");
        std::fs::write(&path, xml.as_bytes()).expect("write dump");
        eprintln!("DUMP: wave5 quarterly → {:?}", path);
    }

    // XSD validation
    let tmp = std::env::temp_dir().join("d300_wave5_quarterly.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write tmp");
    let xsd_result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
        .expect("xmllint");
    if !xsd_result.passed {
        for e in &xsd_result.errors {
            eprintln!("XSD error: {e}");
        }
    }
    assert!(
        xsd_result.passed,
        "Wave5 quarterly must pass XSD: {:?}",
        xsd_result.errors
    );
    let _ = std::fs::remove_file(&tmp);

    // DUK validation — the authoritative check for the tip_decont/luna correlation
    // rule ("tip_decont=T si luna # (02,03,05,06,08,09,11,12)") and for R26_1's
    // membership in the totalPlata_A control sum.
    match run_duk(&xml, "wave5_quarterly") {
        Ok(passed) => assert!(
            passed,
            "Wave5 quarterly: DUK must say 'Validare fara erori'"
        ),
        Err(e) => eprintln!("SKIP DUK Wave5 quarterly: {e}"),
    }
}
