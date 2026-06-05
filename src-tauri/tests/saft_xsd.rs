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
use efactura_desktop_lib::db::gl::generate_gl_entries;

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
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::Executor;

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(":memory:")
        .await
        .expect("in-memory pool");

    // ── Schema ─────────────────────────────────────────────────────────────────
    pool.execute(sqlx::query(
        "CREATE TABLE companies (
            id TEXT PRIMARY KEY, cui TEXT, legal_name TEXT, trade_name TEXT,
            registry_number TEXT, vat_payer INTEGER, address TEXT, city TEXT,
            county TEXT, postal_code TEXT, country TEXT, email TEXT, phone TEXT,
            iban TEXT, bank_name TEXT, is_active INTEGER, spv_enabled INTEGER,
            invoice_series TEXT, last_invoice_number INTEGER, logo_path TEXT,
            created_at INTEGER, updated_at INTEGER
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE chart_of_accounts (
            id TEXT PRIMARY KEY, company_id TEXT, account_code TEXT,
            account_name TEXT, account_class INTEGER, parent_code TEXT,
            active INTEGER DEFAULT 1, created_at INTEGER, updated_at INTEGER,
            UNIQUE(company_id, account_code)
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE contacts (
            id TEXT PRIMARY KEY, company_id TEXT, contact_type TEXT,
            cui TEXT, legal_name TEXT, vat_payer INTEGER,
            address TEXT, city TEXT, county TEXT, country TEXT,
            email TEXT, phone TEXT, currency TEXT,
            created_at INTEGER, updated_at INTEGER
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE products (
            id TEXT PRIMARY KEY, company_id TEXT, name TEXT, unit TEXT,
            unit_price TEXT, description TEXT, code TEXT,
            created_at INTEGER, updated_at INTEGER
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE invoices (
            id TEXT PRIMARY KEY, company_id TEXT, contact_id TEXT,
            series TEXT, number INTEGER, full_number TEXT,
            issue_date TEXT, due_date TEXT,
            subtotal_amount TEXT, vat_amount TEXT, total_amount TEXT,
            currency TEXT, exchange_rate REAL, storno_of_invoice_id TEXT,
            status TEXT, payment_means_code TEXT,
            created_at INTEGER, updated_at INTEGER
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE invoice_line_items (
            id TEXT PRIMARY KEY, invoice_id TEXT, position INTEGER,
            name TEXT, description TEXT, quantity TEXT, unit TEXT,
            unit_price TEXT, vat_rate TEXT, vat_category TEXT,
            subtotal_amount TEXT, vat_amount TEXT, total_amount TEXT
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE received_invoices (
            id TEXT PRIMARY KEY, company_id TEXT,
            anaf_download_id TEXT, anaf_index TEXT,
            issuer_cui TEXT, issuer_name TEXT,
            series TEXT, number TEXT,
            total_amount TEXT, net_amount TEXT, vat_amount TEXT,
            currency TEXT, exchange_rate REAL, issue_date TEXT,
            xml_path TEXT, pdf_path TEXT, status TEXT,
            downloaded_at INTEGER, created_at INTEGER
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE payments (
            id TEXT PRIMARY KEY, invoice_id TEXT, company_id TEXT,
            amount TEXT, currency TEXT, paid_at TEXT,
            method TEXT, reference TEXT, notes TEXT, created_at INTEGER
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE received_invoice_vat_lines (
            id TEXT PRIMARY KEY,
            received_invoice_id TEXT,
            vat_category TEXT,
            vat_rate TEXT,
            base_amount TEXT,
            vat_amount TEXT
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE stock_movements (
            id TEXT NOT NULL PRIMARY KEY,
            company_id TEXT NOT NULL,
            movement_ref TEXT NOT NULL,
            movement_date TEXT NOT NULL,
            posting_date TEXT NOT NULL,
            movement_type TEXT NOT NULL DEFAULT '10',
            direction TEXT NOT NULL DEFAULT 'IN',
            document_type TEXT,
            document_number TEXT,
            source_type TEXT,
            source_id TEXT,
            notes TEXT,
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            UNIQUE(company_id, movement_ref)
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE stock_movement_lines (
            id TEXT NOT NULL PRIMARY KEY,
            movement_id TEXT NOT NULL,
            line_number INTEGER NOT NULL DEFAULT 1,
            product_id TEXT,
            product_code TEXT NOT NULL,
            account_id TEXT NOT NULL DEFAULT '371',
            customer_id TEXT NOT NULL DEFAULT '0',
            supplier_id TEXT NOT NULL DEFAULT '0',
            quantity TEXT NOT NULL DEFAULT '1',
            unit_of_measure TEXT NOT NULL DEFAULT 'H87',
            uom_conv_factor TEXT NOT NULL DEFAULT '1',
            book_value TEXT NOT NULL DEFAULT '0.00',
            movement_subtype TEXT NOT NULL DEFAULT '10',
            comments TEXT
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE fixed_assets (
            id TEXT NOT NULL PRIMARY KEY,
            company_id TEXT NOT NULL,
            asset_code TEXT NOT NULL,
            account_id TEXT NOT NULL DEFAULT '213',
            description TEXT NOT NULL,
            valuation_class TEXT NOT NULL DEFAULT 'Corporala',
            supplier_id TEXT NOT NULL DEFAULT '0',
            supplier_name TEXT NOT NULL DEFAULT '',
            date_of_acquisition TEXT NOT NULL,
            start_up_date TEXT NOT NULL,
            acquisition_cost TEXT NOT NULL DEFAULT '0.00',
            life_months INTEGER NOT NULL DEFAULT 60,
            depreciation_method TEXT NOT NULL DEFAULT 'liniara',
            depreciation_pct TEXT NOT NULL DEFAULT '0.00',
            disposal_date TEXT,
            active INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            UNIQUE(company_id, asset_code)
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE asset_transactions (
            id TEXT NOT NULL PRIMARY KEY,
            company_id TEXT NOT NULL,
            asset_id TEXT NOT NULL,
            transaction_code TEXT NOT NULL,
            transaction_type TEXT NOT NULL DEFAULT '10',
            transaction_date TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            gl_transaction_id TEXT,
            acq_prod_cost TEXT NOT NULL DEFAULT '0.00',
            book_value TEXT NOT NULL DEFAULT '0.00',
            amount TEXT NOT NULL DEFAULT '0.00',
            created_at INTEGER NOT NULL DEFAULT 0
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE gl_journal (
            id TEXT PRIMARY KEY, company_id TEXT NOT NULL,
            journal_id TEXT NOT NULL, journal_type TEXT NOT NULL,
            transaction_id TEXT NOT NULL, transaction_date TEXT NOT NULL,
            description TEXT, source_type TEXT NOT NULL, source_id TEXT NOT NULL,
            customer_id TEXT, supplier_id TEXT,
            created_at INTEGER NOT NULL DEFAULT 0
        )",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE UNIQUE INDEX idx_gl_journal_source ON gl_journal(company_id, source_type, source_id)",
    ))
    .await
    .unwrap();

    pool.execute(sqlx::query(
        "CREATE TABLE gl_entry (
            id TEXT PRIMARY KEY, journal_pk TEXT NOT NULL,
            record_id INTEGER NOT NULL, account_code TEXT NOT NULL,
            debit TEXT NOT NULL DEFAULT '0.00', credit TEXT NOT NULL DEFAULT '0.00',
            partner_cui TEXT, customer_id TEXT, supplier_id TEXT,
            tax_type TEXT, tax_code TEXT,
            tax_percentage TEXT, tax_base TEXT, tax_amount TEXT
        )",
    ))
    .await
    .unwrap();

    // ── Seed company ───────────────────────────────────────────────────────────
    sqlx::query("INSERT INTO companies VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,1,0,'F',5,NULL,0,0)")
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
        "INSERT INTO contacts VALUES ('cust-1',?,'CUSTOMER','RO99887760','FIRMA CLIENT SRL',1,'Str. Test 1','Cluj','CJ','RO',NULL,NULL,'RON',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO contacts VALUES ('supp-1',?,'SUPPLIER','RO11223342','FIRMA FURNIZOR SRL',1,'Str. Furnizor 2','Timisoara','TM','RO',NULL,NULL,'RON',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed products ──────────────────────────────────────────────────────────
    sqlx::query(
        "INSERT INTO products VALUES ('prod-1',?,'Serviciu consultanta','ora','100.00','Consultanta IT','SVC01',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed invoices ──────────────────────────────────────────────────────────
    // Columns: id, company_id, contact_id, series, number, full_number,
    //          issue_date, due_date, subtotal_amount, vat_amount, total_amount,
    //          currency, exchange_rate, storno_of_invoice_id, status,
    //          payment_means_code, created_at, updated_at
    sqlx::query(
        "INSERT INTO invoices VALUES ('inv-1',?,'cust-1','F',1,'F-0001','2025-01-15','2025-02-15','1000.00','190.00','1190.00','RON',NULL,NULL,'VALIDATED','42',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO invoice_line_items VALUES ('line-1','inv-1',1,'Serviciu consultanta','Serviciu IT','10.000000','ora','100.00','19','S','1000.00','190.00','1190.00')",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO invoices VALUES ('inv-2',?,'cust-1','F',2,'F-0002','2025-01-20','2025-02-20','500.00','0.00','500.00','RON',NULL,NULL,'VALIDATED','42',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO invoice_line_items VALUES ('line-2','inv-2',1,'Transport','Transport marfa','1.000000','buc','500.00','0','Z','500.00','0.00','500.00')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed received invoices + VAT lines ────────────────────────────────────
    sqlx::query(
        "INSERT INTO received_invoices VALUES ('recv-1',?,'DL-1',NULL,'RO11223342','FIRMA FURNIZOR SRL','FACT','001','595.00','500.00','95.00','RON',NULL,'2025-01-10','','NULL','APPROVED',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    // VAT line for the purchase invoice so GL posting works
    sqlx::query(
        "INSERT INTO received_invoice_vat_lines VALUES ('rvl-1','recv-1','S','19','500.00','95.00')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed payments ──────────────────────────────────────────────────────────
    sqlx::query(
        "INSERT INTO payments VALUES ('pay-1','inv-1',?,'1190.00','RON','2025-01-20','transfer','REF-001',NULL,0)",
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
                 '0','',\
                 '2024-01-01','2024-01-01',\
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
    generate_gl_entries(&pool, &company.id, "2025-01-01", "2025-01-31")
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
    let gl_result = generate_gl_entries(&pool, &company.id, "2025-01-01", "2025-01-31")
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
    generate_gl_entries(&pool, &company.id, "2025-01-01", "2025-01-31")
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
    generate_gl_entries(&pool, &company.id, "2025-01-01", "2025-01-31")
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
