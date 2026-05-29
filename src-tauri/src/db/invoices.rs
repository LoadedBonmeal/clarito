//! Facturi emise + linii + evenimente.
//!
//! O factură are 3 tabele asociate:
//! - `invoices` — header
//! - `invoice_line_items` — produse/servicii (1..N)
//! - `invoice_events` — istoric (submit, validate, reject)
//!
//! Money: stocat ca TEXT (Decimal string) în DB. Exchange rate rămâne REAL (rată FX, nu bani).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix, InvoiceStatus, Page, Paginated, VALID_VAT_RATES};
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

    pub subtotal_amount: String,
    pub vat_amount: String,
    pub total_amount: String,

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
    pub quantity: String,
    pub unit: String,
    pub unit_price: String,

    pub vat_rate: String,
    pub vat_category: String,

    pub subtotal_amount: String,
    pub vat_amount: String,
    pub total_amount: String,

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

pub async fn list(pool: &SqlitePool, filter: InvoiceFilter) -> AppResult<Paginated<Invoice>> {
    let page = filter.page.unwrap_or_default();

    // Normalizăm filtrele opționale: string gol → None (tratat ca NULL în SQL).
    let company_id = filter.company_id.as_ref().filter(|s| !s.is_empty());
    let date_from = filter.date_from.as_ref().filter(|s| !s.is_empty());
    let date_to = filter.date_to.as_ref().filter(|s| !s.is_empty());
    let query_term = filter.query.as_ref().filter(|s| !s.is_empty());

    // Statusurile sunt o listă cu număr variabil de elemente. Le extindem
    // manual la OR-uri pentru cele 6 valori posibile ale enum-ului, astfel
    // încât SQL-ul rămâne static.
    //
    // Dacă `filter.statuses` e None sau goală, toate statusurile trec.
    let statuses = filter.statuses.as_deref().unwrap_or(&[]);
    let has_status_filter = !statuses.is_empty();
    let want_draft = has_status_filter && statuses.contains(&InvoiceStatus::Draft);
    let want_queued = has_status_filter && statuses.contains(&InvoiceStatus::Queued);
    let want_submitted = has_status_filter && statuses.contains(&InvoiceStatus::Submitted);
    let want_validated = has_status_filter && statuses.contains(&InvoiceStatus::Validated);
    let want_rejected = has_status_filter && statuses.contains(&InvoiceStatus::Rejected);
    let want_storned = has_status_filter && statuses.contains(&InvoiceStatus::Storned);

    // SQL static cu toate filtrele opționale exprimate ca predicate nullable.
    // ?1  company_id       (Option<&str>)
    // ?2  date_from        (Option<&str>)
    // ?3  date_to          (Option<&str>)
    // ?4  query_term       (Option<&str>) — legat fără %; LIKE concatenează în SQL
    // ?5  has_status_filter (bool → i64)
    // ?6..?11  want_DRAFT/QUEUED/SUBMITTED/VALIDATED/REJECTED/STORNED (bool → i64)
    // ?12 limit, ?13 offset
    let count_sql = "\
        SELECT COUNT(*) FROM invoices \
        WHERE (?1 IS NULL OR company_id = ?1) \
          AND (?2 IS NULL OR issue_date >= ?2) \
          AND (?3 IS NULL OR issue_date <= ?3) \
          AND (?4 IS NULL OR full_number LIKE '%' || ?4 || '%' OR notes LIKE '%' || ?4 || '%') \
          AND (NOT ?5 OR status = CASE WHEN ?6  THEN 'DRAFT'     ELSE NULL END \
                      OR status = CASE WHEN ?7  THEN 'QUEUED'    ELSE NULL END \
                      OR status = CASE WHEN ?8  THEN 'SUBMITTED' ELSE NULL END \
                      OR status = CASE WHEN ?9  THEN 'VALIDATED' ELSE NULL END \
                      OR status = CASE WHEN ?10 THEN 'REJECTED'  ELSE NULL END \
                      OR status = CASE WHEN ?11 THEN 'STORNED'   ELSE NULL END)";

    let total: i64 = sqlx::query_scalar(count_sql)
        .bind(company_id)
        .bind(date_from)
        .bind(date_to)
        .bind(query_term)
        .bind(has_status_filter as i64)
        .bind(want_draft as i64)
        .bind(want_queued as i64)
        .bind(want_submitted as i64)
        .bind(want_validated as i64)
        .bind(want_rejected as i64)
        .bind(want_storned as i64)
        .fetch_one(pool)
        .await?;

    let data_sql = "SELECT id, company_id, contact_id, series, number, full_number, \
         issue_date, due_date, currency, exchange_rate, subtotal_amount, vat_amount, total_amount, \
         status, anaf_upload_id, anaf_index, anaf_submitted_at, anaf_validated_at, anaf_rejected_at, \
         xml_path, pdf_path, signature_xml_path, rejection_reason, rejection_code, notes, \
         payment_means_code, created_at, updated_at \
         FROM invoices \
         WHERE (?1 IS NULL OR company_id = ?1) \
           AND (?2 IS NULL OR issue_date >= ?2) \
           AND (?3 IS NULL OR issue_date <= ?3) \
           AND (?4 IS NULL OR full_number LIKE '%' || ?4 || '%' OR notes LIKE '%' || ?4 || '%') \
           AND (NOT ?5 OR status = CASE WHEN ?6  THEN 'DRAFT'     ELSE NULL END \
                       OR status = CASE WHEN ?7  THEN 'QUEUED'    ELSE NULL END \
                       OR status = CASE WHEN ?8  THEN 'SUBMITTED' ELSE NULL END \
                       OR status = CASE WHEN ?9  THEN 'VALIDATED' ELSE NULL END \
                       OR status = CASE WHEN ?10 THEN 'REJECTED'  ELSE NULL END \
                       OR status = CASE WHEN ?11 THEN 'STORNED'   ELSE NULL END) \
         ORDER BY issue_date DESC, number DESC \
         LIMIT ?12 OFFSET ?13";

    let items = sqlx::query_as::<_, Invoice>(data_sql)
        .bind(company_id)
        .bind(date_from)
        .bind(date_to)
        .bind(query_term)
        .bind(has_status_filter as i64)
        .bind(want_draft as i64)
        .bind(want_queued as i64)
        .bind(want_submitted as i64)
        .bind(want_validated as i64)
        .bind(want_rejected as i64)
        .bind(want_storned as i64)
        .bind(page.limit)
        .bind(page.offset)
        .fetch_all(pool)
        .await?;

    Ok(Paginated {
        items,
        total,
        offset: page.offset,
        limit: page.limit,
    })
}

pub async fn get(pool: &SqlitePool, id: &str) -> AppResult<Invoice> {
    sqlx::query_as::<_, Invoice>(
        "SELECT id, company_id, contact_id, series, number, full_number, \
         issue_date, due_date, currency, exchange_rate, subtotal_amount, vat_amount, total_amount, \
         status, anaf_upload_id, anaf_index, anaf_submitted_at, anaf_validated_at, anaf_rejected_at, \
         xml_path, pdf_path, signature_xml_path, rejection_reason, rejection_code, notes, \
         payment_means_code, created_at, updated_at \
         FROM invoices WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn get_with_lines(pool: &SqlitePool, id: &str) -> AppResult<InvoiceWithLines> {
    let invoice = get(pool, id).await?;
    let lines = list_lines(pool, id).await?;
    let events = list_events(pool, id).await?;
    Ok(InvoiceWithLines {
        invoice,
        lines,
        events,
    })
}

async fn list_lines(pool: &SqlitePool, invoice_id: &str) -> AppResult<Vec<LineItem>> {
    Ok(sqlx::query_as::<_, LineItem>(
        "SELECT id, invoice_id, position, name, description, quantity, unit, \
         unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, total_amount, cpv_code \
         FROM invoice_line_items WHERE invoice_id = ?1 ORDER BY position",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await?)
}

async fn list_events(pool: &SqlitePool, invoice_id: &str) -> AppResult<Vec<InvoiceEvent>> {
    Ok(sqlx::query_as::<_, InvoiceEvent>(
        "SELECT id, invoice_id, event_type, message, metadata, created_at \
         FROM invoice_events WHERE invoice_id = ?1 ORDER BY created_at",
    )
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

    for line in &input.lines {
        let rate = Decimal::try_from(line.vat_rate).unwrap_or(Decimal::ZERO);
        if !VALID_VAT_RATES
            .iter()
            .any(|&r| (Decimal::from(r) - rate).abs() < Decimal::new(1, 3))
        {
            return Err(AppError::Validation(format!(
                "Cotă TVA invalidă: {}%. Valori permise: 0, 5, 9, 11, 19, 21.",
                line.vat_rate
            )));
        }
    }

    let invoice_id = new_id();
    let now = now_unix();

    // Calculăm totaluri cu Decimal pentru precizie (money math — niciodată f64).
    let hundred = Decimal::from(100u32);

    let mut subtotal_dec = Decimal::ZERO;
    let mut vat_total_dec = Decimal::ZERO;
    let line_rows: Vec<(String, String, String, String)> = input
        .lines
        .iter()
        .map(|l| {
            let qty = Decimal::try_from(l.quantity).unwrap_or(Decimal::ZERO);
            let price = Decimal::try_from(l.unit_price).unwrap_or(Decimal::ZERO);
            let rate = Decimal::try_from(l.vat_rate).unwrap_or(Decimal::ZERO);
            let ls = (qty * price).round_dp(2);
            let lv = (ls * rate / hundred).round_dp(2);
            let lt = ls + lv;
            subtotal_dec += ls;
            vat_total_dec += lv;
            (
                new_id(),
                ls.round_dp(2).to_string(),
                lv.round_dp(2).to_string(),
                lt.round_dp(2).to_string(),
            )
        })
        .collect();
    let subtotal = subtotal_dec.round_dp(2).to_string();
    let vat_total = vat_total_dec.round_dp(2).to_string();
    let total = (subtotal_dec + vat_total_dec).round_dp(2).to_string();

    let mut tx = pool.begin().await?;

    // Alocăm numărul atomic în aceeași tranzacție pentru a evita goluri de numerotare.
    // `input.number` este ignorat — numărul real e întotdeauna alocat aici.
    sqlx::query("UPDATE companies SET last_invoice_number = last_invoice_number + 1 WHERE id = ?1")
        .bind(&input.company_id)
        .execute(&mut *tx)
        .await?;

    let allocated_number: i64 =
        sqlx::query_scalar("SELECT last_invoice_number FROM companies WHERE id = ?1")
            .bind(&input.company_id)
            .fetch_one(&mut *tx)
            .await?;

    let full_number = format!("{}-{:04}", input.series, allocated_number);

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
    .bind(allocated_number)
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
        .bind(
            Decimal::try_from(line.quantity)
                .unwrap_or(Decimal::ZERO)
                .round_dp(2)
                .to_string(),
        )
        .bind(&line.unit)
        .bind(
            Decimal::try_from(line.unit_price)
                .unwrap_or(Decimal::ZERO)
                .round_dp(2)
                .to_string(),
        )
        .bind(
            Decimal::try_from(line.vat_rate)
                .unwrap_or(Decimal::ZERO)
                .round_dp(2)
                .to_string(),
        )
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

pub async fn list_submitted(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Invoice>> {
    Ok(sqlx::query_as::<_, Invoice>(
        "SELECT id, company_id, contact_id, series, number, full_number, \
         issue_date, due_date, currency, exchange_rate, subtotal_amount, vat_amount, total_amount, \
         status, anaf_upload_id, anaf_index, anaf_submitted_at, anaf_validated_at, anaf_rejected_at, \
         xml_path, pdf_path, signature_xml_path, rejection_reason, rejection_code, notes, \
         payment_means_code, created_at, updated_at \
         FROM invoices \
         WHERE company_id = ?1 AND status = 'SUBMITTED' \
         ORDER BY anaf_submitted_at",
    )
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
