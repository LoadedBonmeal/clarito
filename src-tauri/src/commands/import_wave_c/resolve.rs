//! Wave C W4 — dedup resolver.
// sqlx query_as returns complex tuple types for multi-column rows.
#![allow(clippy::type_complexity)]
//!
//! `resolve_batch` runs AFTER staging rows are inserted (by `commit::stage_parsed`) and BEFORE
//! `commit_batch` writes live tables.  It computes `dedup_key`, `matched_id`, and `resolution`
//! for every staging row and writes them back.
//!
//! Resolution values (matching the DDL in migration 0056):
//!   NEW           – no live match found; safe to create.
//!   MATCH         – found an existing live row; skip creation and reuse its id.
//!   DUP_IN_BATCH  – a duplicate within THIS batch; second occurrence is skipped.
//!   REVIEW        – validation failed (bad CUI, self-CUI, analytic suffix) or ambiguous.
//!   ERROR         – unexpected DB / logic failure on this specific row.

use sqlx::SqlitePool;

use crate::db::companies;
use crate::error::{AppError, AppResult};

use super::canonical_cui;

// ─── Public entry point ──────────────────────────────────────────────────────

/// Compute and persist dedup resolution for every staging row in `batch_id`.
/// Called by `commit_batch` (and may be called independently for preview).
pub async fn resolve_batch(pool: &SqlitePool, batch_id: &str) -> AppResult<()> {
    // Fetch the company_id from the batch row — needed for live-table lookups and the
    // self-CUI guard.
    let company_id: String =
        sqlx::query_scalar("SELECT company_id FROM import_batch WHERE id = ?1")
            .bind(batch_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound)?;

    // Fetch the company's own CUI (for the self-CUI guard on contacts).
    let company = companies::get(pool, &company_id).await?;
    let company_cui_canonical = canonical_cui(&company.cui);

    resolve_contacts(pool, batch_id, &company_id, &company_cui_canonical).await?;
    resolve_products(pool, batch_id, &company_id).await?;
    resolve_accounts(pool, batch_id, &company_id).await?;
    resolve_invoices(pool, batch_id, &company_id).await?;

    Ok(())
}

// ─── Counts rollup ──────────────────────────────────────────────────────────

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct EntityCounts {
    pub new: u32,
    pub matched: u32,
    pub dup_in_batch: u32,
    pub review: u32,
    pub error: u32,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct BatchCounts {
    pub contacts: EntityCounts,
    pub products: EntityCounts,
    pub accounts: EntityCounts,
    pub invoices: EntityCounts,
}

/// Read the resolved resolutions for `batch_id` and compute counts per entity.
/// Call AFTER `resolve_batch` so the resolution column is populated.
pub async fn counts_for_batch(pool: &SqlitePool, batch_id: &str) -> AppResult<BatchCounts> {
    async fn entity_counts(
        pool: &SqlitePool,
        table: &str,
        batch_id: &str,
    ) -> AppResult<EntityCounts> {
        // We can't parameterise table names in SQLite via bind, so we use format! here.
        // The `table` argument is always a hard-coded string literal in this file — no injection risk.
        let rows: Vec<(String, i64)> = sqlx::query_as(&format!(
            "SELECT resolution, COUNT(*) FROM {table} \
             WHERE batch_id = ?1 GROUP BY resolution"
        ))
        .bind(batch_id)
        .fetch_all(pool)
        .await?;
        let mut ec = EntityCounts::default();
        for (res, count) in rows {
            let c = count as u32;
            match res.as_str() {
                "NEW" => ec.new += c,
                "MATCH" => ec.matched += c,
                "DUP_IN_BATCH" => ec.dup_in_batch += c,
                "REVIEW" => ec.review += c,
                "ERROR" => ec.error += c,
                _ => {}
            }
        }
        Ok(ec)
    }

    Ok(BatchCounts {
        contacts: entity_counts(pool, "import_staging_contact", batch_id).await?,
        products: entity_counts(pool, "import_staging_product", batch_id).await?,
        accounts: entity_counts(pool, "import_staging_account", batch_id).await?,
        invoices: entity_counts(pool, "import_staging_invoice", batch_id).await?,
    })
}

// ─── Contacts ────────────────────────────────────────────────────────────────

async fn resolve_contacts(
    pool: &SqlitePool,
    batch_id: &str,
    company_id: &str,
    company_cui_canonical: &str,
) -> AppResult<()> {
    // Fetch all staged contacts for this batch.
    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT id, cui_canonical FROM import_staging_contact WHERE batch_id = ?1")
            .bind(batch_id)
            .fetch_all(pool)
            .await?;

    // Track CUI keys seen in this batch to detect DUP_IN_BATCH.
    let mut seen_in_batch: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for (staging_id, cui_canonical_opt) in rows {
        // Compute dedup_key: canonical CUI (already computed by adapter, just normalise again).
        let raw_canonical = cui_canonical_opt.unwrap_or_default();
        let dedup_key = canonical_cui(&raw_canonical);

        // 1. Missing or empty CUI → REVIEW (cannot safely dedup without CUI).
        if dedup_key.is_empty() {
            set_contact_resolution(pool, &staging_id, "REVIEW", &dedup_key, None, None).await?;
            continue;
        }

        // 2. Validate with mod-11 — failing CUI → REVIEW.
        if companies::validate_cui(&dedup_key).is_err() {
            set_contact_resolution(
                pool,
                &staging_id,
                "REVIEW",
                &dedup_key,
                None,
                Some("CUI fails mod-11 validation"),
            )
            .await?;
            continue;
        }

        // 3. Self-CUI guard — staged partner whose canonical CUI == company's own → REVIEW.
        if dedup_key == company_cui_canonical {
            set_contact_resolution(
                pool,
                &staging_id,
                "REVIEW",
                &dedup_key,
                None,
                Some("CUI matches company's own CUI (self-invoicing)"),
            )
            .await?;
            continue;
        }

        // 4. DUP_IN_BATCH — a previous row in this batch already has the same canonical CUI.
        if let Some(first_id) = seen_in_batch.get(&dedup_key) {
            // Mark the duplicate; if the first one is still NEW, it keeps NEW.
            // We set the dup's matched_id to the first staging row's id for traceability.
            set_contact_resolution(
                pool,
                &staging_id,
                "DUP_IN_BATCH",
                &dedup_key,
                Some(first_id.as_str()),
                None,
            )
            .await?;
            continue;
        }
        seen_in_batch.insert(dedup_key.clone(), staging_id.clone());

        // 5. Check live contacts table.  CRITICAL: canonicalize BOTH sides.
        // The live column `contacts.cui` is stored as the trimmed numeric string (strip-RO,
        // no leading-zero strip) — so "RO123" → stored as "123".  We apply canonical_cui to
        // live values too so "0123" == "123" (leading-zero CUIs).
        //
        // The live table may contain:  "123"  "RO123"  "0000123"
        // We fetch all CUIs for this company and canonicalize them in Rust to ensure parity.
        let live_contacts: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, cui FROM contacts WHERE company_id = ?1 AND cui IS NOT NULL",
        )
        .bind(company_id)
        .fetch_all(pool)
        .await?;

        let mut matched_id: Option<String> = None;
        for (live_id, live_cui) in live_contacts {
            if let Some(ref live_raw) = live_cui {
                if canonical_cui(live_raw) == dedup_key {
                    matched_id = Some(live_id);
                    break;
                }
            }
        }

        if let Some(mid) = matched_id {
            set_contact_resolution(pool, &staging_id, "MATCH", &dedup_key, Some(&mid), None)
                .await?;
        } else {
            set_contact_resolution(pool, &staging_id, "NEW", &dedup_key, None, None).await?;
        }
    }

    Ok(())
}

async fn set_contact_resolution(
    pool: &SqlitePool,
    staging_id: &str,
    resolution: &str,
    dedup_key: &str,
    matched_id: Option<&str>,
    error: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE import_staging_contact \
         SET dedup_key = ?2, matched_id = ?3, resolution = ?4, error = ?5 \
         WHERE id = ?1",
    )
    .bind(staging_id)
    .bind(dedup_key)
    .bind(matched_id)
    .bind(resolution)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

// ─── Products ────────────────────────────────────────────────────────────────

async fn resolve_products(pool: &SqlitePool, batch_id: &str, company_id: &str) -> AppResult<()> {
    let rows: Vec<(
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, barcode, code, name, unit FROM import_staging_product WHERE batch_id = ?1",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;

    // DUP_IN_BATCH tracking: prefer barcode > code > name+unit
    let mut seen_barcode: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut seen_code: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut seen_name_unit: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for (staging_id, barcode, code, name, unit) in rows {
        // Compute dedup_key in priority order.
        let (dedup_key, dedup_kind) = if let Some(ref bc) = barcode {
            let k = bc.trim().to_string();
            if !k.is_empty() {
                (k, "barcode")
            } else {
                dedup_key_from_code_or_name(&code, &name, &unit)
            }
        } else {
            dedup_key_from_code_or_name(&code, &name, &unit)
        };

        if dedup_key.is_empty() {
            // Cannot dedup without any key — REVIEW.
            set_product_resolution(
                pool,
                &staging_id,
                "REVIEW",
                &dedup_key,
                None,
                Some("No barcode, code, or name available for dedup"),
            )
            .await?;
            continue;
        }

        // DUP_IN_BATCH check.
        let dup_map = match dedup_kind {
            "barcode" => &mut seen_barcode,
            "code" => &mut seen_code,
            _ => &mut seen_name_unit,
        };
        if let Some(first_id) = dup_map.get(&dedup_key) {
            set_product_resolution(
                pool,
                &staging_id,
                "DUP_IN_BATCH",
                &dedup_key,
                Some(first_id.as_str()),
                None,
            )
            .await?;
            continue;
        }
        dup_map.insert(dedup_key.clone(), staging_id.clone());

        // Live table lookup.
        let matched_id = match dedup_kind {
            "barcode" => {
                sqlx::query_scalar::<_, String>(
                    "SELECT id FROM products \
                     WHERE company_id = ?1 AND barcode = ?2 LIMIT 1",
                )
                .bind(company_id)
                .bind(&dedup_key)
                .fetch_optional(pool)
                .await?
            }
            "code" => {
                sqlx::query_scalar::<_, String>(
                    "SELECT id FROM products \
                     WHERE company_id = ?1 AND code = ?2 LIMIT 1",
                )
                .bind(company_id)
                .bind(&dedup_key)
                .fetch_optional(pool)
                .await?
            }
            _ => {
                // name+unit composite key: "NAME\0UNIT"
                let parts: Vec<&str> = dedup_key.splitn(2, '\0').collect();
                if parts.len() == 2 {
                    sqlx::query_scalar::<_, String>(
                        "SELECT id FROM products \
                         WHERE company_id = ?1 AND name = ?2 AND unit = ?3 LIMIT 1",
                    )
                    .bind(company_id)
                    .bind(parts[0])
                    .bind(parts[1])
                    .fetch_optional(pool)
                    .await?
                } else {
                    None
                }
            }
        };

        if let Some(mid) = matched_id {
            set_product_resolution(pool, &staging_id, "MATCH", &dedup_key, Some(&mid), None)
                .await?;
        } else {
            set_product_resolution(pool, &staging_id, "NEW", &dedup_key, None, None).await?;
        }
    }

    Ok(())
}

fn dedup_key_from_code_or_name(
    code: &Option<String>,
    name: &Option<String>,
    unit: &Option<String>,
) -> (String, &'static str) {
    if let Some(ref c) = code {
        let k = c.trim().to_string();
        if !k.is_empty() {
            return (k, "code");
        }
    }
    let n = name.as_deref().unwrap_or("").trim().to_string();
    let u = unit.as_deref().unwrap_or("").trim().to_string();
    if !n.is_empty() {
        (format!("{}\0{}", n, u), "name_unit")
    } else {
        (String::new(), "none")
    }
}

async fn set_product_resolution(
    pool: &SqlitePool,
    staging_id: &str,
    resolution: &str,
    dedup_key: &str,
    matched_id: Option<&str>,
    error: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE import_staging_product \
         SET dedup_key = ?2, matched_id = ?3, resolution = ?4, error = ?5 \
         WHERE id = ?1",
    )
    .bind(staging_id)
    .bind(dedup_key)
    .bind(matched_id)
    .bind(resolution)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

// ─── Accounts ────────────────────────────────────────────────────────────────

async fn resolve_accounts(pool: &SqlitePool, batch_id: &str, company_id: &str) -> AppResult<()> {
    let rows: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, account_code, analytic_suffix \
         FROM import_staging_account WHERE batch_id = ?1",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;

    let mut seen_code: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for (staging_id, account_code, analytic_suffix) in rows {
        let code = account_code.as_deref().unwrap_or("").trim().to_string();

        if code.is_empty() {
            set_account_resolution(
                pool,
                &staging_id,
                "REVIEW",
                &code,
                None,
                Some("account_code is empty"),
            )
            .await?;
            continue;
        }

        // An analytic suffix that is not empty and NOT a seeded account → REVIEW.
        // The logic: if the full code matches a seeded standard account, it's fine.
        // If it has an analytic suffix and we can't find the exact code in live, we REVIEW
        // rather than auto-create entity-specific analytics.
        let has_analytic = analytic_suffix
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

        // DUP_IN_BATCH.
        if let Some(first_id) = seen_code.get(&code) {
            set_account_resolution(
                pool,
                &staging_id,
                "DUP_IN_BATCH",
                &code,
                Some(first_id.as_str()),
                None,
            )
            .await?;
            continue;
        }
        seen_code.insert(code.clone(), staging_id.clone());

        // Live lookup — exact match on (company_id, account_code).
        let live_id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM chart_of_accounts \
             WHERE company_id = ?1 AND account_code = ?2 LIMIT 1",
        )
        .bind(company_id)
        .bind(&code)
        .fetch_optional(pool)
        .await?;

        if let Some(mid) = live_id {
            set_account_resolution(pool, &staging_id, "MATCH", &code, Some(&mid), None).await?;
        } else if has_analytic {
            // Non-matched analytic code → REVIEW (do not auto-create entity analytics).
            set_account_resolution(
                pool,
                &staging_id,
                "REVIEW",
                &code,
                None,
                Some("Analytic suffix account not in standard chart — manual review required"),
            )
            .await?;
        } else {
            set_account_resolution(pool, &staging_id, "NEW", &code, None, None).await?;
        }
    }

    Ok(())
}

async fn set_account_resolution(
    pool: &SqlitePool,
    staging_id: &str,
    resolution: &str,
    dedup_key: &str,
    matched_id: Option<&str>,
    error: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE import_staging_account \
         SET dedup_key = ?2, matched_id = ?3, resolution = ?4, error = ?5 \
         WHERE id = ?1",
    )
    .bind(staging_id)
    .bind(dedup_key)
    .bind(matched_id)
    .bind(resolution)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

// ─── Invoices ────────────────────────────────────────────────────────────────

async fn resolve_invoices(pool: &SqlitePool, batch_id: &str, company_id: &str) -> AppResult<()> {
    let rows: Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String, // raw_json — used to compute the import-dedup-id for RECEIVED
    )> = sqlx::query_as(
        "SELECT id, direction, external_id, partner_cui_canonical, series, number, issue_date, raw_json \
         FROM import_staging_invoice WHERE batch_id = ?1",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;

    let mut seen_in_batch: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for (staging_id, direction, external_id, partner_cui, series, number, issue_date, raw_json) in
        rows
    {
        // Compute dedup_key.
        let dedup_key = compute_invoice_dedup_key(
            &external_id,
            &direction,
            &partner_cui,
            &series,
            &number,
            &issue_date,
        );

        // DUP_IN_BATCH.
        if let Some(first_id) = seen_in_batch.get(&dedup_key) {
            set_invoice_resolution(
                pool,
                &staging_id,
                "DUP_IN_BATCH",
                &dedup_key,
                Some(first_id.as_str()),
                None,
            )
            .await?;
            continue;
        }
        if !dedup_key.is_empty() {
            seen_in_batch.insert(dedup_key.clone(), staging_id.clone());
        }

        // Live lookup depends on direction.
        let matched_id = find_live_invoice(
            pool,
            company_id,
            &direction,
            &external_id,
            &series,
            &number,
            &raw_json,
        )
        .await?;

        if let Some(mid) = matched_id {
            set_invoice_resolution(pool, &staging_id, "MATCH", &dedup_key, Some(&mid), None)
                .await?;
            continue;
        }

        // Resolve the partner.  Prefer partner_matched_id (existing contact) over
        // partner_staging_id (a contact being created in this batch).
        // The partner resolution is used at commit time — here we just ensure the FK chain
        // is set.  If neither exists yet, we leave it for commit to handle (it will look up by
        // staging id or create a new one).
        resolve_invoice_partner(pool, batch_id, &staging_id, &partner_cui, company_id).await?;

        set_invoice_resolution(pool, &staging_id, "NEW", &dedup_key, None, None).await?;
    }

    Ok(())
}

fn compute_invoice_dedup_key(
    external_id: &Option<String>,
    direction: &str,
    partner_cui: &Option<String>,
    series: &Option<String>,
    number: &Option<String>,
    issue_date: &Option<String>,
) -> String {
    if let Some(ref eid) = external_id {
        let k = eid.trim().to_string();
        if !k.is_empty() {
            return format!("ext:{}", k);
        }
    }
    // Composite key.
    let dir = direction.trim();
    let cui = partner_cui
        .as_deref()
        .map(canonical_cui)
        .unwrap_or_default();
    let ser = series.as_deref().unwrap_or("").trim().to_string();
    let num = number.as_deref().unwrap_or("").trim().to_string();
    let date = issue_date.as_deref().unwrap_or("").trim().to_string();
    format!("{}|{}|{}|{}|{}", dir, cui, ser, num, date)
}

async fn find_live_invoice(
    pool: &SqlitePool,
    company_id: &str,
    direction: &str,
    external_id: &Option<String>,
    series: &Option<String>,
    number: &Option<String>,
    raw_json: &str,
) -> AppResult<Option<String>> {
    match direction.to_uppercase().as_str() {
        "ISSUED" => {
            // Match by series + numeric number in the invoices table. Parse the staged number the SAME
            // way the committer does (digits → i64) so the preview agrees with what create_imported
            // stores (invoices.number is INTEGER); a non-numeric staged number can't match a live row.
            if let (Some(ref ser), Some(num_i64)) = (
                series,
                number
                    .as_deref()
                    .and_then(super::commit::parse_staged_number),
            ) {
                let matched: Option<String> = sqlx::query_scalar(
                    "SELECT id FROM invoices \
                     WHERE company_id = ?1 AND series = ?2 AND number = ?3 LIMIT 1",
                )
                .bind(company_id)
                .bind(ser.trim())
                .bind(num_i64)
                .fetch_optional(pool)
                .await?;
                if matched.is_some() {
                    return Ok(matched);
                }
            }
        }
        "RECEIVED" => {
            // Match by anaf_download_id (external_id used as import dedup key).
            if let Some(ref eid) = external_id {
                let eid_trimmed = eid.trim();
                if !eid_trimmed.is_empty() {
                    let matched: Option<String> = sqlx::query_scalar(
                        "SELECT id FROM received_invoices \
                         WHERE company_id = ?1 AND anaf_download_id = ?2 LIMIT 1",
                    )
                    .bind(company_id)
                    .bind(eid_trimmed)
                    .fetch_optional(pool)
                    .await?;
                    if matched.is_some() {
                        return Ok(matched);
                    }
                }
            }

            // Also try the import-prefixed SHA256 key — this is the same dedup key that
            // db::received::create_imported uses, so re-importing the same source row
            // can be detected at resolve time and marked MATCH (not NEW+idempotent).
            {
                use sha2::{Digest, Sha256};
                let hash_hex = {
                    let mut h = Sha256::new();
                    h.update(raw_json.as_bytes());
                    format!("{:x}", h.finalize())
                };
                let import_key = format!("import-{}", &hash_hex[..32]);
                let matched: Option<String> = sqlx::query_scalar(
                    "SELECT id FROM received_invoices \
                     WHERE company_id = ?1 AND anaf_download_id = ?2 LIMIT 1",
                )
                .bind(company_id)
                .bind(&import_key)
                .fetch_optional(pool)
                .await?;
                if matched.is_some() {
                    return Ok(matched);
                }
            }

            // Also match by number (invoice number stored in `number` column of received_invoices).
            if let Some(ref num) = number {
                let matched: Option<String> = sqlx::query_scalar(
                    "SELECT id FROM received_invoices \
                     WHERE company_id = ?1 AND number = ?2 LIMIT 1",
                )
                .bind(company_id)
                .bind(num.trim())
                .fetch_optional(pool)
                .await?;
                if matched.is_some() {
                    return Ok(matched);
                }
            }
        }
        _ => {}
    }
    Ok(None)
}

/// Fill `partner_matched_id` or `partner_staging_id` on the invoice staging row.
/// Preference: existing live contact (MATCH) > staged contact in same batch (NEW).
async fn resolve_invoice_partner(
    pool: &SqlitePool,
    batch_id: &str,
    invoice_staging_id: &str,
    partner_cui: &Option<String>,
    company_id: &str,
) -> AppResult<()> {
    let cui_key = match partner_cui {
        Some(ref c) => {
            let k = canonical_cui(c);
            if k.is_empty() {
                return Ok(()); // No CUI → leave partner unresolved.
            }
            k
        }
        None => return Ok(()),
    };

    // 1. Live contact with this canonical CUI.
    let live_contacts: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT id, cui FROM contacts WHERE company_id = ?1 AND cui IS NOT NULL")
            .bind(company_id)
            .fetch_all(pool)
            .await?;

    for (live_id, live_cui) in &live_contacts {
        if let Some(ref lc) = live_cui {
            if canonical_cui(lc) == cui_key {
                sqlx::query(
                    "UPDATE import_staging_invoice \
                     SET partner_matched_id = ?2 WHERE id = ?1",
                )
                .bind(invoice_staging_id)
                .bind(live_id)
                .execute(pool)
                .await?;
                return Ok(());
            }
        }
    }

    // 2. Staged contact in this batch that will be NEW.
    let staged_contact: Option<String> = sqlx::query_scalar(
        "SELECT id FROM import_staging_contact \
         WHERE batch_id = ?1 AND cui_canonical = ?2 AND resolution = 'NEW' LIMIT 1",
    )
    .bind(batch_id)
    .bind(&cui_key)
    .fetch_optional(pool)
    .await?;

    if let Some(sc_id) = staged_contact {
        sqlx::query(
            "UPDATE import_staging_invoice \
             SET partner_staging_id = ?2 WHERE id = ?1",
        )
        .bind(invoice_staging_id)
        .bind(&sc_id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn set_invoice_resolution(
    pool: &SqlitePool,
    staging_id: &str,
    resolution: &str,
    dedup_key: &str,
    matched_id: Option<&str>,
    error: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE import_staging_invoice \
         SET dedup_key = ?2, matched_id = ?3, resolution = ?4, error = ?5 \
         WHERE id = ?1",
    )
    .bind(staging_id)
    .bind(dedup_key)
    .bind(matched_id)
    .bind(resolution)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}
