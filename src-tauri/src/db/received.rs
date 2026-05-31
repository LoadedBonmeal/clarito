//! Facturi primite (downloadate de la ANAF).

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{Page, Paginated, ReceivedStatus};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedInvoice {
    pub id: String,
    pub company_id: String,

    pub anaf_download_id: String,
    pub anaf_index: Option<String>,

    pub issuer_cui: String,
    pub issuer_name: String,
    pub series: Option<String>,
    pub number: Option<String>,

    pub total_amount: String,
    pub net_amount: Option<String>,
    pub vat_amount: Option<String>,
    pub currency: String,
    pub issue_date: String,

    pub xml_path: String,
    pub pdf_path: Option<String>,

    pub status: String,

    pub downloaded_at: i64,
    pub created_at: i64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedFilter {
    pub company_id: Option<String>,
    pub statuses: Option<Vec<ReceivedStatus>>,
    pub page: Option<Page>,
}

pub async fn list(
    pool: &SqlitePool,
    filter: ReceivedFilter,
) -> AppResult<Paginated<ReceivedInvoice>> {
    let page = filter.page.unwrap_or_default();

    let company_id = filter.company_id.as_ref().filter(|s| !s.is_empty());

    // ReceivedStatus has 5 variants: New, Reviewed, Approved, Rejected, Archived.
    // Expand to boolean flags so SQL remains static.
    let statuses = filter.statuses.as_deref().unwrap_or(&[]);
    let has_status_filter = !statuses.is_empty();
    let want_new = has_status_filter && statuses.contains(&ReceivedStatus::New);
    let want_reviewed = has_status_filter && statuses.contains(&ReceivedStatus::Reviewed);
    let want_approved = has_status_filter && statuses.contains(&ReceivedStatus::Approved);
    let want_rejected = has_status_filter && statuses.contains(&ReceivedStatus::Rejected);
    let want_archived = has_status_filter && statuses.contains(&ReceivedStatus::Archived);

    // ?1 company_id, ?2 has_status_filter, ?3..?7 want_* flags, ?8 limit, ?9 offset
    let count_sql = "\
        SELECT COUNT(*) FROM received_invoices \
        WHERE (?1 IS NULL OR company_id = ?1) \
          AND (NOT ?2 OR status = CASE WHEN ?3 THEN 'NEW'      ELSE NULL END \
                      OR status = CASE WHEN ?4 THEN 'REVIEWED' ELSE NULL END \
                      OR status = CASE WHEN ?5 THEN 'APPROVED' ELSE NULL END \
                      OR status = CASE WHEN ?6 THEN 'REJECTED' ELSE NULL END \
                      OR status = CASE WHEN ?7 THEN 'ARCHIVED' ELSE NULL END)";

    let total: i64 = sqlx::query_scalar(count_sql)
        .bind(company_id)
        .bind(has_status_filter as i64)
        .bind(want_new as i64)
        .bind(want_reviewed as i64)
        .bind(want_approved as i64)
        .bind(want_rejected as i64)
        .bind(want_archived as i64)
        .fetch_one(pool)
        .await?;

    let data_sql = "\
        SELECT id, company_id, anaf_download_id, anaf_index, issuer_cui, \
               issuer_name, series, number, total_amount, net_amount, vat_amount, \
               currency, issue_date, xml_path, pdf_path, \
               status, downloaded_at, created_at \
        FROM received_invoices \
        WHERE (?1 IS NULL OR company_id = ?1) \
          AND (NOT ?2 OR status = CASE WHEN ?3 THEN 'NEW'      ELSE NULL END \
                      OR status = CASE WHEN ?4 THEN 'REVIEWED' ELSE NULL END \
                      OR status = CASE WHEN ?5 THEN 'APPROVED' ELSE NULL END \
                      OR status = CASE WHEN ?6 THEN 'REJECTED' ELSE NULL END \
                      OR status = CASE WHEN ?7 THEN 'ARCHIVED' ELSE NULL END) \
        ORDER BY issue_date DESC \
        LIMIT ?8 OFFSET ?9";

    let items = sqlx::query_as::<_, ReceivedInvoice>(data_sql)
        .bind(company_id)
        .bind(has_status_filter as i64)
        .bind(want_new as i64)
        .bind(want_reviewed as i64)
        .bind(want_approved as i64)
        .bind(want_rejected as i64)
        .bind(want_archived as i64)
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

pub async fn get(pool: &SqlitePool, id: &str) -> AppResult<ReceivedInvoice> {
    sqlx::query_as::<_, ReceivedInvoice>(
        "SELECT id, company_id, anaf_download_id, anaf_index, issuer_cui, \
         issuer_name, series, number, total_amount, net_amount, vat_amount, \
         currency, issue_date, xml_path, pdf_path, \
         status, downloaded_at, created_at \
         FROM received_invoices WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

/// Returnează liniile de defalcare TVA pentru o factură primită.
/// Fiecare tuplu: (vat_rate, vat_category, base_amount, vat_amount).
/// Utilizat de Wave B (D300/D394 achiziții) pentru agregare per perioadă.
#[allow(dead_code)]
pub async fn vat_lines_for_invoice(
    pool: &SqlitePool,
    received_invoice_id: &str,
) -> AppResult<Vec<(String, String, String, String)>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT vat_rate, vat_category, base_amount, vat_amount \
         FROM received_invoice_vat_lines \
         WHERE received_invoice_id = ?1 \
         ORDER BY CAST(vat_rate AS REAL) DESC",
    )
    .bind(received_invoice_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            (
                r.try_get::<String, _>("vat_rate").unwrap_or_default(),
                r.try_get::<String, _>("vat_category").unwrap_or_default(),
                r.try_get::<String, _>("base_amount").unwrap_or_default(),
                r.try_get::<String, _>("vat_amount").unwrap_or_default(),
            )
        })
        .collect())
}

pub async fn set_status(pool: &SqlitePool, id: &str, status: ReceivedStatus) -> AppResult<()> {
    let value = serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "NEW".into());
    sqlx::query("UPDATE received_invoices SET status = ?2 WHERE id = ?1")
        .bind(id)
        .bind(&value)
        .execute(pool)
        .await?;
    Ok(())
}
