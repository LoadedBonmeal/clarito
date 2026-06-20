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
    pub exchange_rate: Option<f64>,
    pub issue_date: String,

    pub xml_path: String,
    pub pdf_path: Option<String>,

    pub status: String,

    /// Tipul achiziției intra-UE: "goods" (default) sau "services".
    /// Determină rândul D300: goods→R5/R18, services→R7/R20.
    /// Relevant numai pentru facturile cu vat_category="K".
    pub intra_eu_kind: String,

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

// CODE-01: single source of truth for the ReceivedInvoice projection (mirrors COMPANY_SELECT in
// companies.rs). Keep the column order in lock-step with the `ReceivedInvoice` struct fields above so
// `FromRow` binds positionally; both `list` and `get` build their SQL from this.
const RECEIVED_SELECT: &str = "SELECT id, company_id, anaf_download_id, anaf_index, issuer_cui, \
     issuer_name, series, number, total_amount, net_amount, vat_amount, \
     currency, exchange_rate, issue_date, xml_path, pdf_path, \
     status, intra_eu_kind, downloaded_at, created_at \
     FROM received_invoices";

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

    let data_sql = format!(
        "{RECEIVED_SELECT} \
        WHERE (?1 IS NULL OR company_id = ?1) \
          AND (NOT ?2 OR status = CASE WHEN ?3 THEN 'NEW'      ELSE NULL END \
                      OR status = CASE WHEN ?4 THEN 'REVIEWED' ELSE NULL END \
                      OR status = CASE WHEN ?5 THEN 'APPROVED' ELSE NULL END \
                      OR status = CASE WHEN ?6 THEN 'REJECTED' ELSE NULL END \
                      OR status = CASE WHEN ?7 THEN 'ARCHIVED' ELSE NULL END) \
        ORDER BY issue_date DESC \
        LIMIT ?8 OFFSET ?9"
    );

    let items = sqlx::query_as::<_, ReceivedInvoice>(&data_sql)
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

pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<ReceivedInvoice> {
    sqlx::query_as::<_, ReceivedInvoice>(&format!(
        "{RECEIVED_SELECT} WHERE id = ?1 AND company_id = ?2"
    ))
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

/// Setează tipul achiziției intra-UE pentru o factură primită.
/// `kind` trebuie să fie "goods" sau "services".
pub async fn set_intra_eu_kind(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    kind: &str,
) -> AppResult<()> {
    let rows = sqlx::query(
        "UPDATE received_invoices SET intra_eu_kind = ?1 WHERE id = ?2 AND company_id = ?3",
    )
    .bind(kind)
    .bind(id)
    .bind(company_id)
    .execute(pool)
    .await?
    .rows_affected();
    if rows == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn set_status(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    status: ReceivedStatus,
) -> AppResult<()> {
    let value = serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "NEW".into());
    let rows =
        sqlx::query("UPDATE received_invoices SET status = ?2 WHERE id = ?1 AND company_id = ?3")
            .bind(id)
            .bind(&value)
            .bind(company_id)
            .execute(pool)
            .await?
            .rows_affected();
    if rows == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ─── Imported received invoice ──────────────────────────────────────────────

/// Input for creating a received invoice from the Wave C importer.
/// Mirrors the required columns of `received_invoices` without the ANAF-specific
/// `anaf_index` and `xml_path` (the importer fabricates those).
#[derive(Debug, Clone)]
pub struct CreateImportedReceivedInput {
    pub company_id: String,
    /// The raw JSON representation of the staged invoice (used to derive a
    /// stable dedup hash so re-importing the same row is idempotent).
    pub raw_json: String,
    pub issuer_cui: String,
    pub issuer_name: String,
    pub series: Option<String>,
    pub number: Option<String>,
    pub total_amount: String,
    pub net_amount: Option<String>,
    pub vat_amount: Option<String>,
    pub currency: String,
    pub exchange_rate: Option<f64>,
    pub issue_date: String,
}

/// Insert a received invoice imported from a third-party source.
///
/// The `anaf_download_id` is derived as `"import-" + first-32-hex-chars-of-SHA256(raw_json)`
/// so the UNIQUE constraint on `anaf_download_id` provides idempotency: importing the
/// same source row twice will hit the constraint and return the existing id (no duplicate).
///
/// Returns the id of the newly created (or existing) row.
pub async fn create_imported(
    pool: &SqlitePool,
    input: CreateImportedReceivedInput,
) -> AppResult<String> {
    use crate::db::models::{new_id, now_unix};
    use sha2::{Digest, Sha256};

    // Derive the dedup key from the raw source JSON.
    let hash_hex = {
        let mut h = Sha256::new();
        h.update(input.raw_json.as_bytes());
        format!("{:x}", h.finalize())
    };
    let anaf_download_id = format!("import-{}", &hash_hex[..32]);

    // Check for existing row (idempotent — same raw_json → same anaf_download_id).
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM received_invoices \
         WHERE anaf_download_id = ?1 AND company_id = ?2 LIMIT 1",
    )
    .bind(&anaf_download_id)
    .bind(&input.company_id)
    .fetch_optional(pool)
    .await?;

    if let Some(id) = existing {
        return Ok(id);
    }

    let id = new_id();
    let now = now_unix();

    // Use a placeholder xml_path (no file on disk for imported invoices).
    let xml_path = format!("import:{}", &hash_hex[..16]);

    sqlx::query(
        "INSERT INTO received_invoices \
         (id, company_id, anaf_download_id, anaf_index, issuer_cui, issuer_name, \
          series, number, total_amount, net_amount, vat_amount, \
          currency, exchange_rate, issue_date, xml_path, \
          status, intra_eu_kind, downloaded_at, created_at) \
         VALUES \
         (?1, ?2, ?3, NULL, ?4, ?5, \
          ?6, ?7, ?8, ?9, ?10, \
          ?11, ?12, ?13, ?14, \
          'NEW', 'goods', ?15, ?15)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&anaf_download_id)
    .bind(&input.issuer_cui)
    .bind(&input.issuer_name)
    .bind(&input.series)
    .bind(&input.number)
    .bind(&input.total_amount)
    .bind(&input.net_amount)
    .bind(&input.vat_amount)
    .bind(&input.currency)
    .bind(input.exchange_rate)
    .bind(&input.issue_date)
    .bind(&xml_path)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| {
        // UNIQUE constraint on anaf_download_id (race — unlikely but safe to handle).
        if e.to_string().contains("UNIQUE") {
            // Re-query for the existing id.
            AppError::Conflict(format!(
                "Factură primită importată deja (anaf_download_id = {anaf_download_id})"
            ))
        } else {
            AppError::Database(e)
        }
    })?;

    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co','RO1','T','S','C','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    /// Seed a minimal received_invoice row (all NOT NULL columns covered).
    async fn seed_received(pool: &SqlitePool, id: &str, company_id: &str, total: &str) {
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, \
              total_amount, currency, issue_date, xml_path, status, intra_eu_kind, \
              downloaded_at, created_at) \
             VALUES (?1, ?2, ?3, 'RO999', 'Emitent SRL', ?4, 'RON', '2026-01-15', '/x.xml', \
                     'NEW', 'goods', 1, 1)",
        )
        .bind(id)
        .bind(company_id)
        .bind(id) // anaf_download_id must be unique — reuse id
        .bind(total)
        .execute(pool)
        .await
        .unwrap();
    }

    // ─── Test 1: basic roundtrip — insert + list returns the row ───────────

    #[tokio::test]
    async fn list_returns_inserted_row() {
        let pool = pool().await;
        seed_received(&pool, "ri1", "co", "250.00").await;

        let result = list(
            &pool,
            ReceivedFilter {
                company_id: Some("co".into()),
                statuses: None,
                page: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.total, 1);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, "ri1");
        assert_eq!(result.items[0].total_amount, "250.00");
    }

    #[tokio::test]
    async fn list_cross_company_returns_empty() {
        let pool = pool().await;
        seed_received(&pool, "ri1", "co", "250.00").await;

        // Second company — no rows belong to it.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co2','RO2','T2','S','C','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = list(
            &pool,
            ReceivedFilter {
                company_id: Some("co2".into()),
                statuses: None,
                page: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.total, 0);
        assert!(result.items.is_empty());
    }

    // ─── Test 2: get() is company-scoped (cross-company → NotFound) ────────

    #[tokio::test]
    async fn get_returns_row_for_correct_company() {
        let pool = pool().await;
        seed_received(&pool, "ri1", "co", "100.00").await;

        let row = get(&pool, "ri1", "co").await.unwrap();
        assert_eq!(row.id, "ri1");
        assert_eq!(row.company_id, "co");
    }

    #[tokio::test]
    async fn get_cross_company_returns_not_found() {
        let pool = pool().await;
        seed_received(&pool, "ri1", "co", "100.00").await;

        let err = get(&pool, "ri1", "co2").await;
        assert!(
            matches!(err, Err(AppError::NotFound)),
            "cross-company get should return NotFound"
        );
    }

    // ─── Test 3: set_status / set_intra_eu_kind are company-scoped ─────────

    #[tokio::test]
    async fn set_status_updates_correctly() {
        let pool = pool().await;
        seed_received(&pool, "ri1", "co", "100.00").await;

        set_status(&pool, "ri1", "co", ReceivedStatus::Approved)
            .await
            .unwrap();

        let row = get(&pool, "ri1", "co").await.unwrap();
        assert_eq!(row.status, "APPROVED");
    }

    #[tokio::test]
    async fn set_status_cross_company_returns_not_found() {
        let pool = pool().await;
        seed_received(&pool, "ri1", "co", "100.00").await;

        let err = set_status(&pool, "ri1", "wrong_co", ReceivedStatus::Reviewed).await;
        assert!(
            matches!(err, Err(AppError::NotFound)),
            "cross-company set_status should return NotFound"
        );
    }

    #[tokio::test]
    async fn set_intra_eu_kind_cross_company_returns_not_found() {
        let pool = pool().await;
        seed_received(&pool, "ri1", "co", "100.00").await;

        let err = set_intra_eu_kind(&pool, "ri1", "wrong_co", "services").await;
        assert!(
            matches!(err, Err(AppError::NotFound)),
            "cross-company set_intra_eu_kind should return NotFound"
        );
    }

    // ─── Test 4: total_amount stored as TEXT — garbage survives parse ───────
    //   (The list/get functions return the raw TEXT; there is no parse in those
    //    code paths. This test verifies the column round-trips without panic.)

    #[tokio::test]
    async fn total_amount_garbage_text_does_not_panic_on_list() {
        let pool = pool().await;
        // Insert a row with a deliberately broken total_amount.
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, \
              total_amount, currency, issue_date, xml_path, status, intra_eu_kind, \
              downloaded_at, created_at) \
             VALUES ('bad','co','bad_dl','RO1','X','garbage','RON','2026-01-20','/b.xml','NEW','goods',1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // list() must return the row without panicking.
        let result = list(
            &pool,
            ReceivedFilter {
                company_id: Some("co".into()),
                statuses: None,
                page: None,
            },
        )
        .await
        .unwrap();

        let bad = result.items.iter().find(|r| r.id == "bad").unwrap();
        // The raw text is returned as-is; no crash.
        assert_eq!(bad.total_amount, "garbage");
    }
}
