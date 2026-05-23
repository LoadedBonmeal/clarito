//! Tauri commands — interfața expusă frontend-ului.
//!
//! Fiecare submodul mapează 1:1 cu un modul DB. Commands sunt subțiri:
//! validare minimă + dispatch către layer-ul DB.

pub mod companies;
pub mod contacts;
pub mod invoices;
pub mod received;
pub mod notifications;
pub mod settings;
pub mod license;
pub mod system;
pub mod ubl;
pub mod anaf;
pub mod archive;
pub mod integrations;
pub mod import;
pub mod reports;
