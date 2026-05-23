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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LicenseTier {
    Trial,
    Solo,
    Accountant,
    Firm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum VatCategory {
    /// Standard rate (cota standard 19%)
    S,
    /// Zero-rated (cotă zero pentru export)
    Z,
    /// Exempt without VAT (scutit fără TVA)
    E,
    /// Reverse charge (taxare inversă)
    Ae,
    /// VAT exempt for small business
    K,
    /// Free export item
    G,
    /// Other
    O,
}

// ─── Pagination ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub offset: i64,
    pub limit: i64,
}

impl Default for Page {
    fn default() -> Self {
        Self { offset: 0, limit: 50 }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Paginated<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub offset: i64,
    pub limit: i64,
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Generează un UUID v7 (time-ordered, mai bun pentru index decât v4).
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Timestamp curent unix.
pub fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}
