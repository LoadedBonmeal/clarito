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
use super::parser::{BankStatementParser, ParsedTxn};

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

    // FIX 5: parsers that could not read a currency from the file mark their
    // txns with an EMPTY currency (CSV always; MT940 when :60F:/:62F: carried
    // none). Resolve those from the linked bank account's currency so foreign
    // accounts keep currency-aware matching; fall back to RON otherwise.
    // CAMT053 carries the Ccy attribute per amount, so its txns stay explicit.
    let account_currency: Option<String> = match &bank_account_id {
        Some(acct_id) => {
            sqlx::query_scalar(
                "SELECT currency FROM bank_accounts \
                 WHERE id = ?1 AND company_id = ?2 LIMIT 1",
            )
            .bind(acct_id)
            .bind(&company_id)
            .fetch_optional(&state.db)
            .await?
        }
        None => None,
    };

    let imported = insert_parsed_txns(
        &state.db,
        &stmt_id,
        &company_id,
        &parsed.txns,
        account_currency.as_deref(),
    )
    .await?;

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

/// Insert parsed transactions for a freshly created statement.
///
/// FIX 4: the old per-statement `txn_hash` dedup silently DROPPED legitimate
/// identical transactions — two equal bank fees or two identical POS
/// settlements on the same day share (date, amount, reference) and therefore
/// the same hash, so one vanished while `integrity_ok` stayed true. File-level
/// re-import is already handled by `content_hash` on the statement, so
/// intra-statement dedup is never desirable: instead, count the occurrences of
/// each hash and give the 2nd, 3rd… occurrence a sequence-suffixed hash so
/// every parsed transaction imports as its own row (the UNIQUE(statement_id,
/// txn_hash) constraint stays satisfied).
///
/// FIX 5: a txn whose parser could not determine the currency (empty marker)
/// gets the linked bank account's currency, falling back to RON.
pub(crate) async fn insert_parsed_txns(
    db: &sqlx::SqlitePool,
    stmt_id: &str,
    company_id: &str,
    txns: &[ParsedTxn],
    account_currency: Option<&str>,
) -> AppResult<usize> {
    let mut occurrences: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let mut imported = 0usize;

    for txn in txns {
        let seen = occurrences.entry(txn.txn_hash.as_str()).or_insert(0);
        let hash = if *seen == 0 {
            txn.txn_hash.clone()
        } else {
            format!("{}-{}", txn.txn_hash, *seen)
        };
        *seen += 1;

        let currency = {
            let c = txn.currency.trim();
            if c.is_empty() {
                account_currency
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("RON")
                    .to_string()
            } else {
                c.to_string()
            }
        };

        let txn_id = new_id();
        sqlx::query(
            "INSERT INTO bank_transactions \
             (id, statement_id, company_id, booking_date, value_date, amount, currency, \
              counterparty_name, counterparty_iban, counterparty_cui, reference, txn_hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )
        .bind(&txn_id)
        .bind(stmt_id)
        .bind(company_id)
        .bind(&txn.booking_date)
        .bind(&txn.value_date)
        .bind(txn.amount.to_string())
        .bind(&currency)
        .bind(&txn.counterparty_name)
        .bind(&txn.counterparty_iban)
        .bind(&txn.counterparty_cui)
        .bind(&txn.reference)
        .bind(&hash)
        .execute(db)
        .await?;
        imported += 1;
    }

    Ok(imported)
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
        let txn_currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());

        // Build suggestions only for UNMATCHED transactions
        let suggestions = if status == "UNMATCHED" {
            suggest_matches(
                &state.db,
                &company_id,
                &amount,
                &txn_currency,
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

    // Wave 4 audit: the payment creation and the transaction flag can't share one SQL
    // transaction (the create fns own their pool-level GL posting), so compensate instead —
    // if flagging the transaction fails, best-effort delete the just-created payment rather
    // than leaving an invisible duplicate-risk payment behind an UNMATCHED transaction.
    let flagged = sqlx::query(
        "UPDATE bank_transactions \
         SET status = 'MATCHED', matched_invoice_id = ?1, matched_payment_id = ?2 \
         WHERE id = ?3 AND company_id = ?4",
    )
    .bind(&args.invoice_id)
    .bind(&payment_id)
    .bind(&args.txn_id)
    .bind(&args.company_id)
    .execute(&state.db)
    .await;

    if let Err(e) = flagged {
        let compensate = if args.direction == "issued" {
            payments::delete(&state.db, &payment_id, &args.company_id).await
        } else {
            received_payments::delete(&state.db, &payment_id, &args.company_id).await
        };
        if let Err(comp_err) = compensate {
            tracing::error!(
                "match_bank_txn: flag failed ({e}) AND compensation delete failed ({comp_err}) — \
                 payment {payment_id} may be orphaned"
            );
        }
        return Err(e.into());
    }

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
        // Try the issued side first, then the received side. Wave 4 audit: errors MUST
        // propagate — the old `let _ =` fall-through swallowed the period-lock Validation
        // from the delete fns and reset the transaction anyway, orphaning a payment whose
        // GL sits in a FILED month (re-matching would then duplicate it). A period-lock
        // Validation beats a NotFound when picking which error to surface; when BOTH
        // sides report NotFound the payment is already gone, and resetting the
        // transaction is the correct repair.
        match payments::delete(&state.db, pid, &company_id).await {
            Ok(_) => {}
            Err(issued_err) => match received_payments::delete(&state.db, pid, &company_id).await {
                Ok(_) => {}
                Err(received_err) => {
                    let issued_nf = matches!(issued_err, AppError::NotFound);
                    let received_nf = matches!(received_err, AppError::NotFound);
                    if !(issued_nf && received_nf) {
                        return Err(match (&issued_err, &received_err) {
                            (AppError::Validation(_), _) => issued_err,
                            (_, AppError::Validation(_)) => received_err,
                            _ => issued_err,
                        });
                    }
                    // both NotFound → payment already deleted; proceed with the reset
                }
            },
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

    // ── FIX 4: two genuinely identical txns must BOTH import ──────────────────

    fn make_parsed_txn(amount: &str, reference: &str) -> super::ParsedTxn {
        use super::super::parser::txn_hash;
        let amt = Decimal::from_str(amount).unwrap();
        super::ParsedTxn {
            booking_date: "2026-01-15".into(),
            value_date: None,
            amount: amt,
            currency: "RON".into(),
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_cui: None,
            reference: Some(reference.to_string()),
            txn_hash: txn_hash("2026-01-15", &amt, Some(reference)),
        }
    }

    #[tokio::test]
    async fn identical_txns_in_one_statement_both_imported() {
        let pool = pool().await;
        seed_stmt(&pool, "stmt4", "hash4").await;

        // Two genuinely identical lines (same date/amount/reference — e.g. two
        // equal bank fees on the same day). The parser gives them the SAME hash.
        let t1 = make_parsed_txn("-15.50", "Comision administrare");
        let t2 = make_parsed_txn("-15.50", "Comision administrare");
        assert_eq!(t1.txn_hash, t2.txn_hash, "precondition: identical hashes");

        let imported = insert_parsed_txns(&pool, "stmt4", "co", &[t1, t2], None)
            .await
            .unwrap();
        assert_eq!(imported, 2, "both identical txns must import (FIX 4)");

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM bank_transactions WHERE statement_id='stmt4'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 2, "both rows must be in the DB");

        // Hashes are occurrence-suffixed → distinct in the DB.
        let distinct: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT txn_hash) FROM bank_transactions WHERE statement_id='stmt4'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(distinct, 2, "occurrence-aware hashes must differ");
    }

    // ── FIX 5: parser-defaulted currency resolved from the bank account ──────

    #[tokio::test]
    async fn empty_txn_currency_resolved_from_bank_account() {
        let pool = pool().await;
        seed_stmt(&pool, "stmt5", "hash5").await;
        seed_stmt(&pool, "stmt6", "hash6").await;

        // CSV-style txn: the parser could not determine the currency.
        let mut t = make_parsed_txn("100.00", "Incasare");
        t.currency = String::new();

        // With a EUR bank account → EUR.
        insert_parsed_txns(&pool, "stmt5", "co", &[t.clone()], Some("EUR"))
            .await
            .unwrap();
        let cur: String =
            sqlx::query_scalar("SELECT currency FROM bank_transactions WHERE statement_id='stmt5'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cur, "EUR", "empty currency must resolve to the account's");

        // Without an account → RON fallback.
        insert_parsed_txns(&pool, "stmt6", "co", &[t], None)
            .await
            .unwrap();
        let cur2: String =
            sqlx::query_scalar("SELECT currency FROM bank_transactions WHERE statement_id='stmt6'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cur2, "RON", "no account → RON fallback");
    }

    #[tokio::test]
    async fn explicit_txn_currency_not_overridden_by_account() {
        // MT940/CAMT with an explicit file currency must keep it even when the
        // linked account says something else.
        let pool = pool().await;
        seed_stmt(&pool, "stmt7", "hash7").await;

        let t = make_parsed_txn("100.00", "Incasare"); // currency: RON (explicit)
        insert_parsed_txns(&pool, "stmt7", "co", &[t], Some("EUR"))
            .await
            .unwrap();
        let cur: String =
            sqlx::query_scalar("SELECT currency FROM bank_transactions WHERE statement_id='stmt7'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cur, "RON", "file-carried currency wins over the account's");
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
