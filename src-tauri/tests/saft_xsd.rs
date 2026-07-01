//! Integration test: generate a complete SAF-T D406 XML from synthetic seeded data
//! and validate it against the official ANAF Ro_SAFT_Schema_v249.xsd via `xmllint`.
//!
//! Skips gracefully when the XSD or xmllint are absent, so the standard
//! `cargo test` gate stays green everywhere.
//!
//! Run with:
//!   cd src-tauri && cargo test --test saft_xsd -- --nocapture

use std::path::Path;

use efactura_desktop_lib::anaf_decl::saft::generator::{
    generate_saft_xml, generate_saft_xml_annual,
};
use efactura_desktop_lib::anaf_decl::validation::{validate_with_xsd, xmllint_available};
use efactura_desktop_lib::db::companies::Company;
use efactura_desktop_lib::db::gl::{generate_gl_entries, trial_balance};

// ── Helpers ────────────────────────────────────────────────────────────────────

fn test_company() -> Company {
    Company {
        id: "test-saft-co".to_string(),
        // Valid Romanian CUI (checksum verified): 12345678 → control digit 9
        cui: "RO123456789".to_string(),
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
        last_invoice_number: 5,
        logo_path: None,
        created_at: 0,
        updated_at: 0,
    }
}

/// Build an in-memory SQLite database seeded with minimal data:
///   - One company (test_company)
///   - Chart of accounts (standard seed)
///   - One customer contact
///   - One supplier contact
///   - One product
///   - Two sales invoices with line items
///   - One received (purchase) invoice
///   - One payment
///   - One stock movement with one line (Phase 6a)
///   - One fixed asset (Phase 6b)
async fn setup_test_pool(company: &Company) -> sqlx::SqlitePool {
    use sqlx::SqlitePool;

    // generate_gl_entries opens a transaction AND runs a concurrent pool query, so the pool must hand
    // out ≥2 connections sharing the SAME in-memory DB. `SqlitePool::connect("sqlite::memory:")` (the
    // form the passing gl.rs tests use) provides that; `max_connections(1).connect(":memory:")`
    // deadlocked the tx vs the concurrent query → PoolTimedOut after 30 s.
    let pool = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");

    // ── Schema: run the real migrations so the schema never drifts ─────────────
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations failed");

    // ── Seed company ───────────────────────────────────────────────────────────
    sqlx::query(
        "INSERT INTO companies \
         (id, cui, legal_name, trade_name, registry_number, vat_payer, \
          address, city, county, postal_code, country, email, phone, \
          iban, bank_name, is_active, spv_enabled, invoice_series, \
          last_invoice_number, logo_path, created_at, updated_at, \
          cash_vat, cash_vat_start, cash_vat_end, tax_regime) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,1,0,'F',5,NULL,0,0,0,NULL,NULL,'micro')",
    )
    .bind(&company.id)
    .bind(&company.cui)
    .bind(&company.legal_name)
    .bind(company.trade_name.as_deref())
    .bind(company.registry_number.as_deref())
    .bind(company.vat_payer as i32)
    .bind(&company.address)
    .bind(&company.city)
    .bind(&company.county)
    .bind(company.postal_code.as_deref())
    .bind(&company.country)
    .bind(company.email.as_deref())
    .bind(company.phone.as_deref())
    .bind(company.iban.as_deref())
    .bind(company.bank_name.as_deref())
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed chart of accounts ─────────────────────────────────────────────────
    let accounts = [
        ("101", "Capital", 1i64),
        ("4111", "Clienți", 4),
        ("401", "Furnizori", 4),
        ("5121", "Conturi la bănci în lei", 5),
        ("607", "Cheltuieli privind mărfurile", 6),
        ("707", "Venituri din vânzarea mărfurilor", 7),
        ("4427", "TVA colectată", 4),
        ("4426", "TVA deductibilă", 4),
    ];
    for (i, (code, name, class)) in accounts.iter().enumerate() {
        sqlx::query(
            "INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class, active, created_at, updated_at) VALUES (?,?,?,?,?,1,0,0)",
        )
        .bind(format!("acct-{i}"))
        .bind(&company.id)
        .bind(code)
        .bind(name)
        .bind(class)
        .execute(&pool)
        .await
        .unwrap();
    }

    // ── Seed contacts ──────────────────────────────────────────────────────────
    sqlx::query(
        "INSERT INTO contacts \
         (id, company_id, contact_type, cui, legal_name, vat_payer, \
          address, city, county, country, email, phone, currency, \
          created_at, updated_at, cash_vat, is_individual) \
         VALUES ('cust-1',?,'CUSTOMER','RO99887760','FIRMA CLIENT SRL',1,\
                 'Str. Test 1','Cluj','CJ','RO',NULL,NULL,'RON',0,0,0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO contacts \
         (id, company_id, contact_type, cui, legal_name, vat_payer, \
          address, city, county, country, email, phone, currency, \
          created_at, updated_at, cash_vat, is_individual) \
         VALUES ('supp-1',?,'SUPPLIER','RO11223342','FIRMA FURNIZOR SRL',1,\
                 'Str. Furnizor 2','Timisoara','TM','RO',NULL,NULL,'RON',0,0,0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed products ──────────────────────────────────────────────────────────
    sqlx::query(
        "INSERT INTO products \
         (id, company_id, name, unit, unit_price, vat_rate, vat_category, \
          code, active, created_at, updated_at) \
         VALUES ('prod-1',?,'Serviciu consultanta','ora','100.00','19','S','SVC01',1,0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed invoices ──────────────────────────────────────────────────────────
    sqlx::query(
        "INSERT INTO invoices \
         (id, company_id, contact_id, series, number, full_number, \
          issue_date, due_date, subtotal_amount, vat_amount, total_amount, \
          currency, exchange_rate, storno_of_invoice_id, status, \
          payment_means_code, invoice_kind, created_at, updated_at) \
         VALUES ('inv-1',?,'cust-1','F',1,'F-0001','2025-01-15','2025-02-15',\
                 1000.00,190.00,1190.00,'RON',NULL,NULL,'VALIDATED','42','standard',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO invoice_line_items \
         (id, invoice_id, position, name, description, quantity, unit, \
          unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, \
          total_amount, cpv_code, art331_code, revenue_kind) \
         VALUES ('line-1','inv-1',1,'Serviciu consultanta','Serviciu IT',\
                 10.0,'ora',100.00,19,'S',1000.00,190.00,1190.00,NULL,NULL,'service')",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO invoices \
         (id, company_id, contact_id, series, number, full_number, \
          issue_date, due_date, subtotal_amount, vat_amount, total_amount, \
          currency, exchange_rate, storno_of_invoice_id, status, \
          payment_means_code, invoice_kind, created_at, updated_at) \
         VALUES ('inv-2',?,'cust-1','F',2,'F-0002','2025-01-20','2025-02-20',\
                 500.00,0.00,500.00,'RON',NULL,NULL,'VALIDATED','42','standard',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO invoice_line_items \
         (id, invoice_id, position, name, description, quantity, unit, \
          unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, \
          total_amount, cpv_code, art331_code, revenue_kind) \
         VALUES ('line-2','inv-2',1,'Transport','Transport marfa',\
                 1.0,'buc',500.00,0,'Z',500.00,0.00,500.00,NULL,NULL,'goods')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed received invoices + VAT lines ────────────────────────────────────
    sqlx::query(
        "INSERT INTO received_invoices \
         (id, company_id, anaf_download_id, anaf_index, issuer_cui, issuer_name, \
          series, number, total_amount, net_amount, vat_amount, currency, exchange_rate, \
          issue_date, xml_path, pdf_path, status, is_advance, downloaded_at, created_at) \
         VALUES ('recv-1',?,'DL-1',NULL,'RO11223342','FIRMA FURNIZOR SRL',\
                 'FACT','001',595.00,'500.00','95.00','RON',NULL,'2025-01-10',\
                 '','','APPROVED',0,0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    // VAT line for the purchase invoice so GL posting works
    // Real column order (migration 0012): id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount
    sqlx::query(
        "INSERT INTO received_invoice_vat_lines \
         (id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount) \
         VALUES ('rvl-1','recv-1','19','S','500.00','95.00')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed a FOREIGN-issuer received invoice (Fix A regression guard) ──────
    // A non-numeric/foreign VAT id (German-style "DE811234567") must land in
    // MasterFiles/Suppliers under the SAME id ("04DE811234567", IDType "04" +
    // alphanumeric CUI) that the GL and SourceDocuments reference via
    // `canonical_partner_id(received_invoice_id, cui)`. Reverse-charge (AE) line so
    // post_purchase_invoice's GL posting exercises the foreign-supplier path without
    // needing an extra VAT rate bucket.
    sqlx::query(
        "INSERT INTO received_invoices \
         (id, company_id, anaf_download_id, anaf_index, issuer_cui, issuer_name, \
          series, number, total_amount, net_amount, vat_amount, currency, exchange_rate, \
          issue_date, xml_path, pdf_path, status, is_advance, downloaded_at, created_at) \
         VALUES ('recv-2',?,'DL-2',NULL,'DE811234567','FOREIGN SUPPLIER GMBH',\
                 'INV','100',300.00,'300.00','0.00','RON',NULL,'2025-01-18',\
                 '','','APPROVED',0,0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO received_invoice_vat_lines \
         (id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount) \
         VALUES ('rvl-2','recv-2','19','AE','300.00','0.00')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed payments ──────────────────────────────────────────────────────────
    sqlx::query(
        "INSERT INTO payments \
         (id, invoice_id, company_id, amount, currency, paid_at, \
          method, reference, notes, created_at) \
         VALUES ('pay-1','inv-1',?,'1190.00','RON','2025-01-20','transfer','REF-001',NULL,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed stock movements (Phase 6a) ───────────────────────────────────────
    // One NIR (intrare) with one line — tests MovementOfGoods population
    sqlx::query(
        "INSERT INTO stock_movements \
         (id, company_id, movement_ref, movement_date, posting_date, \
          movement_type, direction, document_type, document_number, created_at, updated_at) \
         VALUES ('sm-1',?,'NIR-001','2025-01-12','2025-01-12','10','IN','NIR','001',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO stock_movement_lines \
         (id, movement_id, line_number, product_code, account_id, customer_id, supplier_id, \
          quantity, unit_of_measure, uom_conv_factor, book_value, movement_subtype) \
         VALUES ('sml-1','sm-1',1,'PRODUS-01','371','0','0011223344','10.000000','H87','1','500.00','10')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed fixed asset (Phase 6b) ───────────────────────────────────────────
    // One active asset — tests Assets section population
    sqlx::query(
        "INSERT INTO fixed_assets \
         (id, company_id, asset_code, account_id, description, valuation_class, \
          supplier_id, supplier_name, date_of_acquisition, start_up_date, \
          acquisition_cost, life_months, depreciation_method, depreciation_pct, \
          active, created_at, updated_at) \
         VALUES ('fa-1',?,'MF-001','213','Laptop Test','Corporala',\
                 '0','','2024-01-01','2024-01-01',\
                 '3000.00',36,'liniara','0.00',1,0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed asset transaction (A-profile) ────────────────────────────────────
    // One depreciation transaction in 2025 — tests AssetTransactions section
    sqlx::query(
        "INSERT INTO asset_transactions \
         (id, company_id, asset_id, transaction_code, transaction_type, transaction_date, \
          description, gl_transaction_id, acq_prod_cost, book_value, amount, created_at) \
         VALUES ('at-1',?,'fa-1','AT-2025-001','30','2025-12-31',\
                 'Amortizare anuala MF-001','GL-2025-001',\
                 '3000.00','1000.00','83.33',0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    pool
}

// ── Main XSD validation test ───────────────────────────────────────────────────

#[tokio::test]
async fn saft_d406_validates_against_official_xsd() {
    // XSD path (relative to src-tauri/ crate root — cwd for `cargo test`)
    // Use the _prod copy whose targetNamespace matches the production d406 namespace.
    let xsd_path = Path::new("tools/anaf/Ro_SAFT_Schema_v249_prod.xsd");

    if !xsd_path.exists() {
        eprintln!(
            "SKIP saft_xsd: XSD not found at {xsd_path:?} — vendor it at \
             src-tauri/tools/anaf/Ro_SAFT_Schema_v249.xsd to enable this gate."
        );
        return;
    }

    if !xmllint_available() {
        eprintln!(
            "SKIP saft_xsd: xmllint not available — install libxml2-utils (Linux) \
             or it ships with macOS Xcode CLT."
        );
        return;
    }

    let company = test_company();
    let pool = setup_test_pool(&company).await;

    // Auto-post GL entries (idempotent) — populates gl_journal / gl_entry
    generate_gl_entries(&pool, &company.id, "2025-01-01", "2025-01-31", false)
        .await
        .expect("generate_gl_entries must not fail");

    let xml = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .expect("generate_saft_xml must not fail");

    eprintln!("Generated SAF-T D406 XML ({} bytes):", xml.len());
    eprintln!("{xml}");

    let tmp = std::env::temp_dir().join("saft_d406_xsd_test.xml");
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
        "SAF-T D406 XML failed XSD validation. Errors:\n{}",
        result.errors.join("\n")
    );

    // Best-effort DUK gate: run the bundled ANAF D406Validator on the same XML (ANAF business
    // rules, beyond the XSD's structural checks). Skips gracefully when the bundled jre-min /
    // D406Validator.jar aren't present (e.g. an unbundled checkout / CI).
    {
        use efactura_desktop_lib::anaf_decl::duk::{run_duk, DukProvider, DukRuntime};
        use efactura_desktop_lib::anaf_decl::DeclKind;
        use std::path::PathBuf;

        let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let java = res.join(if cfg!(windows) {
            "jre-min/bin/java.exe"
        } else {
            "jre-min/bin/java"
        });
        let jar_dir = res.join("duk");

        struct LocalBundle {
            java: PathBuf,
            jar_dir: PathBuf,
        }
        impl DukProvider for LocalBundle {
            fn resolve(&self) -> Option<DukRuntime> {
                if self.java.is_file()
                    && self.jar_dir.join("DUKIntegrator.jar").is_file()
                    && self.jar_dir.join("lib/D406Validator.jar").is_file()
                {
                    Some(DukRuntime {
                        java: self.java.clone(),
                        jar_dir: self.jar_dir.clone(),
                    })
                } else {
                    None
                }
            }
        }

        let duk_tmp = std::env::temp_dir().join("saft_d406_duk_test.xml");
        std::fs::write(&duk_tmp, xml.as_bytes()).expect("write DUK temp XML");
        let outcome = run_duk(&LocalBundle { java, jar_dir }, DeclKind::D406, &duk_tmp)
            .expect("run_duk must not fail");
        let _ = std::fs::remove_file(&duk_tmp);
        match outcome {
            Some(o) => {
                assert!(
                    o.passed,
                    "ANAF D406Validator reported errors on the generated D406:\n{}",
                    o.errors
                        .iter()
                        .map(|e| format!("{e:?}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                eprintln!("DUK D406 VALIDATION PASSED");
            }
            None => eprintln!("SKIP DUK: bundled jre-min/D406Validator not present"),
        }
    }
}

// ── Fix A regression: foreign-supplier SupplierID must not dangle ─────────────
//
// A foreign/non-numeric issuer CUI (e.g. "DE811234567", seeded as recv-2 in
// setup_test_pool) must resolve to the SAME canonical id in MasterFiles/Suppliers as in
// GeneralLedgerEntries/SourceDocuments — "04DE811234567" (IDType "04" + the CUI's own
// alphanumeric characters; see the doc comment on `canonical_partner_id` in
// masterfiles.rs) — so no SupplierID is referenced without a matching MasterFiles entry.
// This id (rather than the previously-assumed all-zero "080000000000000" anonymized
// bucket) is required because the real bundled ANAF D406Validator (DUKIntegrator)
// rejects "08..." outright as an invalid SupplierID format — confirmed empirically by
// running the validator directly against candidate ids — while "04"+alphanumeric passes.
#[tokio::test]
async fn saft_d406_foreign_supplier_id_matches_gl_and_masterfiles() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

    generate_gl_entries(&pool, &company.id, "2025-01-01", "2025-01-31", false)
        .await
        .expect("generate_gl_entries must not fail");

    let xml = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .expect("generate_saft_xml must not fail");

    const FOREIGN_SUPPLIER_ID: &str = "04DE811234567";

    // 1) MasterFiles/Suppliers must contain a Supplier record for this id.
    let masterfiles_end = xml.find("<GeneralLedgerEntries>").unwrap_or(xml.len());
    let masterfiles_section = &xml[..masterfiles_end];
    assert!(
        masterfiles_section.contains(&format!("<SupplierID>{FOREIGN_SUPPLIER_ID}</SupplierID>")),
        "MasterFiles/Suppliers must emit SupplierID {FOREIGN_SUPPLIER_ID} for the \
         foreign-CUI issuer (recv-2, DE811234567) so it isn't a dangling reference: {xml}"
    );

    // 2) GeneralLedgerEntries / SourceDocuments must reference that SAME id (already did
    // before the fix — this pins the invariant so the two sides never diverge again).
    let gle_start = xml
        .find("<GeneralLedgerEntries>")
        .expect("GeneralLedgerEntries present");
    assert!(
        xml[gle_start..].contains(FOREIGN_SUPPLIER_ID),
        "GeneralLedgerEntries/SourceDocuments must reference {FOREIGN_SUPPLIER_ID} \
         for the foreign-CUI issuer: {xml}"
    );

    // 3) If xmllint is available, also confirm the fixture still validates end-to-end
    // (belt-and-suspenders alongside the main saft_d406_validates_against_official_xsd test).
    let xsd_path = Path::new("tools/anaf/Ro_SAFT_Schema_v249_prod.xsd");
    if xsd_path.exists() && xmllint_available() {
        let tmp = std::env::temp_dir().join("saft_d406_foreign_supplier_xsd_test.xml");
        std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");
        let result = validate_with_xsd(xsd_path, &tmp).expect("validate_with_xsd must not fail");
        let _ = std::fs::remove_file(&tmp);
        assert!(
            result.passed,
            "SAF-T D406 XML (with foreign supplier) failed XSD validation. Errors:\n{}",
            result.errors.join("\n")
        );
    } else {
        eprintln!("SKIP xmllint check in foreign-supplier test: XSD or xmllint not available");
    }
}

/// FIX 3 (audit wave 3, P1): D406 Payments must use the SAME FX conversion as GL for a
/// foreign-currency invoice payment. Before the fix, `source_docs::write_payments` passed
/// `None` for the rate (relying on the payment row's OWN `currency`, defaulting to 'RON',
/// with no rate at all) — silently treating the raw EUR numeric amount as if it were RON.
/// `db::gl::post_payment` (via the `cash_ron` computed at ~gl.rs:3448-3463) uses the
/// PAYMENT's own `exchange_rate` (falling back to the INVOICE's rate) to convert the
/// invoice's currency to RON for the bank-side (5124, foreign) leg. This test seeds a EUR
/// invoice (rate 5.0000) + a payment with its OWN distinct rate (5.2000 — simulating FX
/// movement between invoice and payment dates), posts GL, generates SAF-T, and asserts the
/// Payments section's RON amount equals the GL's 5124 (foreign bank) debit for that same
/// payment — NOT the raw EUR amount misinterpreted as RON.
#[tokio::test]
async fn saft_d406_payments_fx_matches_gl_for_foreign_currency_invoice() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

    // A EUR invoice booked at rate 5.0000 (receivable = 200 EUR × 5.0 = 1000.00 RON).
    sqlx::query(
        "INSERT INTO invoices \
         (id, company_id, contact_id, series, number, full_number, \
          issue_date, due_date, subtotal_amount, vat_amount, total_amount, \
          currency, exchange_rate, storno_of_invoice_id, status, \
          payment_means_code, invoice_kind, created_at, updated_at) \
         VALUES ('inv-eur-1',?,'cust-1','F',3,'F-0003','2025-01-05','2025-02-05',\
                 200.00,0.00,200.00,'EUR',5.0,NULL,'VALIDATED','42','standard',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO invoice_line_items \
         (id, invoice_id, position, name, description, quantity, unit, \
          unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, \
          total_amount, cpv_code, art331_code, revenue_kind) \
         VALUES ('line-eur-1','inv-eur-1',1,'Consultanta export','Serviciu extern',\
                 1.0,'buc',200.00,0,'G',200.00,0.00,200.00,NULL,NULL,'service')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // The payment carries its OWN rate (5.2000, distinct from the invoice's 5.0000) — this is
    // exactly the case gl.rs's `pay_fx` (falling back to `inv_fx`) models, and the case the
    // pre-fix `None` in source_docs.rs completely ignored.
    sqlx::query(
        "INSERT INTO payments \
         (id, invoice_id, company_id, amount, currency, paid_at, \
          method, reference, notes, created_at, exchange_rate) \
         VALUES ('pay-eur-1','inv-eur-1',?,'200.00','EUR','2025-01-25','transfer','REF-EUR-1',NULL,0,5.2)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    generate_gl_entries(&pool, &company.id, "2025-01-01", "2025-01-31", false)
        .await
        .expect("generate_gl_entries must not fail");

    let xml = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .expect("generate_saft_xml must not fail");

    // Expected: 200 EUR × 5.2000 (payment's own rate) = 1040.00 RON — this is what gl.rs's
    // `cash_ron` computes for the bank-side leg (foreign=true → account 5124).
    const EXPECTED_PAYMENT_RON: &str = "1040.00";
    // The pre-fix bug would have emitted "200.00" (raw EUR amount, unconverted).
    const BUGGY_UNCONVERTED_RON: &str = "200.00";

    // 1) SAF-T Payments section: find PaymentRefNo REF-EUR-1's PaymentLineAmount/Amount.
    let payments_start = xml.find("<Payments>").expect("Payments section present");
    let payments_section = &xml[payments_start..];
    let ref_pos = payments_section
        .find("REF-EUR-1")
        .expect("payment REF-EUR-1 must be present in Payments section");
    // The PaymentLineAmount/Amount follows PaymentRefNo within the same <Payment> block.
    let after_ref = &payments_section[ref_pos..];
    let amount_tag_start = after_ref
        .find("<Amount>")
        .expect("Amount tag present after ref");
    let amount_value_start = amount_tag_start + "<Amount>".len();
    let amount_tag_end = after_ref[amount_value_start..]
        .find("</Amount>")
        .expect("closing Amount tag");
    let saft_amount = &after_ref[amount_value_start..amount_value_start + amount_tag_end];

    assert_ne!(
        saft_amount, BUGGY_UNCONVERTED_RON,
        "D406 Payments must NOT emit the raw EUR amount unconverted (pre-fix bug): {xml}"
    );
    assert_eq!(
        saft_amount, EXPECTED_PAYMENT_RON,
        "D406 Payments RON amount must equal 200 EUR × payment rate 5.2 = 1040.00: {xml}"
    );

    // 2) Cross-check against the GL: trial_balance for account 5124 (foreign bank, since this
    // is a foreign-currency payment) must show the SAME closing_debit for the period — this is
    // the actual bank-side RON amount gl.rs::post_payment posted for pay-eur-1.
    let tb = trial_balance(&pool, &company.id, "2025-01-01", "2025-01-31")
        .await
        .expect("trial_balance must not fail");
    let bal_5124 = tb
        .rows
        .iter()
        .find(|r| r.account_code == "5124")
        .map(|r| r.closing_debit.clone());
    assert_eq!(
        bal_5124.as_deref(),
        Some(EXPECTED_PAYMENT_RON),
        "GL account 5124 (foreign bank) closing debit must equal 1040.00 (same conversion as \
         D406 Payments) — if these disagree, the FX-mismatch bug (FIX 3) has regressed"
    );

    // 3) XSD validation, if available.
    let xsd_path = Path::new("tools/anaf/Ro_SAFT_Schema_v249_prod.xsd");
    if xsd_path.exists() && xmllint_available() {
        let tmp = std::env::temp_dir().join("saft_d406_payments_fx_test.xml");
        std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");
        let result = validate_with_xsd(xsd_path, &tmp).expect("validate_with_xsd must not fail");
        let _ = std::fs::remove_file(&tmp);
        assert!(
            result.passed,
            "SAF-T D406 XML (FX payments fixture) failed XSD validation. Errors:\n{}",
            result.errors.join("\n")
        );
    } else {
        eprintln!("SKIP xmllint check in FX-payments test: XSD or xmllint not available");
    }
}

// ── Phase 6 annual XSD test ────────────────────────────────────────────────────

#[tokio::test]
async fn saft_d406_annual_validates_against_official_xsd() {
    let xsd_path = Path::new("tools/anaf/Ro_SAFT_Schema_v249_prod.xsd");
    if !xsd_path.exists() || !xmllint_available() {
        eprintln!("SKIP saft_d406_annual_xsd: XSD or xmllint not available");
        return;
    }

    let company = test_company();
    let pool = setup_test_pool(&company).await;

    // Annual A-profile: use full-year period; skip GL auto-post (GLE is empty in A)

    let xml = generate_saft_xml_annual(&pool, &company, "2025-01-01", "2025-12-31")
        .await
        .expect("generate_saft_xml_annual must not fail");

    let tmp = std::env::temp_dir().join("saft_d406_annual_xsd_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");

    let result = validate_with_xsd(xsd_path, &tmp).expect("validate_with_xsd must not fail");

    if !result.passed {
        eprintln!("ANNUAL XSD VALIDATION FAILED:");
        for e in &result.errors {
            eprintln!("  {e}");
        }
    } else {
        eprintln!("ANNUAL XSD VALIDATION PASSED");
    }

    let _ = std::fs::remove_file(&tmp);

    assert!(
        result.passed,
        "Annual SAF-T D406 XML failed XSD validation. Errors:\n{}",
        result.errors.join("\n")
    );
}

// ── Structural tests (no XSD required) ────────────────────────────────────────

#[tokio::test]
async fn saft_d406_has_all_four_mandatory_sections() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

    let xml = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .expect("generate_saft_xml must not fail");

    assert!(
        xml.contains("<AuditFile "),
        "must have AuditFile root: {xml}"
    );
    assert!(
        xml.contains("xmlns=\"mfp:anaf:dgti:d406:declaratie:v1\""),
        "must have correct namespace: {xml}"
    );
    assert!(xml.contains("<Header>"), "must have Header: {xml}");
    assert!(
        xml.contains("<MasterFiles>"),
        "must have MasterFiles: {xml}"
    );
    assert!(
        xml.contains("<GeneralLedgerEntries>"),
        "must have GeneralLedgerEntries: {xml}"
    );
    assert!(
        xml.contains("<SourceDocuments>"),
        "must have SourceDocuments: {xml}"
    );
    assert!(xml.contains("</AuditFile>"), "must close AuditFile: {xml}");
}

#[tokio::test]
async fn saft_d406_header_required_fields() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

    let xml = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .unwrap();

    assert!(
        xml.contains("<AuditFileVersion>2.4.9</AuditFileVersion>"),
        "version: {xml}"
    );
    assert!(
        xml.contains("<AuditFileCountry>RO</AuditFileCountry>"),
        "country: {xml}"
    );
    assert!(
        xml.contains("<SoftwareCompanyName>Lucaris SRL</SoftwareCompanyName>"),
        "swname: {xml}"
    );
    assert!(
        xml.contains("<SoftwareID>efactura-desktop</SoftwareID>"),
        "swid: {xml}"
    );
    assert!(
        xml.contains("<DefaultCurrencyCode>RON</DefaultCurrencyCode>"),
        "currency: {xml}"
    );
    assert!(
        xml.contains("<TaxAccountingBasis>A</TaxAccountingBasis>"),
        "taxbasis: {xml}"
    );
    assert!(
        xml.contains("<SelectionStartDate>2025-01-01</SelectionStartDate>"),
        "start: {xml}"
    );
    assert!(
        xml.contains("<SelectionEndDate>2025-01-31</SelectionEndDate>"),
        "end: {xml}"
    );
    assert!(xml.contains("<ContactPerson>"), "contact: {xml}");
    assert!(
        xml.contains("<Telephone>0721000000</Telephone>"),
        "phone: {xml}"
    );
    assert!(
        xml.contains("<TaxAuthority>ANAF</TaxAuthority>"),
        "authority: {xml}"
    );
}

#[tokio::test]
async fn saft_d406_masterfiles_contain_accounts_and_customers() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

    let xml = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .unwrap();

    assert!(xml.contains("<GeneralLedgerAccounts>"), "GLA: {xml}");
    assert!(xml.contains("<Account>"), "Account: {xml}");
    assert!(
        xml.contains("<AccountID>4111</AccountID>"),
        "4111 account: {xml}"
    );
    assert!(xml.contains("<Customers>"), "Customers: {xml}");
    assert!(xml.contains("<Customer>"), "Customer: {xml}");
    assert!(xml.contains("<Suppliers>"), "Suppliers: {xml}");
    assert!(xml.contains("<TaxTable>"), "TaxTable: {xml}");
    assert!(xml.contains("<UOMTable>"), "UOMTable: {xml}");
    assert!(xml.contains("<Products>"), "Products: {xml}");
    assert!(
        xml.contains("<AnalysisTypeTable>"),
        "AnalysisTypeTable: {xml}"
    );
    assert!(
        xml.contains("<MovementTypeTable>"),
        "MovementTypeTable: {xml}"
    );
    assert!(xml.contains("<Owners>"), "Owners: {xml}");
    assert!(xml.contains("<Assets>"), "Assets: {xml}");
}

#[tokio::test]
async fn saft_d406_masterfiles_account_types_valid() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

    let xml = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .unwrap();

    // AccountType must only be one of Activ, Pasiv, Bifunctional
    for line in xml.lines() {
        if line.contains("<AccountType>") {
            assert!(
                line.contains("Activ") || line.contains("Pasiv") || line.contains("Bifunctional"),
                "Invalid AccountType in: {line}"
            );
        }
    }
}

#[tokio::test]
async fn saft_d406_source_documents_have_invoices() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

    let xml = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .unwrap();

    assert!(xml.contains("<SalesInvoices>"), "SalesInvoices: {xml}");
    assert!(
        xml.contains("<PurchaseInvoices>"),
        "PurchaseInvoices: {xml}"
    );
    assert!(xml.contains("<Payments>"), "Payments: {xml}");
    assert!(xml.contains("<MovementOfGoods>"), "MovementOfGoods: {xml}");
    assert!(xml.contains("<Invoice>"), "Invoice: {xml}");
    assert!(xml.contains("<InvoiceLine>"), "InvoiceLine: {xml}");
    assert!(
        xml.contains("<DebitCreditIndicator>C</DebitCreditIndicator>"),
        "DCI C for sales: {xml}"
    );
    assert!(
        xml.contains("<DebitCreditIndicator>D</DebitCreditIndicator>"),
        "DCI D for purchase: {xml}"
    );
}

#[tokio::test]
async fn saft_d406_gl_entries_populated_and_balanced() {
    let xsd_path = Path::new("tools/anaf/Ro_SAFT_Schema_v249_prod.xsd");

    let company = test_company();
    let pool = setup_test_pool(&company).await;

    // Post GL entries for the seeded data
    let gl_result = generate_gl_entries(&pool, &company.id, "2025-01-01", "2025-01-31", false)
        .await
        .expect("generate_gl_entries must succeed");

    eprintln!("GL post result: {gl_result:?}");
    assert!(
        gl_result.journals_inserted > 0,
        "Expected at least one GL journal inserted, got 0"
    );

    let xml = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .unwrap();

    // GLE must have journals
    assert!(
        xml.contains("<Journal>"),
        "GL must have at least one journal: {xml}"
    );
    assert!(
        !xml.contains("<NumberOfEntries>0</NumberOfEntries>"),
        "GL NumberOfEntries must be > 0 when GL is posted: {xml}"
    );

    // TotalDebit must equal TotalCredit (double-entry balance invariant)
    // Extract the values from the XML
    let total_debit = extract_gle_field(&xml, "TotalDebit");
    let total_credit = extract_gle_field(&xml, "TotalCredit");
    eprintln!("GLE TotalDebit={total_debit} TotalCredit={total_credit}");
    assert_eq!(
        total_debit, total_credit,
        "GL TotalDebit must equal TotalCredit (double-entry balance). TotalDebit={total_debit} TotalCredit={total_credit}"
    );

    // TaxType in GLE must be "300" (not "TVA") for VAT lines
    // Check for "300" appearing in the GL section
    assert!(
        xml.contains("<TaxType>300</TaxType>") || xml.contains("<TaxType>000</TaxType>"),
        "GL TaxType must use DUK codes (300/000), not 'TVA': {xml}"
    );

    // XSD validation if available
    if xsd_path.exists() && xmllint_available() {
        let tmp = std::env::temp_dir().join("saft_gle_test.xml");
        std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");
        let result = validate_with_xsd(xsd_path, &tmp).expect("validate_with_xsd must not fail");
        if !result.passed {
            eprintln!("GLE XSD VALIDATION FAILED:");
            for e in &result.errors {
                eprintln!("  {e}");
            }
        }
        let _ = std::fs::remove_file(&tmp);
        assert!(
            result.passed,
            "SAF-T with populated GLE failed XSD validation. Errors:\n{}",
            result.errors.join("\n")
        );
    }
}

/// Extract the text content of a top-level GLE field (TotalDebit/TotalCredit/NumberOfEntries).
/// Simple string search — finds the first occurrence after <GeneralLedgerEntries>.
fn extract_gle_field(xml: &str, field: &str) -> String {
    let open_tag = format!("<{field}>");
    let close_tag = format!("</{field}>");
    if let Some(start) = xml.find(&open_tag) {
        let after = &xml[start + open_tag.len()..];
        if let Some(end) = after.find(&close_tag) {
            return after[..end].trim().to_string();
        }
    }
    String::new()
}

#[tokio::test]
async fn saft_d406_account_assignments() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

    let xml = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .unwrap();

    // Sales invoice AccountID = 4111 (customer)
    // InvoiceLine AccountID = 707 (revenue)
    // Payment line AccountID = 5121 (bank)
    // Purchase invoice AccountID = 401 (supplier)
    // Purchase line AccountID = 607 (expense)
    assert!(
        xml.contains("<AccountID>4111</AccountID>"),
        "Customer account 4111: {xml}"
    );
    assert!(
        xml.contains("<AccountID>707</AccountID>"),
        "Revenue account 707: {xml}"
    );
    assert!(
        xml.contains("<AccountID>401</AccountID>"),
        "Supplier account 401: {xml}"
    );
    assert!(
        xml.contains("<AccountID>607</AccountID>"),
        "Expense account 607: {xml}"
    );
    assert!(
        xml.contains("<AccountID>5121</AccountID>"),
        "Bank account 5121: {xml}"
    );
}

// ── Phase 6a: MovementOfGoods section present in both L and A declarations ────
// Per DUK production validator rules:
//   L-profile: MovementOfGoods children (StockMovement) are max:0 — empty wrapper.
//   A-profile: MovementOfGoods children are also max:0 — empty wrapper.
//
// The stock_movements table stores movements for future use when DUK lifts this
// restriction; both profiles emit the mandatory empty wrapper for now.

#[tokio::test]
async fn saft_p6a_movement_of_goods_populated_with_seeded_data() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

    // L-path: MovementOfGoods wrapper must be present (empty) in periodic XML
    generate_gl_entries(&pool, &company.id, "2025-01-01", "2025-01-31", false)
        .await
        .expect("generate_gl_entries must not fail");

    let xml = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .expect("generate_saft_xml (periodic) must not fail");

    // MovementOfGoods wrapper must be present in periodic L XML
    assert!(
        xml.contains("<MovementOfGoods>"),
        "MovementOfGoods wrapper missing in periodic XML: {xml}"
    );
    // DUK L-type rejects StockMovement children — must be empty wrapper
    assert!(
        !xml.contains("<StockMovement>"),
        "DUK L-type rejects StockMovement children — periodic XML must NOT have them: {xml}"
    );

    // A-path: MovementOfGoods must also be an empty wrapper
    let xml_annual = generate_saft_xml_annual(&pool, &company, "2025-01-01", "2025-12-31")
        .await
        .expect("generate_saft_xml_annual must not fail");
    assert!(
        xml_annual.contains("<MovementOfGoods>"),
        "MovementOfGoods wrapper missing in annual XML: {xml_annual}"
    );
    assert!(
        !xml_annual.contains("<StockMovement>"),
        "Annual A-path must NOT have StockMovement children: {xml_annual}"
    );
}

// ── Phase 6b: Assets populated (annual declaration) ───────────────────────────

#[tokio::test]
async fn saft_p6b_assets_populated_with_seeded_data() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

    // Use annual variant — periodic L keeps Assets empty to satisfy DUK
    let xml = generate_saft_xml_annual(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .expect("generate_saft_xml_annual must not fail");

    // Assets must be present and contain the seeded fixed asset
    assert!(xml.contains("<Assets>"), "Assets missing: {xml}");
    assert!(
        xml.contains("<Asset>"),
        "Asset element missing — seeded data must produce at least one: {xml}"
    );
    assert!(
        xml.contains("<AssetID>MF-001</AssetID>"),
        "AssetID MF-001 missing: {xml}"
    );
    assert!(
        xml.contains("<AccountID>213</AccountID>"),
        "Asset AccountID 213 missing: {xml}"
    );
    assert!(
        xml.contains("<DateOfAcquisition>2024-01-01</DateOfAcquisition>"),
        "DateOfAcquisition missing: {xml}"
    );
    assert!(
        xml.contains("<Valuations>"),
        "Valuations wrapper missing: {xml}"
    );
    assert!(
        xml.contains("<AssetValuationType>fiscal</AssetValuationType>"),
        "AssetValuationType missing: {xml}"
    );
    assert!(
        xml.contains("<ExtraordinaryDepreciationsForPeriod>"),
        "ExtraordinaryDepreciationsForPeriod wrapper missing (required by XSD): {xml}"
    );
    // Accumulated depreciation: 3000/36 per month * 12 months elapsed (Jan 2024 → Jan 2025)
    // = 83.33 * 12 = 999.96 (rounded)
    assert!(
        xml.contains("<AccumulatedDepreciation>"),
        "AccumulatedDepreciation missing: {xml}"
    );
    assert!(
        xml.contains("<BookValueEnd>"),
        "BookValueEnd missing: {xml}"
    );
}

// ── A-profile section gating ──────────────────────────────────────────────────
// Confirms that for an annual declaration:
//   - MasterFiles: Customers/Suppliers/TaxTable/UOMTable/Products are EMPTY wrappers
//   - MasterFiles: Assets is POPULATED
//   - GeneralLedgerEntries is the EMPTY wrapper (no NumberOfEntries children)
//   - SourceDocuments: SalesInvoices/PurchaseInvoices/Payments/MovementOfGoods are EMPTY
//   - SourceDocuments: AssetTransactions is POPULATED with NumberOfAssetTransactions

#[tokio::test]
async fn saft_annual_a_profile_section_gating() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

    let xml = generate_saft_xml_annual(&pool, &company, "2025-01-01", "2025-12-31")
        .await
        .expect("generate_saft_xml_annual must not fail");

    // HeaderComment must be "A"
    assert!(
        xml.contains("<HeaderComment>A</HeaderComment>"),
        "HeaderComment must be A: {xml}"
    );

    // MasterFiles: Customers/Suppliers/TaxTable/UOMTable/Products must be EMPTY wrappers
    // (no child elements — just the open/close tag pair)
    assert!(
        !xml.contains("<Customer>"),
        "Annual: must NOT have Customer entries: {xml}"
    );
    assert!(
        !xml.contains("<Supplier>"),
        "Annual: must NOT have Supplier entries: {xml}"
    );
    assert!(
        !xml.contains("<TaxTableEntry>"),
        "Annual: must NOT have TaxTableEntry entries: {xml}"
    );
    assert!(
        !xml.contains("<UOMTableEntry>"),
        "Annual: must NOT have UOMTableEntry entries: {xml}"
    );
    assert!(
        !xml.contains("<Product>"),
        "Annual: must NOT have Product entries: {xml}"
    );

    // MasterFiles: Assets must be POPULATED
    assert!(
        xml.contains("<Asset>"),
        "Annual: Assets must be populated: {xml}"
    );
    assert!(
        xml.contains("<AssetID>MF-001</AssetID>"),
        "Annual: AssetID MF-001 must be present: {xml}"
    );

    // GeneralLedgerEntries must be the empty wrapper (no NumberOfEntries / Journal children)
    assert!(
        xml.contains("<GeneralLedgerEntries>"),
        "Annual: GeneralLedgerEntries wrapper must be present: {xml}"
    );
    assert!(
        !xml.contains("<NumberOfEntries>"),
        "Annual: GeneralLedgerEntries must NOT have NumberOfEntries child: {xml}"
    );
    assert!(
        !xml.contains("<Journal>"),
        "Annual: GeneralLedgerEntries must NOT have Journal child: {xml}"
    );

    // SourceDocuments: SalesInvoices/PurchaseInvoices/Payments/MovementOfGoods must be EMPTY
    assert!(
        !xml.contains("<Invoice>"),
        "Annual: must NOT have Invoice entries: {xml}"
    );
    assert!(
        !xml.contains("<Payment>"),
        "Annual: must NOT have Payment entries: {xml}"
    );
    assert!(
        !xml.contains("<StockMovement>"),
        "Annual: must NOT have StockMovement entries: {xml}"
    );

    // SourceDocuments: AssetTransactions must be POPULATED
    assert!(
        xml.contains("<AssetTransactions>"),
        "Annual: AssetTransactions wrapper must be present: {xml}"
    );
    assert!(
        xml.contains("<NumberOfAssetTransactions>1</NumberOfAssetTransactions>"),
        "Annual: NumberOfAssetTransactions must be 1 (one seeded transaction): {xml}"
    );
    assert!(
        xml.contains("<AssetTransaction>"),
        "Annual: AssetTransaction element must be present: {xml}"
    );
    assert!(
        xml.contains("<AssetTransactionID>AT-2025-001</AssetTransactionID>"),
        "Annual: AssetTransactionID must match seeded value: {xml}"
    );
    assert!(
        xml.contains("<AssetTransactionType>30</AssetTransactionType>"),
        "Annual: AssetTransactionType 30 (depreciation) must be present: {xml}"
    );
    assert!(
        xml.contains("<AssetTransactionDate>2025-12-31</AssetTransactionDate>"),
        "Annual: AssetTransactionDate must be present: {xml}"
    );
    assert!(
        xml.contains("<TransactionID>GL-2025-001</TransactionID>"),
        "Annual: TransactionID (GL cross-ref) must be present: {xml}"
    );
}

// ── DUK dump: annual A-profile for live validation ────────────────────────────
// Writes the annual XML to /tmp/decl/d406_annual.xml for DUK gate.
// Also confirms monthly still generates without error (regression guard).

#[tokio::test]
async fn saft_annual_duk_dump() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

    // Build annual XML (full-year period for A-profile)
    let xml_annual = generate_saft_xml_annual(&pool, &company, "2025-01-01", "2025-12-31")
        .await
        .expect("generate_saft_xml_annual must not fail");

    let out_dir = "/tmp/decl";
    let _ = std::fs::create_dir_all(out_dir);
    let annual_path = format!("{out_dir}/d406_annual.xml");
    std::fs::write(&annual_path, xml_annual.as_bytes()).expect("write d406_annual.xml");
    eprintln!(
        "Annual A-profile XML written to {annual_path} ({} bytes)",
        xml_annual.len()
    );

    // XSD validation if available
    let xsd_path = std::path::Path::new("tools/anaf/Ro_SAFT_Schema_v249_prod.xsd");
    if xsd_path.exists() && efactura_desktop_lib::anaf_decl::validation::xmllint_available() {
        let tmp = std::env::temp_dir().join("saft_d406_annual_duk_dump_xsd.xml");
        std::fs::write(&tmp, xml_annual.as_bytes()).expect("write temp XML");
        let result = efactura_desktop_lib::anaf_decl::validation::validate_with_xsd(xsd_path, &tmp)
            .expect("validate_with_xsd must not fail");
        let _ = std::fs::remove_file(&tmp);
        if !result.passed {
            eprintln!("ANNUAL DUK DUMP XSD VALIDATION FAILED:");
            for e in &result.errors {
                eprintln!("  {e}");
            }
        } else {
            eprintln!("ANNUAL DUK DUMP XSD VALIDATION PASSED");
        }
        assert!(
            result.passed,
            "Annual A-profile SAF-T failed XSD validation. Errors:\n{}",
            result.errors.join("\n")
        );
    }

    // Monthly regression: confirm periodic L generator still produces valid XML
    generate_gl_entries(&pool, &company.id, "2025-01-01", "2025-01-31", false)
        .await
        .expect("generate_gl_entries must not fail");
    let xml_monthly = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .expect("generate_saft_xml (monthly) must not fail");

    let monthly_path = format!("{out_dir}/d406_monthly_regression.xml");
    std::fs::write(&monthly_path, xml_monthly.as_bytes())
        .expect("write d406_monthly_regression.xml");
    eprintln!(
        "Monthly L-profile XML written to {monthly_path} ({} bytes)",
        xml_monthly.len()
    );

    // Monthly must still have populated sections
    assert!(
        xml_monthly.contains("<Invoice>"),
        "Monthly regression: must still have Invoice entries: {xml_monthly}"
    );
    assert!(
        xml_monthly.contains("<Journal>"),
        "Monthly regression: must still have Journal entries: {xml_monthly}"
    );
    assert!(
        !xml_monthly.contains("<AssetTransaction>"),
        "Monthly regression: must NOT have AssetTransaction entries: {xml_monthly}"
    );
    assert!(
        xml_monthly.contains("<HeaderComment>L</HeaderComment>"),
        "Monthly regression: HeaderComment must be L: {xml_monthly}"
    );
}
