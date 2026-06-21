//! Chitanțe (cash receipts) — company-scoped.
//!
//! Fiecare chitanță aparține unei singure companii (`company_id`). Toate
//! operațiunile sunt scoped pe `company_id` — cross-company access returnează
//! `NotFound`.
//!
//! Numerotarea este atomică per (company_id, series): în cadrul tranzacției
//! de creare se calculează `MAX(number)+1` pentru seria respectivă, eliminând
//! goluri ilegale între serii diferite (ex. CH + BON).
//!
//! Suma este stocată ca TEXT (convenția Decimal-as-TEXT a aplicației).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

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
///
/// # Numbering
/// Allocates the next receipt number atomically **per (company_id, series)**
/// inside the same transaction:
/// `next = COALESCE(MAX(number), 0) + 1 WHERE company_id=? AND series=?`
/// Single-user desktop — no race condition within the TX.
///
/// # Validation
/// * `amount` must be a valid positive Decimal.
/// * Either `contact_id` or `payer_name` must be provided (payer required by law).
/// * `issue_date` must not be empty.
pub async fn create(
    pool: &SqlitePool,
    company_id: &str,
    input: ReceiptInput,
) -> AppResult<Receipt> {
    // ── Validate amount ───────────────────────────────────────────────────
    let amount_dec = Decimal::from_str(input.amount.trim())
        .map_err(|_| AppError::Validation("Sumă invalidă — folosiți formatul 1234.56".into()))?;
    if amount_dec <= Decimal::ZERO {
        return Err(AppError::Validation("Suma trebuie să fie pozitivă.".into()));
    }
    // Normalize: store canonical decimal string (trims trailing zeros etc.)
    let amount_str = amount_dec.to_string();

    // ── Validate issue_date ────────────────────────────────────────────────
    if input.issue_date.trim().is_empty() {
        return Err(AppError::Validation(
            "Data emiterii este obligatorie.".into(),
        ));
    }

    // ── Require payer ──────────────────────────────────────────────────────
    let has_contact = input.contact_id.as_ref().is_some_and(|s| !s.is_empty());
    let has_name = input
        .payer_name
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty());
    if !has_contact && !has_name {
        return Err(AppError::Validation(
            "Specificați plătitorul (contact sau nume).".into(),
        ));
    }

    let id = new_id();
    let now = now_unix();
    let series = input.series.as_deref().unwrap_or("CH").to_string();

    let mut tx = pool.begin().await?;

    // ── Allocate next number per (company_id, series) ─────────────────────
    let allocated_number: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM receipts \
         WHERE company_id = ?1 AND series = ?2",
    )
    .bind(company_id)
    .bind(&series)
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
    .bind(&amount_str)
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

    /// Use the REAL migrations so receipts.company_id REFERENCES companies(id)
    /// and receipts.contact_id REFERENCES contacts(id) (migration 0015) are
    /// enforced exactly as in production.
    async fn setup_pool() -> sqlx::SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory DB");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");

        // Seed: two companies (only required NOT NULL columns without DEFAULTs).
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, vat_payer, address, city, county, country) VALUES
             ('comp-1', '1111111', 'Firma A SRL', 1, 'Str. A 1', 'Bucuresti', 'B', 'RO'),
             ('comp-2', '2222222', 'Firma B SRL', 1, 'Str. B 2', 'Cluj-Napoca', 'CJ', 'RO')",
        )
        .execute(&pool)
        .await
        .expect("seed companies");

        // Seed: one receipt for comp-1.
        sqlx::query(
            "INSERT INTO receipts (id, company_id, series, number, amount, issue_date) VALUES
             ('r1', 'comp-1', 'CH', 1, '500.00', '2026-01-01')",
        )
        .execute(&pool)
        .await
        .expect("seed receipt");

        // Match the seeded receipt number in companies.
        sqlx::query("UPDATE companies SET last_receipt_number = 1 WHERE id = 'comp-1'")
            .execute(&pool)
            .await
            .expect("update last_receipt_number");

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

    fn sample_input_series(series: &str) -> ReceiptInput {
        ReceiptInput {
            series: Some(series.to_string()),
            ..sample_input()
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
        // comp-2 starts empty — first two creates should get numbers 1 and 2.
        let r1 = create(&pool, "comp-2", sample_input()).await.unwrap();
        let r2 = create(&pool, "comp-2", sample_input()).await.unwrap();
        assert_eq!(r1.number, 1, "first receipt must get number 1");
        assert_eq!(r2.number, 2, "second receipt must get number 2");
    }

    // ── per-series numbering: two series don't share a counter ────────────────

    #[tokio::test]
    async fn r3_per_series_numbering_two_series_independent() {
        let pool = setup_pool().await;
        // Both series start at 0 for comp-2 (no prior receipts).
        let ch1 = create(&pool, "comp-2", sample_input_series("CH"))
            .await
            .unwrap();
        let bon1 = create(&pool, "comp-2", sample_input_series("BON"))
            .await
            .unwrap();
        let ch2 = create(&pool, "comp-2", sample_input_series("CH"))
            .await
            .unwrap();
        let bon2 = create(&pool, "comp-2", sample_input_series("BON"))
            .await
            .unwrap();

        assert_eq!(ch1.number, 1, "CH first must be 1");
        assert_eq!(ch2.number, 2, "CH second must be 2 (no gap from BON)");
        assert_eq!(bon1.number, 1, "BON first must be 1 (independent of CH)");
        assert_eq!(bon2.number, 2, "BON second must be 2");
    }

    // ── amount validation ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn r3_amount_rejects_non_numeric() {
        let pool = setup_pool().await;
        let mut input = sample_input();
        input.amount = "abc".to_string();
        let err = create(&pool, "comp-2", input).await.unwrap_err();
        assert!(
            matches!(err, AppError::Validation(_)),
            "non-numeric amount must fail with Validation"
        );
    }

    #[tokio::test]
    async fn r3_amount_rejects_zero() {
        let pool = setup_pool().await;
        let mut input = sample_input();
        input.amount = "0".to_string();
        let err = create(&pool, "comp-2", input).await.unwrap_err();
        assert!(
            matches!(err, AppError::Validation(_)),
            "zero amount must fail with Validation"
        );
    }

    #[tokio::test]
    async fn r3_amount_rejects_negative() {
        let pool = setup_pool().await;
        let mut input = sample_input();
        input.amount = "-5.00".to_string();
        let err = create(&pool, "comp-2", input).await.unwrap_err();
        assert!(
            matches!(err, AppError::Validation(_)),
            "negative amount must fail with Validation"
        );
    }

    #[tokio::test]
    async fn r3_amount_accepts_valid_decimal() {
        let pool = setup_pool().await;
        let mut input = sample_input();
        input.amount = "1234.56".to_string();
        let r = create(&pool, "comp-2", input).await.unwrap();
        // Stored as normalized decimal string.
        assert_eq!(r.amount, "1234.56");
    }

    // ── payer required ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn r3_payer_required_both_empty_fails() {
        let pool = setup_pool().await;
        let input = ReceiptInput {
            series: None,
            contact_id: None,
            invoice_id: None,
            amount: "100.00".to_string(),
            currency: None,
            issue_date: "2026-06-01".to_string(),
            payer_name: None,
            notes: None,
        };
        let err = create(&pool, "comp-2", input).await.unwrap_err();
        assert!(
            matches!(err, AppError::Validation(_)),
            "missing payer must fail with Validation"
        );
    }

    #[tokio::test]
    async fn r3_payer_required_contact_id_satisfies() {
        let pool = setup_pool().await;
        let input = ReceiptInput {
            contact_id: Some("some-contact-id".to_string()),
            payer_name: None,
            ..sample_input()
        };
        // contact FK is not enforced in test schema (no contacts table) but
        // the payer-presence check itself must pass.
        // The INSERT may fail on FK; we only check that the validation error
        // is NOT the "payer" error.
        let result = create(&pool, "comp-2", input).await;
        if let Err(AppError::Validation(msg)) = &result {
            assert!(
                !msg.contains("plătitor"),
                "should not get payer-validation error when contact_id is set"
            );
        }
    }

    #[tokio::test]
    async fn r3_payer_required_payer_name_satisfies() {
        let pool = setup_pool().await;
        let input = ReceiptInput {
            contact_id: None,
            payer_name: Some("Firma X SRL".to_string()),
            ..sample_input()
        };
        let r = create(&pool, "comp-2", input).await.unwrap();
        assert_eq!(r.payer_name.as_deref(), Some("Firma X SRL"));
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
