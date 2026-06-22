//! Tauri commands — interfața expusă frontend-ului.
//!
//! Fiecare submodul mapează 1:1 cu un modul DB. Commands sunt subțiri:
//! validare minimă + dispatch către layer-ul DB.

/// Validează o dată calendaristică reală `YYYY-MM-DD` la granița IPC. SQLite compară datele ca
/// STRINGURI, deci o dată inexistentă ('2026-02-31', '2026-06-99') trece tăcut printr-un filtru
/// `BETWEEN` și poate sări peste documente — respingem la intrare cu chrono.
pub(crate) fn require_valid_date(label: &str, s: &str) -> crate::error::AppResult<()> {
    if chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").is_err() {
        return Err(crate::error::AppError::Validation(format!(
            "{label} invalidă: '{s}' — folosiți o dată calendaristică reală (AAAA-LL-ZZ)."
        )));
    }
    Ok(())
}

/// Varianta pentru parametri opționali de filtru.
pub(crate) fn require_valid_date_opt(label: &str, s: Option<&str>) -> crate::error::AppResult<()> {
    match s {
        Some(v) if !v.trim().is_empty() => require_valid_date(label, v),
        _ => Ok(()),
    }
}

pub mod accounts;
pub mod advance_invoices;
pub mod anaf;
pub mod archive;
pub mod assets;
pub mod auth;
pub mod avize;
pub mod bank_import;
pub mod bnr;
pub mod companies;
pub mod contacts;
pub mod contracts;
pub mod d301;
pub mod d390;
pub mod d394;
pub mod d700;
pub mod d710;
pub mod declarations;
pub mod deconturi;
pub mod dezmembrari;
pub mod dividends;
pub mod etransport;
pub mod feedback;
pub mod fiscal_receipts;
pub mod fx_revaluation;
pub mod gdpr;
pub mod gl;
pub mod import;
pub mod import_wave_c;
pub mod integrations;
pub mod inventory;
pub mod invoices;
pub mod journals;
pub mod license;
pub mod manual_journal;
pub mod nir;
pub mod notifications;
pub mod orders;
pub mod payment_instruments;
pub mod payments;
pub mod payroll;
pub mod payroll_config;
pub mod pontaj;
pub mod productie;
pub mod products;
pub mod quotes;
pub mod receipts;
pub mod received;
pub mod received_payments;
pub mod recurring;
pub mod reports;
pub mod saft;
pub mod settings;
pub mod stock;
pub mod stock_transfer;
pub mod system;
pub mod ubl;
pub mod vat_rates;
pub mod xlsx;
