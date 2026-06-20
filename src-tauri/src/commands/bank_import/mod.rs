//! Bank statement import — Wave 6 (jurnal de bancă).
//!
//! Supports MT940 (SWIFT), CAMT.053 (ISO 20022), and generic CSV formats.
//! The importer SUGGESTS invoice matches; recording a payment requires explicit
//! user confirmation — it does NOT auto-post to a guessed GL account.
//!
//! GL posting is NOT done here: matching only RECORDS a payment via the EXISTING
//! payments::create / received_payments::create. The GL is derived on-demand by
//! generate_gl_entries (post_payment), so a bank-matched payment posts identically to a
//! manual one — this module never writes its own journal entries.
//!
//! Documented follow-ups (not in scope for Wave 6):
//!   - CSV column-mapping UI for non-standard bank layouts
//!   - GL classification of IGNORED transactions (bank fees → 627, etc.) via manual journal
//!   - FX gain/loss on payment-date rate delta (use exchange_rate field in CreatePaymentInput)

pub mod camt053;
pub mod commands;
pub mod csv_parser;
pub mod matching;
pub mod mt940;
pub mod parser;

pub use commands::{
    create_bank_account, delete_bank_account, ignore_bank_txn, import_bank_statement,
    list_bank_accounts, list_bank_statements, list_bank_transactions, match_bank_txn,
    unmatch_bank_txn,
};
