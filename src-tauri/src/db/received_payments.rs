//! Payment tracking — money PAID to suppliers against received invoices.
//!
//! Buyer-side TVA la încasare (art. 297): the date a supplier invoice is paid is what unlocks
//! the deferred input-VAT deduction, so these rows are the settlement events the D300 + GL
//! buyer-side routing consume (mirroring the sales-side `payments`). Deduction is earmarked
//! per received invoice (Norme pct. 69 works invoice-by-invoice), hence the per-invoice FK.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::new_id;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedPayment {
    pub id: String,
    pub received_invoice_id: String,
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
pub struct CreateReceivedPaymentInput {
    pub received_invoice_id: String,
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
pub struct ReceivedPaymentSummary {
    pub received_invoice_id: String,
    pub total_amount: String,
    pub paid_amount: String,
    pub payment_status: String,
    pub payments: Vec<ReceivedPayment>,
}

pub async fn create(
    pool: &SqlitePool,
    input: CreateReceivedPaymentInput,
) -> AppResult<ReceivedPayment> {
    let mut conn = pool.acquire().await?;
    create_in(&mut conn, input).await
}

/// Period-lock check pe o CONEXIUNE existentă (`period_locks::is_period_locked` cere un pool;
/// `create_in` rulează și în tranzacția lui `match_bank_txn`). Fail-closed: o eroare DB se
/// propagă (nu se tratează drept „deblocat").
async fn is_period_locked_in(
    conn: &mut sqlx::SqliteConnection,
    company_id: &str,
    period: &str,
) -> AppResult<bool> {
    let row: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM period_locks WHERE company_id = ?1 AND period = ?2 LIMIT 1",
    )
    .bind(company_id)
    .bind(period)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.is_some())
}

/// Variantă pe conexiune/tranzacție EXISTENTĂ a lui [`create`] — folosită de `match_bank_txn`
/// pentru a crea plata și a marca tranzacția bancară MATCHED atomic (o singură tranzacție).
pub async fn create_in(
    conn: &mut sqlx::SqliteConnection,
    input: CreateReceivedPaymentInput,
) -> AppResult<ReceivedPayment> {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let amount_dec = Decimal::from_str(input.amount.trim())
        .map_err(|_| AppError::Validation("Sumă invalidă — folosiți formatul 1234.56".into()))?;
    if amount_dec <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Suma plății trebuie să fie pozitivă.".into(),
        ));
    }

    // Wave 4 (v0.7.4 audit) — period-lock guard on CREATE (the mirror of `delete`): a supplier
    // payment booked into a FILED (locked) month never reaches the GL (generate_gl_entries
    // refuses locked periods) yet could not be deleted either → a stuck row silently diverging
    // from the filed declaration (incl. the cash-VAT deduction release). Covers
    // add_received_payment AND match_bank_txn (both go through create_in).
    let period = input.paid_at.get(..7).unwrap_or("");
    if !period.is_empty() && is_period_locked_in(&mut *conn, &input.company_id, period).await? {
        return Err(AppError::Validation(format!(
            "Perioada {period} este blocată (declarație depusă) — plata nu poate fi înregistrată. \
             Înregistrați plata într-o perioadă deschisă sau deblocați perioada și depuneți o \
             declarație rectificativă."
        )));
    }

    // Verify the received invoice belongs to the company AND default the payment currency to
    // the invoice's currency (a EUR supplier invoice paid in RON would otherwise corrupt the
    // paid/remaining and the GL release).
    let invoice_currency: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE(NULLIF(TRIM(currency), ''), 'RON') FROM received_invoices \
         WHERE id = ?1 AND company_id = ?2 LIMIT 1",
    )
    .bind(&input.received_invoice_id)
    .bind(&input.company_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(AppError::Database)?;

    let invoice_currency = match invoice_currency {
        Some(c) => c,
        None => {
            return Err(AppError::Validation(
                "Factura primită nu aparține companiei specificate.".into(),
            ))
        }
    };

    let id = new_id();
    let currency = input.currency.unwrap_or(invoice_currency);
    let method = input.method.unwrap_or_else(|| "transfer".to_string());

    sqlx::query(
        "INSERT INTO received_invoice_payments \
         (id, received_invoice_id, company_id, amount, currency, paid_at, method, reference, notes, exchange_rate) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(&id)
    .bind(&input.received_invoice_id)
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

    Ok(sqlx::query_as::<_, ReceivedPayment>(
        "SELECT id, received_invoice_id, company_id, amount, currency, paid_at, method, \
                reference, notes, created_at \
         FROM received_invoice_payments WHERE id = ?1 AND company_id = ?2",
    )
    .bind(&id)
    .bind(&input.company_id)
    .fetch_one(&mut *conn)
    .await?)
}

pub async fn get_by_id(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<ReceivedPayment> {
    Ok(sqlx::query_as::<_, ReceivedPayment>(
        "SELECT id, received_invoice_id, company_id, amount, currency, paid_at, method, \
                reference, notes, created_at \
         FROM received_invoice_payments WHERE id = ?1 AND company_id = ?2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_one(pool)
    .await?)
}

pub async fn list_for_received_invoice(
    pool: &SqlitePool,
    received_invoice_id: &str,
    company_id: &str,
) -> AppResult<Vec<ReceivedPayment>> {
    Ok(sqlx::query_as::<_, ReceivedPayment>(
        "SELECT id, received_invoice_id, company_id, amount, currency, paid_at, method, \
                reference, notes, created_at \
         FROM received_invoice_payments WHERE received_invoice_id = ?1 AND company_id = ?2 \
         ORDER BY paid_at DESC",
    )
    .bind(received_invoice_id)
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    // Wave 4 audit — period-lock guard: deleting a supplier payment dated in a FILED (locked)
    // month would silently reverse its GL journal (and the cash-VAT deduction release) inside a
    // declared period. Same fail-closed pattern as db/invoices.rs create.
    let paid_at: Option<String> = sqlx::query_scalar(
        "SELECT paid_at FROM received_invoice_payments WHERE id = ?1 AND company_id = ?2",
    )
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

    // Wave 4 audit — atomicity: the payment row + its GL journal go in ONE transaction.
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
    let rows =
        sqlx::query("DELETE FROM received_invoice_payments WHERE id = ?1 AND company_id = ?2")
            .bind(id)
            .bind(company_id)
            .execute(&mut **tx)
            .await?
            .rows_affected();
    if rows == 0 {
        return Err(AppError::NotFound);
    }

    // Remove the GL journal posted by post_received_payment for this received payment id.
    // gl_entry rows cascade-delete when gl_journal rows are deleted (FK ON DELETE CASCADE).
    // Without this, a deleted received payment leaves a permanent orphan RECEIVED_PAYMENT
    // journal and the payable stays "cleared" in the ledger despite no payment existing.
    sqlx::query(
        "DELETE FROM gl_journal \
         WHERE company_id = ?1 AND source_type = 'RECEIVED_PAYMENT' AND source_id = ?2",
    )
    .bind(company_id)
    .bind(id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn summary_for_received_invoice(
    pool: &SqlitePool,
    received_invoice_id: &str,
    company_id: &str,
) -> AppResult<ReceivedPaymentSummary> {
    use rust_decimal::Decimal;
    use std::str::FromStr;

    // total_amount may be stored as REAL or TEXT across migrations → CAST to TEXT.
    let total: Option<String> = sqlx::query_scalar(
        "SELECT CAST(total_amount AS TEXT) FROM received_invoices \
         WHERE id = ?1 AND company_id = ?2",
    )
    .bind(received_invoice_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    let total_str = total.ok_or(AppError::NotFound)?;

    let payment_amounts: Vec<String> = sqlx::query_scalar(
        "SELECT amount FROM received_invoice_payments \
         WHERE received_invoice_id = ?1 AND company_id = ?2",
    )
    .bind(received_invoice_id)
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    let paid_total = payment_amounts
        .iter()
        .map(|s| Decimal::from_str(s).unwrap_or(Decimal::ZERO))
        .fold(Decimal::ZERO, |acc, d| acc + d)
        .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
    let invoice_total = Decimal::from_str(&total_str)
        .unwrap_or(Decimal::ZERO)
        .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);

    let payment_status = if paid_total <= Decimal::ZERO {
        "UNPAID"
    } else if paid_total >= invoice_total {
        "PAID"
    } else {
        "PARTIAL"
    };

    let payments = list_for_received_invoice(pool, received_invoice_id, company_id).await?;

    Ok(ReceivedPaymentSummary {
        received_invoice_id: received_invoice_id.to_string(),
        total_amount: total_str,
        paid_amount: paid_total.to_string(),
        payment_status: payment_status.to_string(),
        payments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed(pool: &SqlitePool) {
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co','12345678','Test SRL','Str 1','Bucuresti','B','RO')",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, total_amount, \
              currency, issue_date, xml_path, status) \
             VALUES ('ri','co','dl1','RO99','Furnizor','1190','RON','2026-03-10','/x.xml','NEW')",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    fn input(amount: &str, paid_at: &str) -> CreateReceivedPaymentInput {
        CreateReceivedPaymentInput {
            received_invoice_id: "ri".into(),
            company_id: "co".into(),
            amount: amount.into(),
            currency: None,
            paid_at: paid_at.into(),
            method: Some("transfer".into()),
            reference: None,
            notes: None,
            exchange_rate: None,
        }
    }

    #[tokio::test]
    async fn create_list_and_summary() {
        let pool = pool().await;
        seed(&pool).await;

        // Partial payment → PARTIAL.
        create(&pool, input("500", "2026-03-20")).await.unwrap();
        let s = summary_for_received_invoice(&pool, "ri", "co")
            .await
            .unwrap();
        assert_eq!(s.payment_status, "PARTIAL");
        assert_eq!(s.paid_amount, "500");
        assert_eq!(s.payments.len(), 1);
        // Currency defaulted to the invoice's RON.
        assert_eq!(s.payments[0].currency, "RON");

        // Settle the rest → PAID.
        create(&pool, input("690", "2026-04-05")).await.unwrap();
        let s2 = summary_for_received_invoice(&pool, "ri", "co")
            .await
            .unwrap();
        assert_eq!(s2.payment_status, "PAID");
        assert_eq!(s2.payments.len(), 2);
    }

    #[tokio::test]
    async fn rejects_non_positive_and_foreign_company() {
        let pool = pool().await;
        seed(&pool).await;
        assert!(create(&pool, input("0", "2026-03-20")).await.is_err());
        assert!(create(&pool, input("-5", "2026-03-20")).await.is_err());
        let mut bad = input("100", "2026-03-20");
        bad.company_id = "other".into();
        assert!(create(&pool, bad).await.is_err());
    }

    #[tokio::test]
    async fn delete_is_company_scoped() {
        let pool = pool().await;
        seed(&pool).await;
        let p = create(&pool, input("100", "2026-03-20")).await.unwrap();
        assert!(delete(&pool, &p.id, "other").await.is_err()); // wrong company
        assert!(delete(&pool, &p.id, "co").await.is_ok());
        assert!(list_for_received_invoice(&pool, "ri", "co")
            .await
            .unwrap()
            .is_empty());
    }

    // ── Wave 4 audit: delete refused when the payment's month is LOCKED ──────

    #[tokio::test]
    async fn delete_received_payment_refused_on_locked_period() {
        let pool = pool().await;
        seed(&pool).await;
        let p = create(&pool, input("500", "2026-03-20")).await.unwrap();

        // Lock the payment's month (a declaration was filed for 2026-03).
        crate::db::period_locks::lock_period(
            &pool,
            "co",
            "2026-03",
            "declaration:D300",
            None,
            None,
        )
        .await
        .unwrap();

        let r = delete(&pool, &p.id, "co").await;
        assert!(
            matches!(r, Err(AppError::Validation(_))),
            "delete in a locked month must be a Validation error, got {r:?}"
        );
        // The payment must still exist.
        assert!(get_by_id(&pool, &p.id, "co").await.is_ok());

        // Unlock → delete succeeds.
        crate::db::period_locks::unlock_period(&pool, "co", "2026-03")
            .await
            .unwrap();
        delete(&pool, &p.id, "co").await.unwrap();
        assert!(get_by_id(&pool, &p.id, "co").await.is_err());
    }

    // ── Wave 4 audit: CREATE refused when the payment's month is LOCKED ──────

    #[tokio::test]
    async fn create_received_payment_refused_on_locked_period() {
        let pool = pool().await;
        seed(&pool).await;

        // Lock 2026-03 (a declaration was filed for that month).
        crate::db::period_locks::lock_period(
            &pool,
            "co",
            "2026-03",
            "declaration:D300",
            None,
            None,
        )
        .await
        .unwrap();

        // Payment dated in the locked month → refused, nothing persisted.
        let r = create(&pool, input("500", "2026-03-20")).await;
        assert!(
            matches!(r, Err(AppError::Validation(_))),
            "create in a locked month must be a Validation error, got {r:?}"
        );
        let cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM received_invoice_payments WHERE company_id = 'co'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cnt, 0, "no payment row persisted for a locked month");

        // An open month succeeds while the lock holds; unlocking reopens the month.
        create(&pool, input("100", "2026-04-05")).await.unwrap();
        crate::db::period_locks::unlock_period(&pool, "co", "2026-03")
            .await
            .unwrap();
        create(&pool, input("500", "2026-03-20")).await.unwrap();
    }

    // ── FIX 2: delete_received_payment cleans its GL journal ─────────────────

    #[tokio::test]
    async fn delete_received_payment_cleans_gl_journal() {
        let pool = pool().await;
        seed(&pool).await;

        let p = create(&pool, input("500", "2026-03-20")).await.unwrap();
        let pid = p.id.clone();

        // Simulate the GL journal that post_received_payment would create.
        sqlx::query(
            "INSERT INTO gl_journal \
             (id, company_id, journal_id, journal_type, transaction_id, transaction_date, \
              source_type, source_id) \
             VALUES ('jrn_rp1', 'co', 'BANCA', 'RECEIVED_PAYMENT', 'jrn_rp1', '2026-03-20', \
                     'RECEIVED_PAYMENT', ?1)",
        )
        .bind(&pid)
        .execute(&pool)
        .await
        .unwrap();

        // Verify the journal exists before delete.
        let before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal \
             WHERE company_id='co' AND source_type='RECEIVED_PAYMENT' AND source_id=?1",
        )
        .bind(&pid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(before, 1, "GL journal must exist before delete");

        // Delete the received payment.
        delete(&pool, &pid, "co").await.unwrap();

        // The gl_journal row must be gone.
        let after: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal \
             WHERE company_id='co' AND source_type='RECEIVED_PAYMENT' AND source_id=?1",
        )
        .bind(&pid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            after, 0,
            "GL journal must be deleted with the received payment"
        );
    }
}
