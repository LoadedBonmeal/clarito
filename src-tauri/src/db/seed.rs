//! Date de test pentru development. Disponibil doar în debug builds.

use sqlx::SqlitePool;

use crate::db::companies::{self, CreateCompanyInput};
use crate::db::contacts::{self, CreateContactInput};
use crate::db::invoices::{self, CreateInvoiceInput, CreateLineInput};
use crate::db::models::{ContactType, InvoiceStatus};
use crate::error::AppResult;

/// Inserează date de test dacă DB-ul e gol. Idempotent.
pub async fn run_if_empty(pool: &SqlitePool) -> AppResult<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM companies")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Ok(());
    }

    tracing::info!("Seeding development data...");

    // Companii de test.
    let acme = companies::create(
        pool,
        CreateCompanyInput {
            cui: "RO12345674".into(),
            legal_name: "ACME România SRL".into(),
            trade_name: Some("ACME".into()),
            registry_number: Some("J40/1234/2020".into()),
            vat_payer: Some(true),
            address: "Calea Victoriei 100".into(),
            city: "București".into(),
            county: "București".into(),
            postal_code: Some("010001".into()),
            country: None,
            email: Some("contact@acme.ro".into()),
            phone: Some("+40712345678".into()),
            iban: Some("RO49AAAA1B31007593840000".into()),
            bank_name: Some("Banca Transilvania".into()),
            invoice_series: Some("ACME".into()),
            tax_regime: None,
        },
    )
    .await?;

    let cluj = companies::create(
        pool,
        CreateCompanyInput {
            cui: "RO98765438".into(),
            legal_name: "Cluj Tech SRL".into(),
            trade_name: None,
            registry_number: Some("J12/567/2018".into()),
            vat_payer: Some(true),
            address: "Strada Memorandumului 5".into(),
            city: "Cluj-Napoca".into(),
            county: "Cluj".into(),
            postal_code: Some("400114".into()),
            country: None,
            email: Some("hello@clujtech.ro".into()),
            phone: None,
            iban: Some("RO12BTRL01234567890123".into()),
            bank_name: Some("BCR".into()),
            invoice_series: Some("CT".into()),
            tax_regime: None,
        },
    )
    .await?;

    // Contacte (client + furnizor).
    let client_a = contacts::create(
        pool,
        CreateContactInput {
            company_id: acme.id.clone(),
            contact_type: ContactType::Customer,
            cui: Some("RO11111111".into()),
            legal_name: "Client Frumos SRL".into(),
            vat_payer: Some(true),
            is_individual: None,
            cash_vat: None,
            address: Some("Bd. Unirii 22".into()),
            city: Some("București".into()),
            county: Some("București".into()),
            country: None,
            email: Some("facturi@clientfrumos.ro".into()),
            phone: None,
            currency: None,
        },
    )
    .await?;

    contacts::create(
        pool,
        CreateContactInput {
            company_id: acme.id.clone(),
            contact_type: ContactType::Supplier,
            cui: Some("RO22222222".into()),
            legal_name: "Furnizor Standard SA".into(),
            vat_payer: Some(true),
            is_individual: None,
            cash_vat: None,
            address: None,
            city: Some("Timișoara".into()),
            county: Some("Timiș".into()),
            country: None,
            email: None,
            phone: None,
            currency: None,
        },
    )
    .await?;

    // Câteva facturi.
    let invoice_draft = invoices::create(
        pool,
        CreateInvoiceInput {
            company_id: acme.id.clone(),
            contact_id: client_a.id.clone(),
            series: "ACME".into(),
            issue_date: "2026-05-01".into(),
            due_date: "2026-05-31".into(),
            currency: None,
            exchange_rate: None,
            notes: Some("Consultanță IT mai 2026".into()),
            payment_means_code: None,
            lines: vec![CreateLineInput {
                name: "Consultanță tehnică".into(),
                description: Some("80 ore × 250 RON".into()),
                quantity: 80.0,
                unit: "h".into(),
                unit_price: 250.0,
                vat_rate: 19.0,
                vat_category: "S".into(),
                cpv_code: None,
                art331_code: None,
                revenue_kind: None,
            }],
        },
    )
    .await?;
    let _ = invoice_draft;

    let invoice_submitted = invoices::create(
        pool,
        CreateInvoiceInput {
            company_id: acme.id.clone(),
            contact_id: client_a.id.clone(),
            series: "ACME".into(),
            issue_date: "2026-05-08".into(),
            due_date: "2026-06-07".into(),
            currency: None,
            exchange_rate: None,
            notes: None,
            payment_means_code: None,
            lines: vec![
                CreateLineInput {
                    name: "Licență software anuală".into(),
                    description: None,
                    quantity: 1.0,
                    unit: "buc".into(),
                    unit_price: 5_000.0,
                    vat_rate: 19.0,
                    vat_category: "S".into(),
                    cpv_code: None,
                    art331_code: None,
                    revenue_kind: None,
                },
                CreateLineInput {
                    name: "Suport prioritar".into(),
                    description: None,
                    quantity: 12.0,
                    unit: "luna".into(),
                    unit_price: 200.0,
                    vat_rate: 19.0,
                    vat_category: "S".into(),
                    cpv_code: None,
                    art331_code: None,
                    revenue_kind: None,
                },
            ],
        },
    )
    .await?;
    invoices::set_status(
        pool,
        &invoice_submitted.id,
        InvoiceStatus::Submitted,
        Some("Trimisă către ANAF".into()),
    )
    .await?;

    let _ = cluj;

    tracing::info!("Seed completed");
    Ok(())
}
