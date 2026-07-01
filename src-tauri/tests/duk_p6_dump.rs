//! Phase 6 DUK dump: generate a SAF-T XML with seeded stock movement + fixed asset
//! and write it to `<temp>/decl/d406_p6.xml` for the DUK validator gate.
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

/// Build an in-memory SQLite pool on the REAL migrations (same pattern as
/// tests/saft_xsd.rs::setup_test_pool) so the schema can never drift from
/// production again, then seed it with named-column INSERTs:
///   - one company, chart of accounts, customer + supplier, product
///   - one sales invoice + line, one received invoice + VAT line, one payment
///   - one stock movement + line (Phase 6a), one fixed asset (Phase 6b)
async fn setup_test_pool(company: &Company) -> sqlx::SqlitePool {
    use sqlx::SqlitePool;

    // `generate_gl_entries` opens a transaction (`pool.begin()`) AND runs a concurrent pool query
    // (`fetch_optional(pool)`), so the pool must hand out ≥2 connections that share the SAME
    // in-memory DB. `SqlitePool::connect("sqlite::memory:")` (the form the passing gl.rs tests use)
    // does exactly that; `max_connections(1).connect(":memory:")` deadlocked the open transaction
    // against the concurrent query → `PoolTimedOut` after 30 s.
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
        ("213", "Instalatii tehnice", 2),
        ("281", "Amortizarea instalatiilor", 2),
        ("371", "Marfuri", 3),
        ("4111", "Clienti", 4),
        ("401", "Furnizori", 4),
        ("5121", "Conturi la banci in lei", 5),
        ("607", "Cheltuieli privind marfurile", 6),
        ("681", "Cheltuieli privind amortizarea", 6),
        ("707", "Venituri din vanzarea marfurilor", 7),
        ("4427", "TVA colectata", 4),
        ("4426", "TVA deductibila", 4),
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
         VALUES ('cust-1',?,'CUSTOMER','RO99887766','FIRMA CLIENT SRL',1,\
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
         VALUES ('supp-1',?,'SUPPLIER','RO11223344','FIRMA FURNIZOR SRL',1,\
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

    // ── Seed invoice + line ────────────────────────────────────────────────────
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

    // ── Seed received invoice + VAT line ──────────────────────────────────────
    sqlx::query(
        "INSERT INTO received_invoices \
         (id, company_id, anaf_download_id, anaf_index, issuer_cui, issuer_name, \
          series, number, total_amount, net_amount, vat_amount, currency, exchange_rate, \
          issue_date, xml_path, pdf_path, status, is_advance, downloaded_at, created_at) \
         VALUES ('recv-1',?,'DL-1',NULL,'RO11223344','FIRMA FURNIZOR SRL',\
                 'FACT','001',595.00,'500.00','95.00','RON',NULL,'2025-01-10',\
                 '','','APPROVED',0,0,0)",
    )
    .bind(&company.id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO received_invoice_vat_lines \
         (id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount) \
         VALUES ('rvl-1','recv-1','19','S','500.00','95.00')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ── Seed payment ───────────────────────────────────────────────────────────
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

    pool
}

#[tokio::test]
async fn dump_p6_xml_for_duk() {
    let company = test_company();
    let pool = setup_test_pool(&company).await;

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
