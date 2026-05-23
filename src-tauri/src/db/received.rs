//! Facturi primite (downloadate de la ANAF).

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix, Page, Paginated, ReceivedStatus};
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

    pub total_amount: f64,
    pub currency: String,
    pub issue_date: String,

    pub xml_path: String,
    pub pdf_path: Option<String>,

    pub status: String,

    pub downloaded_at: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateReceivedInput {
    pub company_id: String,
    pub anaf_download_id: String,
    pub anaf_index: Option<String>,
    pub issuer_cui: String,
    pub issuer_name: String,
    pub series: Option<String>,
    pub number: Option<String>,
    pub total_amount: f64,
    pub currency: String,
    pub issue_date: String,
    pub xml_path: String,
    pub pdf_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedFilter {
    pub company_id: Option<String>,
    pub statuses: Option<Vec<ReceivedStatus>>,
    pub page: Option<Page>,
}

const SELECT_COLUMNS: &str = "id, company_id, anaf_download_id, anaf_index, issuer_cui, \
    issuer_name, series, number, total_amount, currency, issue_date, xml_path, pdf_path, \
    status, downloaded_at, created_at";

pub async fn list(
    pool: &SqlitePool,
    filter: ReceivedFilter,
) -> AppResult<Paginated<ReceivedInvoice>> {
    let page = filter.page.unwrap_or_default();
    let mut where_sql = String::from("1=1");
    let mut binds: Vec<String> = Vec::new();

    if let Some(cid) = &filter.company_id {
        where_sql.push_str(&format!(" AND company_id = ?{}", binds.len() + 1));
        binds.push(cid.clone());
    }
    if let Some(statuses) = &filter.statuses {
        if !statuses.is_empty() {
            let placeholders: Vec<String> = (0..statuses.len())
                .map(|i| format!("?{}", binds.len() + i + 1))
                .collect();
            where_sql.push_str(&format!(" AND status IN ({})", placeholders.join(",")));
            for s in statuses {
                let value = serde_json::to_value(s)
                    .ok()
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                binds.push(value);
            }
        }
    }

    let count_sql = format!("SELECT COUNT(*) FROM received_invoices WHERE {where_sql}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        count_q = count_q.bind(b);
    }
    let total = count_q.fetch_one(pool).await?;

    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM received_invoices WHERE {where_sql} \
         ORDER BY issue_date DESC LIMIT ?{} OFFSET ?{}",
        binds.len() + 1,
        binds.len() + 2
    );
    let mut q = sqlx::query_as::<_, ReceivedInvoice>(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    q = q.bind(page.limit).bind(page.offset);

    Ok(Paginated {
        items: q.fetch_all(pool).await?,
        total,
        offset: page.offset,
        limit: page.limit,
    })
}

pub async fn get(pool: &SqlitePool, id: &str) -> AppResult<ReceivedInvoice> {
    let sql = format!("SELECT {SELECT_COLUMNS} FROM received_invoices WHERE id = ?1");
    sqlx::query_as::<_, ReceivedInvoice>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn create(pool: &SqlitePool, input: CreateReceivedInput) -> AppResult<ReceivedInvoice> {
    let id = new_id();
    let now = now_unix();

    sqlx::query(
        "INSERT INTO received_invoices (
            id, company_id, anaf_download_id, anaf_index,
            issuer_cui, issuer_name, series, number,
            total_amount, currency, issue_date,
            xml_path, pdf_path,
            downloaded_at, created_at
        ) VALUES (
            ?1, ?2, ?3, ?4,
            ?5, ?6, ?7, ?8,
            ?9, ?10, ?11,
            ?12, ?13,
            ?14, ?14
        )",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.anaf_download_id)
    .bind(&input.anaf_index)
    .bind(&input.issuer_cui)
    .bind(&input.issuer_name)
    .bind(&input.series)
    .bind(&input.number)
    .bind(input.total_amount)
    .bind(&input.currency)
    .bind(&input.issue_date)
    .bind(&input.xml_path)
    .bind(&input.pdf_path)
    .bind(now)
    .execute(pool)
    .await?;

    get(pool, &id).await
}

pub async fn set_status(
    pool: &SqlitePool,
    id: &str,
    status: ReceivedStatus,
) -> AppResult<()> {
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
