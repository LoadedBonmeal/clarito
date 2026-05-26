//! Payment tracking — money received against issued invoices.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::new_id;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Payment {
    pub id: String,
    pub invoice_id: String,
    pub company_id: String,
    pub amount: String,
    pub currency: String,
    pub paid_at: String,
    pub method: String,
    pub reference: Option<String>,
    pub notes: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePaymentInput {
    pub invoice_id: String,
    pub company_id: String,
    pub amount: String,
    pub currency: Option<String>,
    pub paid_at: String,
    pub method: Option<String>,
    pub reference: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentSummary {
    pub invoice_id: String,
    pub total_amount: String,
    pub paid_amount: String,
    pub payment_status: String,
    pub payments: Vec<Payment>,
}

const SELECT_COLS: &str =
    "id, invoice_id, company_id, amount, currency, paid_at, method, reference, notes, created_at";

pub async fn create(pool: &SqlitePool, input: CreatePaymentInput) -> AppResult<Payment> {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    Decimal::from_str(&input.amount)
        .map_err(|_| AppError::Validation("Sumă invalidă — folosiți formatul 1234.56".into()))?;

    let id = new_id();
    let currency = input.currency.unwrap_or_else(|| "RON".to_string());
    let method = input.method.unwrap_or_else(|| "transfer".to_string());

    sqlx::query(
        "INSERT INTO payments (id, invoice_id, company_id, amount, currency, paid_at, method, reference, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(&id)
    .bind(&input.invoice_id)
    .bind(&input.company_id)
    .bind(&input.amount)
    .bind(&currency)
    .bind(&input.paid_at)
    .bind(&method)
    .bind(&input.reference)
    .bind(&input.notes)
    .execute(pool)
    .await?;

    get_by_id(pool, &id).await
}

pub async fn get_by_id(pool: &SqlitePool, id: &str) -> AppResult<Payment> {
    let sql = format!("SELECT {SELECT_COLS} FROM payments WHERE id = ?1");
    Ok(sqlx::query_as::<_, Payment>(&sql)
        .bind(id)
        .fetch_one(pool)
        .await?)
}

pub async fn list_for_invoice(
    pool: &SqlitePool,
    invoice_id: &str,
    company_id: &str,
) -> AppResult<Vec<Payment>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM payments WHERE invoice_id = ?1 AND company_id = ?2 ORDER BY paid_at DESC"
    );
    Ok(sqlx::query_as::<_, Payment>(&sql)
        .bind(invoice_id)
        .bind(company_id)
        .fetch_all(pool)
        .await?)
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let rows = sqlx::query(
        "DELETE FROM payments WHERE id = ?1 AND company_id = ?2",
    )
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

pub async fn summary_for_invoice(
    pool: &SqlitePool,
    invoice_id: &str,
    company_id: &str,
) -> AppResult<PaymentSummary> {
    // Fetch invoice total
    let total: Option<String> = sqlx::query_scalar(
        "SELECT total_amount FROM invoices WHERE id = ?1",
    )
    .bind(invoice_id)
    .fetch_optional(pool)
    .await?;

    let total_str = total.ok_or(AppError::NotFound)?;

    // Sum payments
    let paid_sum: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CAST(amount AS REAL)), 0.0) FROM payments WHERE invoice_id = ?1",
    )
    .bind(invoice_id)
    .fetch_one(pool)
    .await?;

    let total_f: f64 = total_str.parse().unwrap_or(0.0);
    let payment_status = if paid_sum <= 0.0 {
        "UNPAID"
    } else if paid_sum >= total_f {
        "PAID"
    } else {
        "PARTIAL"
    };

    let payments = list_for_invoice(pool, invoice_id, company_id).await?;

    Ok(PaymentSummary {
        invoice_id: invoice_id.to_string(),
        total_amount: total_str,
        paid_amount: format!("{paid_sum:.2}"),
        payment_status: payment_status.to_string(),
        payments,
    })
}
