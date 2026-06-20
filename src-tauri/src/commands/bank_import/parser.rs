//! BankStatementParser trait + shared parsed types.
//!
//! All parser implementations are DB-free: they read bytes → return ParsedStatement.
//! Per-record errors go into `warnings`; only structural failures (unreadable input)
//! propagate as `Err`.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// A single parsed transaction line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTxn {
    /// ISO date YYYY-MM-DD booking date.
    pub booking_date: String,
    /// Value date — may be None when the format omits it.
    pub value_date: Option<String>,
    /// Signed amount: positive = credit (money in), negative = debit (money out).
    pub amount: Decimal,
    pub currency: String,
    pub counterparty_name: Option<String>,
    pub counterparty_iban: Option<String>,
    pub counterparty_cui: Option<String>,
    /// Raw description / reference field (:86: content, RmtInf, or CSV description).
    pub reference: Option<String>,
    /// Deterministic per-transaction hash for dedup within statement.
    pub txn_hash: String,
}

/// Result of parsing one statement file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedStatement {
    pub statement_ref: String,
    pub statement_date: String,
    pub opening_balance: Decimal,
    pub closing_balance: Decimal,
    pub currency: String,
    pub txns: Vec<ParsedTxn>,
    /// Non-fatal parse warnings (malformed lines, unknown fields, skipped sections…)
    pub warnings: Vec<String>,
    /// Integrity check result: opening + Σ(txns) ≈ closing.
    /// None when the format provides no balances (e.g. basic CSV).
    pub integrity_ok: Option<bool>,
}

/// Every statement format adapter implements this trait.
pub trait BankStatementParser: Send + Sync {
    fn parse(&self, bytes: &[u8]) -> AppResult<ParsedStatement>;
}

// ─── Shared helpers ───────────────────────────────────────────────────────────

/// Decode raw bytes to String: UTF-8 first, then Windows-1250 fallback.
pub fn decode_bytes(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let (cow, _, _) = encoding_rs::WINDOWS_1250.decode(bytes);
    cow.into_owned()
}

/// Deterministic per-transaction hash for dedup.
/// Uses std DefaultHasher — good enough for dedup; not a security hash.
pub fn txn_hash(booking_date: &str, amount: &Decimal, reference: Option<&str>) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    booking_date.hash(&mut h);
    amount.to_string().hash(&mut h);
    reference.unwrap_or("").hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Compute integrity check: opening + sum ≈ closing within 0.01 tolerance.
/// Returns None when both balances are zero (not provided).
pub fn check_integrity(opening: Decimal, closing: Decimal, txns: &[ParsedTxn]) -> Option<bool> {
    if opening.is_zero() && closing.is_zero() {
        return None;
    }
    let sum: Decimal = txns.iter().map(|t| t.amount).sum();
    let expected = opening + sum;
    let diff = (expected - closing).abs();
    let threshold = Decimal::new(1, 2); // 0.01
    Some(diff <= threshold)
}
