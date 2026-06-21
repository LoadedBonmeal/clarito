//! Oferte / devize (documente comerciale pre-contabile).
//! NO GL, no VAT filing, no e-Factura. Contorizare proprie: OFR-/DEV-.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::invoices::{validate_and_total_lines, CreateInvoiceInput, CreateLineInput};
use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Models ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub id: String,
    pub company_id: String,
    pub contact_id: Option<String>,
    pub kind: String,
    pub series: Option<String>,
    pub number: i64,
    pub full_number: Option<String>,
    pub issue_date: String,
    pub valid_until: Option<String>,
    pub currency: String,
    pub exchange_rate: Option<String>,
    pub subtotal_amount: String,
    pub vat_amount: String,
    pub total_amount: String,
    pub status: String,
    pub notes: Option<String>,
    pub accepted_at: Option<i64>,
    pub converted_invoice_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct QuoteLine {
    pub id: String,
    pub quote_id: String,
    pub position: i64,
    pub name: String,
    pub description: Option<String>,
    pub quantity: String,
    pub unit: Option<String>,
    pub unit_price: String,
    pub vat_rate: String,
    pub vat_category: Option<String>,
    pub subtotal_amount: String,
    pub vat_amount: String,
    pub total_amount: String,
    pub revenue_kind: Option<String>,
    pub cost_section: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteWithLines {
    pub quote: Quote,
    pub lines: Vec<QuoteLine>,
}

// ─── Inputs ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateQuoteLineInput {
    pub name: String,
    pub description: Option<String>,
    pub quantity: f64,
    pub unit: Option<String>,
    pub unit_price: f64,
    pub vat_rate: f64,
    pub vat_category: String,
    pub revenue_kind: Option<String>,
    pub cost_section: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateQuoteInput {
    pub company_id: String,
    pub contact_id: Option<String>,
    pub kind: Option<String>,
    pub series: Option<String>,
    pub issue_date: String,
    pub valid_until: Option<String>,
    pub currency: Option<String>,
    pub exchange_rate: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<CreateQuoteLineInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateQuoteInput {
    pub contact_id: Option<String>,
    pub kind: Option<String>,
    pub series: Option<String>,
    pub issue_date: String,
    pub valid_until: Option<String>,
    pub currency: Option<String>,
    pub exchange_rate: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<CreateQuoteLineInput>,
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Map `CreateQuoteLineInput` → `CreateLineInput` for `validate_and_total_lines`.
fn to_invoice_line(q: &CreateQuoteLineInput) -> CreateLineInput {
    CreateLineInput {
        name: q.name.clone(),
        description: q.description.clone(),
        quantity: q.quantity,
        unit: q.unit.clone().unwrap_or_default(),
        unit_price: q.unit_price,
        vat_rate: q.vat_rate,
        vat_category: q.vat_category.clone(),
        cpv_code: None,
        art331_code: None,
        revenue_kind: q.revenue_kind.clone(),
    }
}

// ─── Queries ───────────────────────────────────────────────────────────────

pub async fn get(pool: &SqlitePool, id: &str) -> AppResult<Quote> {
    sqlx::query_as::<_, Quote>("SELECT * FROM quotes WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn get_with_lines(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<QuoteWithLines> {
    let quote =
        sqlx::query_as::<_, Quote>("SELECT * FROM quotes WHERE id = ?1 AND company_id = ?2")
            .bind(id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .ok_or(AppError::NotFound)?;

    let lines = sqlx::query_as::<_, QuoteLine>(
        "SELECT * FROM quote_lines WHERE quote_id = ?1 ORDER BY position ASC",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    Ok(QuoteWithLines { quote, lines })
}

pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Quote>> {
    let rows = sqlx::query_as::<_, Quote>(
        "SELECT * FROM quotes WHERE company_id = ?1 ORDER BY issue_date DESC, number DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ─── Create ────────────────────────────────────────────────────────────────

pub async fn create(pool: &SqlitePool, input: CreateQuoteInput) -> AppResult<Quote> {
    let kind = input.kind.as_deref().unwrap_or("quote");
    if kind != "quote" && kind != "deviz" {
        return Err(AppError::Validation(format!(
            "kind invalid: '{kind}' — acceptat: 'quote', 'deviz'."
        )));
    }

    // Convert lines for total calculation (pass "" as issue_date — non-fiscal doc).
    let invoice_lines: Vec<CreateLineInput> = input.lines.iter().map(to_invoice_line).collect();
    let (subtotal, vat_total, total, line_rows) = validate_and_total_lines(&invoice_lines, "")?;

    let series_default = if kind == "deviz" { "DEV" } else { "OFR" };
    let series = input
        .series
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(series_default);

    let quote_id = new_id();
    let now = now_unix();
    let currency = input.currency.as_deref().unwrap_or("RON");

    let mut tx = pool.begin().await?;

    // Allocate quote number atomically (never touches last_invoice_number).
    sqlx::query("UPDATE companies SET last_quote_number = last_quote_number + 1 WHERE id = ?1")
        .bind(&input.company_id)
        .execute(&mut *tx)
        .await?;

    let allocated_number: i64 =
        sqlx::query_scalar("SELECT last_quote_number FROM companies WHERE id = ?1")
            .bind(&input.company_id)
            .fetch_one(&mut *tx)
            .await?;

    let full_number = format!("{}-{:04}", series, allocated_number);

    sqlx::query(
        "INSERT INTO quotes (
            id, company_id, contact_id, kind, series, number, full_number,
            issue_date, valid_until, currency, exchange_rate,
            subtotal_amount, vat_amount, total_amount, status, notes,
            accepted_at, converted_invoice_id, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10, ?11,
            ?12, ?13, ?14, 'draft', ?15,
            NULL, NULL, ?16, ?16
        )",
    )
    .bind(&quote_id)
    .bind(&input.company_id)
    .bind(&input.contact_id)
    .bind(kind)
    .bind(series)
    .bind(allocated_number)
    .bind(&full_number)
    .bind(&input.issue_date)
    .bind(&input.valid_until)
    .bind(currency)
    .bind(&input.exchange_rate)
    .bind(&subtotal)
    .bind(&vat_total)
    .bind(&total)
    .bind(&input.notes)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    for (position, (q_line, (line_id, line_sub, line_vat, line_tot))) in
        input.lines.iter().zip(line_rows.iter()).enumerate()
    {
        let qty_str = Decimal::try_from(q_line.quantity)
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(6, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string();
        let price_str = Decimal::try_from(q_line.unit_price)
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string();
        let rate_str = {
            let raw = Decimal::try_from(q_line.vat_rate).unwrap_or(Decimal::ZERO);
            let eff = if q_line.vat_category == "S" {
                raw
            } else {
                Decimal::ZERO
            };
            eff.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                .to_string()
        };

        sqlx::query(
            "INSERT INTO quote_lines (
                id, quote_id, position, name, description,
                quantity, unit, unit_price, vat_rate, vat_category,
                subtotal_amount, vat_amount, total_amount, revenue_kind, cost_section
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15
            )",
        )
        .bind(line_id)
        .bind(&quote_id)
        .bind((position as i64) + 1)
        .bind(&q_line.name)
        .bind(&q_line.description)
        .bind(qty_str)
        .bind(&q_line.unit)
        .bind(price_str)
        .bind(rate_str)
        .bind(&q_line.vat_category)
        .bind(line_sub)
        .bind(line_vat)
        .bind(line_tot)
        .bind(&q_line.revenue_kind)
        .bind(&q_line.cost_section)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    get(pool, &quote_id).await
}

// ─── Update ────────────────────────────────────────────────────────────────

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: UpdateQuoteInput,
) -> AppResult<Quote> {
    let quote =
        sqlx::query_as::<_, Quote>("SELECT * FROM quotes WHERE id = ?1 AND company_id = ?2")
            .bind(id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .ok_or(AppError::NotFound)?;

    if quote.status != "draft" {
        return Err(AppError::Validation(format!(
            "Oferta/devizul poate fi modificat(ă) doar în status 'draft' (curent: '{}').",
            quote.status
        )));
    }

    let kind = input.kind.as_deref().unwrap_or(&quote.kind).to_string();

    let invoice_lines: Vec<CreateLineInput> = input.lines.iter().map(to_invoice_line).collect();
    let (subtotal, vat_total, total, line_rows) = validate_and_total_lines(&invoice_lines, "")?;

    let series_default = if kind == "deviz" { "DEV" } else { "OFR" };
    let series = input
        .series
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(quote.series.as_deref())
        .unwrap_or(series_default);
    let currency = input
        .currency
        .as_deref()
        .unwrap_or(&quote.currency)
        .to_string();
    let now = now_unix();

    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM quote_lines WHERE quote_id = ?1")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "UPDATE quotes SET
            contact_id = ?1, kind = ?2, series = ?3,
            issue_date = ?4, valid_until = ?5, currency = ?6, exchange_rate = ?7,
            subtotal_amount = ?8, vat_amount = ?9, total_amount = ?10,
            notes = ?11, updated_at = ?12
         WHERE id = ?13 AND company_id = ?14",
    )
    .bind(&input.contact_id)
    .bind(&kind)
    .bind(series)
    .bind(&input.issue_date)
    .bind(&input.valid_until)
    .bind(&currency)
    .bind(&input.exchange_rate)
    .bind(&subtotal)
    .bind(&vat_total)
    .bind(&total)
    .bind(&input.notes)
    .bind(now)
    .bind(id)
    .bind(company_id)
    .execute(&mut *tx)
    .await?;

    for (position, (q_line, (line_id, line_sub, line_vat, line_tot))) in
        input.lines.iter().zip(line_rows.iter()).enumerate()
    {
        let qty_str = Decimal::try_from(q_line.quantity)
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(6, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string();
        let price_str = Decimal::try_from(q_line.unit_price)
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string();
        let rate_str = {
            let raw = Decimal::try_from(q_line.vat_rate).unwrap_or(Decimal::ZERO);
            let eff = if q_line.vat_category == "S" {
                raw
            } else {
                Decimal::ZERO
            };
            eff.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                .to_string()
        };

        sqlx::query(
            "INSERT INTO quote_lines (
                id, quote_id, position, name, description,
                quantity, unit, unit_price, vat_rate, vat_category,
                subtotal_amount, vat_amount, total_amount, revenue_kind, cost_section
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15
            )",
        )
        .bind(line_id)
        .bind(id)
        .bind((position as i64) + 1)
        .bind(&q_line.name)
        .bind(&q_line.description)
        .bind(qty_str)
        .bind(&q_line.unit)
        .bind(price_str)
        .bind(rate_str)
        .bind(&q_line.vat_category)
        .bind(line_sub)
        .bind(line_vat)
        .bind(line_tot)
        .bind(&q_line.revenue_kind)
        .bind(&q_line.cost_section)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    get(pool, id).await
}

// ─── Status ────────────────────────────────────────────────────────────────

pub async fn set_status(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    status: &str,
) -> AppResult<Quote> {
    let quote =
        sqlx::query_as::<_, Quote>("SELECT * FROM quotes WHERE id = ?1 AND company_id = ?2")
            .bind(id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .ok_or(AppError::NotFound)?;

    // Status lifecycle validation.
    // "invoiced" may only be set by convert_to_invoice.
    let allowed: &[&str] = match quote.status.as_str() {
        "draft" => &["sent", "accepted", "cancelled"],
        "sent" => &["accepted", "cancelled", "expired"],
        "accepted" => &["cancelled"],
        _ => &[],
    };

    if !allowed.contains(&status) {
        return Err(AppError::Validation(format!(
            "Tranziție status invalidă: '{}' → '{status}'.",
            quote.status
        )));
    }

    let now = now_unix();
    let accepted_at = if status == "accepted" {
        Some(now)
    } else {
        None
    };

    if accepted_at.is_some() {
        sqlx::query(
            "UPDATE quotes SET status = ?1, accepted_at = ?2, updated_at = ?3
             WHERE id = ?4 AND company_id = ?5",
        )
        .bind(status)
        .bind(accepted_at)
        .bind(now)
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            "UPDATE quotes SET status = ?1, updated_at = ?2
             WHERE id = ?3 AND company_id = ?4",
        )
        .bind(status)
        .bind(now)
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    }

    get(pool, id).await
}

// ─── Delete ────────────────────────────────────────────────────────────────

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let quote =
        sqlx::query_as::<_, Quote>("SELECT * FROM quotes WHERE id = ?1 AND company_id = ?2")
            .bind(id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .ok_or(AppError::NotFound)?;

    if !["draft", "cancelled", "expired"].contains(&quote.status.as_str()) {
        return Err(AppError::Validation(format!(
            "Oferta/devizul poate fi șters(ă) doar în status 'draft', 'cancelled' sau 'expired' \
             (curent: '{}').",
            quote.status
        )));
    }

    sqlx::query("DELETE FROM quotes WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;

    Ok(())
}

// ─── Convert to invoice ────────────────────────────────────────────────────

pub async fn convert_to_invoice(
    pool: &SqlitePool,
    company_id: &str,
    quote_id: &str,
) -> AppResult<crate::db::invoices::Invoice> {
    // Atomic compare-and-set guard: claim the quote for conversion before creating any invoice.
    // This prevents a double fiscal number if the caller retries after a crash between
    // invoice creation and the status stamp below.
    let rows_affected = sqlx::query(
        "UPDATE quotes SET status = 'invoicing' \
         WHERE id = ?1 AND company_id = ?2 AND status = 'accepted' AND converted_invoice_id IS NULL",
    )
    .bind(quote_id)
    .bind(company_id)
    .execute(pool)
    .await?
    .rows_affected();

    if rows_affected == 0 {
        // Either already converting/converted, wrong status, or wrong company.
        let qwl = get_with_lines(pool, quote_id, company_id).await?;
        let quote = &qwl.quote;
        if quote.converted_invoice_id.is_some() || quote.status == "invoiced" {
            return Err(AppError::Validation(
                "Oferta/devizul a fost deja convertit(ă) într-o factură.".into(),
            ));
        }
        return Err(AppError::Validation(format!(
            "Oferta/devizul trebuie să fie în status 'accepted' pentru conversie (curent: '{}').",
            quote.status
        )));
    }

    let qwl = get_with_lines(pool, quote_id, company_id).await?;
    let quote = &qwl.quote;

    let issue_date = crate::db::models::now_unix();
    let issue_date_str = chrono::DateTime::from_timestamp(issue_date, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| quote.issue_date.clone());

    let inv_lines: Vec<CreateLineInput> = qwl
        .lines
        .iter()
        .map(|l| {
            let qty = l.quantity.parse::<f64>().unwrap_or(1.0);
            let price = l.unit_price.parse::<f64>().unwrap_or(0.0);
            let rate = l.vat_rate.parse::<f64>().unwrap_or(0.0);
            CreateLineInput {
                name: l.name.clone(),
                description: l.description.clone(),
                quantity: qty,
                unit: l.unit.clone().unwrap_or_default(),
                unit_price: price,
                vat_rate: rate,
                vat_category: l.vat_category.clone().unwrap_or_else(|| "S".into()),
                cpv_code: None,
                art331_code: None,
                revenue_kind: l.revenue_kind.clone(),
            }
        })
        .collect();

    let invoice_input = CreateInvoiceInput {
        company_id: company_id.to_string(),
        contact_id: quote.contact_id.clone().unwrap_or_default(),
        series: "FACT".to_string(),
        issue_date: issue_date_str.clone(),
        due_date: issue_date_str,
        currency: Some(quote.currency.clone()),
        exchange_rate: None,
        notes: quote.notes.clone(),
        payment_means_code: None,
        lines: inv_lines,
    };

    let invoice = crate::db::invoices::create(pool, invoice_input).await?;

    // Stamp quote: invoiced + converted_invoice_id.
    let now = now_unix();
    sqlx::query(
        "UPDATE quotes SET status = 'invoiced', converted_invoice_id = ?1, updated_at = ?2
         WHERE id = ?3 AND company_id = ?4",
    )
    .bind(&invoice.id)
    .bind(now)
    .bind(quote_id)
    .bind(company_id)
    .execute(pool)
    .await?;

    Ok(invoice)
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory DB");

        sqlx::query(
            "CREATE TABLE companies (
                id TEXT PRIMARY KEY,
                legal_name TEXT NOT NULL,
                last_invoice_number INTEGER NOT NULL DEFAULT 0,
                last_quote_number INTEGER NOT NULL DEFAULT 0,
                last_order_number INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE contacts (
                id TEXT PRIMARY KEY,
                company_id TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE invoices (
                id TEXT PRIMARY KEY,
                company_id TEXT NOT NULL,
                contact_id TEXT NOT NULL,
                series TEXT NOT NULL,
                number INTEGER NOT NULL,
                full_number TEXT NOT NULL,
                issue_date TEXT NOT NULL,
                due_date TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'RON',
                exchange_rate REAL,
                subtotal_amount TEXT NOT NULL,
                vat_amount TEXT NOT NULL,
                total_amount TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'DRAFT',
                anaf_upload_id TEXT,
                anaf_index TEXT,
                anaf_submitted_at INTEGER,
                anaf_validated_at INTEGER,
                anaf_rejected_at INTEGER,
                xml_path TEXT,
                pdf_path TEXT,
                signature_xml_path TEXT,
                rejection_reason TEXT,
                rejection_code TEXT,
                notes TEXT,
                payment_means_code TEXT NOT NULL DEFAULT '30',
                storno_of_invoice_id TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE(company_id, series, number)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE invoice_line_items (
                id TEXT PRIMARY KEY,
                invoice_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                name TEXT NOT NULL,
                description TEXT,
                quantity TEXT NOT NULL,
                unit TEXT NOT NULL DEFAULT '',
                unit_price TEXT NOT NULL,
                vat_rate TEXT NOT NULL,
                vat_category TEXT NOT NULL DEFAULT 'S',
                subtotal_amount TEXT NOT NULL,
                vat_amount TEXT NOT NULL,
                total_amount TEXT NOT NULL,
                cpv_code TEXT,
                art331_code TEXT,
                revenue_kind TEXT NOT NULL DEFAULT 'goods'
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE invoice_events (
                id TEXT PRIMARY KEY,
                invoice_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                message TEXT NOT NULL,
                metadata TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE quotes (
                id TEXT PRIMARY KEY NOT NULL,
                company_id TEXT NOT NULL,
                contact_id TEXT,
                kind TEXT NOT NULL DEFAULT 'quote',
                series TEXT,
                number INTEGER NOT NULL,
                full_number TEXT,
                issue_date TEXT NOT NULL,
                valid_until TEXT,
                currency TEXT NOT NULL DEFAULT 'RON',
                exchange_rate TEXT,
                subtotal_amount TEXT NOT NULL,
                vat_amount TEXT NOT NULL,
                total_amount TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'draft',
                notes TEXT,
                accepted_at INTEGER,
                converted_invoice_id TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE(company_id, series, number)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE quote_lines (
                id TEXT PRIMARY KEY NOT NULL,
                quote_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                name TEXT NOT NULL,
                description TEXT,
                quantity TEXT NOT NULL,
                unit TEXT,
                unit_price TEXT NOT NULL,
                vat_rate TEXT NOT NULL,
                vat_category TEXT,
                subtotal_amount TEXT NOT NULL,
                vat_amount TEXT NOT NULL,
                total_amount TEXT NOT NULL,
                revenue_kind TEXT,
                cost_section TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Seed a company.
        sqlx::query(
            "INSERT INTO companies (id, legal_name, last_invoice_number, last_quote_number, last_order_number)
             VALUES ('co1', 'Test SRL', 0, 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    fn sample_line() -> CreateQuoteLineInput {
        CreateQuoteLineInput {
            name: "Serviciu consultanță".into(),
            description: None,
            quantity: 2.0,
            unit: Some("ora".into()),
            unit_price: 100.0,
            vat_rate: 21.0,
            vat_category: "S".into(),
            revenue_kind: Some("service".into()),
            cost_section: None,
        }
    }

    fn sample_create_input() -> CreateQuoteInput {
        CreateQuoteInput {
            company_id: "co1".into(),
            contact_id: None,
            kind: None,
            series: None,
            issue_date: "2026-06-21".into(),
            valid_until: None,
            currency: None,
            exchange_rate: None,
            notes: None,
            lines: vec![sample_line()],
        }
    }

    #[tokio::test]
    async fn own_counter_does_not_touch_invoice_number() {
        let pool = setup_pool().await;

        create(&pool, sample_create_input()).await.unwrap();
        create(&pool, sample_create_input()).await.unwrap();

        let qnum: i64 =
            sqlx::query_scalar("SELECT last_quote_number FROM companies WHERE id='co1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let inum: i64 =
            sqlx::query_scalar("SELECT last_invoice_number FROM companies WHERE id='co1'")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(qnum, 2, "last_quote_number should be 2 after 2 quotes");
        assert_eq!(inum, 0, "last_invoice_number must stay 0");
    }

    #[tokio::test]
    async fn convert_quote_creates_draft_invoice() {
        let pool = setup_pool().await;

        let quote = create(&pool, sample_create_input()).await.unwrap();
        set_status(&pool, &quote.id, "co1", "accepted")
            .await
            .unwrap();

        let invoice = convert_to_invoice(&pool, "co1", &quote.id).await.unwrap();
        assert!(!invoice.id.is_empty());
        assert_eq!(invoice.status, "DRAFT");

        let updated_quote = get(&pool, &quote.id).await.unwrap();
        assert_eq!(updated_quote.status, "invoiced");
        assert_eq!(
            updated_quote.converted_invoice_id.as_deref(),
            Some(invoice.id.as_str())
        );
    }

    #[tokio::test]
    async fn convert_quote_idempotent() {
        let pool = setup_pool().await;

        let quote = create(&pool, sample_create_input()).await.unwrap();
        set_status(&pool, &quote.id, "co1", "accepted")
            .await
            .unwrap();
        convert_to_invoice(&pool, "co1", &quote.id).await.unwrap();

        // Second convert must fail.
        let result = convert_to_invoice(&pool, "co1", &quote.id).await;
        assert!(
            result.is_err(),
            "Converting an already-invoiced quote must return Err"
        );
    }

    #[tokio::test]
    async fn totals_match_shared_calculator() {
        let pool = setup_pool().await;

        let quote = create(&pool, sample_create_input()).await.unwrap();

        // Recalculate manually.
        let inv_lines: Vec<CreateLineInput> = sample_create_input()
            .lines
            .iter()
            .map(to_invoice_line)
            .collect();
        let (expected_sub, expected_vat, expected_total, _) =
            validate_and_total_lines(&inv_lines, "").unwrap();

        assert_eq!(quote.subtotal_amount, expected_sub);
        assert_eq!(quote.vat_amount, expected_vat);
        assert_eq!(quote.total_amount, expected_total);
    }

    #[tokio::test]
    async fn convert_in_invoicing_state_is_refused() {
        // FIX 5: simulate a crash after the compare-and-set claim (status='invoicing')
        // but before the stamp to 'invoiced'. A retry must be refused — no second invoice.
        let pool = setup_pool().await;

        let quote = create(&pool, sample_create_input()).await.unwrap();
        set_status(&pool, &quote.id, "co1", "accepted")
            .await
            .unwrap();

        // Manually force the 'invoicing' sentinel (simulates a crash mid-convert).
        sqlx::query("UPDATE quotes SET status='invoicing' WHERE id=?1")
            .bind(&quote.id)
            .execute(&pool)
            .await
            .unwrap();

        // A retry of convert_to_invoice must be refused — no second invoice minted.
        let result = convert_to_invoice(&pool, "co1", &quote.id).await;
        assert!(
            result.is_err(),
            "convert must refuse when status is 'invoicing' (FIX 5 — race guard)"
        );

        // Confirm no invoice was created for this quote.
        let inv_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(inv_count, 0, "no invoice must have been created");
    }
}
