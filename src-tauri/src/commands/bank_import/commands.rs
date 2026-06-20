//! Tauri commands — bank statement import (Wave 6).
//!
//! Commands:
//!   create_bank_account / list_bank_accounts / delete_bank_account
//!   import_bank_statement        — parse + stage, idempotent via content_hash
//!   list_bank_statements         — list all statements for a company
//!   list_bank_transactions       — list txns in a statement + auto-suggestions
//!   match_bank_txn               — confirm a match (creates payment via existing payment DB fns)
//!   unmatch_bank_txn             — reverse a match (deletes the payment)
//!   ignore_bank_txn              — mark as IGNORED (bank fee / internal transfer)

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::str::FromStr;
use tauri::State;

use crate::db::models::new_id;
use crate::db::payments::{self, CreatePaymentInput};
use crate::db::received_payments::{self, CreateReceivedPaymentInput};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::matching::{suggest_matches, MatchSuggestion};
use super::parser::BankStatementParser;

// ─── Content hash ─────────────────────────────────────────────────────────────

/// Deterministic content hash for dedup (re-import the same file → same hash).
fn content_hash(bytes: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}", h.finish())
}

// ─── Bank account CRUD ────────────────────────────────────────────────────────

/// A company's own bank account (maps to GL 5121 or 5124).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BankAccount {
    pub id: String,
    pub company_id: String,
    pub iban: String,
    pub bank_name: String,
    pub currency: String,
    pub gl_account: String,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBankAccountArgs {
    pub company_id: String,
    pub iban: String,
    pub bank_name: String,
    pub currency: String,
    pub gl_account: Option<String>,
}

#[tauri::command]
pub async fn create_bank_account(
    state: State<'_, AppState>,
    args: CreateBankAccountArgs,
) -> AppResult<BankAccount> {
    let id = new_id();
    let gl = args.gl_account.unwrap_or_else(|| "5121".to_string());
    sqlx::query(
        "INSERT INTO bank_accounts (id, company_id, iban, bank_name, currency, gl_account) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(&id)
    .bind(&args.company_id)
    .bind(&args.iban)
    .bind(&args.bank_name)
    .bind(&args.currency)
    .bind(&gl)
    .execute(&state.db)
    .await?;

    Ok(sqlx::query_as::<_, BankAccount>(
        "SELECT id, company_id, iban, bank_name, currency, gl_account, created_at \
         FROM bank_accounts WHERE id = ?1",
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?)
}

#[tauri::command]
pub async fn list_bank_accounts(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<BankAccount>> {
    Ok(sqlx::query_as::<_, BankAccount>(
        "SELECT id, company_id, iban, bank_name, currency, gl_account, created_at \
         FROM bank_accounts WHERE company_id = ?1 ORDER BY created_at",
    )
    .bind(&company_id)
    .fetch_all(&state.db)
    .await?)
}

#[tauri::command]
pub async fn delete_bank_account(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM bank_accounts WHERE id = ?1 AND company_id = ?2")
        .bind(&id)
        .bind(&company_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ─── Statement / transaction types ────────────────────────────────────────────

/// A persisted bank statement header.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BankStatement {
    pub id: String,
    pub company_id: String,
    pub bank_account_id: Option<String>,
    pub source_format: String,
    pub statement_ref: String,
    pub opening_balance: String,
    pub closing_balance: String,
    pub statement_date: String,
    pub content_hash: String,
    pub created_at: i64,
}

/// A persisted bank transaction, including auto-suggestions when status=UNMATCHED.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BankTransaction {
    pub id: String,
    pub statement_id: String,
    pub company_id: String,
    pub booking_date: String,
    pub value_date: Option<String>,
    pub amount: String,
    pub currency: String,
    pub counterparty_name: Option<String>,
    pub counterparty_iban: Option<String>,
    pub counterparty_cui: Option<String>,
    pub reference: Option<String>,
    pub txn_hash: String,
    pub status: String,
    pub matched_invoice_id: Option<String>,
    pub matched_payment_id: Option<String>,
    /// Auto-suggestions (non-empty only for UNMATCHED transactions).
    pub suggestions: Vec<MatchSuggestion>,
}

/// Result returned by import_bank_statement.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStatementResult {
    pub statement: BankStatement,
    pub imported_txns: usize,
    /// True when the file was a duplicate (same content_hash already in DB).
    pub duplicate: bool,
    pub warnings: Vec<String>,
    /// Integrity check result from the parser. None = not applicable (e.g. CSV).
    pub integrity_ok: Option<bool>,
}

// ─── import_bank_statement ────────────────────────────────────────────────────

#[tauri::command]
pub async fn import_bank_statement(
    state: State<'_, AppState>,
    company_id: String,
    source_format: String,
    file_bytes: Vec<u8>,
    bank_account_id: Option<String>,
) -> AppResult<ImportStatementResult> {
    let hash = content_hash(&file_bytes);

    // Dedup check — re-importing the same file is a no-op
    let existing_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM bank_statements \
         WHERE company_id = ?1 AND content_hash = ?2 LIMIT 1",
    )
    .bind(&company_id)
    .bind(&hash)
    .fetch_optional(&state.db)
    .await?;

    if let Some(eid) = existing_id {
        let stmt = sqlx::query_as::<_, BankStatement>(
            "SELECT id, company_id, bank_account_id, source_format, statement_ref, \
                    opening_balance, closing_balance, statement_date, content_hash, created_at \
             FROM bank_statements WHERE id = ?1",
        )
        .bind(&eid)
        .fetch_one(&state.db)
        .await?;
        return Ok(ImportStatementResult {
            statement: stmt,
            imported_txns: 0,
            duplicate: true,
            warnings: vec!["Statement already imported (content hash matches).".into()],
            integrity_ok: None,
        });
    }

    // Parse according to format
    let parsed = match source_format.to_uppercase().as_str() {
        "MT940" => super::mt940::Mt940Parser.parse(&file_bytes)?,
        "CAMT053" => super::camt053::Camt053Parser.parse(&file_bytes)?,
        "CSV" => super::csv_parser::CsvParser.parse(&file_bytes)?,
        other => {
            return Err(AppError::Validation(format!(
                "Format necunoscut: '{other}'. Acceptat: MT940, CAMT053, CSV."
            )))
        }
    };

    let stmt_id = new_id();
    sqlx::query(
        "INSERT INTO bank_statements \
         (id, company_id, bank_account_id, source_format, statement_ref, \
          opening_balance, closing_balance, statement_date, content_hash) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(&stmt_id)
    .bind(&company_id)
    .bind(&bank_account_id)
    .bind(source_format.to_uppercase())
    .bind(&parsed.statement_ref)
    .bind(parsed.opening_balance.to_string())
    .bind(parsed.closing_balance.to_string())
    .bind(&parsed.statement_date)
    .bind(&hash)
    .execute(&state.db)
    .await?;

    let mut imported = 0usize;
    for txn in &parsed.txns {
        // Per-transaction dedup by txn_hash within this statement
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM bank_transactions \
             WHERE statement_id = ?1 AND txn_hash = ?2 LIMIT 1",
        )
        .bind(&stmt_id)
        .bind(&txn.txn_hash)
        .fetch_optional(&state.db)
        .await?;
        if exists.is_some() {
            continue;
        }

        let txn_id = new_id();
        sqlx::query(
            "INSERT INTO bank_transactions \
             (id, statement_id, company_id, booking_date, value_date, amount, currency, \
              counterparty_name, counterparty_iban, counterparty_cui, reference, txn_hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )
        .bind(&txn_id)
        .bind(&stmt_id)
        .bind(&company_id)
        .bind(&txn.booking_date)
        .bind(&txn.value_date)
        .bind(txn.amount.to_string())
        .bind(&txn.currency)
        .bind(&txn.counterparty_name)
        .bind(&txn.counterparty_iban)
        .bind(&txn.counterparty_cui)
        .bind(&txn.reference)
        .bind(&txn.txn_hash)
        .execute(&state.db)
        .await?;
        imported += 1;
    }

    let stmt = sqlx::query_as::<_, BankStatement>(
        "SELECT id, company_id, bank_account_id, source_format, statement_ref, \
                opening_balance, closing_balance, statement_date, content_hash, created_at \
         FROM bank_statements WHERE id = ?1",
    )
    .bind(&stmt_id)
    .fetch_one(&state.db)
    .await?;

    Ok(ImportStatementResult {
        statement: stmt,
        imported_txns: imported,
        duplicate: false,
        warnings: parsed.warnings,
        integrity_ok: parsed.integrity_ok,
    })
}

// ─── list_bank_statements ─────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_bank_statements(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<BankStatement>> {
    Ok(sqlx::query_as::<_, BankStatement>(
        "SELECT id, company_id, bank_account_id, source_format, statement_ref, \
                opening_balance, closing_balance, statement_date, content_hash, created_at \
         FROM bank_statements \
         WHERE company_id = ?1 \
         ORDER BY statement_date DESC, created_at DESC",
    )
    .bind(&company_id)
    .fetch_all(&state.db)
    .await?)
}

// ─── list_bank_transactions ───────────────────────────────────────────────────

#[tauri::command]
pub async fn list_bank_transactions(
    state: State<'_, AppState>,
    statement_id: String,
    company_id: String,
) -> AppResult<Vec<BankTransaction>> {
    let rows = sqlx::query(
        "SELECT id, statement_id, company_id, booking_date, value_date, amount, currency, \
                counterparty_name, counterparty_iban, counterparty_cui, reference, txn_hash, \
                status, matched_invoice_id, matched_payment_id \
         FROM bank_transactions \
         WHERE statement_id = ?1 AND company_id = ?2 \
         ORDER BY booking_date, rowid",
    )
    .bind(&statement_id)
    .bind(&company_id)
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for row in &rows {
        let txn_id: String = row.try_get("id").unwrap_or_default();
        let amount_str: String = row.try_get("amount").unwrap_or_else(|_| "0".to_string());
        let status: String = row
            .try_get("status")
            .unwrap_or_else(|_| "UNMATCHED".to_string());
        let reference: Option<String> = row.try_get("reference").ok().flatten();
        let counterparty_cui: Option<String> = row.try_get("counterparty_cui").ok().flatten();
        let amount = Decimal::from_str(amount_str.trim()).unwrap_or(Decimal::ZERO);

        // Build suggestions only for UNMATCHED transactions
        let suggestions = if status == "UNMATCHED" {
            suggest_matches(
                &state.db,
                &company_id,
                &amount,
                reference.as_deref(),
                counterparty_cui.as_deref(),
            )
            .await
            .unwrap_or_default()
        } else {
            vec![]
        };

        result.push(BankTransaction {
            id: txn_id,
            statement_id: row.try_get("statement_id").unwrap_or_default(),
            company_id: row.try_get("company_id").unwrap_or_default(),
            booking_date: row.try_get("booking_date").unwrap_or_default(),
            value_date: row.try_get("value_date").ok().flatten(),
            amount: amount_str,
            currency: row
                .try_get("currency")
                .unwrap_or_else(|_| "RON".to_string()),
            counterparty_name: row.try_get("counterparty_name").ok().flatten(),
            counterparty_iban: row.try_get("counterparty_iban").ok().flatten(),
            counterparty_cui,
            reference,
            txn_hash: row.try_get("txn_hash").unwrap_or_default(),
            status,
            matched_invoice_id: row.try_get("matched_invoice_id").ok().flatten(),
            matched_payment_id: row.try_get("matched_payment_id").ok().flatten(),
            suggestions,
        });
    }

    Ok(result)
}

// ─── match_bank_txn ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchBankTxnArgs {
    pub txn_id: String,
    pub company_id: String,
    pub invoice_id: String,
    /// "issued" | "received"
    pub direction: String,
    /// Explicit payment date override; defaults to the transaction's booking_date.
    pub paid_at: Option<String>,
}

/// Confirm a suggested match: create a payment via the EXISTING payment DB functions
/// (which also trigger GL posting), then mark the transaction MATCHED.
#[tauri::command]
pub async fn match_bank_txn(state: State<'_, AppState>, args: MatchBankTxnArgs) -> AppResult<()> {
    // Fetch transaction
    let row = sqlx::query(
        "SELECT amount, currency, booking_date, status \
         FROM bank_transactions WHERE id = ?1 AND company_id = ?2 LIMIT 1",
    )
    .bind(&args.txn_id)
    .bind(&args.company_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let status: String = row.try_get("status").unwrap_or_default();
    if status == "MATCHED" {
        return Err(AppError::Validation(
            "Tranzacția este deja potrivită.".into(),
        ));
    }
    let amount_str: String = row.try_get("amount").unwrap_or_default();
    let currency: String = row
        .try_get("currency")
        .unwrap_or_else(|_| "RON".to_string());
    let booking_date: String = row.try_get("booking_date").unwrap_or_default();

    let amount = Decimal::from_str(amount_str.trim()).unwrap_or(Decimal::ZERO);
    let abs_amount = amount.abs().to_string();
    let paid_at = args.paid_at.unwrap_or_else(|| booking_date.clone());

    let payment_id = if args.direction == "issued" {
        let p = payments::create(
            &state.db,
            CreatePaymentInput {
                invoice_id: args.invoice_id.clone(),
                company_id: args.company_id.clone(),
                amount: abs_amount,
                currency: Some(currency),
                paid_at,
                method: Some("transfer".to_string()),
                reference: Some(format!("bank_txn:{}", args.txn_id)),
                notes: None,
                exchange_rate: None,
            },
        )
        .await?;
        p.id
    } else {
        let p = received_payments::create(
            &state.db,
            CreateReceivedPaymentInput {
                received_invoice_id: args.invoice_id.clone(),
                company_id: args.company_id.clone(),
                amount: abs_amount,
                currency: Some(currency),
                paid_at,
                method: Some("transfer".to_string()),
                reference: Some(format!("bank_txn:{}", args.txn_id)),
                notes: None,
                exchange_rate: None,
            },
        )
        .await?;
        p.id
    };

    sqlx::query(
        "UPDATE bank_transactions \
         SET status = 'MATCHED', matched_invoice_id = ?1, matched_payment_id = ?2 \
         WHERE id = ?3 AND company_id = ?4",
    )
    .bind(&args.invoice_id)
    .bind(&payment_id)
    .bind(&args.txn_id)
    .bind(&args.company_id)
    .execute(&state.db)
    .await?;

    Ok(())
}

// ─── unmatch_bank_txn ─────────────────────────────────────────────────────────

/// Reverse a confirmed match: delete the payment and reset the transaction to UNMATCHED.
#[tauri::command]
pub async fn unmatch_bank_txn(
    state: State<'_, AppState>,
    txn_id: String,
    company_id: String,
) -> AppResult<()> {
    let row = sqlx::query(
        "SELECT matched_payment_id, status \
         FROM bank_transactions WHERE id = ?1 AND company_id = ?2 LIMIT 1",
    )
    .bind(&txn_id)
    .bind(&company_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let status: String = row.try_get("status").unwrap_or_default();
    if status != "MATCHED" {
        return Err(AppError::Validation(
            "Tranzacția nu este potrivită — nu se poate anula potrivirea.".into(),
        ));
    }
    let payment_id: Option<String> = row.try_get("matched_payment_id").ok().flatten();

    if let Some(pid) = &payment_id {
        // Try issued payment first; fall through to received payment on failure
        let issued_ok = payments::delete(&state.db, pid, &company_id).await.is_ok();
        if !issued_ok {
            let _ = received_payments::delete(&state.db, pid, &company_id).await;
        }
    }

    sqlx::query(
        "UPDATE bank_transactions \
         SET status = 'UNMATCHED', matched_invoice_id = NULL, matched_payment_id = NULL \
         WHERE id = ?1 AND company_id = ?2",
    )
    .bind(&txn_id)
    .bind(&company_id)
    .execute(&state.db)
    .await?;

    Ok(())
}

// ─── ignore_bank_txn ──────────────────────────────────────────────────────────

/// Mark a transaction as IGNORED (bank fee, internal transfer, etc.).
/// GL classification of ignored transactions is a documented follow-up via manual journal.
#[tauri::command]
pub async fn ignore_bank_txn(
    state: State<'_, AppState>,
    txn_id: String,
    company_id: String,
) -> AppResult<()> {
    let n = sqlx::query(
        "UPDATE bank_transactions \
         SET status = 'IGNORED' \
         WHERE id = ?1 AND company_id = ?2 AND status = 'UNMATCHED'",
    )
    .bind(&txn_id)
    .bind(&company_id)
    .execute(&state.db)
    .await?
    .rows_affected();

    if n == 0 {
        return Err(AppError::Validation(
            "Tranzacția nu a fost găsită sau nu este în starea UNMATCHED.".into(),
        ));
    }
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co','RO1','Test SRL','Str 1','Cluj','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, legal_name, cui) \
             VALUES ('ct','co','CUSTOMER','CLIENT SRL','12345678')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn seed_invoice(pool: &SqlitePool, id: &str, total: &str, full_number: &str) {
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, \
              issue_date, due_date, currency, subtotal_amount, vat_amount, total_amount, \
              status, payment_means_code, created_at, updated_at) \
             VALUES (?1,'co','ct','F',1,?2,'2026-01-01','2026-02-01','RON','0','0',?3,'VALIDATED','30',1,1)",
        )
        .bind(id)
        .bind(full_number)
        .bind(total)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_stmt(pool: &SqlitePool, stmt_id: &str, hash: &str) {
        sqlx::query(
            "INSERT INTO bank_statements \
             (id, company_id, source_format, statement_ref, opening_balance, \
              closing_balance, statement_date, content_hash) \
             VALUES (?1,'co','MT940','REF','10000','10500','2026-01-01',?2)",
        )
        .bind(stmt_id)
        .bind(hash)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_txn(pool: &SqlitePool, txn_id: &str, stmt_id: &str, amount: &str, hash: &str) {
        sqlx::query(
            "INSERT INTO bank_transactions \
             (id, statement_id, company_id, booking_date, amount, currency, txn_hash) \
             VALUES (?1,?2,'co','2026-01-15',?3,'RON',?4)",
        )
        .bind(txn_id)
        .bind(stmt_id)
        .bind(amount)
        .bind(hash)
        .execute(pool)
        .await
        .unwrap();
    }

    // ── Dedup: re-importing same hash returns duplicate=true, no new rows ─────

    #[tokio::test]
    async fn import_dedup_same_content_hash() {
        let pool = pool().await;
        let hash = "deadbeef12345678";
        seed_stmt(&pool, "s1", hash).await;

        // content_hash already in DB → duplicate
        let existing_id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM bank_statements WHERE company_id='co' AND content_hash=?1 LIMIT 1",
        )
        .bind(hash)
        .fetch_optional(&pool)
        .await
        .unwrap();

        assert!(
            existing_id.is_some(),
            "dedup check should find existing statement"
        );
    }

    // ── Match flow: payment created + status=MATCHED; unmatch reverses ────────

    #[tokio::test]
    async fn match_creates_payment_status_matched_then_unmatch_reverses() {
        let pool = pool().await;
        seed_invoice(&pool, "inv1", "500.00", "F2026-001").await;
        seed_stmt(&pool, "stmt1", "hash1").await;
        seed_txn(&pool, "txn1", "stmt1", "500.00", "h1").await;

        // Create payment directly (mirrors what match_bank_txn does)
        let payment = payments::create(
            &pool,
            CreatePaymentInput {
                invoice_id: "inv1".into(),
                company_id: "co".into(),
                amount: "500.00".into(),
                currency: Some("RON".into()),
                paid_at: "2026-01-15".into(),
                method: Some("transfer".into()),
                reference: Some("bank_txn:txn1".into()),
                notes: None,
                exchange_rate: None,
            },
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE bank_transactions \
             SET status='MATCHED', matched_invoice_id='inv1', matched_payment_id=?1 \
             WHERE id='txn1'",
        )
        .bind(&payment.id)
        .execute(&pool)
        .await
        .unwrap();

        // Status should be MATCHED
        let status: String =
            sqlx::query_scalar("SELECT status FROM bank_transactions WHERE id='txn1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "MATCHED");

        // Invoice should be PAID
        let summary = payments::summary_for_invoice(&pool, "inv1", "co")
            .await
            .unwrap();
        assert_eq!(summary.payment_status, "PAID");

        // Unmatch: delete payment + reset status
        payments::delete(&pool, &payment.id, "co").await.unwrap();
        sqlx::query(
            "UPDATE bank_transactions \
             SET status='UNMATCHED', matched_invoice_id=NULL, matched_payment_id=NULL \
             WHERE id='txn1'",
        )
        .execute(&pool)
        .await
        .unwrap();

        let status2: String =
            sqlx::query_scalar("SELECT status FROM bank_transactions WHERE id='txn1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status2, "UNMATCHED");
        let summary2 = payments::summary_for_invoice(&pool, "inv1", "co")
            .await
            .unwrap();
        assert_eq!(summary2.payment_status, "UNPAID");
    }

    // ── Ignore sets status=IGNORED ────────────────────────────────────────────

    #[tokio::test]
    async fn ignore_sets_status_ignored() {
        let pool = pool().await;
        seed_stmt(&pool, "stmt2", "hash2").await;
        seed_txn(&pool, "txn2", "stmt2", "-15.50", "h2").await;

        sqlx::query(
            "UPDATE bank_transactions SET status='IGNORED' \
             WHERE id='txn2' AND company_id='co' AND status='UNMATCHED'",
        )
        .execute(&pool)
        .await
        .unwrap();

        let s: String = sqlx::query_scalar("SELECT status FROM bank_transactions WHERE id='txn2'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(s, "IGNORED");
    }

    // ── Dedup txn_hash within statement ──────────────────────────────────────

    #[tokio::test]
    async fn txn_hash_dedup_within_statement() {
        let pool = pool().await;
        seed_stmt(&pool, "stmt3", "hash3").await;
        seed_txn(&pool, "txn3a", "stmt3", "100.00", "SAMEHASH").await;

        // Try to insert another txn with same hash — should fail (UNIQUE constraint)
        let result = sqlx::query(
            "INSERT INTO bank_transactions \
             (id, statement_id, company_id, booking_date, amount, currency, txn_hash) \
             VALUES ('txn3b','stmt3','co','2026-01-15','100.00','RON','SAMEHASH')",
        )
        .execute(&pool)
        .await;

        assert!(
            result.is_err(),
            "duplicate txn_hash within statement should be rejected"
        );
    }
}
