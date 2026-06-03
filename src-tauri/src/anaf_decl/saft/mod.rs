//! SAF-T D406 generator (Phase 4).
//!
//! Structure:
//!   `generator.rs`   — top-level `generate_saft_xml` assembling all four mandatory sections
//!   `masterfiles.rs` — GeneralLedgerAccounts / Customers / Suppliers / TaxTable / UOMTable /
//!                      Products / AnalysisTypeTable / MovementTypeTable / Owners / Assets
//!   `source_docs.rs` — SalesInvoices / PurchaseInvoices / Payments / MovementOfGoods
//!
//! Usage (from commands layer):
//! ```no_run
//! use efactura_desktop_lib::anaf_decl::saft::generator::generate_saft_xml;
//! ```

pub mod generator;
pub mod masterfiles;
pub mod source_docs;
