//! Tauri commands — interfața expusă frontend-ului.
//!
//! Fiecare submodul mapează 1:1 cu un modul DB. Commands sunt subțiri:
//! validare minimă + dispatch către layer-ul DB.

pub mod anaf;
pub mod archive;
pub mod companies;
pub mod contacts;
pub mod feedback;
pub mod gdpr;
pub mod import;
pub mod integrations;
pub mod invoices;
pub mod license;
pub mod notifications;
pub mod payments;
pub mod received;
pub mod recurring;
pub mod reports;
pub mod saft;
pub mod settings;
pub mod system;
pub mod ubl;
pub mod xlsx;
