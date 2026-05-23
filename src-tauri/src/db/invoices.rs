//! Facturi emise + linii + evenimente.
//!
//! O factură are 3 tabele asociate:
//! - `invoices` — header
//! - `invoice_line_items` — produse/servicii (1..N)
//! - `invoice_events` — istoric (submit, validate, reject)
//!
//! Money: la nivel DB folosim `f64` (REAL). Pentru calcule, convertește la
//! `rust_decimal::Decimal` în business logic.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix, InvoiceStatus, Page, Paginated};
use crate::error::{AppError, AppResult};

// ─── Models ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Invoice {
    pub id: String,
    pub company_id: String,
    pub contact_id: String,

    pub series: String,
    pub number: i64,
    pub full_number: String,

    pub issue_date: String,
    pub due_date: String,

    pub currency: String,
    pub exchange_rate: Option<f64>,

    pub subtotal_amount: f64,
    pub vat_amount: f64,
    pub total_amount: f64,

    pub status: String,

    pub anaf_upload_id: Option<String>,
    pub anaf_index: Option<String>,
    pub anaf_submitted_at: Option<i64>,
    pub anaf_validated_at: Option<i64>,
    pub anaf_rejected_at: Option<i64>,

    pub xml_path: Option<String>,
    pub pdf_path: Option<String>,
    pub signature_xml_path: Option<String>,

    pub rejection_reason: Option<String>,
    pub rejection_code: Option<String>,

    pub notes: Option<String>,
    pub payment_means_code: String,

    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct LineItem {
    pub id: String,
    pub invoice_id: String,

    pub position: i64,
    pub name: String,
    pub description: Option<String>,
    pub quantity: f64,
    pub unit: String,
    pub unit_price: f64,

    pub vat_rate: f64,
    pub vat_category: String,

    pub subtotal_amount: f64,
    pub vat_amount: f64,
    pub total_amount: f64,

    pub cpv_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceEvent {
    pub id: String,
    pub invoice_id: String,
    pub event_type: String,
    pub message: String,
    pub metadata: Option<String>,
    pub created_at: i64,
}

/// Bundle returnat pentru pagina de detaliu factură.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceWithLines {
    pub invoice: Invoice,
    pub lines: Vec<LineItem>,
    pub events: Vec<InvoiceEvent>,
}

// ─── Inputs ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLineInput {
    pub name: String,
    pub description: Option<String>,
    pub quantity: f64,
    pub unit: String,
    pub unit_price: f64,
    pub vat_rate: f64,
    pub vat_category: String,
    pub cpv_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInvoiceInput {
    pub company_id: String,
    pub contact_id: String,
    pub series: String,
    pub number: i64,
    pub issue_date: String,
    pub due_date: String,
    pub currency: Option<String>,
    pub exchange_rate: Option<f64>,
    pub notes: Option<String>,
    pub payment_means_code: Option<String>,
    pub lines: Vec<CreateLineInput>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceFilter {
    pub company_id: Option<String>,
    pub statuses: Option<Vec<InvoiceStatus>>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub query: Option<String>,
    pub page: Option<Page>,
}

// ─── Queries: list / get ───────────────────────────────────────────────────

const SELECT_INVOICE: &str = "id, company_id, contact_id, series, number, full_number, \
    issue_date, due_date, currency, exchange_rate, subtotal_amount, vat_amount, total_amount, \
    status, anaf_upload_id, anaf_index, anaf_submitted_at, anaf_validated_at, anaf_rejected_at, \
    xml_path, pdf_path, signature_xml_path, rejection_reason, rejection_code, notes, \
    payment_means_code, created_at, updated_at";

const SELECT_LINE: &str = "id, invoice_id, position, name, description, quantity, unit, \
    unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, total_amount, cpv_code";

const SELECT_EVENT: &str = "id, invoice_id, event_type, message, metadata, created_at";

pub async fn list(pool: &SqlitePool, filter: InvoiceFilter) -> AppResult<Paginated<Invoice>> {
    let page = filter.page.unwrap_or_default();

    // Construim WHERE-ul progresiv. Binding-urile sunt apoi adăugate în
    // aceeași ordine.
    let mut where_clauses = vec!["1=1".to_string()];
    let mut binds: Vec<String> = Vec::new();

    if let Some(cid) = &filter.company_id {
        where_clauses.push(format!("company_id = ?{}", binds.len() + 1));
        binds.push(cid.clone());
    }
    if let Some(statuses) = &filter.statuses {
        if !statuses.is_empty() {
            let placeholders: Vec<String> = statuses
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", binds.len() + i + 1))
                .collect();
            where_clauses.push(format!("status IN ({})", placeholders.join(",")));
            binds.extend(statuses.iter().map(|s| s.as_str().to_string()));
        }
    }
    if let Some(from) = &filter.date_from {
        where_clauses.push(format!("issue_date >= ?{}", binds.len() + 1));
        binds.push(from.clone());
    }
    if let Some(to) = &filter.date_to {
        where_clauses.push(format!("issue_date <= ?{}", binds.len() + 1));
        binds.push(to.clone());
    }
    if let Some(query) = &filter.query {
        where_clauses.push(format!(
            "(full_number LIKE ?{} OR notes LIKE ?{})",
            binds.len() + 1,
            binds.len() + 1
        ));
        binds.push(format!("%{query}%"));
    }

    let where_sql = where_clauses.join(" AND ");

    // Count total (separat de pagination).
    let count_sql = format!("SELECT COUNT(*) FROM invoices WHERE {where_sql}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        count_q = count_q.bind(b);
    }
    let total: i64 = count_q.fetch_one(pool).await?;

    // Page de date.
    let limit_offset = format!(
        " ORDER BY issue_date DESC, number DESC LIMIT ?{} OFFSET ?{}",
        binds.len() + 1,
        binds.len() + 2
    );
    let sql = format!("SELECT {SELECT_INVOICE} FROM invoices WHERE {where_sql}{limit_offset}");

    let mut q = sqlx::query_as::<_, Invoice>(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    q = q.bind(page.limit).bind(page.offset);

    let items = q.fetch_all(pool).await?;

    Ok(Paginated {
        items,
        total,
        offset: page.offset,
        limit: page.limit,
    })
}

pub async fn get(pool: &SqlitePool, id: &str) -> AppResult<Invoice> {
    let sql = format!("SELECT {SELECT_INVOICE} FROM invoices WHERE id = ?1");
    sqlx::query_as::<_, Invoice>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn get_with_lines(pool: &SqlitePool, id: &str) -> AppResult<InvoiceWithLines> {
    let invoice = get(pool, id).await?;
    let lines = list_lines(pool, id).await?;
    let events = list_events(pool, id).await?;
    Ok(InvoiceWithLines { invoice, lines, events })
}

async fn list_lines(pool: &SqlitePool, invoice_id: &str) -> AppResult<Vec<LineItem>> {
    let sql = format!(
        "SELECT {SELECT_LINE} FROM invoice_line_items WHERE invoice_id = ?1 ORDER BY position"
    );
    Ok(sqlx::query_as::<_, LineItem>(&sql)
        .bind(invoice_id)
        .fetch_all(pool)
        .await?)
}

async fn list_events(pool: &SqlitePool, invoice_id: &str) -> AppResult<Vec<InvoiceEvent>> {
    let sql = format!(
        "SELECT {SELECT_EVENT} FROM invoice_events WHERE invoice_id = ?1 ORDER BY created_at"
    );
    Ok(sqlx::query_as::<_, InvoiceEvent>(&sql)
        .bind(invoice_id)
        .fetch_all(pool)
        .await?)
}

// ─── Create / Update ───────────────────────────────────────────────────────

/// Creează factură + liniile asociate într-o tranzacție. Totalurile sunt
/// calculate aici (sumă subtotal + VAT din linii).
pub async fn create(pool: &SqlitePool, input: CreateInvoiceInput) -> AppResult<Invoice> {
    if input.lines.is_empty() {
        return Err(AppError::Validation(
            "Factura trebuie să aibă cel puțin o linie.".into(),
        ));
    }

    let invoice_id = new_id();
    let now = now_unix();
    let full_number = format!("{}-{:04}", input.series, input.number);

    // Calculăm totaluri cu Decimal pentru precizie (money math — niciodată f64).
    use rust_decimal::Decimal;
    use rust_decimal::prelude::ToPrimitive;
    let hundred = Decimal::from(100u32);

    let mut subtotal_dec = Decimal::ZERO;
    let mut vat_total_dec = Decimal::ZERO;
    let line_rows: Vec<(String, f64, f64, f64)> = input
        .lines
        .iter()
        .map(|l| {
            let qty   = Decimal::try_from(l.quantity).unwrap_or(Decimal::ZERO);
            let price = Decimal::try_from(l.unit_price).unwrap_or(Decimal::ZERO);
            let rate  = Decimal::try_from(l.vat_rate).unwrap_or(Decimal::ZERO);
            let ls = (qty * price).round_dp(2);
            let lv = (ls * rate / hundred).round_dp(2);
            let lt = ls + lv;
            subtotal_dec += ls;
            vat_total_dec += lv;
            (
                new_id(),
                ls.to_f64().unwrap_or(0.0),
                lv.to_f64().unwrap_or(0.0),
                lt.to_f64().unwrap_or(0.0),
            )
        })
        .collect();
    let subtotal  = subtotal_dec.to_f64().unwrap_or(0.0);
    let vat_total = vat_total_dec.to_f64().unwrap_or(0.0);
    let total     = (subtotal_dec + vat_total_dec).to_f64().unwrap_or(0.0);

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO invoices (
            id, company_id, contact_id, series, number, full_number,
            issue_date, due_date, currency, exchange_rate,
            subtotal_amount, vat_amount, total_amount, status, notes,
            payment_means_code, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, 'DRAFT', ?14,
            ?15, ?16, ?16
        )",
    )
    .bind(&invoice_id)
    .bind(&input.company_id)
    .bind(&input.contact_id)
    .bind(&input.series)
    .bind(input.number)
    .bind(&full_number)
    .bind(&input.issue_date)
    .bind(&input.due_date)
    .bind(input.currency.as_deref().unwrap_or("RON"))
    .bind(input.exchange_rate)
    .bind(subtotal)
    .bind(vat_total)
    .bind(total)
    .bind(&input.notes)
    .bind(input.payment_means_code.as_deref().unwrap_or("30"))
    .bind(now)
    .execute(&mut *tx)
    .await?;

    for (position, (line, (line_id, line_subtotal, line_vat, line_total))) in
        input.lines.iter().zip(line_rows.iter()).enumerate()
    {
        sqlx::query(
            "INSERT INTO invoice_line_items (
                id, invoice_id, position, name, description,
                quantity, unit, unit_price, vat_rate, vat_category,
                subtotal_amount, vat_amount, total_amount, cpv_code
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14
            )",
        )
        .bind(line_id)
        .bind(&invoice_id)
        .bind((position as i64) + 1)
        .bind(&line.name)
        .bind(&line.description)
        .bind(line.quantity)
        .bind(&line.unit)
        .bind(line.unit_price)
        .bind(line.vat_rate)
        .bind(&line.vat_category)
        .bind(line_subtotal)
        .bind(line_vat)
        .bind(line_total)
        .bind(&line.cpv_code)
        .execute(&mut *tx)
        .await?;
    }

    // Eveniment audit.
    sqlx::query(
        "INSERT INTO invoice_events (id, invoice_id, event_type, message, created_at)
         VALUES (?1, ?2, 'CREATED', 'Factură creată ca ciornă', ?3)",
    )
    .bind(new_id())
    .bind(&invoice_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get(pool, &invoice_id).await
}

// ─── Status transitions ────────────────────────────────────────────────────

pub async fn set_status(
    pool: &SqlitePool,
    id: &str,
    status: InvoiceStatus,
    message: Option<String>,
) -> AppResult<()> {
    let now = now_unix();
    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE invoices SET status = ?2, updated_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(status.as_str())
        .bind(now)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "INSERT INTO invoice_events (id, invoice_id, event_type, message, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(new_id())
    .bind(id)
    .bind(format!("STATUS_{}", status.as_str()))
    .bind(message.unwrap_or_else(|| format!("Status schimbat în {}", status.as_str())))
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn mark_submitted(pool: &SqlitePool, id: &str, upload_id: &str) -> AppResult<()> {
    let now = now_unix();
    sqlx::query(
        "UPDATE invoices SET
            status            = 'SUBMITTED',
            anaf_upload_id    = ?2,
            anaf_submitted_at = ?3,
            updated_at        = ?3
        WHERE id = ?1",
    )
    .bind(id)
    .bind(upload_id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_xml_path(pool: &SqlitePool, id: &str, path: &str) -> AppResult<()> {
    let now = now_unix();
    sqlx::query("UPDATE invoices SET xml_path = ?2, updated_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(path)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_validated(
    pool: &SqlitePool,
    id: &str,
    anaf_index: Option<String>,
) -> AppResult<()> {
    let now = now_unix();
    sqlx::query(
        "UPDATE invoices SET
            status           = 'VALIDATED',
            anaf_index       = ?2,
            anaf_validated_at = ?3,
            updated_at        = ?3
        WHERE id = ?1",
    )
    .bind(id)
    .bind(anaf_index)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_rejected(
    pool: &SqlitePool,
    id: &str,
    reason: Option<String>,
    code: Option<String>,
) -> AppResult<()> {
    let now = now_unix();
    sqlx::query(
        "UPDATE invoices SET
            status           = 'REJECTED',
            rejection_reason = ?2,
            rejection_code   = ?3,
            anaf_rejected_at = ?4,
            updated_at       = ?4
        WHERE id = ?1",
    )
    .bind(id)
    .bind(reason)
    .bind(code)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_submitted(
    pool: &SqlitePool,
    company_id: &str,
) -> AppResult<Vec<Invoice>> {
    let sql = format!(
        "SELECT {SELECT_INVOICE} FROM invoices \
         WHERE company_id = ?1 AND status = 'SUBMITTED' \
         ORDER BY anaf_submitted_at"
    );
    Ok(sqlx::query_as::<_, Invoice>(&sql)
        .bind(company_id)
        .fetch_all(pool)
        .await?)
}

pub async fn set_pdf_path(pool: &SqlitePool, id: &str, path: &str) -> AppResult<()> {
    let now = now_unix();
    sqlx::query("UPDATE invoices SET pdf_path = ?2, updated_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(path)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let invoice = get(pool, id).await?;
    if invoice.status != "DRAFT" {
        return Err(AppError::Validation(
            "Se pot șterge doar ciornele. Pentru facturile trimise folosiți Storno.".into(),
        ));
    }
    sqlx::query("DELETE FROM invoices WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
