//! Phase 6 DUK dump: generate a SAF-T XML with seeded stock movement + fixed asset
//! and write it to /tmp/decl/d406_p6.xml for the DUK validator gate.
//!
//! Run with:
//!   cd src-tauri && cargo test --test duk_p6_dump -- --nocapture

use efactura_desktop_lib::anaf_decl::saft::generator::{
    generate_saft_xml, generate_saft_xml_annual,
};
use efactura_desktop_lib::db::companies::Company;
use efactura_desktop_lib::db::gl::generate_gl_entries;

fn test_company() -> Company {
    Company {
        id: "test-saft-co".to_string(),
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

#[tokio::test]
async fn dump_p6_xml_for_duk() {
    use sqlx::Executor;
    use sqlx::SqlitePool;

    let company = test_company();

    // `generate_gl_entries` opens a transaction (`pool.begin()`) AND runs a concurrent pool query
    // (`fetch_optional(pool)`), so the pool must hand out ≥2 connections that share the SAME in-memory
    // DB. `SqlitePool::connect("sqlite::memory:")` (the form the passing gl.rs tests use via
    // `setup_pool`) does exactly that. The old `max_connections(1).connect(":memory:")` deadlocked the
    // open transaction against the concurrent query → `PoolTimedOut` after 30 s.
    let pool = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");

    // ── Schema ────────────────────────────────────────────────────────────────

    pool.execute(sqlx::query(
        "CREATE TABLE companies (
            id TEXT PRIMARY KEY, cui TEXT, legal_name TEXT, trade_name TEXT,
            registry_number TEXT, vat_payer INTEGER, address TEXT, city TEXT,
            county TEXT, postal_code TEXT, country TEXT, email TEXT, phone TEXT,
            iban TEXT, bank_name TEXT, is_active INTEGER, spv_enabled INTEGER,
            invoice_series TEXT, last_invoice_number INTEGER, logo_path TEXT,
            created_at INTEGER, updated_at INTEGER,
            -- cash-VAT (TVA la încasare): read by generate_gl_entries to decide art. 282/297 deferral
            cash_vat INTEGER DEFAULT 0, cash_vat_start TEXT, cash_vat_end TEXT
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
            created_at INTEGER, updated_at INTEGER,
            cash_vat INTEGER DEFAULT 0
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
            subtotal_amount TEXT, vat_amount TEXT, total_amount TEXT,
            cpv_code TEXT, art331_code TEXT, revenue_kind TEXT DEFAULT 'goods'
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
            method TEXT, reference TEXT, notes TEXT, created_at INTEGER,
            exchange_rate TEXT, received_invoice_id TEXT
        )",
    ))
    .await
    .unwrap();

    // received_invoice_payments (supplier-invoice payments, migration 0027 + later exchange_rate ALTER):
    // the GL cash-VAT release path joins it. The test seeds no rows, so an empty table satisfies the query.
    pool.execute(sqlx::query(
        "CREATE TABLE received_invoice_payments (
            id TEXT PRIMARY KEY, received_invoice_id TEXT, company_id TEXT,
            amount TEXT, currency TEXT, paid_at TEXT, method TEXT,
            reference TEXT, notes TEXT, created_at INTEGER, exchange_rate TEXT
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

    // ── Seed ─────────────────────────────────────────────────────────────────

    sqlx::query("INSERT INTO companies VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,1,0,'F',5,NULL,0,0,0,NULL,NULL)")
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

    let accounts = [
        ("101", "Capital", 1i64),
        ("213", "Instalatii tehnice", 2i64),
        ("281", "Amortizarea instalatiilor", 2i64),
        ("371", "Marfuri", 3i64),
        ("4111", "Clienti", 4i64),
        ("401", "Furnizori", 4i64),
        ("5121", "Conturi la banci in lei", 5i64),
        ("607", "Cheltuieli privind marfurile", 6i64),
        ("681", "Cheltuieli privind amortizarea", 6i64),
        ("707", "Venituri din vanzarea marfurilor", 7i64),
        ("4427", "TVA colectata", 4i64),
        ("4426", "TVA deductibila", 4i64),
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

    sqlx::query(
        "INSERT INTO contacts VALUES ('cust-1',?,'CUSTOMER','RO99887766','FIRMA CLIENT SRL',1,'Str. Test 1','Cluj','CJ','RO',NULL,NULL,'RON',0,0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO contacts VALUES ('supp-1',?,'SUPPLIER','RO11223344','FIRMA FURNIZOR SRL',1,'Str. Furnizor 2','Timisoara','TM','RO',NULL,NULL,'RON',0,0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO products VALUES ('prod-1',?,'Serviciu consultanta','ora','100.00','Consultanta IT','SVC01',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO invoices VALUES ('inv-1',?,'cust-1','F',1,'F-0001','2025-01-15','2025-02-15','1000.00','190.00','1190.00','RON',NULL,NULL,'VALIDATED','42',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO invoice_line_items VALUES ('line-1','inv-1',1,'Serviciu consultanta','Serviciu IT','10.000000','ora','100.00','19','S','1000.00','190.00','1190.00',NULL,NULL,'service')",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO received_invoices VALUES ('recv-1',?,'DL-1',NULL,'RO11223344','FIRMA FURNIZOR SRL','FACT','001','595.00','500.00','95.00','RON',NULL,'2025-01-10','','NULL','APPROVED',0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO received_invoice_vat_lines VALUES ('rvl-1','recv-1','S','19','500.00','95.00')",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO payments VALUES ('pay-1','inv-1',?,'1190.00','RON','2025-01-20','transfer','REF-001',NULL,0,NULL,NULL)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    // ── Phase 6a seed: stock movement + line ─────────────────────────────────

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
         VALUES ('sml-1','sm-1',1,'SVC01','371','0','0011223344','10.000000','H87','1','500.00','10')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ── Phase 6b seed: fixed asset ────────────────────────────────────────────

    sqlx::query(
        "INSERT INTO fixed_assets \
         (id, company_id, asset_code, account_id, description, valuation_class, \
          supplier_id, supplier_name, date_of_acquisition, start_up_date, \
          acquisition_cost, life_months, depreciation_method, depreciation_pct, \
          active, created_at, updated_at) \
         VALUES ('fa-1',?,'MF-001','213','Laptop Test','Corporala',\
                 '0011223344','FIRMA FURNIZOR SRL',\
                 '2024-01-01','2024-01-01',\
                 '3000.00',36,'liniara','0.00',1,0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    // ── Generate ──────────────────────────────────────────────────────────────

    generate_gl_entries(&pool, &company.id, "2025-01-01", "2025-01-31", false)
        .await
        .expect("GL posting must succeed");

    // ── Periodic (L) declaration: Assets must be empty to pass DUK ───────────
    let xml_periodic = generate_saft_xml(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .expect("generate_saft_xml (periodic) must not fail");

    eprintln!(
        "Generated Phase 6 periodic SAF-T XML ({} bytes)",
        xml_periodic.len()
    );

    // Periodic: MovementOfGoods must be the EMPTY wrapper (DUK L-type rejects children)
    // Assets must also be the empty wrapper
    assert!(
        xml_periodic.contains("<MovementOfGoods>"),
        "MovementOfGoods wrapper must be present in periodic XML"
    );
    assert!(
        !xml_periodic.contains("<StockMovement>"),
        "StockMovement must NOT be present in periodic XML (DUK L-type rejects it)"
    );
    assert!(
        !xml_periodic.contains("<Asset>"),
        "Asset must NOT be present in periodic XML (DUK L-type rejects it)"
    );

    // Write periodic XML for an optional manual DUK validator run. Use a portable temp dir (NOT a
    // hardcoded /tmp/decl, which doesn't exist on Windows and fails if the dir is absent).
    let dump_dir = std::env::temp_dir().join("decl");
    let _ = std::fs::create_dir_all(&dump_dir);
    let out_path = dump_dir.join("d406_p6.xml");
    std::fs::write(&out_path, xml_periodic.as_bytes()).expect("write d406_p6.xml");
    eprintln!("Periodic XML written to {}", out_path.display());

    // ── Annual (A) declaration: Assets must be populated ─────────────────────
    let xml_annual = generate_saft_xml_annual(&pool, &company, "2025-01-01", "2025-01-31")
        .await
        .expect("generate_saft_xml_annual (annual) must not fail");

    eprintln!(
        "Generated Phase 6 annual SAF-T XML ({} bytes)",
        xml_annual.len()
    );

    assert!(
        xml_annual.contains("<Asset>"),
        "Asset must be present in annual XML"
    );
    assert!(
        xml_annual.contains("<ExtraordinaryDepreciationsForPeriod>"),
        "ExtraordinaryDepreciationsForPeriod must be present in annual XML"
    );
    assert!(
        xml_annual.contains("<AccumulatedDepreciation>"),
        "AccumulatedDepreciation must be present in annual XML"
    );

    // Write annual XML for reference (same portable temp dir).
    let annual_path = dump_dir.join("d406_p6_annual.xml");
    std::fs::write(&annual_path, xml_annual.as_bytes()).expect("write d406_p6_annual.xml");
    eprintln!("Annual XML written to {}", annual_path.display());

    eprintln!("Phase 6 content assertions passed.");
}
