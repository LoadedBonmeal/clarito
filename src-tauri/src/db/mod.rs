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
pub mod accruals;
pub mod advance_invoices;
pub mod aging;
pub mod assets;
pub mod avize;
pub mod capital_goods;
pub mod concedii;
pub mod declaration_filings;
pub mod deconturi;
pub mod dezmembrari;
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
pub mod payroll_retineri;
pub mod payroll_sporuri;
pub mod period_locks;
pub mod pontaj;
pub mod productie;
pub mod products;
pub mod provisions;
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
    use std::collections::BTreeSet;

    /// Tables that exist in migrations/ but are intentionally NOT guarded.
    /// Keep this empty unless a table genuinely must be exempt (document why).
    const EXCLUDED_TABLES: &[&str] = &[];

    /// Sentinel tables that MUST be present in the derived set.  If the
    /// migration parser ever breaks (e.g. an SQL style change), the derived
    /// set would silently shrink and the guard would stop protecting tables —
    /// these assertions turn that into a loud test failure instead.
    const SENTINEL_TABLES: &[&str] = &[
        "invoices",
        "payments",
        "productie_orders",
        // Newest tables (migrations 0082+) that the old hand-maintained list
        // was missing — keep them here so a regression is caught immediately.
        "asset_revaluations",
        "fx_treasury_revaluation",
        "accruals",
        "provisions",
        "capital_goods",
        "capital_good_adjustments",
    ];

    /// Derive the set of migration-managed tables at test runtime by scanning
    /// `migrations/*.sql` for `CREATE TABLE` names.  Replaces the old
    /// hand-maintained list, which had drifted (6 newest tables missing).
    ///
    /// Exclusions:
    /// - `*_new` temporaries used during schema rebuilds;
    /// - tables consumed by `ALTER TABLE <old> RENAME TO <new>` (the `<old>`
    ///   name no longer exists after the migration runs);
    /// - anything in [`EXCLUDED_TABLES`].
    fn migration_tables() -> BTreeSet<String> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let migrations_dir = std::path::PathBuf::from(manifest_dir).join("migrations");

        let mut created: BTreeSet<String> = BTreeSet::new();
        let mut renamed_away: BTreeSet<String> = BTreeSet::new();

        let entries = std::fs::read_dir(&migrations_dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", migrations_dir.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "sql").unwrap_or(false) {
                let content = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
                scan_migration_sql(&content, &mut created, &mut renamed_away);
            }
        }

        // `ALTER TABLE old RENAME TO new` sources are gone after migration.
        for gone in &renamed_away {
            created.remove(gone);
        }
        // Defensive: `*_new` rebuild temporaries never survive migrations.
        created.retain(|t| !t.ends_with("_new"));
        for excluded in EXCLUDED_TABLES {
            created.remove(*excluded);
        }
        created
    }

    /// Extract `CREATE TABLE` names and `ALTER TABLE ... RENAME TO` sources
    /// from one migration file's SQL text.
    fn scan_migration_sql(
        content: &str,
        created: &mut BTreeSet<String>,
        renamed_away: &mut BTreeSet<String>,
    ) {
        for line in content.lines() {
            let upper = line.trim().to_uppercase();

            if let Some(idx) = upper.find("CREATE TABLE") {
                let after = upper[idx + "CREATE TABLE".len()..].trim_start();
                let after = after.strip_prefix("IF NOT EXISTS").unwrap_or(after);
                if let Some(name) = first_identifier(after) {
                    created.insert(name);
                }
            }

            // `ALTER TABLE <old> RENAME TO <new>` — but NOT `RENAME COLUMN x TO y`.
            if upper.contains("ALTER TABLE")
                && upper.contains("RENAME TO")
                && !upper.contains("RENAME COLUMN")
            {
                if let Some(idx) = upper.find("ALTER TABLE") {
                    let after = upper[idx + "ALTER TABLE".len()..].trim_start();
                    if let Some(old) = first_identifier(after) {
                        renamed_away.insert(old);
                    }
                }
                // The rename target exists post-migration; record it as created.
                if let Some(idx) = upper.find("RENAME TO") {
                    let after = upper[idx + "RENAME TO".len()..].trim_start();
                    if let Some(new) = first_identifier(after) {
                        created.insert(new);
                    }
                }
            }
        }
    }

    /// First SQL identifier in `s` (lowercased), tolerating leading
    /// whitespace and quote characters.
    fn first_identifier(s: &str) -> Option<String> {
        let s = s.trim_start().trim_start_matches(['"', '`', '\'', '[']);
        let name: String = s
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            None
        } else {
            Some(name.to_lowercase())
        }
    }

    /// Pre-existing known violations that have not yet been migrated to
    /// sqlx::migrate!.  Keyed as `("src/path/relative/to/src-tauri", "table")`.
    /// Do NOT add new entries here — fix the fixture instead.
    /// Remove entries as those files get converted.
    // All hand-rolled CREATE TABLE fixtures have been converted to sqlx::migrate!.
    // KNOWN_REMAINING is empty — no pre-existing violations remain.
    const KNOWN_REMAINING: &[(&str, &str)] = &[];

    #[test]
    fn no_hand_rolled_create_table_in_test_modules() {
        use std::path::PathBuf;

        // Locate src-tauri/src/ relative to the manifest (CARGO_MANIFEST_DIR is
        // set by cargo test to the crate root, i.e. src-tauri/).
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let src_dir = PathBuf::from(manifest_dir).join("src");

        // Derive the guarded table set from migrations/ so newly added tables
        // are protected automatically (no hand-maintained list to go stale).
        let tables = migration_tables();
        for sentinel in SENTINEL_TABLES {
            assert!(
                tables.contains(*sentinel),
                "migration-table derivation lost sentinel table `{}` — the SQL \
                 parser in migration_tables() no longer matches migrations/*.sql.\n\
                 Derived set ({} tables): {:?}",
                sentinel,
                tables.len(),
                tables
            );
        }

        let mut violations: Vec<String> = Vec::new();
        collect_violations(&src_dir, manifest_dir, &tables, &mut violations);

        assert!(
            violations.is_empty(),
            "NEW hand-rolled CREATE TABLE for migration-managed tables found in #[cfg(test)] code.\n\
             Switch these fixtures to sqlx::migrate!(\"./migrations\").run(&pool) instead.\n\
             Do NOT add them to KNOWN_REMAINING — fix them:\n\n{}\n\n\
             Guarded tables derived from migrations/ ({}): {:?}",
            violations.join("\n"),
            tables.len(),
            tables
        );
    }

    /// Walk `dir` recursively, scanning each .rs file for inline CREATE TABLE
    /// statements that appear inside a `#[cfg(test)]` section.
    fn collect_violations(
        dir: &std::path::Path,
        crate_root: &str,
        tables: &BTreeSet<String>,
        violations: &mut Vec<String>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_violations(&path, crate_root, tables, violations);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                scan_file(&path, crate_root, tables, violations);
            }
        }
    }

    fn scan_file(
        path: &std::path::Path,
        crate_root: &str,
        tables: &BTreeSet<String>,
        violations: &mut Vec<String>,
    ) {
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
                if tables.contains(&table_name) {
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
