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
pub mod rbac;
pub mod received;
pub mod settings;
pub mod users;

pub mod contracts;
pub mod payment_instruments;

pub mod accounts;
pub mod aging;
pub mod assets;
pub mod concedii;
pub mod declaration_filings;
pub mod deconturi;
pub mod dividends;
pub mod fiscal_receipts;
pub mod fx_revaluation;
pub mod gestiune;
pub mod gl;
pub mod inventory;
pub mod nir;
pub mod orders;
pub mod payments;
pub mod payroll;
pub mod payroll_config;
pub mod productie;
pub mod products;
pub mod quotes;
pub mod receipts;
pub mod received_payments;
pub mod recurring;
pub mod seed;
pub mod stock;
pub mod stock_transfer;
pub mod stock_valuation;
pub mod vat_rates;
