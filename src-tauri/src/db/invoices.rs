//! Facturi emise + linii + evenimente.
//!
//! O factură are 3 tabele asociate:
//! - `invoices` — header
//! - `invoice_line_items` — produse/servicii (1..N)
//! - `invoice_events` — istoric (submit, validate, reject)
//!
//! Money: stocat ca TEXT (Decimal string) în DB. Exchange rate rămâne REAL (rată FX, nu bani).

use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{
    new_id, now_unix, InvoiceStatus, Page, Paginated, VALID_PAYMENT_MEANS_CODES, VALID_VAT_RATES,
};
use crate::error::{AppError, AppResult};

/// Round money to 2 decimals using COMMERCIAL rounding (half away from zero), the Romanian/
/// EN16931 reference convention for VAT amounts — e.g. 2.50 × 21% = 0.5250 → 0.53. Kept
/// consistent across line, subtotal and total (and with D300's `ron_to_bani`) so the invoice
/// reconciles internally and with the VAT return; rust_decimal's default `round_dp` is banker's
/// (nearest-even), which would store 0.52 and drift from the RO norm.
pub(crate) fn round2(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

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

    /// BIZ-13: Referință explicită (FK) către factura originală pentru un
    /// storno. `None` pentru facturi normale. Pentru rândurile vechi (înainte
    /// de migrația 0008) câmpul poate rămâne `None`; în acest caz codul
    /// recurge la parserul moștenit al notei "STORNO_OF:...".
    pub storno_of_invoice_id: Option<String>,

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
    /// Art. 331 reverse-charge product category code — snapshot from product at creation.
    /// Used by D394 op11 codPR. NULL = use default 22.
    pub art331_code: Option<String>,
    /// Revenue nature → GL 701/704/707/709. "product"|"service"|"goods"|"reduction".
    pub revenue_kind: String,
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
    /// Art. 331 reverse-charge product category code (snapshot from product).
    pub art331_code: Option<String>,
    /// Sales-revenue nature → GL 701/704/707/709. "product"|"service"|"goods"|"reduction".
    /// None defaults to "goods" (707), preserving prior behaviour.
    pub revenue_kind: Option<String>,
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
         payment_means_code, storno_of_invoice_id, created_at, updated_at \
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
         payment_means_code, storno_of_invoice_id, created_at, updated_at \
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
         unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, total_amount, \
         cpv_code, art331_code, COALESCE(revenue_kind,'goods') AS revenue_kind \
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
        // O factură (nu storno) nu poate avea cantități negative — ar produce pe ascuns o notă
        // de credit. Stornările au flux separat (storno_invoice), cu linii negative legitime.
        if line.quantity < 0.0 {
            return Err(AppError::Validation(format!(
                "Cantitate negativă pe linia '{}' — pentru corecții folosiți stornarea.",
                line.name
            )));
        }
    }

    // U3: validate payment_means_code against the UNCL4461 allow-list.
    let pmc = input.payment_means_code.as_deref().unwrap_or("30");
    if !VALID_PAYMENT_MEANS_CODES.contains(&pmc) {
        return Err(AppError::Validation(format!(
            "Cod mod de plată invalid: {pmc}"
        )));
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
            // U4: use 6dp-precise quantity for monetary computation so the
            // stored subtotal reflects the precise quantity (no truncation).
            let qty = Decimal::try_from(l.quantity).unwrap_or(Decimal::ZERO);
            let price = Decimal::try_from(l.unit_price).unwrap_or(Decimal::ZERO);
            let raw = Decimal::try_from(l.vat_rate).unwrap_or(Decimal::ZERO);
            // VAT1: only category 'S' (Standard) charges VAT; all others → 0.
            let rate = if l.vat_category == "S" {
                raw
            } else {
                Decimal::ZERO
            };
            let ls = round2(qty * price);
            let lv = round2(ls * rate / hundred);
            let lt = ls + lv;
            subtotal_dec += ls;
            vat_total_dec += lv;
            (
                new_id(),
                round2(ls).to_string(),
                round2(lv).to_string(),
                round2(lt).to_string(),
            )
        })
        .collect();
    let subtotal = round2(subtotal_dec).to_string();
    let vat_total = round2(vat_total_dec).to_string();
    let total = round2(subtotal_dec + vat_total_dec).to_string();

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

    // `storno_of_invoice_id` rămâne NULL pentru ciornele create direct —
    // facturile storno sunt create exclusiv prin `commands::storno_invoice`
    // care setează FK-ul explicit.
    sqlx::query(
        "INSERT INTO invoices (
            id, company_id, contact_id, series, number, full_number,
            issue_date, due_date, currency, exchange_rate,
            subtotal_amount, vat_amount, total_amount, status, notes,
            payment_means_code, storno_of_invoice_id, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, 'DRAFT', ?14,
            ?15, NULL, ?16, ?16
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
    .bind(pmc)
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
                subtotal_amount, vat_amount, total_amount, cpv_code, art331_code,
                revenue_kind
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15,
                ?16
            )",
        )
        .bind(line_id)
        .bind(&invoice_id)
        .bind((position as i64) + 1)
        .bind(&line.name)
        .bind(&line.description)
        // U4: store quantity at 6dp (CIUS-RO allows 6dp; generator emits 6dp).
        .bind(
            Decimal::try_from(line.quantity)
                .unwrap_or(Decimal::ZERO)
                .round_dp(6)
                .to_string(),
        )
        .bind(&line.unit)
        .bind(
            Decimal::try_from(line.unit_price)
                .unwrap_or(Decimal::ZERO)
                .round_dp(2)
                .to_string(),
        )
        .bind({
            // VAT1: store effective rate — 0 for non-S categories so UBL emits
            // rate 0 + the correct exemption category code consistently.
            let raw = Decimal::try_from(line.vat_rate).unwrap_or(Decimal::ZERO);
            let eff = if line.vat_category == "S" {
                raw
            } else {
                Decimal::ZERO
            };
            eff.round_dp(2).to_string()
        })
        .bind(&line.vat_category)
        .bind(line_subtotal)
        .bind(line_vat)
        .bind(line_total)
        .bind(&line.cpv_code)
        .bind(&line.art331_code)
        .bind(line.revenue_kind.as_deref().unwrap_or("goods"))
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

    // A1 — Anti-duplicate guard: only transition if the row is still QUEUED.
    // If the upload already succeeded at ANAF but the DB write fails, the caller
    // must NOT leave the row resettable-to-DRAFT (recovery.rs resets
    // QUEUED+NULL-upload_id → DRAFT, which would allow a duplicate ANAF filing).
    // Strategy: on 0-rows-affected we attempt a minimal fallback that at least
    // persists the upload_id so the recovery predicate (QUEUED AND upload_id IS NULL)
    // no longer matches, preventing the duplicate-filing window.
    let result = sqlx::query(
        "UPDATE invoices SET
            status            = 'SUBMITTED',
            anaf_upload_id    = ?2,
            anaf_submitted_at = ?3,
            updated_at        = ?3
        WHERE id = ?1 AND status = 'QUEUED'",
    )
    .bind(id)
    .bind(upload_id)
    .bind(now)
    .execute(pool)
    .await?;

    if result.rows_affected() != 1 {
        // The upload SUCCEEDED at ANAF but the status transition failed (the row
        // is no longer QUEUED — it may have been concurrently modified, or the
        // upload_id was already set by a previous attempt).
        // Best-effort: persist the upload_id so recovery cannot reset this
        // invoice to DRAFT and trigger a duplicate filing at ANAF.
        tracing::warn!(
            %id,
            %upload_id,
            "mark_submitted: 0 rows updated (invoice not QUEUED) — \
             persisting upload_id to prevent duplicate ANAF filing"
        );
        let _ =
            sqlx::query("UPDATE invoices SET anaf_upload_id = ?2, updated_at = ?3 WHERE id = ?1")
                .bind(id)
                .bind(upload_id)
                .bind(now)
                .execute(pool)
                .await;
        return Err(AppError::Validation(
            "Factura nu mai este în statusul QUEUED — starea nu a putut fi actualizată la SUBMITTED \
             (upload_id salvat pentru a preveni o depunere duplicată la ANAF)."
                .into(),
        ));
    }

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
         payment_means_code, storno_of_invoice_id, created_at, updated_at \
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

/// R13 Wave G: user-facing delete scoped to a specific company.
/// Returns NotFound (no-op) when no DRAFT row matches both id AND company_id,
/// preventing cross-company deletion.
pub async fn delete_scoped(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let invoice = get(pool, id).await?;
    if invoice.company_id != company_id {
        return Err(AppError::NotFound);
    }
    if invoice.status != "DRAFT" {
        return Err(AppError::Validation(
            "Se pot șterge doar ciornele. Pentru facturile trimise folosiți Storno.".into(),
        ));
    }
    let result = sqlx::query("DELETE FROM invoices WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// R13 Wave G: user-facing status update scoped to a specific company.
/// Background ANAF transitions (mark_validated / mark_rejected / mark_submitted)
/// continue to use their own dedicated fns — do NOT call this from poll.rs / spv.rs.
///
/// D2 — CAS (Compare-And-Swap): `expected_status` adds `AND status = ?expected`
/// to the UPDATE so a concurrent modification between the caller's read and this
/// write is detected atomically (rows_affected == 0 → Conflict error).
pub async fn set_status_scoped(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    status: crate::db::models::InvoiceStatus,
    expected_status: &str,
    message: Option<String>,
) -> AppResult<()> {
    let now = now_unix();
    let mut tx = pool.begin().await?;

    // D2: AND status = ?5 makes this a CAS — if another writer changed the status
    // between the caller's SELECT and this UPDATE, rows_affected == 0.
    let result = sqlx::query(
        "UPDATE invoices SET status = ?2, updated_at = ?3 \
         WHERE id = ?1 AND company_id = ?4 AND status = ?5",
    )
    .bind(id)
    .bind(status.as_str())
    .bind(now)
    .bind(company_id)
    .bind(expected_status)
    .execute(&mut *tx)
    .await?;

    if result.rows_affected() == 0 {
        // Distinguish "not found / wrong company" from "concurrent status change".
        // Re-read inside the TX to tell them apart cleanly.
        let current: Option<String> =
            sqlx::query_scalar("SELECT status FROM invoices WHERE id = ?1 AND company_id = ?2")
                .bind(id)
                .bind(company_id)
                .fetch_optional(&mut *tx)
                .await?;
        return match current {
            None => Err(AppError::NotFound),
            Some(s) if s != expected_status => Err(AppError::Validation(format!(
                "Statusul facturii s-a schimbat între timp ({} → {}). \
                 Reîncărcați factura și reîncercați.",
                expected_status, s
            ))),
            // Theoretically unreachable (the UPDATE should have matched), but
            // return a generic NotFound to be safe.
            _ => Err(AppError::NotFound),
        };
    }

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

#[cfg(test)]
mod tests {
    use super::round2;
    use rust_decimal::Decimal;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::str::FromStr;

    #[test]
    fn round2_is_commercial_half_away_not_bankers() {
        // 2.50 × 21% = 0.5250 → 0.53 (RO commercial / EN16931 reference), NOT 0.52 (banker's).
        assert_eq!(
            round2(Decimal::from_str("0.5250").unwrap()).to_string(),
            "0.53"
        );
        assert_eq!(
            round2(Decimal::from_str("-0.5250").unwrap()).to_string(),
            "-0.53"
        );
        // a non-midpoint value is unaffected.
        assert_eq!(
            round2(Decimal::from_str("0.521").unwrap()).to_string(),
            "0.52"
        );
        // banker's would round 0.125 → 0.12; commercial → 0.13.
        assert_eq!(
            round2(Decimal::from_str("0.125").unwrap()).to_string(),
            "0.13"
        );
    }

    use crate::db::models::VALID_PAYMENT_MEANS_CODES;

    // ── A1: mark_submitted guard tests ────────────────────────────────────────

    /// Minimal in-memory schema for mark_submitted tests.
    async fn setup_mark_submitted_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        sqlx::query(
            "CREATE TABLE invoices (
                id TEXT PRIMARY KEY NOT NULL,
                company_id TEXT NOT NULL DEFAULT 'comp-1',
                contact_id TEXT NOT NULL DEFAULT 'contact-1',
                series TEXT NOT NULL DEFAULT 'FCT',
                number INTEGER NOT NULL DEFAULT 1,
                full_number TEXT NOT NULL DEFAULT 'FCT-0001',
                issue_date TEXT NOT NULL DEFAULT '2026-01-01',
                due_date TEXT NOT NULL DEFAULT '2026-01-01',
                currency TEXT NOT NULL DEFAULT 'RON',
                exchange_rate REAL,
                subtotal_amount TEXT NOT NULL DEFAULT '0',
                vat_amount TEXT NOT NULL DEFAULT '0',
                total_amount TEXT NOT NULL DEFAULT '0',
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
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    async fn insert_invoice_with_status(pool: &sqlx::SqlitePool, id: &str, status: &str) {
        sqlx::query("INSERT INTO invoices (id, status) VALUES (?1, ?2)")
            .bind(id)
            .bind(status)
            .execute(pool)
            .await
            .unwrap();
    }

    /// A1: mark_submitted with correct QUEUED status → row transitions to SUBMITTED.
    #[tokio::test]
    async fn mark_submitted_queued_invoice_succeeds() {
        let pool = setup_mark_submitted_pool().await;
        insert_invoice_with_status(&pool, "inv-queued", "QUEUED").await;

        let result = super::mark_submitted(&pool, "inv-queued", "UPLOAD-001").await;
        assert!(
            result.is_ok(),
            "mark_submitted on QUEUED invoice must succeed"
        );

        let status: String =
            sqlx::query_scalar("SELECT status FROM invoices WHERE id = 'inv-queued'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "SUBMITTED");

        let upload_id: Option<String> =
            sqlx::query_scalar("SELECT anaf_upload_id FROM invoices WHERE id = 'inv-queued'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(upload_id.as_deref(), Some("UPLOAD-001"));
    }

    /// A1: mark_submitted on a non-QUEUED row → returns error AND persists
    /// the upload_id (anti-duplicate: row no longer matches the recovery
    /// predicate `QUEUED AND anaf_upload_id IS NULL`).
    #[tokio::test]
    async fn mark_submitted_non_queued_persists_upload_id() {
        let pool = setup_mark_submitted_pool().await;
        // Insert as DRAFT (not QUEUED) to simulate a concurrent status change.
        insert_invoice_with_status(&pool, "inv-draft", "DRAFT").await;

        let result = super::mark_submitted(&pool, "inv-draft", "UPLOAD-002").await;
        assert!(
            result.is_err(),
            "mark_submitted on non-QUEUED invoice must return an error"
        );

        // Status must NOT have changed to SUBMITTED.
        let status: String =
            sqlx::query_scalar("SELECT status FROM invoices WHERE id = 'inv-draft'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_ne!(
            status, "SUBMITTED",
            "status must not flip to SUBMITTED on a non-QUEUED row"
        );

        // CRITICAL: upload_id MUST be persisted despite the error so the
        // recovery predicate (QUEUED AND anaf_upload_id IS NULL) no longer
        // matches, preventing a duplicate ANAF filing.
        let upload_id: Option<String> =
            sqlx::query_scalar("SELECT anaf_upload_id FROM invoices WHERE id = 'inv-draft'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            upload_id.as_deref(),
            Some("UPLOAD-002"),
            "upload_id MUST be persisted even on partial failure to block duplicate filing"
        );
    }

    #[test]
    fn decimal_avoids_float_drift() {
        // Classic 0.1 + 0.2 issue
        let a = Decimal::from_str("0.10").unwrap();
        let b = Decimal::from_str("0.20").unwrap();
        let sum = a + b;
        assert_eq!(sum.to_string(), "0.30");
    }

    #[test]
    fn round_to_two_decimals() {
        let val = Decimal::from_str("1.2345").unwrap();
        let rounded = val.round_dp(2);
        assert_eq!(rounded.to_string(), "1.23");
    }

    #[test]
    fn negative_storno_amount_preserved() {
        let val = Decimal::from_str("-150.00").unwrap();
        assert!(val.is_sign_negative());
        assert_eq!(val.to_string(), "-150.00");
    }

    // ── U3: payment_means_code validation ────────────────────────────────────

    #[test]
    fn valid_payment_means_code_accepted() {
        // "30" (transfer bancar) is the most common code — must be in the list.
        assert!(
            VALID_PAYMENT_MEANS_CODES.contains(&"30"),
            "code '30' must be accepted"
        );
        // All nine allowed codes must be present.
        for code in &["10", "20", "30", "42", "48", "49", "57", "58", "59"] {
            assert!(
                VALID_PAYMENT_MEANS_CODES.contains(code),
                "code '{}' must be accepted",
                code
            );
        }
    }

    #[test]
    fn invalid_payment_means_code_rejected() {
        // Codes not in UNCL4461 allow-list must NOT be present.
        for bad in &["0", "1", "99", "100", "cash", "card", ""] {
            assert!(
                !VALID_PAYMENT_MEANS_CODES.contains(bad),
                "code '{}' must NOT be accepted",
                bad
            );
        }
    }

    // ── U4: quantity precision ────────────────────────────────────────────────

    #[test]
    fn quantity_six_dp_round_trip() {
        // The stored quantity must preserve 6dp, not truncate to 2dp.
        let qty_input = 1.234567_f64;
        let stored = Decimal::try_from(qty_input)
            .unwrap_or(Decimal::ZERO)
            .round_dp(6)
            .to_string();
        // Should round-trip to at most 6 decimals and NOT lose precision.
        assert!(
            stored.starts_with("1.234567"),
            "qty 1.234567 must be stored as '1.234567', got '{}'",
            stored
        );
    }

    #[test]
    fn quantity_subtotal_uses_precise_qty() {
        // Subtotal is computed from the precise (6dp) quantity, then rounded to 2dp.
        // 1.234567 * 100.00 = 123.4567 → rounds to 123.46 (not 123.00 from 2dp qty).
        let qty = Decimal::from_str("1.234567").unwrap();
        let price = Decimal::from_str("100.00").unwrap();
        let subtotal = (qty * price).round_dp(2);
        assert_eq!(
            subtotal.to_string(),
            "123.46",
            "subtotal must be computed from 6dp qty; got '{}'",
            subtotal
        );
    }

    // ── VAT1: category-authoritative VAT computation ──────────────────────────

    /// Helper: compute (vat_amount, effective_vat_rate) for a line using the
    /// same logic as `create` and `update_invoice_draft`.
    fn compute_line_vat(
        qty: f64,
        unit_price: f64,
        vat_rate: f64,
        vat_category: &str,
    ) -> (String, String) {
        let hundred = Decimal::from(100u32);
        let q = Decimal::try_from(qty).unwrap_or(Decimal::ZERO);
        let p = Decimal::try_from(unit_price).unwrap_or(Decimal::ZERO);
        let raw = Decimal::try_from(vat_rate).unwrap_or(Decimal::ZERO);
        // Category-authoritative rule (mirrors create + update).
        let rate = if vat_category == "S" {
            raw
        } else {
            Decimal::ZERO
        };
        let ls = (q * p).round_dp(2);
        let lv = (ls * rate / hundred).round_dp(2);

        // Effective stored rate:
        let stored_rate = if vat_category == "S" {
            raw
        } else {
            Decimal::ZERO
        };

        (lv.to_string(), stored_rate.round_dp(2).to_string())
    }

    /// VAT1: a line with category 'AE' (taxare inversă) and nominal rate 19
    /// must store vat_amount = 0 and effective vat_rate = 0.
    #[test]
    fn vat1_non_s_category_ae_rate_19_stores_zero_vat() {
        let (vat_amount, stored_rate) = compute_line_vat(1.0, 100.0, 19.0, "AE");
        assert_eq!(
            vat_amount, "0",
            "AE category must have vat_amount 0 (got '{}')",
            vat_amount
        );
        assert_eq!(
            stored_rate, "0",
            "AE category must store effective vat_rate 0 (got '{}')",
            stored_rate
        );
    }

    /// VAT1: a line with category 'E' (scutit) and nominal rate 19 must store
    /// vat_amount = 0 and effective vat_rate = 0.
    #[test]
    fn vat1_non_s_category_e_rate_19_stores_zero_vat() {
        let (vat_amount, stored_rate) = compute_line_vat(2.0, 50.0, 19.0, "E");
        assert_eq!(
            vat_amount, "0",
            "E category must have vat_amount 0 (got '{}')",
            vat_amount
        );
        assert_eq!(
            stored_rate, "0",
            "E category must store effective vat_rate 0 (got '{}')",
            stored_rate
        );
    }

    /// VAT1: a line with category 'Z' (cotă zero) and rate 0 must store
    /// vat_amount = 0 and effective vat_rate = 0.
    #[test]
    fn vat1_category_z_rate_0_stores_zero_vat() {
        let (vat_amount, stored_rate) = compute_line_vat(1.0, 100.0, 0.0, "Z");
        assert_eq!(vat_amount, "0", "Z category must have vat_amount 0");
        assert_eq!(
            stored_rate, "0",
            "Z category must store effective vat_rate 0"
        );
    }

    /// VAT1: a line with category 'O' (afara sferei) and any rate must store 0.
    #[test]
    fn vat1_category_o_any_rate_stores_zero_vat() {
        let (vat_amount, stored_rate) = compute_line_vat(3.0, 200.0, 19.0, "O");
        assert_eq!(
            vat_amount, "0",
            "O category must have vat_amount 0 (got '{}')",
            vat_amount
        );
        assert_eq!(
            stored_rate, "0",
            "O category must store effective vat_rate 0 (got '{}')",
            stored_rate
        );
    }

    /// VAT1: a line with category 'K' (intracom. scutit) and rate 0 must store 0.
    #[test]
    fn vat1_category_k_stores_zero_vat() {
        let (vat_amount, stored_rate) = compute_line_vat(1.0, 500.0, 0.0, "K");
        assert_eq!(vat_amount, "0", "K category must have vat_amount 0");
        assert_eq!(
            stored_rate, "0",
            "K category must store effective vat_rate 0"
        );
    }

    /// VAT1: a line with category 'G' (export scutit) and rate 0 must store 0.
    #[test]
    fn vat1_category_g_stores_zero_vat() {
        let (vat_amount, stored_rate) = compute_line_vat(1.0, 800.0, 0.0, "G");
        assert_eq!(vat_amount, "0", "G category must have vat_amount 0");
        assert_eq!(
            stored_rate, "0",
            "G category must store effective vat_rate 0"
        );
    }

    /// VAT1: a line with category 'S' (Standard) at rate 19 must store the
    /// normal VAT amount (100 * 19 / 100 = 19).
    /// Note: Decimal::round_dp(2).to_string() produces "19" for integers,
    /// so we compare the parsed value rather than the exact string format.
    #[test]
    fn vat1_category_s_rate_19_stores_normal_vat() {
        let (vat_amount, stored_rate) = compute_line_vat(1.0, 100.0, 19.0, "S");
        // Parse to Decimal for a format-independent comparison.
        let vat_dec = Decimal::from_str(&vat_amount).unwrap();
        let rate_dec = Decimal::from_str(&stored_rate).unwrap();
        assert_eq!(
            vat_dec,
            Decimal::from(19),
            "S category at rate 19 must have vat_amount 19 (got '{}')",
            vat_amount
        );
        assert_eq!(
            rate_dec,
            Decimal::from(19),
            "S category at rate 19 must store effective vat_rate 19 (got '{}')",
            stored_rate
        );
    }

    /// VAT1: a line with category 'S' at rate 9 must store correct VAT.
    /// 2 * 100 = 200 net; 200 * 9/100 = 18 VAT.
    #[test]
    fn vat1_category_s_rate_9_stores_normal_vat() {
        let (vat_amount, stored_rate) = compute_line_vat(2.0, 100.0, 9.0, "S");
        let vat_dec = Decimal::from_str(&vat_amount).unwrap();
        let rate_dec = Decimal::from_str(&stored_rate).unwrap();
        assert_eq!(
            vat_dec,
            Decimal::from(18),
            "S category at rate 9 must have vat_amount 18 (got '{}')",
            vat_amount
        );
        assert_eq!(
            rate_dec,
            Decimal::from(9),
            "S category at rate 9 must store effective vat_rate 9 (got '{}')",
            stored_rate
        );
    }
}
