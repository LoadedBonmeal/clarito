//! Tauri commands — interfața expusă frontend-ului.
//!
//! Fiecare submodul mapează 1:1 cu un modul DB. Commands sunt subțiri:
//! validare minimă + dispatch către layer-ul DB.

pub mod accounts;
pub mod anaf;
pub mod archive;
pub mod assets;
pub mod bnr;
pub mod companies;
pub mod contacts;
pub mod d390;
pub mod d394;
pub mod declarations;
pub mod feedback;
pub mod gdpr;
pub mod gl;
pub mod import;
pub mod integrations;
pub mod invoices;
pub mod journals;
pub mod license;
pub mod notifications;
pub mod payments;
pub mod products;
pub mod receipts;
pub mod received;
pub mod received_payments;
pub mod recurring;
pub mod reports;
pub mod saft;
pub mod settings;
pub mod stock;
pub mod system;
pub mod ubl;
pub mod vat_rates;
pub mod xlsx;
