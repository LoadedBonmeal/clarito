//! Layer-ul de acces la date.
//!
//! Fiecare entitate are propriul modul cu:
//! - struct DB (derive `FromRow`)
//! - input types (Create / Update)
//! - funcții async pentru query-uri
//!
//! Toate funcțiile primesc `&SqlitePool` ca prim argument.

pub mod models;
pub mod pool;

pub mod audit;
pub mod certificates;
pub mod companies;
pub mod contacts;
pub mod invoices;
pub mod license;
pub mod notifications;
pub mod received;
pub mod settings;

pub mod payments;
pub mod products;
pub mod recurring;
pub mod seed;
pub mod vat_rates;
