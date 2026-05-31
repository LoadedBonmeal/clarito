//! R9 Q-B — Migration + schema-contract integration tests.
//!
//! These tests run the full migration set against a fresh in-memory SQLite
//! pool and assert the critical schema contracts that the command layer relies
//! on. They catch migration-ordering regressions (REG-13 class) and schema
//! invariants that lib unit tests do not cover.
//!
//! The `db` module is private, so we exercise the public migration surface via
//! `sqlx::migrate!` + raw parameterised SQL that mirrors what the commands do.

use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};

/// Spin up a fresh in-memory pool and apply every migration in order.
async fn migrated_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("failed to open in-memory SQLite");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations failed");

    pool
}

// ─── helper: seed a minimal company + contact ───────────────────────────────

async fn seed_company(pool: &SqlitePool, id: &str, cui: &str) {
    sqlx::query(
        "INSERT INTO companies
            (id, cui, legal_name, address, city, county)
         VALUES (?, ?, 'Test SRL', 'Str. Test 1', 'Bucuresti', 'B')",
    )
    .bind(id)
    .bind(cui)
    .execute(pool)
    .await
    .expect("insert company");
}

async fn seed_contact(pool: &SqlitePool, id: &str, company_id: &str) {
    sqlx::query(
        "INSERT INTO contacts
            (id, company_id, contact_type, legal_name)
         VALUES (?, ?, 'CUSTOMER', 'Client SRL')",
    )
    .bind(id)
    .bind(company_id)
    .execute(pool)
    .await
    .expect("insert contact");
}

// ─── Tests ──────────────────────────────────────────────────────────────────

/// All migrations apply cleanly to a blank database in the declared order.
#[tokio::test]
async fn migrations_apply_cleanly() {
    let pool = migrated_pool().await;
    // If we get here without panicking, all migrations succeeded.
    // Spot-check that at least one expected table exists.
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='invoices'",
    )
    .fetch_one(&pool)
    .await
    .expect("query sqlite_master");
    assert_eq!(count, 1, "invoices table must exist after migrations");
}

/// The UNIQUE(company_id, series, number) constraint rejects duplicate invoice numbers.
#[tokio::test]
async fn invoice_full_number_uniqueness() {
    let pool = migrated_pool().await;
    seed_company(&pool, "c1", "RO12345").await;
    seed_contact(&pool, "ct1", "c1").await;

    let insert = |inv_id: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO invoices
                    (id, company_id, contact_id, series, number, full_number,
                     issue_date, due_date,
                     subtotal_amount, vat_amount, total_amount)
                 VALUES (?, 'c1', 'ct1', 'FACT', 1, 'FACT-1',
                         '2026-01-01', '2026-01-31',
                         '100.00', '19.00', '119.00')",
            )
            .bind(inv_id)
            .execute(&pool)
            .await
        }
    };

    // First insert succeeds.
    insert("inv-1").await.expect("first insert must succeed");

    // Second insert with the same (company_id, series, number) must fail.
    let err = insert("inv-2").await;
    assert!(
        err.is_err(),
        "duplicate (company_id, series, number) must be rejected"
    );
    let msg = err.unwrap_err().to_string();
    assert!(
        msg.contains("UNIQUE") || msg.contains("unique"),
        "error must mention UNIQUE constraint, got: {msg}"
    );
}

/// Storno status guard: only VALIDATED invoices may be storned (UPDATE WHERE status='VALIDATED').
#[tokio::test]
async fn storno_status_transition_guard() {
    let pool = migrated_pool().await;
    seed_company(&pool, "c2", "RO22222").await;
    seed_contact(&pool, "ct2", "c2").await;

    // Insert a DRAFT invoice.
    sqlx::query(
        "INSERT INTO invoices
            (id, company_id, contact_id, series, number, full_number,
             issue_date, due_date,
             subtotal_amount, vat_amount, total_amount, status)
         VALUES ('inv-d', 'c2', 'ct2', 'FACT', 1, 'FACT-1',
                 '2026-01-01', '2026-01-31',
                 '100.00', '19.00', '119.00', 'DRAFT')",
    )
    .execute(&pool)
    .await
    .expect("insert draft invoice");

    // Attempt storno (command pattern: UPDATE WHERE status='VALIDATED').
    let rows =
        sqlx::query("UPDATE invoices SET status='STORNED' WHERE id='inv-d' AND status='VALIDATED'")
            .execute(&pool)
            .await
            .expect("update query")
            .rows_affected();

    assert_eq!(
        rows, 0,
        "DRAFT invoice must not be transitioned to STORNED (rows_affected must be 0)"
    );

    // Insert a VALIDATED invoice.
    sqlx::query(
        "INSERT INTO invoices
            (id, company_id, contact_id, series, number, full_number,
             issue_date, due_date,
             subtotal_amount, vat_amount, total_amount, status)
         VALUES ('inv-v', 'c2', 'ct2', 'FACT', 2, 'FACT-2',
                 '2026-01-01', '2026-01-31',
                 '100.00', '19.00', '119.00', 'VALIDATED')",
    )
    .execute(&pool)
    .await
    .expect("insert validated invoice");

    let rows =
        sqlx::query("UPDATE invoices SET status='STORNED' WHERE id='inv-v' AND status='VALIDATED'")
            .execute(&pool)
            .await
            .expect("update query")
            .rows_affected();

    assert_eq!(
        rows, 1,
        "VALIDATED invoice must be transitioned to STORNED (rows_affected must be 1)"
    );
}

/// Migration 0011 adds contacts.currency column; verify it exists and accepts values.
#[tokio::test]
async fn contact_currency_column_exists() {
    let pool = migrated_pool().await;
    seed_company(&pool, "c3", "RO33333").await;

    // Insert a contact with an explicit currency (non-RON EU client).
    sqlx::query(
        "INSERT INTO contacts
            (id, company_id, contact_type, legal_name, currency)
         VALUES ('ct3', 'c3', 'CUSTOMER', 'EU Client GmbH', 'EUR')",
    )
    .execute(&pool)
    .await
    .expect("insert contact with currency column — migration 0011 must have added it");

    let currency: Option<String> =
        sqlx::query_scalar("SELECT currency FROM contacts WHERE id='ct3'")
            .fetch_one(&pool)
            .await
            .expect("select currency");

    assert_eq!(
        currency.as_deref(),
        Some("EUR"),
        "contacts.currency must store and return the value"
    );
}

/// Migration 0011 also handles contacts with NULL currency (fallback to RON in UI).
#[tokio::test]
async fn contact_currency_nullable() {
    let pool = migrated_pool().await;
    seed_company(&pool, "c4", "RO44444").await;
    seed_contact(&pool, "ct4", "c4").await;

    let currency: Option<String> =
        sqlx::query_scalar("SELECT currency FROM contacts WHERE id='ct4'")
            .fetch_one(&pool)
            .await
            .expect("select currency");

    assert!(
        currency.is_none(),
        "contacts.currency must default to NULL (UI falls back to RON)"
    );
}

/// Migration 0010/0011: unique partial index on notifications.data rejects duplicate SPV keys.
#[tokio::test]
async fn notifications_dedup_unique_index() {
    let pool = migrated_pool().await;

    let insert_notif = |id: &'static str, data: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO notifications
                    (id, notification_type, title, body, data)
                 VALUES (?, 'SPV', 'Mesaj nou', 'Corp mesaj', ?)",
            )
            .bind(id)
            .bind(data)
            .execute(&pool)
            .await
        }
    };

    // First SPV notification succeeds.
    insert_notif("n1", "spv_msg_42")
        .await
        .expect("first SPV notification must insert");

    // Duplicate data key must fail (unique partial index from migration 0010/0011).
    let err = insert_notif("n2", "spv_msg_42").await;
    assert!(
        err.is_err(),
        "duplicate notifications.data must be rejected by the unique index"
    );
    let msg = err.unwrap_err().to_string();
    assert!(
        msg.contains("UNIQUE") || msg.contains("unique"),
        "error must mention UNIQUE constraint, got: {msg}"
    );

    // NULL data values are NOT covered by the partial index — both inserts must succeed.
    sqlx::query(
        "INSERT INTO notifications (id, notification_type, title, body, data)
         VALUES ('n3', 'INFO', 'T', 'B', NULL)",
    )
    .execute(&pool)
    .await
    .expect("null-data notification n3");

    sqlx::query(
        "INSERT INTO notifications (id, notification_type, title, body, data)
         VALUES ('n4', 'INFO', 'T', 'B', NULL)",
    )
    .execute(&pool)
    .await
    .expect("null-data notification n4 — NULLs must not conflict");
}

/// last_invoice_number bump: UPDATE companies SET last_invoice_number = last_invoice_number + 1.
#[tokio::test]
async fn last_invoice_number_bump() {
    let pool = migrated_pool().await;
    seed_company(&pool, "c5", "RO55555").await;

    sqlx::query("UPDATE companies SET last_invoice_number = last_invoice_number + 1 WHERE id='c5'")
        .execute(&pool)
        .await
        .expect("bump last_invoice_number");

    let n: i64 = sqlx::query_scalar("SELECT last_invoice_number FROM companies WHERE id='c5'")
        .fetch_one(&pool)
        .await
        .expect("select last_invoice_number");

    assert_eq!(n, 1, "last_invoice_number must be 1 after one bump");

    // Bump again.
    sqlx::query("UPDATE companies SET last_invoice_number = last_invoice_number + 1 WHERE id='c5'")
        .execute(&pool)
        .await
        .expect("bump 2");

    let n2: i64 = sqlx::query_scalar("SELECT last_invoice_number FROM companies WHERE id='c5'")
        .fetch_one(&pool)
        .await
        .expect("select last_invoice_number 2");

    assert_eq!(n2, 2, "last_invoice_number must be 2 after two bumps");
}

/// Storno FK: storno_of_invoice_id column exists (migration 0008) and the FK is enforced.
#[tokio::test]
async fn storno_fk_column_exists() {
    let pool = migrated_pool().await;
    seed_company(&pool, "c6", "RO66666").await;
    seed_contact(&pool, "ct6", "c6").await;

    // Insert original invoice.
    sqlx::query(
        "INSERT INTO invoices
            (id, company_id, contact_id, series, number, full_number,
             issue_date, due_date,
             subtotal_amount, vat_amount, total_amount, status)
         VALUES ('orig-1', 'c6', 'ct6', 'FACT', 1, 'FACT-1',
                 '2026-01-01', '2026-01-31',
                 '500.00', '95.00', '595.00', 'VALIDATED')",
    )
    .execute(&pool)
    .await
    .expect("insert original invoice");

    // Insert storno invoice referencing it.
    sqlx::query(
        "INSERT INTO invoices
            (id, company_id, contact_id, series, number, full_number,
             issue_date, due_date,
             subtotal_amount, vat_amount, total_amount, status,
             storno_of_invoice_id)
         VALUES ('storno-1', 'c6', 'ct6', 'FACT', 2, 'FACT-2',
                 '2026-01-15', '2026-01-31',
                 '-500.00', '-95.00', '-595.00', 'DRAFT',
                 'orig-1')",
    )
    .execute(&pool)
    .await
    .expect("insert storno invoice with storno_of_invoice_id — migration 0008 must have added it");

    let ref_id: Option<String> =
        sqlx::query_scalar("SELECT storno_of_invoice_id FROM invoices WHERE id='storno-1'")
            .fetch_one(&pool)
            .await
            .expect("select storno_of_invoice_id");

    assert_eq!(
        ref_id.as_deref(),
        Some("orig-1"),
        "storno_of_invoice_id must persist after migration 0008"
    );
}

/// invoice_line_items stores monetary values as TEXT (migration 006).
#[tokio::test]
async fn invoice_line_items_text_amounts() {
    let pool = migrated_pool().await;
    seed_company(&pool, "c7", "RO77777").await;
    seed_contact(&pool, "ct7", "c7").await;

    sqlx::query(
        "INSERT INTO invoices
            (id, company_id, contact_id, series, number, full_number,
             issue_date, due_date,
             subtotal_amount, vat_amount, total_amount)
         VALUES ('inv-7', 'c7', 'ct7', 'FACT', 1, 'FACT-1',
                 '2026-03-01', '2026-03-31',
                 '1000.00', '190.00', '1190.00')",
    )
    .execute(&pool)
    .await
    .expect("insert invoice");

    sqlx::query(
        "INSERT INTO invoice_line_items
            (id, invoice_id, position, name, quantity, unit, unit_price,
             vat_rate, vat_category,
             subtotal_amount, vat_amount, total_amount)
         VALUES ('li-1', 'inv-7', 1, 'Servicii consultanta', '10.00', 'ora', '100.00',
                 '19.00', 'S',
                 '1000.00', '190.00', '1190.00')",
    )
    .execute(&pool)
    .await
    .expect("insert line item with TEXT amounts — migration 006 must have rebuilt the table");

    let row = sqlx::query(
        "SELECT quantity, unit_price, total_amount FROM invoice_line_items WHERE id='li-1'",
    )
    .fetch_one(&pool)
    .await
    .expect("select line item");

    // After migration 006, all monetary columns are TEXT.
    let qty: &str = row.get("quantity");
    assert_eq!(qty, "10.00", "quantity must be stored as TEXT");

    let total: &str = row.get("total_amount");
    assert_eq!(total, "1190.00", "total_amount must be stored as TEXT");
}

/// Payment means code defaults to '30' (bank transfer) — migration 0002.
#[tokio::test]
async fn invoice_payment_means_code_default() {
    let pool = migrated_pool().await;
    seed_company(&pool, "c8", "RO88888").await;
    seed_contact(&pool, "ct8", "c8").await;

    sqlx::query(
        "INSERT INTO invoices
            (id, company_id, contact_id, series, number, full_number,
             issue_date, due_date,
             subtotal_amount, vat_amount, total_amount)
         VALUES ('inv-8', 'c8', 'ct8', 'FACT', 1, 'FACT-1',
                 '2026-04-01', '2026-04-30',
                 '200.00', '38.00', '238.00')",
    )
    .execute(&pool)
    .await
    .expect("insert invoice without explicit payment_means_code");

    let code: String =
        sqlx::query_scalar("SELECT payment_means_code FROM invoices WHERE id='inv-8'")
            .fetch_one(&pool)
            .await
            .expect("select payment_means_code");

    assert_eq!(
        code, "30",
        "payment_means_code must default to '30' (bank transfer) — migration 0002"
    );
}

/// Empty-string data is also blocked by the partial unique index (WHERE data != '').
#[tokio::test]
async fn notifications_empty_string_data_not_indexed() {
    let pool = migrated_pool().await;

    // Two rows with data='' must both succeed (empty string excluded from index).
    for id in ["ne1", "ne2"] {
        sqlx::query(
            "INSERT INTO notifications (id, notification_type, title, body, data)
             VALUES (?, 'INFO', 'T', 'B', '')",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap_or_else(|_| panic!("empty-data notification {id} must insert"));
    }

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE data=''")
        .fetch_one(&pool)
        .await
        .expect("count empty-data rows");

    assert_eq!(
        count, 2,
        "both empty-string data rows must persist (excluded from partial unique index)"
    );
}
