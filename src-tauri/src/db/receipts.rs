//! Chitanțe (cash receipts) — company-scoped.
//!
//! Fiecare chitanță aparține unei singure companii (`company_id`). Toate
//! operațiunile sunt scoped pe `company_id` — cross-company access returnează
//! `NotFound`.
//!
//! Numerotarea este atomică: `UPDATE companies SET last_receipt_number = last_receipt_number + 1`
//! + `SELECT` în aceeași tranzacție (identic cu pattern-ul de la facturi).
//!
//! Suma este stocată ca TEXT (convenția Decimal-as-TEXT a aplicației).

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Model ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub id: String,
    pub company_id: String,
    pub series: String,
    pub number: i64,
    pub contact_id: Option<String>,
    pub invoice_id: Option<String>,
    pub amount: String,
    pub currency: String,
    pub issue_date: String,
    pub payer_name: Option<String>,
    pub notes: Option<String>,
    pub pdf_path: Option<String>,
    pub created_at: i64,
}

impl Receipt {
    /// Numărul complet afișabil: e.g. `CH-1`, `CH-42`.
    pub fn full_number(&self) -> String {
        format!("{}-{}", self.series, self.number)
    }
}

// ─── Input ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptInput {
    pub series: Option<String>,
    pub contact_id: Option<String>,
    pub invoice_id: Option<String>,
    pub amount: String,
    pub currency: Option<String>,
    pub issue_date: String,
    pub payer_name: Option<String>,
    pub notes: Option<String>,
}

// ─── Queries ───────────────────────────────────────────────────────────────

/// List receipts for a company, ordered newest first.
/// Always company-scoped: every row is filtered by `company_id = ?`.
pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Receipt>> {
    let items = sqlx::query_as::<_, Receipt>(
        "SELECT id, company_id, series, number, contact_id, invoice_id, \
         amount, currency, issue_date, payer_name, notes, pdf_path, created_at \
         FROM receipts \
         WHERE company_id = ?1 \
         ORDER BY created_at DESC, number DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(items)
}

/// Fetch a single receipt by id; verify ownership.
/// Cross-company access returns `NotFound`.
pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Receipt> {
    let receipt = sqlx::query_as::<_, Receipt>(
        "SELECT id, company_id, series, number, contact_id, invoice_id, \
         amount, currency, issue_date, payer_name, notes, pdf_path, created_at \
         FROM receipts WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    // R14 company isolation: cross-company access returns NotFound.
    if receipt.company_id != company_id {
        return Err(AppError::NotFound);
    }
    Ok(receipt)
}

/// Create a new receipt for the given company.
/// Allocates the next receipt number atomically in a transaction (mirrors
/// invoice numbering: bumps `companies.last_receipt_number`).
pub async fn create(
    pool: &SqlitePool,
    company_id: &str,
    input: ReceiptInput,
) -> AppResult<Receipt> {
    if input.amount.trim().is_empty() {
        return Err(AppError::Validation(
            "Suma chitanței este obligatorie.".into(),
        ));
    }
    if input.issue_date.trim().is_empty() {
        return Err(AppError::Validation(
            "Data emiterii este obligatorie.".into(),
        ));
    }

    let id = new_id();
    let now = now_unix();
    let series = input.series.as_deref().unwrap_or("CH").to_string();

    let mut tx = pool.begin().await?;

    // Alocăm numărul atomic în aceeași tranzacție.
    sqlx::query("UPDATE companies SET last_receipt_number = last_receipt_number + 1 WHERE id = ?1")
        .bind(company_id)
        .execute(&mut *tx)
        .await?;

    let allocated_number: i64 =
        sqlx::query_scalar("SELECT last_receipt_number FROM companies WHERE id = ?1")
            .bind(company_id)
            .fetch_one(&mut *tx)
            .await?;

    sqlx::query(
        "INSERT INTO receipts (
            id, company_id, series, number, contact_id, invoice_id,
            amount, currency, issue_date, payer_name, notes, created_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10, ?11, ?12
        )",
    )
    .bind(&id)
    .bind(company_id)
    .bind(&series)
    .bind(allocated_number)
    .bind(&input.contact_id)
    .bind(&input.invoice_id)
    .bind(&input.amount)
    .bind(input.currency.as_deref().unwrap_or("RON"))
    .bind(&input.issue_date)
    .bind(&input.payer_name)
    .bind(&input.notes)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get(pool, &id, company_id).await
}

/// Delete a receipt. Verifies ownership first; cross-company returns NotFound.
pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    // Verify ownership for clear NotFound on cross-company attempts.
    get(pool, id, company_id).await?;
    let res = sqlx::query("DELETE FROM receipts WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Store the PDF path after generation.
pub async fn set_pdf_path(pool: &SqlitePool, id: &str, path: &str) -> AppResult<()> {
    sqlx::query("UPDATE receipts SET pdf_path = ?2 WHERE id = ?1")
        .bind(id)
        .bind(path)
        .execute(pool)
        .await?;
    Ok(())
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Minimal in-memory schema for receipts tests.
    async fn setup_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        sqlx::query(
            "CREATE TABLE companies (
                id                   TEXT    PRIMARY KEY,
                cui                  TEXT    NOT NULL,
                legal_name           TEXT    NOT NULL,
                trade_name           TEXT,
                registry_number      TEXT,
                vat_payer            INTEGER NOT NULL DEFAULT 1,
                address              TEXT    NOT NULL DEFAULT '',
                city                 TEXT    NOT NULL DEFAULT '',
                county               TEXT    NOT NULL DEFAULT '',
                postal_code          TEXT,
                country              TEXT    NOT NULL DEFAULT 'RO',
                email                TEXT,
                phone                TEXT,
                iban                 TEXT,
                bank_name            TEXT,
                is_active            INTEGER NOT NULL DEFAULT 1,
                spv_enabled          INTEGER NOT NULL DEFAULT 0,
                invoice_series       TEXT    NOT NULL DEFAULT 'FACT',
                last_invoice_number  INTEGER NOT NULL DEFAULT 0,
                last_receipt_number  INTEGER NOT NULL DEFAULT 0,
                logo_path            TEXT,
                created_at           INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at           INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE receipts (
                id          TEXT    PRIMARY KEY,
                company_id  TEXT    NOT NULL,
                series      TEXT    NOT NULL DEFAULT 'CH',
                number      INTEGER NOT NULL,
                contact_id  TEXT,
                invoice_id  TEXT,
                amount      TEXT    NOT NULL,
                currency    TEXT    NOT NULL DEFAULT 'RON',
                issue_date  TEXT    NOT NULL,
                payer_name  TEXT,
                notes       TEXT,
                pdf_path    TEXT,
                created_at  INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Seed: two companies.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county) VALUES
             ('comp-1', '1111111', 'Firma A SRL', 'Str. A 1', 'Bucuresti', 'B'),
             ('comp-2', '2222222', 'Firma B SRL', 'Str. B 2', 'Cluj-Napoca', 'CJ')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Seed: one receipt for comp-1.
        sqlx::query(
            "INSERT INTO receipts (id, company_id, series, number, amount, issue_date) VALUES
             ('r1', 'comp-1', 'CH', 1, '500.00', '2026-01-01')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Match the seeded receipt number in companies.
        sqlx::query("UPDATE companies SET last_receipt_number = 1 WHERE id = 'comp-1'")
            .execute(&pool)
            .await
            .unwrap();

        pool
    }

    fn sample_input() -> ReceiptInput {
        ReceiptInput {
            series: None,
            contact_id: None,
            invoice_id: None,
            amount: "250.00".to_string(),
            currency: None,
            issue_date: "2026-06-01".to_string(),
            payer_name: Some("Ion Popescu".to_string()),
            notes: None,
        }
    }

    // ── get: wrong company → NotFound ────────────────────────────────────────

    #[tokio::test]
    async fn wave3_receipt_get_wrong_company_returns_not_found() {
        let pool = setup_pool().await;
        let result = get(&pool, "r1", "comp-2").await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "get with wrong company_id must return NotFound"
        );
    }

    #[tokio::test]
    async fn wave3_receipt_get_correct_company_succeeds() {
        let pool = setup_pool().await;
        let result = get(&pool, "r1", "comp-1").await;
        assert!(result.is_ok(), "get with correct company_id must succeed");
        assert_eq!(result.unwrap().amount, "500.00");
    }

    // ── delete: wrong company → NotFound ─────────────────────────────────────

    #[tokio::test]
    async fn wave3_receipt_delete_wrong_company_returns_not_found() {
        let pool = setup_pool().await;
        let result = delete(&pool, "r1", "comp-2").await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "delete with wrong company_id must return NotFound"
        );
        // Receipt must still exist.
        let still_there = get(&pool, "r1", "comp-1").await;
        assert!(still_there.is_ok(), "receipt must not have been deleted");
    }

    #[tokio::test]
    async fn wave3_receipt_delete_correct_company_succeeds() {
        let pool = setup_pool().await;
        let result = delete(&pool, "r1", "comp-1").await;
        assert!(
            result.is_ok(),
            "delete with correct company_id must succeed"
        );
        let gone = get(&pool, "r1", "comp-1").await;
        assert!(
            matches!(gone, Err(AppError::NotFound)),
            "receipt must be gone after correct-company delete"
        );
    }

    // ── create: sequential numbering per company ─────────────────────────────

    #[tokio::test]
    async fn wave3_receipt_create_allocates_sequential_numbers() {
        let pool = setup_pool().await;
        // comp-2 starts at 0 — first two creates should get numbers 1 and 2.
        let r1 = create(&pool, "comp-2", sample_input()).await.unwrap();
        let r2 = create(&pool, "comp-2", sample_input()).await.unwrap();
        assert_eq!(r1.number, 1, "first receipt must get number 1");
        assert_eq!(r2.number, 2, "second receipt must get number 2");
    }

    // ── cross-company isolation: list ─────────────────────────────────────────

    #[tokio::test]
    async fn wave3_receipt_list_cross_company_isolation() {
        let pool = setup_pool().await;
        // comp-1 has r1 pre-seeded; comp-2 has none.
        let comp1_list = list(&pool, "comp-1").await.unwrap();
        assert_eq!(comp1_list.len(), 1, "comp-1 must see only its receipts");
        assert_eq!(comp1_list[0].id, "r1");

        let comp2_list = list(&pool, "comp-2").await.unwrap();
        assert!(
            comp2_list.is_empty(),
            "comp-2 must not see comp-1's receipts"
        );

        // Create a receipt for comp-2 and verify isolation.
        create(&pool, "comp-2", sample_input()).await.unwrap();
        let comp1_after = list(&pool, "comp-1").await.unwrap();
        assert_eq!(
            comp1_after.len(),
            1,
            "comp-1 must still see only its own receipts after comp-2 creates one"
        );
        let comp2_after = list(&pool, "comp-2").await.unwrap();
        assert_eq!(comp2_after.len(), 1, "comp-2 must see only its own receipt");
    }

    // ── full_number helper ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn wave3_receipt_full_number_format() {
        let pool = setup_pool().await;
        let r = get(&pool, "r1", "comp-1").await.unwrap();
        assert_eq!(r.full_number(), "CH-1");
    }
}
