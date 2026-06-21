//! Mișcări de stoc (MovementOfGoods SAF-T section).
//!
//! Fiecare mișcare aparține unei companii (company_id).
//! MVP: înregistrare manuală; UI planificat pentru P7.
//!
//! Valorile monetare și cantitățile sunt stocate ca TEXT
//! (convenția Decimal-as-TEXT a aplicației).

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Models ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StockMovement {
    pub id: String,
    pub company_id: String,
    pub movement_ref: String,
    pub movement_date: String,
    pub posting_date: String,
    pub movement_type: String,
    pub direction: String,
    pub document_type: Option<String>,
    pub document_number: Option<String>,
    pub source_type: Option<String>,
    pub source_id: Option<String>,
    pub notes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StockMovementLine {
    pub id: String,
    pub movement_id: String,
    pub line_number: i64,
    pub product_id: Option<String>,
    pub product_code: String,
    pub account_id: String,
    pub customer_id: String,
    pub supplier_id: String,
    pub quantity: String,
    pub unit_of_measure: String,
    pub uom_conv_factor: String,
    pub book_value: String,
    pub movement_subtype: String,
    pub comments: Option<String>,
}

// ─── Inputs ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockMovementLineInput {
    pub product_id: Option<String>,
    pub product_code: String,
    pub account_id: Option<String>,
    pub customer_id: Option<String>,
    pub supplier_id: Option<String>,
    pub quantity: String,
    pub unit_of_measure: Option<String>,
    pub uom_conv_factor: Option<String>,
    pub book_value: Option<String>,
    pub movement_subtype: String,
    pub comments: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockMovementInput {
    pub movement_ref: String,
    pub movement_date: String,
    pub posting_date: Option<String>,
    pub movement_type: String,
    pub direction: Option<String>,
    pub document_type: Option<String>,
    pub document_number: Option<String>,
    pub source_type: Option<String>,
    pub source_id: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<StockMovementLineInput>,
}

// ─── Composed result ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockMovementWithLines {
    #[serde(flatten)]
    pub movement: StockMovement,
    pub lines: Vec<StockMovementLine>,
}

// ─── Queries ───────────────────────────────────────────────────────────────

/// List stock movements for a company in a date range (inclusive).
pub async fn list(
    pool: &SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<Vec<StockMovementWithLines>> {
    let movements = sqlx::query_as::<_, StockMovement>(
        "SELECT id, company_id, movement_ref, movement_date, posting_date, \
                movement_type, direction, document_type, document_number, \
                source_type, source_id, notes, created_at, updated_at \
         FROM stock_movements \
         WHERE company_id = ?1 \
           AND movement_date >= ?2 \
           AND movement_date <= ?3 \
         ORDER BY movement_date ASC, movement_ref ASC",
    )
    .bind(company_id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(pool)
    .await?;

    let mut result = Vec::with_capacity(movements.len());
    for m in movements {
        let lines = fetch_lines(pool, &m.id).await?;
        result.push(StockMovementWithLines { movement: m, lines });
    }
    Ok(result)
}

/// Fetch a single stock movement with its lines. Verifies company ownership.
pub async fn get(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<StockMovementWithLines> {
    let movement = sqlx::query_as::<_, StockMovement>(
        "SELECT id, company_id, movement_ref, movement_date, posting_date, \
                movement_type, direction, document_type, document_number, \
                source_type, source_id, notes, created_at, updated_at \
         FROM stock_movements WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    if movement.company_id != company_id {
        return Err(AppError::NotFound);
    }

    let lines = fetch_lines(pool, &movement.id).await?;
    Ok(StockMovementWithLines { movement, lines })
}

/// Create a new stock movement with its lines (all in one transaction).
pub async fn create(
    pool: &SqlitePool,
    company_id: &str,
    input: StockMovementInput,
) -> AppResult<StockMovementWithLines> {
    if input.lines.is_empty() {
        return Err(AppError::Validation(
            "O mișcare de stoc trebuie să aibă cel puțin o linie.".into(),
        ));
    }

    // Dup-check: movement_ref must be unique per company.
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM stock_movements WHERE company_id = ?1 AND movement_ref = ?2 LIMIT 1",
    )
    .bind(company_id)
    .bind(&input.movement_ref)
    .fetch_optional(pool)
    .await?;
    if existing.is_some() {
        return Err(AppError::Validation(format!(
            "Există deja o mișcare cu referința '{}' pentru această companie.",
            input.movement_ref
        )));
    }

    let id = new_id();
    let now = now_unix();
    let posting_date = input
        .posting_date
        .as_deref()
        .unwrap_or(&input.movement_date)
        .to_string();
    let direction = input.direction.as_deref().unwrap_or("IN").to_string();

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO stock_movements (
            id, company_id, movement_ref, movement_date, posting_date,
            movement_type, direction, document_type, document_number,
            source_type, source_id, notes, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
    )
    .bind(&id)
    .bind(company_id)
    .bind(&input.movement_ref)
    .bind(&input.movement_date)
    .bind(&posting_date)
    .bind(&input.movement_type)
    .bind(&direction)
    .bind(&input.document_type)
    .bind(&input.document_number)
    .bind(&input.source_type)
    .bind(&input.source_id)
    .bind(&input.notes)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    for (i, line) in input.lines.iter().enumerate() {
        let line_id = new_id();
        sqlx::query(
            "INSERT INTO stock_movement_lines (
                id, movement_id, line_number, product_id, product_code,
                account_id, customer_id, supplier_id, quantity,
                unit_of_measure, uom_conv_factor, book_value, movement_subtype, comments
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        )
        .bind(&line_id)
        .bind(&id)
        .bind((i as i64) + 1)
        .bind(&line.product_id)
        .bind(&line.product_code)
        .bind(line.account_id.as_deref().unwrap_or("371"))
        .bind(line.customer_id.as_deref().unwrap_or("0"))
        .bind(line.supplier_id.as_deref().unwrap_or("0"))
        .bind(&line.quantity)
        .bind(line.unit_of_measure.as_deref().unwrap_or("H87"))
        .bind(line.uom_conv_factor.as_deref().unwrap_or("1"))
        .bind(line.book_value.as_deref().unwrap_or("0.00"))
        .bind(&line.movement_subtype)
        .bind(&line.comments)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    get(pool, &id, company_id).await
}

/// Delete a stock movement (cascades to lines). Verifies ownership first.
pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let movement = sqlx::query_as::<_, StockMovement>(
        "SELECT id, company_id, movement_ref, movement_date, posting_date, \
                movement_type, direction, document_type, document_number, \
                source_type, source_id, notes, created_at, updated_at \
         FROM stock_movements WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    if movement.company_id != company_id {
        return Err(AppError::NotFound);
    }

    let res = sqlx::query("DELETE FROM stock_movements WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ─── Internal helpers ──────────────────────────────────────────────────────

async fn fetch_lines(pool: &SqlitePool, movement_id: &str) -> AppResult<Vec<StockMovementLine>> {
    let lines = sqlx::query_as::<_, StockMovementLine>(
        "SELECT id, movement_id, line_number, product_id, product_code, \
                account_id, customer_id, supplier_id, quantity, \
                unit_of_measure, uom_conv_factor, book_value, movement_subtype, comments \
         FROM stock_movement_lines \
         WHERE movement_id = ?1 \
         ORDER BY line_number ASC",
    )
    .bind(movement_id)
    .fetch_all(pool)
    .await?;
    Ok(lines)
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Use the REAL migrations so ON DELETE CASCADE FKs on stock_movement_lines
    /// (migration 0019) are enforced exactly as in production.
    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory DB");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");

        // Seed the company row required by stock_movements.company_id FK.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, vat_payer, address, city, county, country) \
             VALUES ('co-1', 'RO1234', 'Test SRL', 1, 'Str 1', 'Buc', 'B', 'RO')",
        )
        .execute(&pool)
        .await
        .expect("seed company");

        pool
    }

    fn sample_input() -> StockMovementInput {
        StockMovementInput {
            movement_ref: "NIR-001".into(),
            movement_date: "2025-01-15".into(),
            posting_date: None,
            movement_type: "10".into(),
            direction: Some("IN".into()),
            document_type: Some("NIR".into()),
            document_number: Some("001".into()),
            source_type: None,
            source_id: None,
            notes: None,
            lines: vec![StockMovementLineInput {
                product_id: None,
                product_code: "PRODUS-01".into(),
                account_id: Some("371".into()),
                customer_id: Some("0".into()),
                supplier_id: Some("0011223344".into()),
                quantity: "10.000000".into(),
                unit_of_measure: Some("H87".into()),
                uom_conv_factor: Some("1".into()),
                book_value: Some("500.00".into()),
                movement_subtype: "10".into(),
                comments: None,
            }],
        }
    }

    #[tokio::test]
    async fn create_and_get_round_trip() {
        let pool = setup_pool().await;
        let result = create(&pool, "co-1", sample_input()).await.unwrap();
        assert_eq!(result.movement.movement_ref, "NIR-001");
        assert_eq!(result.lines.len(), 1);
        assert_eq!(result.lines[0].product_code, "PRODUS-01");

        let fetched = get(&pool, &result.movement.id, "co-1").await.unwrap();
        assert_eq!(fetched.movement.id, result.movement.id);
    }

    #[tokio::test]
    async fn duplicate_ref_rejected() {
        let pool = setup_pool().await;
        create(&pool, "co-1", sample_input()).await.unwrap();
        let err = create(&pool, "co-1", sample_input()).await.unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn cross_company_get_returns_not_found() {
        let pool = setup_pool().await;
        let m = create(&pool, "co-1", sample_input()).await.unwrap();
        let err = get(&pool, &m.movement.id, "co-2").await.unwrap_err();
        assert!(matches!(err, AppError::NotFound));
    }

    #[tokio::test]
    async fn delete_removes_movement_and_lines() {
        let pool = setup_pool().await;
        let m = create(&pool, "co-1", sample_input()).await.unwrap();
        delete(&pool, &m.movement.id, "co-1").await.unwrap();
        let err = get(&pool, &m.movement.id, "co-1").await.unwrap_err();
        assert!(matches!(err, AppError::NotFound));
    }

    #[tokio::test]
    async fn list_returns_movements_in_period() {
        let pool = setup_pool().await;
        create(&pool, "co-1", sample_input()).await.unwrap();
        let list_result = list(&pool, "co-1", "2025-01-01", "2025-01-31")
            .await
            .unwrap();
        assert_eq!(list_result.len(), 1);
        assert_eq!(list_result[0].movement.movement_ref, "NIR-001");
    }
}
