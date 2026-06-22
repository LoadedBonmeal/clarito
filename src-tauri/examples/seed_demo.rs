//! Demo-data seeder — populates the app's SQLite DB with sample firms + data so the whole app can be
//! exercised (dashboard, invoices, contacts, products, payroll/D112, declarations, dividends/D205/D207,
//! GL). Uses the app's REAL `db::*::create` functions, so VAT/totals/GL/validation are all correct.
//!
//! Run with the APP CLOSED:  cargo run --bin seed_demo
//! It is idempotent by company name (skips a firm that already exists).

use efactura_desktop_lib::anaf_decl::valid_cnp;
use efactura_desktop_lib::db::accounts;
use efactura_desktop_lib::db::companies::{self, CreateCompanyInput};
use efactura_desktop_lib::db::contacts::{self, CreateContactInput};
use efactura_desktop_lib::db::dividends::{self, DividendInput};
use efactura_desktop_lib::db::invoices::{self, CreateInvoiceInput, CreateLineInput};
use efactura_desktop_lib::db::models::ContactType;
use efactura_desktop_lib::db::payroll::{self, CreateEmployeeInput};
use efactura_desktop_lib::db::products::{self, ProductInput};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

type R<T> = Result<T, Box<dyn std::error::Error>>;

/// Append the valid mod-11 control digit to a CUI body (so `companies::validate_cui` passes).
fn cui(body: &str) -> String {
    for c in 0..10u8 {
        let v = format!("{body}{c}");
        if companies::validate_cui(&v).is_ok() {
            return v;
        }
    }
    panic!("no valid CUI control digit for {body}");
}

/// Append the valid mod-11 control digit to a 12-digit CNP body.
fn cnp(body12: &str) -> String {
    for c in 0..10u8 {
        let v = format!("{body12}{c}");
        if valid_cnp(&v) {
            return v;
        }
    }
    panic!("no valid CNP control digit for {body12}");
}

fn line(name: &str, qty: f64, price: f64, vat: f64, kind: &str) -> CreateLineInput {
    CreateLineInput {
        name: name.into(),
        description: None,
        quantity: qty,
        unit: "buc".into(),
        unit_price: price,
        vat_rate: vat,
        vat_category: "S".into(),
        cpv_code: None,
        art331_code: None,
        revenue_kind: Some(kind.into()),
    }
}

#[tokio::main]
async fn main() -> R<()> {
    let home = std::env::var("HOME")?;
    let db_path = format!("{home}/Library/Application Support/com.lucaris.efactura/data.db");
    if !std::path::Path::new(&db_path).exists() {
        return Err(format!("app DB not found at {db_path} — launch Clarito once first").into());
    }
    println!("→ connecting to {db_path}");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!("sqlite://{db_path}"))
        .await?;

    seed_firm(
        &pool,
        "DEMO Tehnologii SRL",
        &cui("4026831"),
        "micro",
        "Str. Victoriei 10, et. 3",
        "București",
        "B",
        "DTH",
    )
    .await?;

    seed_firm(
        &pool,
        "DEMO Distribuție SRL",
        &cui("1798415"),
        "profit",
        "Bd. 21 Decembrie 25",
        "Cluj-Napoca",
        "CJ",
        "DDS",
    )
    .await?;

    println!("✅ Demo seed complete — relaunch Clarito and pick a DEMO firm.");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn seed_firm(
    pool: &SqlitePool,
    name: &str,
    cui_full: &str,
    regime: &str,
    address: &str,
    city: &str,
    county: &str,
    series: &str,
) -> R<()> {
    // Idempotent: skip if a firm with this name already exists.
    if companies::list(pool)
        .await?
        .iter()
        .any(|c| c.legal_name == name)
    {
        println!("• {name} already exists — skipping");
        return Ok(());
    }

    let co = companies::create(
        pool,
        CreateCompanyInput {
            cui: format!("RO{cui_full}"),
            legal_name: name.into(),
            trade_name: None,
            registry_number: Some("J40/1234/2024".into()),
            vat_payer: Some(true),
            address: address.into(),
            city: city.into(),
            county: county.into(),
            postal_code: Some("010101".into()),
            country: Some("RO".into()),
            email: Some("contact@demo.ro".into()),
            phone: Some("0721 000 000".into()),
            iban: Some("RO49RNCB0000000000000001".into()),
            bank_name: Some("Banca Demo".into()),
            invoice_series: Some(series.into()),
            tax_regime: Some(regime.into()),
        },
    )
    .await?;
    let cid = co.id.clone();
    let _ = accounts::seed_standard(pool, &cid).await;
    println!("• {name}  (CUI {cui_full}, {regime}) — id {cid}");

    // ── Contacts: 4 clients + 2 suppliers ──
    let client_specs = [
        ("Vertex Media SRL", "Customer", "3984115"),
        ("Aurora Trade SRL", "Customer", "2645018"),
        ("Carpat Logistic SRL", "Customer", "1942330"),
        ("Lumen Studio SRL", "Customer", "4011220"),
    ];
    let mut client_ids = Vec::new();
    for (cn, ct, body) in client_specs {
        let c = contacts::create(
            pool,
            CreateContactInput {
                company_id: cid.clone(),
                contact_type: if ct == "Customer" {
                    ContactType::Customer
                } else {
                    ContactType::Supplier
                },
                cui: Some(format!("RO{}", cui(body))),
                legal_name: cn.into(),
                vat_payer: Some(true),
                is_individual: Some(false),
                cash_vat: Some(false),
                address: Some(format!("Str. Exemplu {}", cn.len())),
                city: Some("București".into()),
                county: Some("B".into()),
                country: Some("RO".into()),
                email: Some("office@partener.ro".into()),
                phone: Some("0312 000 111".into()),
                currency: Some("RON".into()),
                iban: None,
                bank_name: None,
                swift: None,
                payment_term_days: None,
            },
        )
        .await?;
        client_ids.push(c.id);
    }
    for (sn, body) in [
        ("Furnizor Alpha SRL", "5012118"),
        ("Utilități Beta SA", "2233441"),
    ] {
        contacts::create(
            pool,
            CreateContactInput {
                company_id: cid.clone(),
                contact_type: ContactType::Supplier,
                cui: Some(format!("RO{}", cui(body))),
                legal_name: sn.into(),
                vat_payer: Some(true),
                is_individual: Some(false),
                cash_vat: Some(false),
                address: Some("Calea Furnizorilor 5".into()),
                city: Some("Brașov".into()),
                county: Some("BV".into()),
                country: Some("RO".into()),
                email: None,
                phone: None,
                currency: Some("RON".into()),
                iban: None,
                bank_name: None,
                swift: None,
                payment_term_days: None,
            },
        )
        .await?;
    }

    // ── Products ──
    let prods = [
        ("Licență software (lună)", "499.00", "21", "service"),
        ("Consultanță IT (oră)", "250.00", "21", "service"),
        ("Mentenanță server", "1200.00", "21", "service"),
        ("Manual tehnic", "85.00", "11", "goods"),
        ("Curs online", "320.00", "21", "service"),
    ];
    for (pn, price, vat, kind) in prods {
        products::create(
            pool,
            &cid,
            ProductInput {
                name: pn.into(),
                unit: Some("buc".into()),
                unit_price: Some(price.into()),
                vat_rate: Some(vat.into()),
                vat_category: Some("S".into()),
                code: None,
                stock_qty: if kind == "goods" {
                    Some("50".into())
                } else {
                    None
                },
                art331_code: None,
                barcode: None,
                is_service: Some(kind == "service"),
                // product_type and product_group_id derived from is_service in the DB layer.
                product_type: None,
                product_group_id: None,
                active: Some(true),
            },
        )
        .await?;
    }

    // ── Sales invoices (spread across 2026 so the dashboard charts have data) ──
    let inv_specs: [(&str, usize, Vec<CreateLineInput>); 5] = [
        (
            "2026-02-12",
            0,
            vec![line("Consultanță IT (oră)", 12.0, 250.0, 21.0, "service")],
        ),
        (
            "2026-03-18",
            1,
            vec![
                line("Licență software (lună)", 3.0, 499.0, 21.0, "service"),
                line("Manual tehnic", 5.0, 85.0, 11.0, "goods"),
            ],
        ),
        (
            "2026-04-09",
            2,
            vec![line("Mentenanță server", 1.0, 1200.0, 21.0, "service")],
        ),
        (
            "2026-05-21",
            3,
            vec![line("Curs online", 8.0, 320.0, 21.0, "service")],
        ),
        (
            "2026-06-03",
            0,
            vec![line("Consultanță IT (oră)", 20.0, 250.0, 21.0, "service")],
        ),
    ];
    for (issue, client_idx, lines) in inv_specs {
        let due = next_month_same_day(issue);
        invoices::create(
            pool,
            CreateInvoiceInput {
                company_id: cid.clone(),
                contact_id: client_ids[client_idx].clone(),
                series: series.into(),
                issue_date: issue.into(),
                due_date: due,
                currency: Some("RON".into()),
                exchange_rate: None,
                notes: None,
                payment_means_code: Some("42".into()),
                lines,
            },
        )
        .await?;
    }

    // ── Employees + a payroll run (May 2026) ──
    let emps = [
        ("Popescu Andrei", cnp("196010141001"), "8500"),
        ("Ionescu Maria", cnp("296030541002"), "6200"),
        ("Georgescu Radu", cnp("191205041003"), "12000"),
    ];
    for (en, ecnp, gross) in &emps {
        payroll::create(
            pool,
            CreateEmployeeInput {
                company_id: cid.clone(),
                cnp: ecnp.clone(),
                full_name: (*en).into(),
                gross_salary: (*gross).into(),
                personal_deduction: None,
                employment_date: Some("2024-01-15".into()),
                contract_end_date: None,
                tip_asigurat: None,
                pensionar: Some(false),
                tip_contract: None,
                ore_norma: Some(8),
                exceptie_cas_min: None,
                sediu_cif: None,
                beneficiar_suma_netaxabila: Some(false),
                functia: None,
                cod_cor: None,
            },
        )
        .await?;
    }
    let _ = payroll::run_payroll(pool, &cid, "2026-05-01", "2026-05-31").await;

    // ── Dividends (one resident → D205, one non-resident → D207 flag) ──
    dividends::create(
        pool,
        DividendInput {
            company_id: cid.clone(),
            distribution_date: "2026-03-15".into(),
            payment_date: Some("2026-03-25".into()),
            gross_amount: "40000".into(),
            interim_2025: false,
            shareholder: Some("Popescu Andrei".into()),
            beneficiary_cnp: Some(cnp("196010141001")),
            beneficiary_resident: true,
            beneficiary_type: None,
            beneficiary_country: None,
            beneficiary_foreign_tax_id: None,
            note: None,
        },
    )
    .await?;
    dividends::create(
        pool,
        DividendInput {
            company_id: cid.clone(),
            distribution_date: "2026-04-10".into(),
            payment_date: Some("2026-04-20".into()),
            gross_amount: "15000".into(),
            interim_2025: false,
            shareholder: Some("John Smith (UK)".into()),
            beneficiary_cnp: None,
            beneficiary_resident: false,
            beneficiary_type: None,
            beneficiary_country: Some("GB".into()), // D207: țara de rezidență (Stat_R)
            beneficiary_foreign_tax_id: Some("GB123456789".into()), // cifS
            note: None,
        },
    )
    .await?;

    Ok(())
}

/// "2026-02-12" → "2026-03-12" (due date = ~30 days later, month-rolled).
fn next_month_same_day(iso: &str) -> String {
    let (y, m, d) = (
        iso[0..4].parse::<i32>().unwrap(),
        iso[5..7].parse::<u32>().unwrap(),
        &iso[8..10],
    );
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    format!("{ny:04}-{nm:02}-{d}")
}
