//! Tipuri partajate între module DB (enums status, pagination, filtre comune).

use serde::{Deserialize, Serialize};

// ─── Status enums ──────────────────────────────────────────────────────────
// Stocate ca TEXT în DB; serializate identic în JSON pentru frontend.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum InvoiceStatus {
    Draft,
    Queued,
    Submitted,
    Validated,
    Rejected,
    Storned,
}

impl InvoiceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Queued => "QUEUED",
            Self::Submitted => "SUBMITTED",
            Self::Validated => "VALIDATED",
            Self::Rejected => "REJECTED",
            Self::Storned => "STORNED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ReceivedStatus {
    New,
    Reviewed,
    Approved,
    Rejected,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ContactType {
    Customer,
    Supplier,
    Both,
}

// ─── Pagination ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub offset: i64,
    pub limit: i64,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 500,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Paginated<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub offset: i64,
    pub limit: i64,
}

// ─── VAT rates ─────────────────────────────────────────────────────────────

/// Cotele TVA valide conform legislației RO (2025+)
pub const VALID_VAT_RATES: &[i64] = &[0, 5, 9, 11, 19, 21];

/// Coduri UNCL4461 acceptate de ANAF pentru `PaymentMeansCode`.
/// Sursa: CIUS-RO / EN 16931 lista de coduri UNCL4461 permise.
pub const VALID_PAYMENT_MEANS_CODES: &[&str] =
    &["10", "20", "30", "42", "48", "49", "57", "58", "59"];

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Generează un UUID v7 (time-ordered, mai bun pentru index decât v4).
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Timestamp curent unix.
pub fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Parse a stored Decimal-as-TEXT money value, logging (never silently zeroing) a corrupted one.
/// An empty string is treated as a legitimately-absent amount (0, no log). Use this instead of
/// `Decimal::from_str(..).unwrap_or(ZERO)` on every money read — a malformed amount must leave a
/// trace, otherwise reconciliation breaks with no signal.
pub fn dec_logged(context: &str, s: &str) -> rust_decimal::Decimal {
    match std::str::FromStr::from_str(s.trim()) {
        Ok(d) => d,
        Err(_) => {
            if !s.trim().is_empty() {
                tracing::warn!(context, value = %s, "valoare monetară invalidă — se folosește 0");
            }
            rust_decimal::Decimal::ZERO
        }
    }
}
