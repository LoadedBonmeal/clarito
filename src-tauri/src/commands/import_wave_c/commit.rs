//! Wave C W4 — staging writer + commit engine + Tauri commands.
// sqlx query_as returns complex tuple types for multi-column rows; suppress the lint.
#![allow(clippy::type_complexity)]
//!
//! Public surface:
//!   `stage_parsed`      – insert an import_batch + all staging rows (pure of file I/O).
//!   `commit_batch`      – run resolver, then loop accounts→contacts→products→invoices,
//!                         calling existing validated create fns, per-row isolated tx.
//!   `import_wave_c_stage`   } Tauri commands
//!   `import_wave_c_preview` }
//!   `import_wave_c_commit`  }
//!
//! HARD CONSTRAINTS obeyed:
//!   • NEVER calls db::gl::generate_gl_entries.
//!   • Issued invoices → status DRAFT (historical imports; no GL posting).
//!   • Received invoices → db::received::create_imported (SHA256 dedup).
//!   • Per-row isolated commit (mirror commands/import.rs:~343).
//!   • resolution MATCH  → SKIP (reuse matched_id, no create).
//!   • resolution REVIEW | ERROR | DUP_IN_BATCH → SKIP, left for user.
//!   • resolution NEW + per-row create fn error → catch, set ERROR, continue.

use tauri::State;

use crate::db::models::{new_id, now_unix, ContactType};
use crate::db::{accounts, contacts, invoices, products, received};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::adapter::ImportAdapter;
use super::resolve::{counts_for_batch, resolve_batch, BatchCounts};
use super::{
    ImportInput, SourceKind, StagedAccount, StagedContact, StagedData, StagedInvoice, StagedProduct,
};

// ─── Public types ────────────────────────────────────────────────────────────

/// Per-entity breakdown returned by `commit_batch`.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityReport {
    pub created: u32,
    pub matched: u32,
    pub skipped: u32,
    pub errors: u32,
}

/// Full result of a `commit_batch` call.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitReport {
    pub batch_id: String,
    pub contacts: EntityReport,
    pub products: EntityReport,
    pub accounts: EntityReport,
    pub invoices: EntityReport,
    pub errors: Vec<String>,
}

/// Options forwarded from the frontend for commit.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitOptions {
    /// If true, only commit NEW rows whose partner is fully resolved (ignore invoices with
    /// unresolved partners).  Default false = commit what we can.
    pub skip_unresolved_partners: Option<bool>,
}

// ─── Stage parsed data ───────────────────────────────────────────────────────

/// Insert an `import_batch` row + all staging rows derived from `StagedData`.
/// Returns the new `batch_id`.  PURE of file I/O (testable directly).
pub async fn stage_parsed(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    source: SourceKind,
    source_label: Option<&str>,
    staged: StagedData,
) -> AppResult<String> {
    let batch_id = new_id();
    let now = now_unix();
    let source_str = source.to_string();

    sqlx::query(
        "INSERT INTO import_batch \
         (id, company_id, source, source_label, status, created_at) \
         VALUES (?1, ?2, ?3, ?4, 'PARSED', ?5)",
    )
    .bind(&batch_id)
    .bind(company_id)
    .bind(&source_str)
    .bind(source_label)
    .bind(now)
    .execute(pool)
    .await?;

    // Insert contacts.
    for c in &staged.contacts {
        insert_staged_contact(pool, &batch_id, c).await?;
    }
    // Insert products.
    for p in &staged.products {
        insert_staged_product(pool, &batch_id, p).await?;
    }
    // Insert accounts.
    for a in &staged.accounts {
        insert_staged_account(pool, &batch_id, a).await?;
    }
    // Insert invoices + lines.
    for inv in &staged.invoices {
        insert_staged_invoice(pool, &batch_id, inv).await?;
    }

    Ok(batch_id)
}

async fn insert_staged_contact(
    pool: &sqlx::SqlitePool,
    batch_id: &str,
    c: &StagedContact,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO import_staging_contact \
         (id, batch_id, source, raw_json, source_code, contact_type, \
          cui_raw, cui_canonical, legal_name, vat_payer, is_individual, \
          address, city, county, country, email, phone, dedup_key, resolution) \
         VALUES \
         (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, 'NEW')",
    )
    .bind(&c.id)
    .bind(batch_id)
    .bind(&c.source)
    .bind(&c.raw_json)
    .bind(&c.source_code)
    .bind(&c.contact_type)
    .bind(&c.cui_raw)
    .bind(&c.cui_canonical)
    .bind(&c.legal_name)
    .bind(c.vat_payer.map(|v| v as i64))
    .bind(c.is_individual.map(|v| v as i64))
    .bind(&c.address)
    .bind(&c.city)
    .bind(&c.county)
    .bind(&c.country)
    .bind(&c.email)
    .bind(&c.phone)
    .bind(&c.dedup_key)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_staged_product(
    pool: &sqlx::SqlitePool,
    batch_id: &str,
    p: &StagedProduct,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO import_staging_product \
         (id, batch_id, source, raw_json, source_code, name, unit, unit_price, \
          vat_rate, vat_category, code, barcode, stock_qty, is_service, dedup_key, resolution) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 'NEW')",
    )
    .bind(&p.id)
    .bind(batch_id)
    .bind(&p.source)
    .bind(&p.raw_json)
    .bind(&p.source_code)
    .bind(&p.name)
    .bind(&p.unit)
    .bind(&p.unit_price)
    .bind(&p.vat_rate)
    .bind(&p.vat_category)
    .bind(&p.code)
    .bind(&p.barcode)
    .bind(&p.stock_qty)
    .bind(p.is_service.map(|v| v as i64))
    .bind(&p.dedup_key)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_staged_account(
    pool: &sqlx::SqlitePool,
    batch_id: &str,
    a: &StagedAccount,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO import_staging_account \
         (id, batch_id, source, raw_json, account_code, synthetic_code, analytic_suffix, \
          account_name, account_class, dedup_key, resolution) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'NEW')",
    )
    .bind(&a.id)
    .bind(batch_id)
    .bind(&a.source)
    .bind(&a.raw_json)
    .bind(&a.account_code)
    .bind(&a.synthetic_code)
    .bind(&a.analytic_suffix)
    .bind(&a.account_name)
    .bind(a.account_class)
    .bind(&a.dedup_key)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_staged_invoice(
    pool: &sqlx::SqlitePool,
    batch_id: &str,
    inv: &StagedInvoice,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO import_staging_invoice \
         (id, batch_id, source, raw_json, direction, external_id, partner_cui_canonical, \
          partner_name, series, number, full_number, issue_date, due_date, currency, exchange_rate, \
          reverse_charge, cash_vat, subtotal_amount, vat_amount, total_amount, dedup_key, resolution) \
         VALUES \
         (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, 'NEW')",
    )
    .bind(&inv.id)
    .bind(batch_id)
    .bind(&inv.source)
    .bind(&inv.raw_json)
    .bind(&inv.direction)
    .bind(&inv.external_id)
    .bind(&inv.partner_cui_canonical)
    .bind(&inv.partner_name)
    .bind(&inv.series)
    .bind(&inv.number)
    .bind(&inv.full_number)
    .bind(&inv.issue_date)
    .bind(&inv.due_date)
    .bind(&inv.currency)
    .bind(inv.exchange_rate)
    .bind(inv.reverse_charge.map(|v| v as i64))
    .bind(inv.cash_vat.map(|v| v as i64))
    .bind(&inv.subtotal_amount)
    .bind(&inv.vat_amount)
    .bind(&inv.total_amount)
    .bind(&inv.dedup_key)
    .execute(pool)
    .await?;

    // Insert lines.
    for line in &inv.lines {
        sqlx::query(
            "INSERT INTO import_staging_invoice_line \
             (id, invoice_staging_id, position, name, description, product_code, \
              quantity, unit, unit_price, vat_rate, vat_category, \
              subtotal_amount, vat_amount, total_amount, account_code, warehouse) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        )
        .bind(&line.id)
        .bind(&inv.id)
        .bind(line.position)
        .bind(&line.name)
        .bind(&line.description)
        .bind(&line.product_code)
        .bind(&line.quantity)
        .bind(&line.unit)
        .bind(&line.unit_price)
        .bind(&line.vat_rate)
        .bind(&line.vat_category)
        .bind(&line.subtotal_amount)
        .bind(&line.vat_amount)
        .bind(&line.total_amount)
        .bind(&line.account_code)
        .bind(&line.warehouse)
        .execute(pool)
        .await?;
    }

    Ok(())
}

// ─── Commit batch ────────────────────────────────────────────────────────────

/// Run `resolve_batch`, then commit all NEW rows in dependency order:
/// accounts → contacts → products → invoices.
/// Each row is committed in its own isolated transaction (mirror import.rs:~343).
pub async fn commit_batch(
    pool: &sqlx::SqlitePool,
    batch_id: &str,
    _options: CommitOptions,
) -> AppResult<CommitReport> {
    resolve_batch(pool, batch_id).await?;

    let company_id: String =
        sqlx::query_scalar("SELECT company_id FROM import_batch WHERE id = ?1")
            .bind(batch_id)
            .fetch_optional(pool)
            .await?
            .ok_or(AppError::NotFound)?;

    let mut report = CommitReport {
        batch_id: batch_id.to_string(),
        ..Default::default()
    };

    // ── 1. Accounts ──────────────────────────────────────────────────────────
    commit_accounts(pool, batch_id, &company_id, &mut report).await?;

    // ── 2. Contacts ──────────────────────────────────────────────────────────
    commit_contacts(pool, batch_id, &company_id, &mut report).await?;

    // ── 3. Products ──────────────────────────────────────────────────────────
    commit_products(pool, batch_id, &company_id, &mut report).await?;

    // ── 4. Invoices ──────────────────────────────────────────────────────────
    commit_invoices(pool, batch_id, &company_id, &mut report).await?;

    // Mark batch COMMITTED.
    let now = now_unix();
    let counts_json =
        serde_json::to_string(&counts_for_batch(pool, batch_id).await?).unwrap_or_default();
    sqlx::query(
        "UPDATE import_batch \
         SET status = 'COMMITTED', committed_at = ?2, counts_json = ?3 \
         WHERE id = ?1",
    )
    .bind(batch_id)
    .bind(now)
    .bind(&counts_json)
    .execute(pool)
    .await?;

    Ok(report)
}

// ── Accounts commit ──────────────────────────────────────────────────────────

async fn commit_accounts(
    pool: &sqlx::SqlitePool,
    batch_id: &str,
    company_id: &str,
    report: &mut CommitReport,
) -> AppResult<()> {
    let rows: Vec<(String, String, Option<String>, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT id, resolution, account_code, account_name, account_class \
             FROM import_staging_account WHERE batch_id = ?1",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;

    for (staging_id, resolution, account_code, account_name, account_class) in rows {
        match resolution.as_str() {
            "MATCH" => {
                report.accounts.matched += 1;
            }
            "NEW" => {
                let code = match account_code {
                    Some(ref c) if !c.trim().is_empty() => c.clone(),
                    _ => {
                        set_staging_error(
                            pool,
                            "import_staging_account",
                            &staging_id,
                            "account_code is required",
                        )
                        .await?;
                        report.accounts.errors += 1;
                        continue;
                    }
                };
                let input = accounts::AccountInput {
                    account_code: code.trim().to_string(),
                    account_name: account_name
                        .as_deref()
                        .unwrap_or("(import)")
                        .trim()
                        .to_string(),
                    account_class,
                    parent_code: None,
                    active: Some(true),
                };
                match accounts::create(pool, company_id, input).await {
                    Ok(acc) => {
                        write_matched_id(pool, "import_staging_account", &staging_id, &acc.id)
                            .await?;
                        report.accounts.created += 1;
                    }
                    Err(e) => {
                        set_staging_error(
                            pool,
                            "import_staging_account",
                            &staging_id,
                            &e.to_string(),
                        )
                        .await?;
                        report
                            .errors
                            .push(format!("account staging {staging_id}: {e}"));
                        report.accounts.errors += 1;
                    }
                }
            }
            _ => {
                // DUP_IN_BATCH | REVIEW | ERROR — skip.
                report.accounts.skipped += 1;
            }
        }
    }
    Ok(())
}

// ── Contacts commit ──────────────────────────────────────────────────────────

async fn commit_contacts(
    pool: &sqlx::SqlitePool,
    batch_id: &str,
    company_id: &str,
    report: &mut CommitReport,
) -> AppResult<()> {
    let rows: Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, resolution, contact_type, cui_canonical, legal_name, \
         country, vat_payer, is_individual, address, city, county, email, phone, matched_id \
         FROM import_staging_contact WHERE batch_id = ?1",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;

    for (
        staging_id,
        resolution,
        contact_type_str,
        cui_canonical,
        legal_name,
        country,
        vat_payer,
        is_individual,
        address,
        city,
        county,
        email,
        phone,
        matched_id,
    ) in rows
    {
        match resolution.as_str() {
            "MATCH" => {
                // Already matched — ensure matched_id is written (it was set by resolve).
                // Write it again in case of re-run.
                if let Some(ref mid) = matched_id {
                    write_matched_id(pool, "import_staging_contact", &staging_id, mid).await?;
                }
                report.contacts.matched += 1;
            }
            "NEW" => {
                let name = match legal_name {
                    Some(ref n) if !n.trim().is_empty() => n.trim().to_string(),
                    _ => {
                        set_staging_error(
                            pool,
                            "import_staging_contact",
                            &staging_id,
                            "legal_name is required",
                        )
                        .await?;
                        report.contacts.errors += 1;
                        continue;
                    }
                };
                let ct = match contact_type_str
                    .as_deref()
                    .unwrap_or("CUSTOMER")
                    .to_uppercase()
                    .as_str()
                {
                    "SUPPLIER" => ContactType::Supplier,
                    "BOTH" => ContactType::Both,
                    _ => ContactType::Customer,
                };
                let input = contacts::CreateContactInput {
                    company_id: company_id.to_string(),
                    contact_type: ct,
                    cui: cui_canonical.clone(),
                    legal_name: name,
                    vat_payer: vat_payer.map(|v| v != 0),
                    is_individual: is_individual.map(|v| v != 0),
                    cash_vat: None,
                    address,
                    city,
                    county,
                    country,
                    email,
                    phone,
                    currency: None,
                    iban: None,
                    bank_name: None,
                    swift: None,
                    payment_term_days: None,
                };
                match contacts::create(pool, input).await {
                    Ok(c) => {
                        write_matched_id(pool, "import_staging_contact", &staging_id, &c.id)
                            .await?;
                        report.contacts.created += 1;
                    }
                    Err(e) => {
                        set_staging_error(
                            pool,
                            "import_staging_contact",
                            &staging_id,
                            &e.to_string(),
                        )
                        .await?;
                        report
                            .errors
                            .push(format!("contact staging {staging_id}: {e}"));
                        report.contacts.errors += 1;
                    }
                }
            }
            _ => {
                report.contacts.skipped += 1;
            }
        }
    }
    Ok(())
}

// ── Products commit ──────────────────────────────────────────────────────────

async fn commit_products(
    pool: &sqlx::SqlitePool,
    batch_id: &str,
    company_id: &str,
    report: &mut CommitReport,
) -> AppResult<()> {
    let rows: Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, resolution, name, unit, unit_price, vat_rate, vat_category, \
         code, barcode, stock_qty \
         FROM import_staging_product WHERE batch_id = ?1",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;

    for (
        staging_id,
        resolution,
        name,
        unit,
        unit_price,
        vat_rate,
        vat_category,
        code,
        barcode,
        stock_qty,
    ) in rows
    {
        match resolution.as_str() {
            "MATCH" => {
                report.products.matched += 1;
            }
            "NEW" => {
                let product_name = match name {
                    Some(ref n) if !n.trim().is_empty() => n.trim().to_string(),
                    _ => {
                        set_staging_error(
                            pool,
                            "import_staging_product",
                            &staging_id,
                            "name is required",
                        )
                        .await?;
                        report.products.errors += 1;
                        continue;
                    }
                };
                let input = products::ProductInput {
                    name: product_name,
                    unit,
                    unit_price,
                    vat_rate,
                    vat_category,
                    code,
                    barcode,
                    stock_qty,
                    art331_code: None,
                    active: Some(true),
                };
                match products::create(pool, company_id, input).await {
                    Ok(p) => {
                        write_matched_id(pool, "import_staging_product", &staging_id, &p.id)
                            .await?;
                        report.products.created += 1;
                    }
                    Err(e) => {
                        set_staging_error(
                            pool,
                            "import_staging_product",
                            &staging_id,
                            &e.to_string(),
                        )
                        .await?;
                        report
                            .errors
                            .push(format!("product staging {staging_id}: {e}"));
                        report.products.errors += 1;
                    }
                }
            }
            _ => {
                report.products.skipped += 1;
            }
        }
    }
    Ok(())
}

// ── Invoices commit ──────────────────────────────────────────────────────────

/// Extract the numeric value of a staged invoice-number string (digits only): "0153" → 153,
/// "FACT-0153" → 153, "" / "ABC" → None. The original display form is preserved in `full_number`.
/// Shared with the resolver so the preview match and the commit insert agree on the parsed number.
pub(crate) fn parse_staged_number(s: &str) -> Option<i64> {
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<i64>().ok()
    }
}

async fn commit_invoices(
    pool: &sqlx::SqlitePool,
    batch_id: &str,
    company_id: &str,
    report: &mut CommitReport,
) -> AppResult<()> {
    // Fetch all invoice staging rows.
    let rows: Vec<(
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<f64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, resolution, direction, partner_matched_id, partner_staging_id, \
         partner_name, series, issue_date, exchange_rate, currency, \
         total_amount, vat_amount, subtotal_amount, due_date, number, full_number \
         FROM import_staging_invoice WHERE batch_id = ?1",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;

    for (
        staging_id,
        resolution,
        direction,
        partner_matched_id,
        partner_staging_id,
        partner_name,
        series,
        issue_date,
        exchange_rate,
        currency,
        total_amount,
        vat_amount,
        subtotal_amount,
        due_date,
        number,
        full_number,
    ) in rows
    {
        match resolution.as_str() {
            "MATCH" => {
                report.invoices.matched += 1;
                continue;
            }
            "NEW" => {}
            _ => {
                report.invoices.skipped += 1;
                continue;
            }
        }

        // Resolve partner contact_id.
        // Prefer partner_matched_id (live contact) > partner_staging_id (just-committed contact
        // whose matched_id was written by commit_contacts above).
        let contact_id: Option<String> = if let Some(ref mid) = partner_matched_id {
            Some(mid.clone())
        } else if let Some(ref sid) = partner_staging_id {
            // Look up the matched_id that commit_contacts just wrote.
            // fetch_optional returns Option<String>; no flatten needed.
            sqlx::query_scalar::<_, String>(
                "SELECT matched_id FROM import_staging_contact WHERE id = ?1",
            )
            .bind(sid)
            .fetch_optional(pool)
            .await?
        } else {
            None
        };

        // Fetch lines for this invoice.
        let lines: Vec<(
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT name, quantity, unit, unit_price, vat_rate, vat_category \
             FROM import_staging_invoice_line \
             WHERE invoice_staging_id = ?1 ORDER BY position",
        )
        .bind(&staging_id)
        .fetch_all(pool)
        .await?;

        let invoice_date = issue_date.as_deref().unwrap_or("2000-01-01").to_string();
        let due = due_date.as_deref().unwrap_or(&invoice_date).to_string();
        let ser = series.as_deref().unwrap_or("IMP").to_string();

        match direction.to_uppercase().as_str() {
            "ISSUED" => {
                // Need a valid contact_id for issued invoices.
                let cid = match contact_id {
                    Some(ref id) => id.clone(),
                    None => {
                        // Try to find or create a placeholder contact from partner_name.
                        match find_or_create_partner_placeholder(
                            pool,
                            company_id,
                            partner_name.as_deref(),
                        )
                        .await
                        {
                            Ok(id) => id,
                            Err(e) => {
                                set_staging_error(
                                    pool,
                                    "import_staging_invoice",
                                    &staging_id,
                                    &format!("partner contact unresolved: {e}"),
                                )
                                .await?;
                                report.errors.push(format!(
                                    "invoice staging {staging_id}: partner unresolved: {e}"
                                ));
                                report.invoices.errors += 1;
                                continue;
                            }
                        }
                    }
                };

                // Build CreateLineInputs.
                let create_lines: Vec<invoices::CreateLineInput> = lines
                    .into_iter()
                    .filter_map(|(name, qty, unit, price, vat_rate, vat_cat)| {
                        let n = name?.trim().to_string();
                        if n.is_empty() {
                            return None;
                        }
                        let quantity = qty
                            .as_deref()
                            .and_then(|s| s.trim().parse::<f64>().ok())
                            .unwrap_or(1.0);
                        let unit_price = price
                            .as_deref()
                            .and_then(|s| s.trim().parse::<f64>().ok())
                            .unwrap_or(0.0);
                        let vat = vat_rate
                            .as_deref()
                            .and_then(|s| s.trim().parse::<f64>().ok())
                            .unwrap_or(0.0);
                        let cat = vat_cat
                            .as_deref()
                            .unwrap_or(if vat > 0.0 { "S" } else { "Z" })
                            .to_string();
                        Some(invoices::CreateLineInput {
                            name: n,
                            description: None,
                            quantity,
                            unit: unit.unwrap_or_else(|| "buc".into()),
                            unit_price,
                            vat_rate: vat,
                            vat_category: cat,
                            cpv_code: None,
                            art331_code: None,
                            revenue_kind: None,
                        })
                    })
                    .collect();

                if create_lines.is_empty() {
                    set_staging_error(
                        pool,
                        "import_staging_invoice",
                        &staging_id,
                        "no valid lines to import",
                    )
                    .await?;
                    report.invoices.errors += 1;
                    continue;
                }

                // Preserve the ORIGINAL invoice number (legal identifier). create_imported keeps
                // series+number and does NOT touch companies.last_invoice_number; a number with no
                // digits can't be a valid issued invoice → ERROR (kept for manual review).
                let number_i64 = match number.as_deref().and_then(parse_staged_number) {
                    Some(n) => n,
                    None => {
                        set_staging_error(
                            pool,
                            "import_staging_invoice",
                            &staging_id,
                            "număr factură lipsă sau nenumeric — nu poate fi importată ca factură emisă",
                        )
                        .await?;
                        report
                            .errors
                            .push(format!("invoice staging {staging_id}: număr invalid"));
                        report.invoices.errors += 1;
                        continue;
                    }
                };

                let input = invoices::CreateImportedInvoiceInput {
                    company_id: company_id.to_string(),
                    contact_id: cid,
                    series: ser,
                    number: number_i64,
                    full_number: full_number.clone(),
                    issue_date: invoice_date,
                    due_date: Some(due),
                    currency: currency.or_else(|| Some("RON".into())),
                    exchange_rate,
                    notes: None,
                    payment_means_code: Some("30".into()),
                    lines: create_lines,
                };

                // create_imported is idempotent on (company, series, number): a re-import returns the
                // existing invoice instead of a duplicate, and the issued invoice lands as DRAFT.
                match invoices::create_imported(pool, input).await {
                    Ok(inv) => {
                        write_matched_id(pool, "import_staging_invoice", &staging_id, &inv.id)
                            .await?;
                        report.invoices.created += 1;
                    }
                    Err(e) => {
                        set_staging_error(
                            pool,
                            "import_staging_invoice",
                            &staging_id,
                            &e.to_string(),
                        )
                        .await?;
                        report
                            .errors
                            .push(format!("invoice staging {staging_id}: {e}"));
                        report.invoices.errors += 1;
                    }
                }
            }

            "RECEIVED" => {
                // raw_json drives the SHA256 idempotency key in received::create_imported.
                let raw_json = sqlx::query_scalar::<_, String>(
                    "SELECT raw_json FROM import_staging_invoice WHERE id = ?1",
                )
                .bind(&staging_id)
                .fetch_one(pool)
                .await?;

                // Resolve the issuer (supplier) CUI from the resolved partner contact.
                let issuer_cui: String = if let Some(ref cid) = contact_id {
                    sqlx::query_scalar::<_, Option<String>>(
                        "SELECT cui FROM contacts WHERE id = ?1 LIMIT 1",
                    )
                    .bind(cid)
                    .fetch_optional(pool)
                    .await?
                    .flatten()
                    .unwrap_or_default()
                } else {
                    String::new()
                };

                let input = received::CreateImportedReceivedInput {
                    company_id: company_id.to_string(),
                    raw_json,
                    issuer_cui,
                    issuer_name: partner_name.as_deref().unwrap_or("(import)").to_string(),
                    series: series.clone(),
                    // Preserve the supplier's original document number (data fidelity + enables the
                    // number-based RECEIVED dedup match on re-import).
                    number: number.clone(),
                    total_amount: total_amount.as_deref().unwrap_or("0.00").to_string(),
                    net_amount: subtotal_amount,
                    vat_amount,
                    currency: currency.unwrap_or_else(|| "RON".into()),
                    exchange_rate,
                    issue_date: invoice_date,
                };

                match received::create_imported(pool, input).await {
                    Ok(id) => {
                        write_matched_id(pool, "import_staging_invoice", &staging_id, &id).await?;
                        report.invoices.created += 1;
                    }
                    Err(AppError::Conflict(_)) => {
                        // Idempotent duplicate — count as matched.
                        report.invoices.matched += 1;
                    }
                    Err(e) => {
                        set_staging_error(
                            pool,
                            "import_staging_invoice",
                            &staging_id,
                            &e.to_string(),
                        )
                        .await?;
                        report
                            .errors
                            .push(format!("invoice staging {staging_id}: {e}"));
                        report.invoices.errors += 1;
                    }
                }
            }

            _ => {
                set_staging_error(
                    pool,
                    "import_staging_invoice",
                    &staging_id,
                    &format!("unknown direction: {direction}"),
                )
                .await?;
                report.invoices.errors += 1;
            }
        }
    }
    Ok(())
}

/// Find an existing placeholder contact by name (blank-CUI), or create one.
async fn find_or_create_partner_placeholder(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    partner_name: Option<&str>,
) -> AppResult<String> {
    let name = partner_name.unwrap_or("(partener necunoscut)").trim();
    if name.is_empty() {
        return Err(AppError::Validation(
            "partner_name is required when no contact is resolved".into(),
        ));
    }

    // Try to find an existing contact with this exact name and no CUI.
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM contacts \
         WHERE company_id = ?1 AND (cui IS NULL OR cui = '') AND legal_name = ?2 LIMIT 1",
    )
    .bind(company_id)
    .bind(name)
    .fetch_optional(pool)
    .await?;

    if let Some(id) = existing {
        return Ok(id);
    }

    let input = contacts::CreateContactInput {
        company_id: company_id.to_string(),
        contact_type: ContactType::Both,
        cui: None,
        legal_name: name.to_string(),
        vat_payer: Some(false),
        is_individual: Some(false),
        cash_vat: None,
        address: None,
        city: None,
        county: None,
        country: Some("RO".into()),
        email: None,
        phone: None,
        currency: None,
        iban: None,
        bank_name: None,
        swift: None,
        payment_term_days: None,
    };
    let c = contacts::create(pool, input).await?;
    Ok(c.id)
}

// ─── DB helpers ──────────────────────────────────────────────────────────────

async fn write_matched_id(
    pool: &sqlx::SqlitePool,
    table: &str,
    staging_id: &str,
    matched_id: &str,
) -> AppResult<()> {
    sqlx::query(&format!("UPDATE {table} SET matched_id = ?2 WHERE id = ?1"))
        .bind(staging_id)
        .bind(matched_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn set_staging_error(
    pool: &sqlx::SqlitePool,
    table: &str,
    staging_id: &str,
    error: &str,
) -> AppResult<()> {
    sqlx::query(&format!(
        "UPDATE {table} SET resolution = 'ERROR', error = ?2 WHERE id = ?1"
    ))
    .bind(staging_id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

// ─── Tauri commands ──────────────────────────────────────────────────────────

/// Stage result returned to the frontend.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageResult {
    pub batch_id: String,
    pub counts: BatchCounts,
    pub warnings: Vec<String>,
}

/// Preview result returned to the frontend.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResult {
    pub batch_id: String,
    pub counts: BatchCounts,
    pub sample_contacts: Vec<serde_json::Value>,
    pub sample_products: Vec<serde_json::Value>,
    pub sample_accounts: Vec<serde_json::Value>,
    pub sample_invoices: Vec<serde_json::Value>,
}

/// Detect columns in a source file (currently only meaningful for SAGA DBF).
/// Returns the list of field names + sample values from the first DBF record.
/// For all other sources this returns an empty vec (schema is fixed/known).
///
/// `source` must be one of the `SourceKind` SCREAMING_SNAKE_CASE names.
#[tauri::command]
pub async fn import_wave_c_detect_columns(
    company_id: String,
    source: String,
    file_paths: Vec<String>,
) -> AppResult<Vec<super::DetectedColumn>> {
    let source_kind: SourceKind = match source.to_uppercase().as_str() {
        "SMARTBILL_XML" => SourceKind::SmartbillXml,
        "SMARTBILL_REST" => SourceKind::SmartbillRest,
        "SAGA_XML" => SourceKind::SagaXml,
        "SAGA_DBF" => SourceKind::SagaDbf,
        "WINMENTOR_TXT" => SourceKind::WinmentorTxt,
        _ => {
            return Err(AppError::Validation(format!(
                "Unknown import source: {source}"
            )))
        }
    };

    // Only SAGA DBF has meaningful column detection.
    match source_kind {
        SourceKind::SagaDbf => {
            let bytes = read_first_file(&file_paths).await?;
            let input = ImportInput::Bytes(bytes);
            let adapter = super::SagaDbfAdapter;
            // detect_columns is DB-free, so no state/pool needed.
            let _ = company_id; // provided for API symmetry
            adapter.detect_columns(&input)
        }
        _ => Ok(vec![]),
    }
}

/// Parse files from `file_paths` using the adapter for `source`, stage all rows,
/// run the resolver, and return the batch_id + resolution counts.
///
/// `source` must be one of the `SourceKind` SCREAMING_SNAKE_CASE names.
/// `column_map` is optional: when provided it overrides the adapter's default
/// synonym table (used for SAGA DBF after the user confirms the column mapping).
#[tauri::command]
pub async fn import_wave_c_stage(
    state: State<'_, AppState>,
    company_id: String,
    source: String,
    file_paths: Vec<String>,
    column_map: Option<std::collections::HashMap<String, String>>,
) -> AppResult<StageResult> {
    use crate::commands::import_wave_c::ParseCtx;

    let pool = &state.db;

    let source_kind: SourceKind = match source.to_uppercase().as_str() {
        "SMARTBILL_XML" => SourceKind::SmartbillXml,
        "SMARTBILL_REST" => SourceKind::SmartbillRest,
        "SAGA_XML" => SourceKind::SagaXml,
        "SAGA_DBF" => SourceKind::SagaDbf,
        "WINMENTOR_TXT" => SourceKind::WinmentorTxt,
        _ => {
            return Err(AppError::Validation(format!(
                "Unknown import source: {source}"
            )))
        }
    };

    // Fetch company CUI for the ParseCtx.
    let company = crate::db::companies::get(pool, &company_id).await?;
    let company_cui_canonical = super::canonical_cui(&company.cui);

    let ctx = ParseCtx {
        company_cui_canonical: &company_cui_canonical,
        column_map: column_map.as_ref(),
    };

    // Build adapter input and parse.
    // Note: ImportAdapter::parse takes &ImportInput (not by value).
    let staged = match source_kind {
        SourceKind::SmartbillXml => {
            let bytes = read_first_file(&file_paths).await?;
            let input = ImportInput::Bytes(bytes);
            let adapter = super::smartbill_xml::SmartBillXmlAdapter;
            adapter.parse(&input, &ctx)?
        }
        SourceKind::SagaXml => {
            let bytes = read_first_file(&file_paths).await?;
            let input = ImportInput::Bytes(bytes);
            let adapter = super::saga_xml::SagaXmlAdapter;
            adapter.parse(&input, &ctx)?
        }
        SourceKind::SagaDbf => {
            // SagaDbfAdapter::parse requires ImportInput::Bytes (one file at a time).
            let bytes = read_first_file(&file_paths).await?;
            let input = ImportInput::Bytes(bytes);
            let adapter = super::SagaDbfAdapter;
            adapter.parse(&input, &ctx)?
        }
        SourceKind::WinmentorTxt => {
            let bytes = read_first_file(&file_paths).await?;
            let input = ImportInput::Bytes(bytes);
            let adapter = super::winmentor::WinMentorTxtAdapter;
            adapter.parse(&input, &ctx)?
        }
        SourceKind::SmartbillRest => {
            // SmartBillRest uses credentials from OS keychain + settings, not file_paths.
            // We pass a RestCreds input; the adapter loads credentials internally.
            let input = ImportInput::RestCreds {
                company_id: company_id.clone(),
            };
            let adapter = super::SmartBillRestAdapter;
            adapter.parse(&input, &ctx)?
        }
    };

    let warnings = staged.warnings.clone();
    let source_label = Some(format!("{source_kind} import"));

    let batch_id = stage_parsed(
        pool,
        &company_id,
        source_kind,
        source_label.as_deref(),
        staged,
    )
    .await?;

    // Persist the user-confirmed column map (the SAGA DBF mapping step) on the batch — the
    // import_batch.column_map column exists (migration 0056) and keeps the mapping for audit / re-parse.
    if let Some(ref map) = column_map {
        if let Ok(json) = serde_json::to_string(map) {
            sqlx::query("UPDATE import_batch SET column_map = ?1 WHERE id = ?2")
                .bind(json)
                .bind(&batch_id)
                .execute(pool)
                .await?;
        }
    }

    // Run resolver immediately after staging.
    resolve_batch(pool, &batch_id).await?;

    let counts = counts_for_batch(pool, &batch_id).await?;

    Ok(StageResult {
        batch_id,
        counts,
        warnings,
    })
}

async fn read_first_file(file_paths: &[String]) -> AppResult<Vec<u8>> {
    let path = file_paths
        .first()
        .ok_or_else(|| AppError::Validation("At least one file path is required".into()))?;
    tokio::fs::read(path)
        .await
        .map_err(|e| AppError::Other(format!("Cannot read file {path}: {e}")))
}

/// Return resolution counts + a sample of staging rows (up to 10 per entity).
#[tauri::command]
pub async fn import_wave_c_preview(
    state: State<'_, AppState>,
    batch_id: String,
) -> AppResult<PreviewResult> {
    let pool = &state.db;

    let counts = counts_for_batch(pool, &batch_id).await?;

    let sample_contacts = sample_rows(
        pool,
        "SELECT id, resolution, legal_name, cui_canonical, error \
         FROM import_staging_contact WHERE batch_id = ?1 LIMIT 10",
        &batch_id,
    )
    .await?;

    let sample_products = sample_rows(
        pool,
        "SELECT id, resolution, name, code, barcode, error \
         FROM import_staging_product WHERE batch_id = ?1 LIMIT 10",
        &batch_id,
    )
    .await?;

    let sample_accounts = sample_rows(
        pool,
        "SELECT id, resolution, account_code, account_name, error \
         FROM import_staging_account WHERE batch_id = ?1 LIMIT 10",
        &batch_id,
    )
    .await?;

    let sample_invoices = sample_rows(
        pool,
        "SELECT id, resolution, direction, series, issue_date, total_amount, error \
         FROM import_staging_invoice WHERE batch_id = ?1 LIMIT 10",
        &batch_id,
    )
    .await?;

    Ok(PreviewResult {
        batch_id,
        counts,
        sample_contacts,
        sample_products,
        sample_accounts,
        sample_invoices,
    })
}

async fn sample_rows(
    pool: &sqlx::SqlitePool,
    sql: &str,
    batch_id: &str,
) -> AppResult<Vec<serde_json::Value>> {
    use sqlx::{Column, Row};
    let rows = sqlx::query(sql).bind(batch_id).fetch_all(pool).await?;
    let mut result = Vec::new();
    for row in rows {
        let mut map = serde_json::Map::new();
        for (i, col) in row.columns().iter().enumerate() {
            let val: serde_json::Value = match row.try_get::<Option<String>, _>(i) {
                Ok(Some(s)) => serde_json::Value::String(s),
                Ok(None) => serde_json::Value::Null,
                Err(_) => serde_json::Value::Null,
            };
            map.insert(col.name().to_string(), val);
        }
        result.push(serde_json::Value::Object(map));
    }
    Ok(result)
}

/// Commit all NEW rows in the batch.  Returns a `CommitReport`.
#[tauri::command]
pub async fn import_wave_c_commit(
    state: State<'_, AppState>,
    batch_id: String,
    options: Option<CommitOptions>,
) -> AppResult<CommitReport> {
    let pool = &state.db;
    commit_batch(pool, &batch_id, options.unwrap_or_default()).await
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::import_wave_c::resolve::resolve_batch;
    use crate::commands::import_wave_c::{
        canonical_cui, SourceKind, StagedAccount, StagedContact, StagedData, StagedInvoice,
        StagedLine, StagedProduct,
    };

    /// Set up an in-memory pool with all migrations applied.
    async fn test_pool() -> sqlx::SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    /// Seed a minimal company row.
    async fn seed_company(pool: &sqlx::SqlitePool, id: &str, cui: &str) {
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES (?1, ?2, 'Test SRL', 'Str Test', 'Cluj', 'CJ', 'RO')",
        )
        .bind(id)
        .bind(cui)
        .execute(pool)
        .await
        .unwrap();
    }

    fn make_staged_contact(id: &str, cui: &str) -> StagedContact {
        StagedContact {
            id: id.to_string(),
            source: "TEST".into(),
            raw_json: format!(r#"{{"cui":"{}"}}"#, cui),
            source_code: None,
            contact_type: Some("CUSTOMER".into()),
            cui_raw: Some(cui.to_string()),
            cui_canonical: Some(canonical_cui(cui)),
            legal_name: Some(format!("Contact {}", id)),
            vat_payer: Some(false),
            is_individual: Some(false),
            address: None,
            city: None,
            county: None,
            country: Some("RO".into()),
            email: None,
            phone: None,
            dedup_key: None,
        }
    }

    fn make_staged_product(id: &str, name: &str, barcode: Option<&str>) -> StagedProduct {
        StagedProduct {
            id: id.to_string(),
            source: "TEST".into(),
            raw_json: format!(r#"{{"name":"{}"}}"#, name),
            source_code: None,
            name: Some(name.to_string()),
            unit: Some("buc".into()),
            unit_price: Some("100.00".into()),
            vat_rate: Some("21".into()),
            vat_category: Some("S".into()),
            code: Some(id.to_string()),
            barcode: barcode.map(|b| b.to_string()),
            stock_qty: None,
            is_service: Some(false),
            dedup_key: None,
        }
    }

    fn make_staged_invoice_issued(
        id: &str,
        series: &str,
        number: &str,
        partner_cui: &str,
    ) -> StagedInvoice {
        StagedInvoice {
            id: id.to_string(),
            source: "TEST".into(),
            raw_json: format!(r#"{{"id":"{}"}}"#, id),
            direction: "ISSUED".into(),
            external_id: None,
            partner_cui_canonical: Some(canonical_cui(partner_cui)),
            partner_name: Some("Test Partner SRL".into()),
            series: Some(series.to_string()),
            number: Some(number.to_string()),
            full_number: Some(format!("{series}-{number}")),
            issue_date: Some("2024-06-01".into()),
            due_date: Some("2024-07-01".into()),
            currency: Some("RON".into()),
            exchange_rate: None,
            reverse_charge: None,
            cash_vat: None,
            subtotal_amount: Some("1000.00".into()),
            vat_amount: Some("210.00".into()),
            total_amount: Some("1210.00".into()),
            dedup_key: None,
            lines: vec![StagedLine {
                id: format!("{id}-line1"),
                position: 1,
                name: Some("Servicii".into()),
                description: None,
                product_code: None,
                quantity: Some("1".into()),
                unit: Some("buc".into()),
                unit_price: Some("1000.00".into()),
                vat_rate: Some("21".into()),
                vat_category: Some("S".into()),
                subtotal_amount: Some("1000.00".into()),
                vat_amount: Some("210.00".into()),
                total_amount: Some("1210.00".into()),
                account_code: None,
                warehouse: None,
            }],
        }
    }

    fn make_staged_invoice_received(id: &str, number: &str, partner_cui: &str) -> StagedInvoice {
        StagedInvoice {
            id: id.to_string(),
            source: "TEST".into(),
            raw_json: format!(r#"{{"id":"{}","number":"{}"}}"#, id, number),
            direction: "RECEIVED".into(),
            external_id: Some(format!("ext-{}", id)),
            partner_cui_canonical: Some(canonical_cui(partner_cui)),
            partner_name: Some("Furnizor SRL".into()),
            series: Some("FF".into()),
            number: Some(number.to_string()),
            full_number: Some(format!("FF-{number}")),
            issue_date: Some("2024-06-01".into()),
            due_date: Some("2024-07-01".into()),
            currency: Some("RON".into()),
            exchange_rate: None,
            reverse_charge: None,
            cash_vat: None,
            subtotal_amount: Some("500.00".into()),
            vat_amount: Some("105.00".into()),
            total_amount: Some("605.00".into()),
            dedup_key: None,
            lines: vec![],
        }
    }

    // ── Test 1: Full round-trip ──────────────────────────────────────────────
    // stage_parsed → resolve_batch → commit_batch
    // Asserts contacts/products/accounts/issued(DRAFT)+received invoices were created.

    #[tokio::test]
    async fn test_full_roundtrip() {
        let pool = test_pool().await;
        // Company CUI must pass mod-11 — use a known-valid CUI.
        seed_company(&pool, "co1", "12345674").await;

        // Partner contact CUI (different from company). Use a known valid CUI.
        let partner_cui = "19"; // valid mod-11

        let staged = StagedData {
            contacts: vec![make_staged_contact("c1", partner_cui)],
            products: vec![make_staged_product("p1", "Produs A", Some("1234567890123"))],
            accounts: vec![StagedAccount {
                id: "a1".into(),
                source: "TEST".into(),
                raw_json: r#"{"code":"4111"}"#.into(),
                account_code: Some("4111".into()),
                synthetic_code: Some("4111".into()),
                analytic_suffix: None,
                account_name: Some("Furnizori".into()),
                account_class: Some(4),
                dedup_key: None,
            }],
            invoices: vec![make_staged_invoice_issued("inv1", "FACT", "1", partner_cui)],
            warnings: vec![],
        };

        let batch_id = stage_parsed(&pool, "co1", SourceKind::SagaXml, None, staged)
            .await
            .unwrap();

        let report = commit_batch(&pool, &batch_id, CommitOptions::default())
            .await
            .unwrap();

        assert_eq!(report.contacts.created, 1, "contact should be created");
        assert_eq!(report.products.created, 1, "product should be created");
        assert_eq!(report.accounts.created, 1, "account should be created");
        assert_eq!(report.invoices.created, 1, "invoice should be created");
        assert_eq!(report.contacts.errors, 0);
        assert_eq!(report.products.errors, 0);
        assert_eq!(report.invoices.errors, 0);

        // Verify contact exists in live table.
        let contact_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE company_id = 'co1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(contact_count, 1);

        // Verify issued invoice is DRAFT.
        let status: String =
            sqlx::query_scalar("SELECT status FROM invoices WHERE company_id = 'co1' LIMIT 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "DRAFT", "issued invoices must be DRAFT");
    }

    // ── Test 1b: issued invoice keeps its ORIGINAL number; counter not inflated ──
    #[tokio::test]
    async fn test_issued_invoice_preserves_original_number() {
        let pool = test_pool().await;
        seed_company(&pool, "co1", "12345674").await;
        let partner_cui = "19";
        let staged = StagedData {
            contacts: vec![make_staged_contact("c1", partner_cui)],
            products: vec![],
            accounts: vec![],
            invoices: vec![make_staged_invoice_issued(
                "inv1",
                "FACT",
                "1000",
                partner_cui,
            )],
            warnings: vec![],
        };
        let batch_id = stage_parsed(&pool, "co1", SourceKind::SagaXml, None, staged)
            .await
            .unwrap();
        let report = commit_batch(&pool, &batch_id, CommitOptions::default())
            .await
            .unwrap();
        assert_eq!(report.invoices.created, 1);

        // The ORIGINAL number 1000 is preserved (NOT reallocated to 1).
        let number: i64 = sqlx::query_scalar(
            "SELECT number FROM invoices WHERE company_id='co1' AND series='FACT' LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            number, 1000,
            "imported issued invoice must keep its original number"
        );

        // The live numbering series must NOT be inflated by a historical import.
        let last: i64 =
            sqlx::query_scalar("SELECT last_invoice_number FROM companies WHERE id='co1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            last, 0,
            "import must NOT touch companies.last_invoice_number"
        );
    }

    // ── Test 1c: re-importing the same ISSUED invoice is idempotent (no duplicate) ──
    #[tokio::test]
    async fn test_issued_invoice_idempotent_reimport() {
        let pool = test_pool().await;
        seed_company(&pool, "co1", "12345674").await;
        let partner_cui = "19";
        // Each batch needs FRESH staging-row ids (a real parse generates new UUIDs), but the SAME
        // series+number+partner CUI — that identity is what the dedup must catch on re-import.
        let make = |suffix: &str| StagedData {
            contacts: vec![make_staged_contact(&format!("c1-{suffix}"), partner_cui)],
            products: vec![],
            accounts: vec![],
            invoices: vec![make_staged_invoice_issued(
                &format!("inv1-{suffix}"),
                "FACT",
                "1000",
                partner_cui,
            )],
            warnings: vec![],
        };

        let b1 = stage_parsed(&pool, "co1", SourceKind::SagaXml, None, make("a"))
            .await
            .unwrap();
        commit_batch(&pool, &b1, CommitOptions::default())
            .await
            .unwrap();

        let b2 = stage_parsed(&pool, "co1", SourceKind::SagaXml, None, make("b"))
            .await
            .unwrap();
        let r2 = commit_batch(&pool, &b2, CommitOptions::default())
            .await
            .unwrap();

        // Exactly ONE FACT-1000 invoice exists after the second import.
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM invoices WHERE company_id='co1' AND series='FACT' AND number=1000",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            count, 1,
            "re-import must not create a duplicate issued invoice"
        );
        assert_eq!(
            r2.invoices.created, 0,
            "second import creates no new invoice"
        );
        assert_eq!(
            r2.invoices.matched, 1,
            "second import matches the existing invoice"
        );
    }

    // ── Test 2: Dedup MATCH ──────────────────────────────────────────────────
    // Pre-seed an existing contact+product; stage same canonical CUI/barcode →
    // resolution == MATCH, no duplicate created.

    #[tokio::test]
    async fn test_dedup_match_no_duplicate() {
        let pool = test_pool().await;
        seed_company(&pool, "co1", "12345674").await;

        let partner_cui = "19";

        // Pre-seed contact.
        sqlx::query(
            "INSERT INTO contacts \
             (id, company_id, contact_type, cui, legal_name, vat_payer, is_individual, cash_vat, \
              country, created_at, updated_at) \
             VALUES ('existing-c', 'co1', 'CUSTOMER', ?1, 'Pre-seeded', 0, 0, 0, 'RO', 1, 1)",
        )
        .bind(partner_cui)
        .execute(&pool)
        .await
        .unwrap();

        // Pre-seed product.
        sqlx::query(
            "INSERT INTO products \
             (id, company_id, name, unit, unit_price, vat_rate, vat_category, barcode, active, created_at, updated_at) \
             VALUES ('existing-p', 'co1', 'Produs A', 'buc', '100.00', '21', 'S', '1234567890123', 1, 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let staged = StagedData {
            contacts: vec![make_staged_contact("c-new", partner_cui)],
            products: vec![make_staged_product(
                "p-new",
                "Produs A",
                Some("1234567890123"),
            )],
            accounts: vec![],
            invoices: vec![],
            warnings: vec![],
        };

        let batch_id = stage_parsed(&pool, "co1", SourceKind::SagaXml, None, staged)
            .await
            .unwrap();

        let report = commit_batch(&pool, &batch_id, CommitOptions::default())
            .await
            .unwrap();

        assert_eq!(
            report.contacts.created, 0,
            "should not create duplicate contact"
        );
        assert_eq!(report.contacts.matched, 1, "should MATCH existing contact");
        assert_eq!(
            report.products.created, 0,
            "should not create duplicate product"
        );
        assert_eq!(report.products.matched, 1, "should MATCH existing product");

        // No duplicate in live tables.
        let contact_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE company_id = 'co1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(contact_count, 1, "still only one contact in live table");
    }

    // ── Test 3: Idempotency ──────────────────────────────────────────────────
    // Commit the same source rows twice (two batches) → second yields MATCH/skip.

    #[tokio::test]
    async fn test_idempotency_second_batch_matches() {
        let pool = test_pool().await;
        seed_company(&pool, "co1", "12345674").await;

        let partner_cui = "19";

        // Use fresh UUIDs each call so stage_parsed can insert into two different batches
        // without hitting the UNIQUE constraint on import_staging_*.id.
        // The raw_json for the received invoice MUST be stable (deterministic) across both
        // calls so create_imported derives the same SHA256 anaf_download_id for both —
        // that's what gives us idempotency.
        let stable_raw_json = r#"{"source":"test","number":"100"}"#.to_string();
        let make_staged = move || {
            let c_id = new_id();
            let i_id = new_id();
            let mut inv = make_staged_invoice_received(&i_id, "100", partner_cui);
            inv.raw_json = stable_raw_json.clone();
            StagedData {
                contacts: vec![make_staged_contact(&c_id, partner_cui)],
                products: vec![],
                accounts: vec![],
                invoices: vec![inv],
                warnings: vec![],
            }
        };

        // First batch.
        let batch1 = stage_parsed(&pool, "co1", SourceKind::SagaXml, None, make_staged())
            .await
            .unwrap();
        let r1 = commit_batch(&pool, &batch1, CommitOptions::default())
            .await
            .unwrap();
        assert_eq!(r1.contacts.created, 1);
        assert_eq!(r1.invoices.created, 1);

        // Second batch — same data.
        let batch2 = stage_parsed(&pool, "co1", SourceKind::SagaXml, None, make_staged())
            .await
            .unwrap();
        let r2 = commit_batch(&pool, &batch2, CommitOptions::default())
            .await
            .unwrap();

        // Contact is MATCH (exists), invoice is MATCH (anaf_download_id or RECEIVED match).
        assert_eq!(r2.contacts.created, 0, "no new contact on second run");
        assert_eq!(r2.contacts.matched, 1);
        // Invoice may be MATCH or idempotently handled.
        assert_eq!(r2.invoices.created, 0, "no second received invoice");

        let recv_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM received_invoices WHERE company_id = 'co1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(recv_count, 1, "exactly one received invoice");
    }

    // ── Test 4: GL safety ────────────────────────────────────────────────────
    // After commit: zero rows in gl_journal/gl_entry, issued invoices are DRAFT.

    #[tokio::test]
    async fn test_gl_safety_no_entries_posted() {
        let pool = test_pool().await;
        seed_company(&pool, "co1", "12345674").await;

        let partner_cui = "19";
        let staged = StagedData {
            contacts: vec![make_staged_contact("c1", partner_cui)],
            products: vec![],
            accounts: vec![],
            invoices: vec![make_staged_invoice_issued("inv1", "FACT", "1", partner_cui)],
            warnings: vec![],
        };

        let batch_id = stage_parsed(&pool, "co1", SourceKind::SagaXml, None, staged)
            .await
            .unwrap();
        commit_batch(&pool, &batch_id, CommitOptions::default())
            .await
            .unwrap();

        // gl_journal must be empty.
        let gl_journal_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM gl_journal")
            .fetch_one(&pool)
            .await
            .unwrap_or(0);
        assert_eq!(gl_journal_count, 0, "importer must NOT post GL entries");

        // Issued invoice must be DRAFT.
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM invoices WHERE company_id = 'co1' LIMIT 1")
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert_eq!(
            status.as_deref(),
            Some("DRAFT"),
            "imported issued invoice must be DRAFT"
        );
    }

    // ── Test 5: REVIEW routing for invalid CUI ───────────────────────────────

    #[tokio::test]
    async fn test_review_routing_invalid_cui() {
        let pool = test_pool().await;
        seed_company(&pool, "co1", "12345674").await;

        // "99999999" does NOT pass mod-11.
        let invalid_cui = "99999999";

        let staged = StagedData {
            contacts: vec![make_staged_contact("c-bad", invalid_cui)],
            products: vec![],
            accounts: vec![],
            invoices: vec![],
            warnings: vec![],
        };

        let batch_id = stage_parsed(&pool, "co1", SourceKind::SagaXml, None, staged)
            .await
            .unwrap();

        // Only resolve — don't commit.
        resolve_batch(&pool, &batch_id).await.unwrap();

        let resolution: String = sqlx::query_scalar(
            "SELECT resolution FROM import_staging_contact WHERE batch_id = ?1 LIMIT 1",
        )
        .bind(&batch_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(resolution, "REVIEW", "invalid CUI must be REVIEW");

        // Commit — REVIEW rows must NOT be created.
        let report = commit_batch(&pool, &batch_id, CommitOptions::default())
            .await
            .unwrap();
        assert_eq!(report.contacts.created, 0);
        assert_eq!(report.contacts.skipped, 1);

        let contact_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE company_id = 'co1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            contact_count, 0,
            "no contact row must be created for REVIEW"
        );
    }

    // ── Test 6: Leading-zero CUI (W1 watch-item) ─────────────────────────────
    // Pre-seed contact with CUI "19"; stage with "019" and "RO019"
    // → resolves MATCH (both sides canonicalized).

    #[tokio::test]
    async fn test_leading_zero_cui_matches_existing() {
        let pool = test_pool().await;
        seed_company(&pool, "co1", "12345674").await;

        let canonical = "19"; // valid mod-11

        // Pre-seed with the bare numeric CUI (no leading zero).
        sqlx::query(
            "INSERT INTO contacts \
             (id, company_id, contact_type, cui, legal_name, vat_payer, is_individual, cash_vat, \
              country, created_at, updated_at) \
             VALUES ('existing', 'co1', 'CUSTOMER', ?1, 'Existing', 0, 0, 0, 'RO', 1, 1)",
        )
        .bind(canonical)
        .execute(&pool)
        .await
        .unwrap();

        // Stage with leading zero and RO prefix variants.
        let staged_with_zero = make_staged_contact("c-zero", &format!("0{canonical}"));
        let staged_with_ro = make_staged_contact("c-ro", &format!("RO0{canonical}"));

        // Two separate batches (to avoid DUP_IN_BATCH between them — they'd dup each other).
        {
            let staged = StagedData {
                contacts: vec![staged_with_zero],
                products: vec![],
                accounts: vec![],
                invoices: vec![],
                warnings: vec![],
            };
            let batch_id = stage_parsed(&pool, "co1", SourceKind::SagaXml, None, staged)
                .await
                .unwrap();
            resolve_batch(&pool, &batch_id).await.unwrap();

            let resolution: String = sqlx::query_scalar(
                "SELECT resolution FROM import_staging_contact WHERE batch_id = ?1 LIMIT 1",
            )
            .bind(&batch_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(
                resolution, "MATCH",
                "leading-zero CUI should MATCH existing contact"
            );
        }

        {
            let staged = StagedData {
                contacts: vec![staged_with_ro],
                products: vec![],
                accounts: vec![],
                invoices: vec![],
                warnings: vec![],
            };
            let batch_id = stage_parsed(&pool, "co1", SourceKind::SagaXml, None, staged)
                .await
                .unwrap();
            resolve_batch(&pool, &batch_id).await.unwrap();

            let resolution: String = sqlx::query_scalar(
                "SELECT resolution FROM import_staging_contact WHERE batch_id = ?1 LIMIT 1",
            )
            .bind(&batch_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(
                resolution, "MATCH",
                "RO-prefixed leading-zero CUI should MATCH existing contact"
            );
        }

        // Confirm still only one contact in live table.
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE company_id = 'co1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "no duplicate contact created");
    }
}
