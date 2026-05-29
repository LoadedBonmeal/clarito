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

pub async fn create(pool: &SqlitePool, input: CreatePaymentInput) -> AppResult<Payment> {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let amount_dec = Decimal::from_str(input.amount.trim())
        .map_err(|_| AppError::Validation("Sumă invalidă — folosiți formatul 1234.56".into()))?;
    if amount_dec <= Decimal::ZERO {
        return Err(AppError::Validation("Suma plății trebuie să fie pozitivă.".into()));
    }

    // Verify the invoice belongs to the given company before inserting
    let invoice_exists: Option<String> = sqlx::query_scalar(
        "SELECT id FROM invoices WHERE id = ?1 AND company_id = ?2 LIMIT 1",
    )
    .bind(&input.invoice_id)
    .bind(&input.company_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?;

    if invoice_exists.is_none() {
        return Err(AppError::Validation(
            "Factura nu aparține companiei specificate.".into(),
        ));
    }

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
    Ok(sqlx::query_as::<_, Payment>(
        "SELECT id, invoice_id, company_id, amount, currency, paid_at, method, reference, notes, created_at \
         FROM payments WHERE id = ?1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?)
}

pub async fn list_for_invoice(
    pool: &SqlitePool,
    invoice_id: &str,
    company_id: &str,
) -> AppResult<Vec<Payment>> {
    Ok(sqlx::query_as::<_, Payment>(
        "SELECT id, invoice_id, company_id, amount, currency, paid_at, method, reference, notes, created_at \
         FROM payments WHERE invoice_id = ?1 AND company_id = ?2 ORDER BY paid_at DESC",
    )
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

pub async fn list_all_summaries(
    pool: &SqlitePool,
    company_id: &str,
) -> AppResult<Vec<PaymentSummary>> {
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::str::FromStr;
    use sqlx::Row;

    // Fetch all invoices for the company — total_amount stored as TEXT
    let invoice_rows = sqlx::query(
        "SELECT id, total_amount FROM invoices WHERE company_id = ?1",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    // Fetch all payments for this company's invoices in ONE query
    let payment_rows = sqlx::query(
        "SELECT p.id, p.invoice_id, p.company_id, p.amount, p.currency, p.paid_at, \
                p.method, p.reference, p.notes, p.created_at \
         FROM payments p \
         INNER JOIN invoices i ON i.id = p.invoice_id \
         WHERE i.company_id = ?1 \
         ORDER BY p.paid_at DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    // Aggregate payments and build per-invoice lists
    let mut paid_map: HashMap<String, Decimal> = HashMap::new();
    let mut payments_by_invoice: HashMap<String, Vec<Payment>> = HashMap::new();

    for row in payment_rows {
        let invoice_id: String = row.try_get("invoice_id").map_err(AppError::Database)?;
        let amount_str: String = row.try_get("amount").unwrap_or_else(|_| "0".to_string());
        let amount = Decimal::from_str(&amount_str).unwrap_or(Decimal::ZERO);
        *paid_map.entry(invoice_id.clone()).or_insert(Decimal::ZERO) += amount;

        let payment = Payment {
            id: row.try_get("id").map_err(AppError::Database)?,
            invoice_id: invoice_id.clone(),
            company_id: row.try_get("company_id").map_err(AppError::Database)?,
            amount: amount_str,
            currency: row.try_get("currency").unwrap_or_else(|_| "RON".to_string()),
            paid_at: row.try_get("paid_at").map_err(AppError::Database)?,
            method: row.try_get("method").unwrap_or_else(|_| "transfer".to_string()),
            reference: row.try_get("reference").ok().flatten(),
            notes: row.try_get("notes").ok().flatten(),
            created_at: row.try_get("created_at").unwrap_or(0),
        };
        payments_by_invoice.entry(invoice_id).or_default().push(payment);
    }

    // Build one PaymentSummary per invoice
    let mut out = Vec::with_capacity(invoice_rows.len());
    for row in invoice_rows {
        let invoice_id: String = row.try_get("id").map_err(AppError::Database)?;
        let total_str: String = row.try_get("total_amount").unwrap_or_else(|_| "0".to_string());
        let total = Decimal::from_str(&total_str).unwrap_or(Decimal::ZERO).round_dp(2);
        let paid = paid_map.get(&invoice_id).copied().unwrap_or(Decimal::ZERO).round_dp(2);

        let payment_status = if paid <= Decimal::ZERO {
            "UNPAID"
        } else if paid >= total {
            "PAID"
        } else {
            "PARTIAL"
        };

        let payments = payments_by_invoice.remove(&invoice_id).unwrap_or_default();

        out.push(PaymentSummary {
            invoice_id,
            total_amount: total_str,
            paid_amount: paid.to_string(),
            payment_status: payment_status.to_string(),
            payments,
        });
    }

    Ok(out)
}

pub async fn summary_for_invoice(
    pool: &SqlitePool,
    invoice_id: &str,
    company_id: &str,
) -> AppResult<PaymentSummary> {
    // Fetch invoice total — scoped to company_id to prevent cross-company leakage
    let total: Option<String> = sqlx::query_scalar(
        "SELECT total_amount FROM invoices WHERE id = ?1 AND company_id = ?2",
    )
    .bind(invoice_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;

    use rust_decimal::Decimal;
    use std::str::FromStr;

    let total_str = total.ok_or(AppError::NotFound)?;

    // Sum payments with Decimal precision — fetch each amount as TEXT to avoid
    // any REAL/f64 cast that could lose precision.
    let payment_rows: Vec<String> = sqlx::query_scalar(
        "SELECT amount FROM payments WHERE invoice_id = ?1 AND company_id = ?2",
    )
    .bind(invoice_id)
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    let paid_total = payment_rows
        .iter()
        .map(|s| Decimal::from_str(s).unwrap_or(Decimal::ZERO))
        .fold(Decimal::ZERO, |acc, d| acc + d)
        .round_dp(2);

    let invoice_total = Decimal::from_str(&total_str)
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);

    let payment_status = if paid_total <= Decimal::ZERO {
        "UNPAID"
    } else if paid_total >= invoice_total {
        "PAID"
    } else {
        "PARTIAL"
    };

    let payments = list_for_invoice(pool, invoice_id, company_id).await?;

    Ok(PaymentSummary {
        invoice_id: invoice_id.to_string(),
        total_amount: total_str,
        paid_amount: paid_total.round_dp(2).to_string(),
        payment_status: payment_status.to_string(),
        payments,
    })
}
