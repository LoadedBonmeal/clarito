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
pub mod payroll_diurna;
pub mod period_locks;
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

// ─── Guard: no hand-rolled CREATE TABLE inside #[cfg(test)] ──────────────────
//
// This test walks src-tauri/src/ and asserts that no test module contains an
// inline `CREATE TABLE <name>` for a table that already exists in migrations/.
// It catches future regressions where a new hand-rolled fixture diverges from
// the real schema — the exact class of bug that hid the `status='invoicing'`
// production failure until almost shipping.
#[cfg(test)]
mod no_inline_test_schema {
    /// Tables defined in migrations/ (derive this list by grepping migrations/ for
    /// `CREATE TABLE`).  Only list "real" tables (exclude *_new temporaries used
    /// during schema rebuilds and *_v view aliases).
    const MIGRATION_TABLES: &[&str] = &[
        "account_mapping",
        "asset_depreciation",
        "asset_transactions",
        "audit_archive",
        "audit_log",
        "bank_accounts",
        "bank_statements",
        "bank_transactions",
        "bom",
        "bom_lines",
        "certificates",
        "chart_of_accounts",
        "companies",
        "contacts",
        "contracts",
        "declaration_filings",
        "dividends",
        "employees",
        "etransport_declarations",
        "expense_lines",
        "expense_reports",
        "fiscal_receipt_invoice_links",
        "fiscal_receipt_vat_lines",
        "fiscal_receipts",
        "fixed_assets",
        "fx_revaluation",
        "gestiune",
        "gl_entry",
        "gl_journal",
        "import_batch",
        "import_staging_account",
        "import_staging_contact",
        "import_staging_invoice",
        "import_staging_invoice_line",
        "import_staging_product",
        "inventory_lines",
        "inventory_sessions",
        "invoice_events",
        "invoice_line_items",
        "invoices",
        "license",
        "medical_leaves",
        "nir_documents",
        "nir_lines",
        "notifications",
        "order_lines",
        "orders",
        "payment_instruments",
        "payments",
        "payroll_config",
        "payroll_extra_income",
        "period_locks",
        "product_groups",
        "productie_orders",
        "products",
        "quote_lines",
        "quotes",
        "receipts",
        "received_invoice_payments",
        "received_invoice_vat_lines",
        "received_invoices",
        "recurring_invoices",
        "registru_inventar_entries",
        "secondary_offices",
        "settings",
        "stock_ledger",
        "stock_movement_lines",
        "stock_movements",
        "stock_transfers",
        "treasury_advances",
        "users",
        "vat_rates",
    ];

    /// Pre-existing known violations that have not yet been migrated to
    /// sqlx::migrate!.  Keyed as `("src/path/relative/to/src-tauri", "table")`.
    /// Do NOT add new entries here — fix the fixture instead.
    /// Remove entries as those files get converted.
    const KNOWN_REMAINING: &[(&str, &str)] = &[
        // ── db/ ──────────────────────────────────────────────────────────────
        ("src/db/products.rs", "companies"),
        ("src/db/products.rs", "products"),
        ("src/db/products.rs", "product_groups"),
        ("src/db/products.rs", "account_mapping"),
        ("src/db/vat_rates.rs", "vat_rates"),
        ("src/db/audit.rs", "audit_log"),
        ("src/db/contacts.rs", "companies"),
        ("src/db/contacts.rs", "contacts"),
        ("src/db/contacts.rs", "invoices"),
        ("src/db/accounts.rs", "chart_of_accounts"),
        ("src/db/period_locks.rs", "companies"),
        ("src/db/period_locks.rs", "period_locks"),
        ("src/db/declaration_filings.rs", "declaration_filings"),
        ("src/db/declaration_filings.rs", "companies"),
        ("src/db/declaration_filings.rs", "period_locks"),
        ("src/db/invoices.rs", "invoices"),
        ("src/db/companies.rs", "companies"),
        ("src/db/assets.rs", "companies"),
        ("src/db/assets.rs", "fixed_assets"),
        ("src/db/assets.rs", "asset_transactions"),
        // ── commands/ ────────────────────────────────────────────────────────
        ("src/commands/system.rs", "audit_log"),
        ("src/commands/invoices.rs", "companies"),
        ("src/commands/invoices.rs", "contacts"),
        ("src/commands/invoices.rs", "invoices"),
        ("src/commands/invoices.rs", "invoice_line_items"),
        ("src/commands/invoices.rs", "invoice_events"),
    ];

    #[test]
    fn no_hand_rolled_create_table_in_test_modules() {
        use std::path::PathBuf;

        // Locate src-tauri/src/ relative to the manifest (CARGO_MANIFEST_DIR is
        // set by cargo test to the crate root, i.e. src-tauri/).
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let src_dir = PathBuf::from(manifest_dir).join("src");

        let mut violations: Vec<String> = Vec::new();
        collect_violations(&src_dir, manifest_dir, &mut violations);

        assert!(
            violations.is_empty(),
            "NEW hand-rolled CREATE TABLE for migration-managed tables found in #[cfg(test)] code.\n\
             Switch these fixtures to sqlx::migrate!(\"./migrations\").run(&pool) instead.\n\
             Do NOT add them to KNOWN_REMAINING — fix them:\n\n{}",
            violations.join("\n")
        );
    }

    /// Walk `dir` recursively, scanning each .rs file for inline CREATE TABLE
    /// statements that appear inside a `#[cfg(test)]` section.
    fn collect_violations(dir: &std::path::Path, crate_root: &str, violations: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_violations(&path, crate_root, violations);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                scan_file(&path, crate_root, violations);
            }
        }
    }

    fn scan_file(path: &std::path::Path, crate_root: &str, violations: &mut Vec<String>) {
        let Ok(content) = std::fs::read_to_string(path) else {
            return;
        };

        // Compute the path relative to the crate root (e.g. "src/db/users.rs").
        let rel_path = path
            .strip_prefix(crate_root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().into_owned());

        // Simple heuristic: look for `CREATE TABLE` (case-insensitive) in any
        // line that follows a `#[cfg(test)]` marker in the file.  We track
        // whether we are "inside" a test section by watching for the marker.
        let mut in_test_section = false;
        for (lineno, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.contains("#[cfg(test)]") {
                in_test_section = true;
            }
            if !in_test_section {
                continue;
            }
            // Detect `CREATE TABLE <name>` (with or without `IF NOT EXISTS`).
            let upper = trimmed.to_uppercase();
            if upper.contains("CREATE TABLE") {
                let after = upper
                    .find("CREATE TABLE")
                    .map(|i| &upper[i + "CREATE TABLE".len()..])
                    .unwrap_or("")
                    .trim_start();
                let after = if let Some(stripped) = after.strip_prefix("IF NOT EXISTS") {
                    stripped.trim_start()
                } else {
                    after
                };
                // Extract the table name (first word).
                let table_name = after
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("")
                    .to_lowercase();
                if MIGRATION_TABLES.contains(&table_name.as_str()) {
                    // Skip pre-existing known violations (ratchet).
                    let is_known = KNOWN_REMAINING
                        .iter()
                        .any(|(f, t)| rel_path.ends_with(f) && *t == table_name.as_str());
                    if !is_known {
                        violations.push(format!(
                            "  {}:{} — inline CREATE TABLE {}",
                            rel_path,
                            lineno + 1,
                            table_name
                        ));
                    }
                }
            }
        }
    }
}
