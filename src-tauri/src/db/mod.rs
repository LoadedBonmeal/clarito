//! Layer-ul de acces la date.
//!
//! Fiecare entitate are propriul modul cu:
//! - struct DB (derive `FromRow`)
//! - input types (Create / Update)
//! - funcții async pentru query-uri
//!
//! Toate funcțiile primesc `&SqlitePool` ca prim argument.

pub mod pool;
pub mod models;

pub mod companies;
pub mod contacts;
pub mod certificates;
pub mod invoices;
pub mod received;
pub mod notifications;
pub mod settings;
pub mod license;

pub mod seed;
pub mod payments;
pub mod recurring;
