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
    /// Payment-date BNR exchange rate (for FX gain/loss). None → invoice rate (no FX diff).
    pub exchange_rate: Option<f64>,
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
    let mut conn = pool.acquire().await?;
    create_in(&mut conn, input).await
}

/// Variantă pe conexiune/tranzacție EXISTENTĂ a lui [`create`] — folosită de `match_bank_txn`
/// pentru a crea plata și a marca tranzacția bancară MATCHED atomic (o singură tranzacție).
pub async fn create_in(
    conn: &mut sqlx::SqliteConnection,
    input: CreatePaymentInput,
) -> AppResult<Payment> {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let amount_dec = Decimal::from_str(input.amount.trim())
        .map_err(|_| AppError::Validation("Sumă invalidă — folosiți formatul 1234.56".into()))?;
    if amount_dec <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Suma plății trebuie să fie pozitivă.".into(),
        ));
    }

    // Verify the invoice belongs to the given company AND fetch its currency, so a
    // payment defaults to the INVOICE's currency (not always RON). Otherwise a payment
    // on a EUR invoice would be stored as RON, corrupting paid/remaining and the GL.
    let invoice_currency: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE(NULLIF(TRIM(currency), ''), 'RON') FROM invoices \
         WHERE id = ?1 AND company_id = ?2 LIMIT 1",
    )
    .bind(&input.invoice_id)
    .bind(&input.company_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(AppError::Database)?;

    let invoice_currency = match invoice_currency {
        Some(c) => c,
        None => {
            return Err(AppError::Validation(
                "Factura nu aparține companiei specificate.".into(),
            ))
        }
    };

    let id = new_id();
    let currency = input.currency.unwrap_or(invoice_currency);
    let method = input.method.unwrap_or_else(|| "transfer".to_string());

    sqlx::query(
        "INSERT INTO payments (id, invoice_id, company_id, amount, currency, paid_at, method, reference, notes, exchange_rate)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
    .bind(input.exchange_rate)
    .execute(&mut *conn)
    .await?;

    Ok(sqlx::query_as::<_, Payment>(
        "SELECT id, invoice_id, company_id, amount, currency, paid_at, method, reference, notes, created_at \
         FROM payments WHERE id = ?1 AND company_id = ?2",
    )
    .bind(&id)
    .bind(&input.company_id)
    .fetch_one(&mut *conn)
    .await?)
}

pub async fn get_by_id(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Payment> {
    Ok(sqlx::query_as::<_, Payment>(
        "SELECT id, invoice_id, company_id, amount, currency, paid_at, method, reference, notes, created_at \
         FROM payments WHERE id = ?1 AND company_id = ?2",
    )
    .bind(id)
    .bind(company_id)
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
    // Wave 4 audit — period-lock guard: deleting a payment dated in a FILED (locked) month
    // would silently reverse its GL journal inside a declared period (D300/SAF-T already
    // reported the cleared receivable). Same fail-closed pattern as db/invoices.rs create.
    let paid_at: Option<String> =
        sqlx::query_scalar("SELECT paid_at FROM payments WHERE id = ?1 AND company_id = ?2")
            .bind(id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?;
    let paid_at = paid_at.ok_or(AppError::NotFound)?;
    let period = paid_at.get(..7).unwrap_or("");
    if !period.is_empty()
        && crate::db::period_locks::is_period_locked(pool, company_id, period).await?
    {
        return Err(AppError::Validation(format!(
            "Perioada {period} este blocată (declarație depusă) — plata nu poate fi ștearsă. \
             Deblocați perioada pentru a continua."
        )));
    }

    // Wave 4 audit — atomicity: the payment row + its GL journal go in ONE transaction, so a
    // failure between the two statements can no longer leave an orphan PAYMENT journal.
    let mut tx = pool.begin().await?;
    delete_in(&mut tx, id, company_id).await?;
    tx.commit().await?;
    Ok(())
}

/// Variantă pe tranzacție EXISTENTĂ a lui [`delete`] — FĂRĂ guard de period-lock (apelanții
/// trebuie să fi verificat deja blocarea perioadei). Folosită de `delete` și de
/// `unmatch_bank_txn` (plata + resetarea tranzacției bancare într-o singură tranzacție).
pub async fn delete_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    company_id: &str,
) -> AppResult<()> {
    // Delete the payment row first so we can distinguish NotFound (0 rows) from
    // a successful delete.  Then remove the GL journal for this payment (gl_entry
    // cascades via its FK on gl_journal.id — no separate DELETE needed).
    // Without this, a deleted payment leaves a permanent orphan PAYMENT journal
    // and the receivable stays "cleared" in the ledger despite no payment existing.
    let rows = sqlx::query("DELETE FROM payments WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();

    if rows == 0 {
        return Err(AppError::NotFound);
    }

    // Remove the GL journal posted by post_payment for this payment id.
    // gl_entry rows cascade-delete when gl_journal rows are deleted (FK ON DELETE CASCADE).
    sqlx::query(
        "DELETE FROM gl_journal \
         WHERE company_id = ?1 AND source_type = 'PAYMENT' AND source_id = ?2",
    )
    .bind(company_id)
    .bind(id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn list_all_summaries(
    pool: &SqlitePool,
    company_id: &str,
) -> AppResult<Vec<PaymentSummary>> {
    use rust_decimal::Decimal;
    use sqlx::Row;
    use std::collections::HashMap;

    // Fetch all invoices for the company — total_amount stored as TEXT
    let invoice_rows = sqlx::query("SELECT id, total_amount FROM invoices WHERE company_id = ?1")
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
        // Logged parse — a corrupted payment amount must not silently zero the running sum.
        let amount = crate::db::models::dec_logged("payments.paid_map", &amount_str);
        *paid_map.entry(invoice_id.clone()).or_insert(Decimal::ZERO) += amount;

        let payment = Payment {
            id: row.try_get("id").map_err(AppError::Database)?,
            invoice_id: invoice_id.clone(),
            company_id: row.try_get("company_id").map_err(AppError::Database)?,
            amount: amount_str,
            currency: row
                .try_get("currency")
                .unwrap_or_else(|_| "RON".to_string()),
            paid_at: row.try_get("paid_at").map_err(AppError::Database)?,
            method: row
                .try_get("method")
                .unwrap_or_else(|_| "transfer".to_string()),
            reference: row.try_get("reference").ok().flatten(),
            notes: row.try_get("notes").ok().flatten(),
            created_at: row.try_get("created_at").unwrap_or(0),
        };
        payments_by_invoice
            .entry(invoice_id)
            .or_default()
            .push(payment);
    }

    // Build one PaymentSummary per invoice
    let mut out = Vec::with_capacity(invoice_rows.len());
    for row in invoice_rows {
        let invoice_id: String = row.try_get("id").map_err(AppError::Database)?;
        let total_str: String = row
            .try_get("total_amount")
            .unwrap_or_else(|_| "0".to_string());
        let total = crate::db::invoices::round2(crate::db::models::dec_logged(
            "payments.summaries.total",
            &total_str,
        ));
        let paid = paid_map
            .get(&invoice_id)
            .copied()
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);

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
    let total: Option<String> =
        sqlx::query_scalar("SELECT total_amount FROM invoices WHERE id = ?1 AND company_id = ?2")
            .bind(invoice_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?;

    use rust_decimal::Decimal;

    let total_str = total.ok_or(AppError::NotFound)?;

    // Sum payments with Decimal precision — fetch each amount as TEXT to avoid
    // any REAL/f64 cast that could lose precision.
    let payment_rows: Vec<String> =
        sqlx::query_scalar("SELECT amount FROM payments WHERE invoice_id = ?1 AND company_id = ?2")
            .bind(invoice_id)
            .bind(company_id)
            .fetch_all(pool)
            .await
            .map_err(AppError::Database)?;

    let paid_total = crate::db::invoices::round2(
        payment_rows
            .iter()
            .map(|s| crate::db::models::dec_logged("payments.summary.amount", s))
            .fold(Decimal::ZERO, |acc, d| acc + d),
    );

    let invoice_total = crate::db::invoices::round2(crate::db::models::dec_logged(
        "payments.summary.total",
        &total_str,
    ));

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
        paid_amount: paid_total
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string(),
        payment_status: payment_status.to_string(),
        payments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shared fixture: in-memory DB with migrations + one company + one contact.
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
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, legal_name) \
             VALUES ('ct','co','CUSTOMER','Cumpărător SRL')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    /// Helper: insert a minimal RON invoice (total_amount as TEXT per migration 006).
    async fn seed_invoice(
        pool: &SqlitePool,
        id: &str,
        company_id: &str,
        total: &str,
        currency: &str,
    ) {
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, \
              issue_date, due_date, currency, subtotal_amount, vat_amount, total_amount, \
              status, payment_means_code, created_at, updated_at) \
             VALUES (?1,?2,'ct','F',1,'F/1','2026-01-01','2026-02-01',?3,'0','0',?4,'VALIDATED','30',1,1)",
        )
        .bind(id)
        .bind(company_id)
        .bind(currency)
        .bind(total)
        .execute(pool)
        .await
        .unwrap();
    }

    // ─── Test 1: create() rejects amount ≤ 0 ───────────────────────────────

    #[tokio::test]
    async fn create_rejects_zero_amount() {
        let pool = pool().await;
        seed_invoice(&pool, "inv1", "co", "100.00", "RON").await;
        let err = create(
            &pool,
            CreatePaymentInput {
                invoice_id: "inv1".into(),
                company_id: "co".into(),
                amount: "0".into(),
                currency: None,
                paid_at: "2026-01-10".into(),
                method: None,
                reference: None,
                notes: None,
                exchange_rate: None,
            },
        )
        .await;
        assert!(
            matches!(err, Err(AppError::Validation(_))),
            "zero amount should be a Validation error"
        );
    }

    #[tokio::test]
    async fn create_rejects_negative_amount() {
        let pool = pool().await;
        seed_invoice(&pool, "inv1", "co", "100.00", "RON").await;
        let err = create(
            &pool,
            CreatePaymentInput {
                invoice_id: "inv1".into(),
                company_id: "co".into(),
                amount: "-50".into(),
                currency: None,
                paid_at: "2026-01-10".into(),
                method: None,
                reference: None,
                notes: None,
                exchange_rate: None,
            },
        )
        .await;
        assert!(matches!(err, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn create_rejects_garbage_amount() {
        let pool = pool().await;
        seed_invoice(&pool, "inv1", "co", "100.00", "RON").await;
        let err = create(
            &pool,
            CreatePaymentInput {
                invoice_id: "inv1".into(),
                company_id: "co".into(),
                amount: "abc".into(),
                currency: None,
                paid_at: "2026-01-10".into(),
                method: None,
                reference: None,
                notes: None,
                exchange_rate: None,
            },
        )
        .await;
        assert!(
            matches!(err, Err(AppError::Validation(_))),
            "garbage amount should be a Validation error"
        );
    }

    // ─── Test 2: create() rejects invoice from another company ─────────────

    #[tokio::test]
    async fn create_rejects_cross_company_invoice() {
        let pool = pool().await;
        // Seed a second company and its invoice.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co2','RO2','T2','S','C','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, legal_name) \
             VALUES ('ct2','co2','CUSTOMER','Alt SRL')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, \
              issue_date, due_date, currency, subtotal_amount, vat_amount, total_amount, \
              status, payment_means_code, created_at, updated_at) \
             VALUES ('inv2','co2','ct2','F',1,'F/1','2026-01-01','2026-02-01','RON','0','0','200.00','VALIDATED','30',1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Try to register a payment for 'inv2' but claim company 'co'.
        let err = create(
            &pool,
            CreatePaymentInput {
                invoice_id: "inv2".into(),
                company_id: "co".into(), // wrong company
                amount: "50".into(),
                currency: None,
                paid_at: "2026-01-10".into(),
                method: None,
                reference: None,
                notes: None,
                exchange_rate: None,
            },
        )
        .await;
        assert!(
            matches!(err, Err(AppError::Validation(_))),
            "cross-company invoice should be rejected as Validation error"
        );
    }

    // ─── Test 3: create() inherits invoice currency when input.currency is None ──

    #[tokio::test]
    async fn create_defaults_to_invoice_currency() {
        let pool = pool().await;
        // Seed a EUR invoice.
        seed_invoice(&pool, "inv_eur", "co", "500.00", "EUR").await;

        let payment = create(
            &pool,
            CreatePaymentInput {
                invoice_id: "inv_eur".into(),
                company_id: "co".into(),
                amount: "100".into(),
                currency: None, // must inherit EUR from invoice
                paid_at: "2026-01-15".into(),
                method: None,
                reference: None,
                notes: None,
                exchange_rate: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(
            payment.currency, "EUR",
            "payment should inherit invoice currency EUR"
        );
    }

    // ─── Test 4: summary math — PAID / PARTIAL ──────────────────────────────

    #[tokio::test]
    async fn summary_fully_paid() {
        let pool = pool().await;
        seed_invoice(&pool, "inv1", "co", "100.00", "RON").await;

        // Two payments summing to exactly 100.
        for (id, amt) in [("p1", "40"), ("p2", "60")] {
            create(
                &pool,
                CreatePaymentInput {
                    invoice_id: "inv1".into(),
                    company_id: "co".into(),
                    amount: amt.into(),
                    currency: None,
                    paid_at: "2026-01-10".into(),
                    method: None,
                    reference: Some(id.into()),
                    notes: None,
                    exchange_rate: None,
                },
            )
            .await
            .unwrap();
        }

        let s = summary_for_invoice(&pool, "inv1", "co").await.unwrap();
        assert_eq!(s.payment_status, "PAID");
        assert_eq!(s.paid_amount, "100");
    }

    #[tokio::test]
    async fn summary_partial_payment() {
        let pool = pool().await;
        seed_invoice(&pool, "inv1", "co", "100.00", "RON").await;

        create(
            &pool,
            CreatePaymentInput {
                invoice_id: "inv1".into(),
                company_id: "co".into(),
                amount: "40".into(),
                currency: None,
                paid_at: "2026-01-10".into(),
                method: None,
                reference: None,
                notes: None,
                exchange_rate: None,
            },
        )
        .await
        .unwrap();

        let s = summary_for_invoice(&pool, "inv1", "co").await.unwrap();
        assert_eq!(s.payment_status, "PARTIAL");
    }

    #[tokio::test]
    async fn list_all_summaries_paid_and_partial() {
        let pool = pool().await;
        seed_invoice(&pool, "inv1", "co", "100.00", "RON").await;

        // Fully paid invoice.
        for amt in ["40", "60"] {
            create(
                &pool,
                CreatePaymentInput {
                    invoice_id: "inv1".into(),
                    company_id: "co".into(),
                    amount: amt.into(),
                    currency: None,
                    paid_at: "2026-01-10".into(),
                    method: None,
                    reference: None,
                    notes: None,
                    exchange_rate: None,
                },
            )
            .await
            .unwrap();
        }

        // Second invoice — partial (need a different series/number to satisfy UNIQUE).
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, \
              issue_date, due_date, currency, subtotal_amount, vat_amount, total_amount, \
              status, payment_means_code, created_at, updated_at) \
             VALUES ('inv2','co','ct','F',2,'F/2','2026-01-01','2026-02-01','RON','0','0','200.00','VALIDATED','30',1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        create(
            &pool,
            CreatePaymentInput {
                invoice_id: "inv2".into(),
                company_id: "co".into(),
                amount: "50".into(),
                currency: None,
                paid_at: "2026-01-11".into(),
                method: None,
                reference: None,
                notes: None,
                exchange_rate: None,
            },
        )
        .await
        .unwrap();

        let summaries = list_all_summaries(&pool, "co").await.unwrap();
        let get = |id: &str| summaries.iter().find(|s| s.invoice_id == id).unwrap();

        assert_eq!(get("inv1").payment_status, "PAID");
        assert_eq!(get("inv2").payment_status, "PARTIAL");
    }

    // ─── Test GL-cleanup: delete_payment removes its gl_journal (FIX 2) ─────

    #[tokio::test]
    async fn delete_payment_cleans_gl_journal() {
        let pool = pool().await;
        seed_invoice(&pool, "inv1", "co", "100.00", "RON").await;

        // Add a payment.
        let payment = create(
            &pool,
            CreatePaymentInput {
                invoice_id: "inv1".into(),
                company_id: "co".into(),
                amount: "100".into(),
                currency: None,
                paid_at: "2026-01-10".into(),
                method: None,
                reference: None,
                notes: None,
                exchange_rate: None,
            },
        )
        .await
        .unwrap();

        let pid = payment.id.clone();

        // Simulate the GL journal that post_payment would create for this payment.
        sqlx::query(
            "INSERT INTO gl_journal \
             (id, company_id, journal_id, journal_type, transaction_id, transaction_date, \
              source_type, source_id) \
             VALUES ('jrn1', 'co', 'BANCA', 'PAYMENT', 'jrn1', '2026-01-10', 'PAYMENT', ?1)",
        )
        .bind(&pid)
        .execute(&pool)
        .await
        .unwrap();

        // Verify the journal exists before delete.
        let before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal \
             WHERE company_id='co' AND source_type='PAYMENT' AND source_id=?1",
        )
        .bind(&pid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(before, 1, "GL journal must exist before delete");

        // Delete the payment.
        delete(&pool, &pid, "co").await.unwrap();

        // The gl_journal row must be gone after delete.
        let after: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal \
             WHERE company_id='co' AND source_type='PAYMENT' AND source_id=?1",
        )
        .bind(&pid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(after, 0, "GL journal must be deleted with the payment");
    }

    // ─── Wave 4 audit: delete refused when the payment's month is LOCKED ───

    #[tokio::test]
    async fn delete_payment_refused_on_locked_period() {
        let pool = pool().await;
        seed_invoice(&pool, "inv1", "co", "100.00", "RON").await;
        let payment = create(
            &pool,
            CreatePaymentInput {
                invoice_id: "inv1".into(),
                company_id: "co".into(),
                amount: "100".into(),
                currency: None,
                paid_at: "2026-01-10".into(),
                method: None,
                reference: None,
                notes: None,
                exchange_rate: None,
            },
        )
        .await
        .unwrap();

        // Lock the payment's month (a declaration was filed for 2026-01).
        crate::db::period_locks::lock_period(
            &pool,
            "co",
            "2026-01",
            "declaration:D300",
            None,
            None,
        )
        .await
        .unwrap();

        let r = delete(&pool, &payment.id, "co").await;
        assert!(
            matches!(r, Err(AppError::Validation(_))),
            "delete in a locked month must be a Validation error, got {r:?}"
        );
        // The payment must still exist.
        assert!(get_by_id(&pool, &payment.id, "co").await.is_ok());

        // Unlock → delete succeeds.
        crate::db::period_locks::unlock_period(&pool, "co", "2026-01")
            .await
            .unwrap();
        delete(&pool, &payment.id, "co").await.unwrap();
        assert!(get_by_id(&pool, &payment.id, "co").await.is_err());
    }

    // ─── Test 5: corrupted amount does NOT panic — treated as 0 ────────────

    #[tokio::test]
    async fn corrupted_payment_amount_treated_as_zero_no_panic() {
        let pool = pool().await;
        seed_invoice(&pool, "inv1", "co", "100.00", "RON").await;

        // Insert a valid payment first.
        create(
            &pool,
            CreatePaymentInput {
                invoice_id: "inv1".into(),
                company_id: "co".into(),
                amount: "30".into(),
                currency: None,
                paid_at: "2026-01-10".into(),
                method: None,
                reference: None,
                notes: None,
                exchange_rate: None,
            },
        )
        .await
        .unwrap();

        // Corrupt the amount directly in the DB.
        sqlx::query("UPDATE payments SET amount = 'garbage' WHERE invoice_id = 'inv1'")
            .execute(&pool)
            .await
            .unwrap();

        // summary_for_invoice must not panic and must treat garbage as 0.
        let s = summary_for_invoice(&pool, "inv1", "co").await.unwrap();
        assert_eq!(
            s.paid_amount, "0",
            "corrupted amount should be treated as 0 via dec_logged path"
        );
        assert_eq!(s.payment_status, "UNPAID");

        // list_all_summaries must also survive without panic.
        let all = list_all_summaries(&pool, "co").await.unwrap();
        let inv = all.iter().find(|s| s.invoice_id == "inv1").unwrap();
        assert_eq!(inv.paid_amount, "0");
        assert_eq!(inv.payment_status, "UNPAID");
    }
}
