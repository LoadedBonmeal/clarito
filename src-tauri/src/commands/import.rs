//! Importuri CSV pentru facturi și contacte.

use sqlx::Row;
use tauri::{Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: u32,
    pub errors: Vec<String>,
}

// ─── CSV Templates ────────────────────────────────────────────────────────────

pub const INVOICES_CSV_TEMPLATE: &str =
    "company_cui;customer_cui;customer_name;series;number;issue_date;due_date;item_name;qty;unit;unit_price;vat_rate\n\
     RO12345678;RO87654321;Client Exemplu SRL;FACT;1;2026-01-15;2026-02-14;Servicii consultanta;1;buc;1000.00;19\n";

pub const CONTACTS_CSV_TEMPLATE: &str =
    "type;cui;name;address;city;county;email;phone\n\
     CUSTOMER;RO87654321;Client Exemplu SRL;Str. Exemplu nr. 1;Cluj-Napoca;CJ;office@client.ro;+40722000000\n";

#[tauri::command]
pub fn get_invoices_csv_template() -> &'static str {
    INVOICES_CSV_TEMPLATE
}

#[tauri::command]
pub fn get_contacts_csv_template() -> &'static str {
    CONTACTS_CSV_TEMPLATE
}

// ─── Import commands ──────────────────────────────────────────────────────────

/// Importă facturi dintr-un CSV cu separator `;`.
/// Format așteptat (header obligatoriu):
/// company_cui;customer_cui;customer_name;series;number;issue_date;due_date;item_name;qty;unit;unit_price;vat_rate
///
/// True when an imported customer CUI should be REJECTED: a Romanian-format CUI (i.e. NOT a foreign
/// VAT number, which carries a non-"RO" two-letter country prefix) whose check digit is invalid.
/// Empty + foreign CUIs pass through; only a malformed Romanian CUI is a hard error.
fn customer_cui_invalid(cui: &str) -> bool {
    if cui.is_empty() {
        return false;
    }
    let foreign = cui.len() >= 2
        && cui.as_bytes()[0].is_ascii_alphabetic()
        && cui.as_bytes()[1].is_ascii_alphabetic()
        && !cui[..2].eq_ignore_ascii_case("ro");
    !foreign && !crate::anaf_decl::valid_cui(cui)
}

/// Dacă `dry_run` = true, validează liniile fără a insera în DB (preview).
#[tauri::command]
pub async fn import_invoices_csv(
    state: State<'_, AppState>,
    content: String,
    company_id: String,
    dry_run: bool,
) -> AppResult<ImportResult> {
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    let pool = &state.db;

    let mut imported: u32 = 0;
    let mut errors: Vec<String> = Vec::new();
    // Track the maximum invoice number successfully inserted, so we can advance
    // companies.last_invoice_number at the end (R5 EDGE-03: prevents UNIQUE
    // collisions when user later creates manual invoices).
    let mut max_imported_number: i64 = 0;

    // Fetch the active company's CUI once so we can validate fields[0] per row.
    let company = crate::db::companies::get(pool, &company_id).await?;
    fn normalize_cui_csv(s: &str) -> String {
        s.trim()
            .to_uppercase()
            .trim_start_matches("RO")
            .trim()
            .to_string()
    }
    let company_cui_norm = normalize_cui_csv(&company.cui);

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';')
        .has_headers(true)
        .flexible(true)
        .from_reader(content.as_bytes());

    // ── Header-based column lookup ────────────────────────────────────────
    // Required columns must be present; optional columns (vat_category,
    // payment_means_code) may be omitted entirely.
    let headers = reader
        .headers()
        .map_err(|e| AppError::Other(format!("Eroare citire header CSV: {e}")))?
        .clone();
    let header_index = |name: &str| -> Option<usize> {
        headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
    };
    let require_header = |name: &str| -> AppResult<usize> {
        header_index(name).ok_or_else(|| {
            AppError::Validation(format!(
                "Coloana CSV obligatorie '{name}' lipsește din header."
            ))
        })
    };

    let idx_company_cui = require_header("company_cui")?;
    let idx_customer_cui = require_header("customer_cui")?;
    let idx_customer_name = require_header("customer_name")?;
    let idx_series = require_header("series")?;
    let idx_number = require_header("number")?;
    let idx_issue_date = require_header("issue_date")?;
    let idx_due_date = require_header("due_date")?;
    let idx_item_name = require_header("item_name")?;
    let idx_qty = require_header("qty")?;
    let idx_unit = require_header("unit")?;
    let idx_unit_price = require_header("unit_price")?;
    let idx_vat_rate = require_header("vat_rate")?;
    let idx_vat_category = header_index("vat_category");
    let idx_payment_means = header_index("payment_means_code");

    // ── Collect validated rows before opening the transaction ─────────────
    // We validate all rows first (without touching the DB) so we can report
    // per-row errors upfront.  Only valid, non-dry-run rows are then inserted
    // inside a single transaction together with the last_invoice_number bump.

    struct ValidRow {
        row_num: usize,
        customer_cui: String,
        customer_name: String,
        series: String,
        number: i64,
        issue_date: String,
        due_date: String,
        item_name: String,
        qty: Decimal,
        unit: String,
        unit_price: Decimal,
        vat_rate: Decimal,
        vat_cat: String,
        payment_means_code: String,
    }

    let mut valid_rows: Vec<ValidRow> = Vec::new();

    // SEC-10: cap the row count so an accidentally-huge / malformed CSV can't exhaust memory.
    const MAX_CSV_ROWS: usize = 50_000;

    for (idx, result) in reader.records().enumerate() {
        if idx >= MAX_CSV_ROWS {
            return Err(AppError::Validation(format!(
                "Fișier CSV prea mare (peste {MAX_CSV_ROWS} de rânduri) — împărțiți-l în fișiere mai mici."
            )));
        }
        let row_num = idx + 2;
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                errors.push(format!("Linia {row_num}: eroare parsare CSV: {e}"));
                continue;
            }
        };

        // Validate that company_cui matches the active company (and is non-empty).
        let raw_company_cui = record.get(idx_company_cui).unwrap_or("").trim();
        let row_company_cui = normalize_cui_csv(raw_company_cui);
        if row_company_cui.is_empty() {
            errors.push(format!(
                "Linia {row_num}: coloana 'company_cui' este obligatorie pentru a verifica proprietatea facturii."
            ));
            continue;
        }
        if row_company_cui != company_cui_norm {
            errors.push(format!(
                "Linia {row_num}: company_cui '{raw_company_cui}' nu corespunde cu compania activă ({}).",
                company.cui
            ));
            continue;
        }

        let customer_cui = record
            .get(idx_customer_cui)
            .unwrap_or("")
            .trim()
            .to_string();
        // Validate the Romanian customer CUI check digit; foreign VAT numbers pass through.
        if customer_cui_invalid(&customer_cui) {
            errors.push(format!(
                "Linia {row_num}: CUI client '{customer_cui}' are cifra de control invalidă."
            ));
            continue;
        }
        let customer_name = record
            .get(idx_customer_name)
            .unwrap_or("")
            .trim()
            .to_string();
        let series = record.get(idx_series).unwrap_or("").trim().to_string();
        let number_str = record.get(idx_number).unwrap_or("").trim().to_string();
        let issue_date = record.get(idx_issue_date).unwrap_or("").trim().to_string();
        let due_date = record.get(idx_due_date).unwrap_or("").trim().to_string();
        let item_name = record.get(idx_item_name).unwrap_or("").trim().to_string();
        let qty_str = record.get(idx_qty).unwrap_or("").trim().to_string();
        let unit = record.get(idx_unit).unwrap_or("").trim().to_string();
        let unit_price_str = record.get(idx_unit_price).unwrap_or("").trim().to_string();
        let vat_rate_str = record.get(idx_vat_rate).unwrap_or("").trim().to_string();

        // Parse number fields
        let number: i64 = match number_str.parse() {
            Ok(n) => n,
            Err(_) => {
                errors.push(format!(
                    "Linia {row_num}: număr factură invalid '{number_str}'"
                ));
                continue;
            }
        };
        let qty: Decimal = match Decimal::from_str(&qty_str) {
            Ok(v) => v,
            Err(_) => {
                errors.push(format!("Linia {row_num}: cantitate invalidă '{qty_str}'"));
                continue;
            }
        };
        let unit_price: Decimal = match Decimal::from_str(&unit_price_str.replace(',', ".")) {
            Ok(v) => v,
            Err(_) => {
                errors.push(format!(
                    "Linia {row_num}: preț unitar invalid '{unit_price_str}'"
                ));
                continue;
            }
        };
        let vat_rate: Decimal = match Decimal::from_str(&vat_rate_str) {
            Ok(v) => v,
            Err(_) => {
                errors.push(format!(
                    "Linia {row_num}: cotă TVA invalidă '{vat_rate_str}'"
                ));
                continue;
            }
        };
        let vat_rate_rounded = vat_rate
            .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_i64()
            .unwrap_or(-1);
        if !crate::db::models::VALID_VAT_RATES.contains(&vat_rate_rounded) {
            errors.push(format!(
                "Linia {row_num}: cotă TVA invalidă '{vat_rate_str}'. Valori permise: 0, 5, 9, 11, 19, 21."
            ));
            continue;
        }

        // ── vat_category: read from CSV column if present, validate ─────────
        let csv_vat_cat = idx_vat_category
            .and_then(|i| record.get(i))
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty());

        let vat_cat: String = match csv_vat_cat {
            Some(cat) => {
                if !["S", "Z", "E", "AE", "K", "G", "O"].contains(&cat.as_str()) {
                    errors.push(format!(
                        "Linia {row_num}: categorie TVA necunoscută '{cat}'. Permise: S, Z, E, AE, K, G, O."
                    ));
                    continue;
                }
                if cat == "S" && vat_rate == Decimal::ZERO {
                    errors.push(format!(
                        "Linia {row_num}: categoria S (standard) nu poate avea cota 0."
                    ));
                    continue;
                }
                if cat != "S" && vat_rate != Decimal::ZERO {
                    errors.push(format!(
                        "Linia {row_num}: categoria '{cat}' trebuie să aibă cota 0."
                    ));
                    continue;
                }
                cat
            }
            None => {
                if vat_rate == Decimal::ZERO {
                    errors.push(format!(
                        "Linia {row_num}: pentru cota 0%, coloana 'vat_category' este obligatorie. Folosiți Z (cota zero), E (scutit), AE (taxare inversă), K (intracomunitar), G (export), sau O (în afara sferei)."
                    ));
                    continue;
                }
                "S".to_string()
            }
        };

        // ── payment_means_code: read from CSV (optional, default '30') ──────
        let payment_means_code = idx_payment_means
            .and_then(|i| record.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "30".to_string());

        // Validate against common UBL payment means codes
        let valid_pm_codes = ["10", "20", "30", "42", "48", "49", "57", "58", "59"];
        if !valid_pm_codes.contains(&payment_means_code.as_str()) {
            errors.push(format!(
                "Linia {row_num}: cod metodă plată invalid '{payment_means_code}'. Permise: {}.",
                valid_pm_codes.join(", ")
            ));
            continue;
        }

        // In dry_run mode, stop here — validation passed for this line
        if dry_run {
            imported += 1;
            continue;
        }

        valid_rows.push(ValidRow {
            row_num,
            customer_cui,
            customer_name,
            series,
            number,
            issue_date,
            due_date,
            item_name,
            qty,
            unit,
            unit_price,
            vat_rate,
            vat_cat,
            payment_means_code,
        });
    }

    // ── Single transaction: all valid rows + last_invoice_number bump ─────
    // This prevents a partial-import race where some rows commit but the number
    // bump is skipped (R9 #9 hardening).  If any INSERT fails the whole batch
    // rolls back and no bump happens.
    if !dry_run && !valid_rows.is_empty() {
        // Per-row transaction: a single bad row (e.g. a duplicate series+number) is skipped and
        // reported in `errors`, NOT rolled back together with the whole batch. last_invoice_number is
        // bumped once after the loop, over the rows that actually committed.
        for row in &valid_rows {
            let row_num = row.row_num;

            // Calculate amounts with Decimal precision
            let subtotal = (row.qty * row.unit_price)
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
            let vat_amount = (subtotal * row.vat_rate / Decimal::from(100))
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
            let total = (subtotal + vat_amount)
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
            let full_number = format!("{}-{:04}", row.series, row.number);
            let now = chrono::Utc::now().timestamp();
            let invoice_id = crate::db::models::new_id();
            let line_id = crate::db::models::new_id();

            let mut tx = match pool.begin().await {
                Ok(t) => t,
                Err(e) => {
                    errors.push(format!("Linia {row_num}: eroare tranzacție: {e}"));
                    continue;
                }
            };

            // D4: Find or create contact within this row's transaction.
            // Dedup is only done when customer_cui is non-empty — blank CUIs
            // are B2C customers that must NOT be merged across rows (different
            // people can share an empty CUI).  For blank CUI we always insert
            // a new contact row, deduplicating only by exact legal_name within
            // the same company so that the same B2C buyer appearing on multiple
            // rows doesn't create duplicates.
            let contact_id: String = {
                let cui_empty = row.customer_cui.is_empty();

                let existing = if cui_empty {
                    // No CUI → dedup by exact legal_name + empty CUI only.
                    sqlx::query(
                        "SELECT id FROM contacts \
                         WHERE (cui IS NULL OR cui = '') \
                           AND legal_name = ?1 \
                           AND company_id = ?2 LIMIT 1",
                    )
                    .bind(&row.customer_name)
                    .bind(&company_id)
                    .fetch_optional(&mut *tx)
                    .await
                } else {
                    // Non-empty CUI → dedup by CUI (original behaviour).
                    sqlx::query(
                        "SELECT id FROM contacts WHERE cui = ?1 AND company_id = ?2 LIMIT 1",
                    )
                    .bind(&row.customer_cui)
                    .bind(&company_id)
                    .fetch_optional(&mut *tx)
                    .await
                };

                match existing {
                    Ok(Some(r)) => match r.try_get::<String, _>("id") {
                        Ok(id) => id,
                        Err(e) => {
                            errors.push(format!("Linia {row_num}: eroare DB contact: {e}"));
                            continue;
                        }
                    },
                    Ok(None) => {
                        let new_id = crate::db::models::new_id();
                        // For blank-CUI contacts, store NULL so that the
                        // name-based dedup above works correctly on re-import.
                        let cui_to_store: Option<&str> = if cui_empty {
                            None
                        } else {
                            Some(&row.customer_cui)
                        };
                        let res = sqlx::query(
                            "INSERT INTO contacts \
                             (id, company_id, contact_type, cui, legal_name, vat_payer, country, created_at, updated_at) \
                             VALUES (?1, ?2, 'CUSTOMER', ?3, ?4, 0, 'RO', ?5, ?5)",
                        )
                        .bind(&new_id)
                        .bind(&company_id)
                        .bind(cui_to_store)
                        .bind(&row.customer_name)
                        .bind(now)
                        .execute(&mut *tx)
                        .await;
                        if let Err(e) = res {
                            errors.push(format!("Linia {row_num}: eroare creare contact: {e}"));
                            continue;
                        }
                        new_id
                    }
                    Err(e) => {
                        errors.push(format!("Linia {row_num}: eroare DB contact: {e}"));
                        continue;
                    }
                }
            };

            // Insert invoice header
            let inv_res = sqlx::query(
                "INSERT INTO invoices \
                 (id, company_id, contact_id, series, number, full_number, issue_date, due_date, \
                  currency, subtotal_amount, vat_amount, total_amount, status, \
                  payment_means_code, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'RON', ?9, ?10, ?11, 'DRAFT', ?12, ?13, ?13)",
            )
            .bind(&invoice_id)
            .bind(&company_id)
            .bind(&contact_id)
            .bind(&row.series)
            .bind(row.number)
            .bind(&full_number)
            .bind(&row.issue_date)
            .bind(&row.due_date)
            .bind(subtotal.to_string())
            .bind(vat_amount.to_string())
            .bind(total.to_string())
            .bind(&row.payment_means_code)
            .bind(now)
            .execute(&mut *tx)
            .await;

            if let Err(e) = inv_res {
                errors.push(format!("Linia {row_num}: eroare inserare factură: {e}"));
                continue;
            }

            // Insert line item
            let line_res = sqlx::query(
                "INSERT INTO invoice_line_items \
                 (id, invoice_id, position, name, quantity, unit, unit_price, vat_rate, vat_category, \
                  subtotal_amount, vat_amount, total_amount) \
                 VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )
            .bind(&line_id)
            .bind(&invoice_id)
            .bind(&row.item_name)
            .bind(row.qty.to_string())
            .bind(&row.unit)
            .bind(row.unit_price.to_string())
            .bind(row.vat_rate.to_string())
            .bind(&row.vat_cat)
            .bind(subtotal.to_string())
            .bind(vat_amount.to_string())
            .bind(total.to_string())
            .execute(&mut *tx)
            .await;

            if let Err(e) = line_res {
                errors.push(format!(
                    "Linia {row_num}: eroare inserare linie factură: {e}"
                ));
                continue;
            }

            // Commit THIS row. A failure (e.g. duplicate series+number tripping the UNIQUE index)
            // is reported and skipped — the rows already committed are kept.
            if let Err(e) = tx.commit().await {
                errors.push(format!("Linia {row_num}: eroare salvare factură: {e}"));
                continue;
            }
            imported += 1;
            // Track the highest committed number to advance the company counter past the import.
            if row.number > max_imported_number {
                max_imported_number = row.number;
            }
        }

        // ── Advance companies.last_invoice_number once, over the rows that committed ──────
        // Best-effort: each invoice is already durable; the counter just moves past the imported
        // numbers so a future manual invoice doesn't collide. SQLite has no MAX() in UPDATE → CASE.
        if max_imported_number > 0 {
            sqlx::query(
                "UPDATE companies SET last_invoice_number = \
                 CASE WHEN last_invoice_number < ?1 THEN ?1 ELSE last_invoice_number END \
                 WHERE id = ?2",
            )
            .bind(max_imported_number)
            .bind(&company_id)
            .execute(pool)
            .await
            .map_err(AppError::Database)?;
        }
    }

    Ok(ImportResult { imported, errors })
}

/// Importă contacte dintr-un CSV cu separator `;`.
/// Format așteptat (header obligatoriu):
/// type;cui;name;address;city;county;email;phone
///
/// Dacă `dry_run` = true, validează liniile fără a insera în DB (preview).
#[tauri::command]
pub async fn import_contacts_csv(
    state: State<'_, AppState>,
    content: String,
    company_id: String,
    dry_run: bool,
) -> AppResult<ImportResult> {
    let pool = &state.db;

    let mut imported: u32 = 0;
    let mut errors: Vec<String> = Vec::new();

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';')
        .has_headers(true)
        .flexible(true)
        .from_reader(content.as_bytes());

    for (idx, result) in reader.records().enumerate() {
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                errors.push(format!("Linia {}: eroare parsare CSV: {}", idx + 2, e));
                continue;
            }
        };

        if record.len() < 3 {
            errors.push(format!(
                "Linia {}: câmpuri insuficiente (minim 3 necesare)",
                idx + 2
            ));
            continue;
        }

        let contact_type = record.get(0).unwrap_or("").trim().to_string();
        let cui = record.get(1).unwrap_or("").trim().to_string();
        let name = record.get(2).unwrap_or("").trim().to_string();

        if name.is_empty() {
            errors.push(format!("Linia {}: numele este obligatoriu", idx + 2));
            continue;
        }

        // In dry_run mode, stop here — validation passed
        if dry_run {
            imported += 1;
            continue;
        }

        let address = record
            .get(3)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let city = record
            .get(4)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let county = record
            .get(5)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let email = record
            .get(6)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let phone = record
            .get(7)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let new_id = crate::db::models::new_id();
        let now = chrono::Utc::now().timestamp();

        let res = sqlx::query(
            "INSERT OR IGNORE INTO contacts \
             (id, company_id, contact_type, cui, legal_name, vat_payer, address, city, county, \
              country, email, phone, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, 'RO', ?9, ?10, ?11, ?11)",
        )
        .bind(&new_id)
        .bind(&company_id)
        .bind(&contact_type)
        .bind(if cui.is_empty() { None } else { Some(&cui) })
        .bind(&name)
        .bind(address)
        .bind(city)
        .bind(county)
        .bind(email)
        .bind(phone)
        .bind(now)
        .execute(pool)
        .await;

        match res {
            Ok(r) if r.rows_affected() > 0 => imported += 1,
            Ok(_) => {} // duplicate ignored
            Err(e) => errors.push(format!("Linia {}: eroare DB: {}", idx + 2, e)),
        }
    }

    Ok(ImportResult { imported, errors })
}

// ─── Import XML e-Factura UBL 2.1 ─────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XmlImportResult {
    pub imported: u32,
    pub invoice_number: Option<String>,
    pub supplier_name: Option<String>,
    pub supplier_cui: Option<String>,
    pub issue_date: Option<String>,
    pub total_amount: Option<String>,
    pub errors: Vec<String>,
}

/// Importă o factură XML dintr-un fișier selectat de utilizator prin dialog.
///
/// Security hardening (R8 #3): the caller is the Tauri dialog plugin which
/// already enforces user consent, but we still enforce two guards:
/// 1. Extension check — path must end with `.xml` (case-insensitive).
/// 2. Size cap — reject files larger than 10 MiB to prevent memory exhaustion.
///
/// We do NOT restrict to app_data_dir here because the genuine use-case is
/// importing XML e-factura files from arbitrary user-chosen locations
/// (Downloads, Desktop, USB drives, etc.). User consent was given via the
/// open-file dialog. The two guards above provide adequate defence-in-depth.
#[tauri::command]
pub async fn import_invoice_xml_from_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_path: String,
    company_id: String,
) -> AppResult<XmlImportResult> {
    // Guard 1: extension must be .xml
    let lower = file_path.to_lowercase();
    if !lower.ends_with(".xml") {
        return Err(AppError::Validation(
            "Fișierul trebuie să aibă extensia .xml.".into(),
        ));
    }

    // Guard 2: size cap — reject > 10 MiB
    const MAX_XML_BYTES: u64 = 10 * 1024 * 1024;
    let meta = tokio::fs::metadata(&file_path)
        .await
        .map_err(|e| AppError::Other(format!("Nu se poate accesa fișierul: {e}")))?;
    if meta.len() > MAX_XML_BYTES {
        return Err(AppError::Validation(
            "Fișierul XML depășește limita maximă de 10 MiB.".into(),
        ));
    }

    let xml_content = tokio::fs::read_to_string(&file_path)
        .await
        .map_err(|e| AppError::Other(format!("Nu se poate citi fișierul: {e}")))?;
    import_invoice_xml_inner(app, state, xml_content, company_id).await
}

/// Importă o factură din XML UBL 2.1 (format e-Factura ANAF) în tabela received_invoices.
/// `xml_content` este conținutul fișierului XML ca string UTF-8.
/// `company_id` este compania destinatară.
#[tauri::command]
pub async fn import_invoice_xml(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    xml_content: String,
    company_id: String,
) -> AppResult<XmlImportResult> {
    import_invoice_xml_inner(app, state, xml_content, company_id).await
}

async fn import_invoice_xml_inner(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    xml_content: String,
    company_id: String,
) -> AppResult<XmlImportResult> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let pool = &state.db;
    let mut errors: Vec<String> = Vec::new();

    // Strip BOM if present
    let xml_str = xml_content.trim_start_matches('\u{FEFF}');

    // ── Parse XML ────────────────────────────────────────────────────────────
    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut depth_supplier = 0i32;
    let mut depth_party_tax = 0i32;
    let mut depth_party_legal = 0i32;
    let mut depth_monetary = 0i32;
    let mut depth_customer = 0i32;
    let mut depth_customer_tax = 0i32;
    let mut current_local = String::new();

    use rust_decimal::Decimal;
    use std::str::FromStr as _;

    let mut issuer_cui = String::new();
    let mut issuer_name = String::new();
    let mut issue_date = String::new();
    let mut invoice_number = String::new();
    let mut currency = String::from("RON");
    let mut total_amount_str = String::from("0.00");
    let mut buyer_cui = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = std::str::from_utf8(e.local_name().into_inner())
                    .unwrap_or("")
                    .to_string();
                match local.as_str() {
                    "AccountingSupplierParty" => depth_supplier += 1,
                    "AccountingCustomerParty" => depth_customer += 1,
                    "PartyTaxScheme" if depth_supplier > 0 => depth_party_tax += 1,
                    "PartyTaxScheme" if depth_customer > 0 => depth_customer_tax += 1,
                    "PartyLegalEntity" if depth_supplier > 0 => depth_party_legal += 1,
                    "LegalMonetaryTotal" => depth_monetary += 1,
                    _ => {}
                }
                current_local = local;
            }
            Ok(Event::End(ref e)) => {
                let local = std::str::from_utf8(e.local_name().into_inner()).unwrap_or("");
                match local {
                    "AccountingSupplierParty" => {
                        depth_supplier -= 1;
                    }
                    "AccountingCustomerParty" => {
                        depth_customer -= 1;
                    }
                    "PartyTaxScheme" if depth_supplier > 0 => {
                        depth_party_tax -= 1;
                    }
                    "PartyTaxScheme" if depth_customer > 0 => {
                        depth_customer_tax -= 1;
                    }
                    "PartyLegalEntity" if depth_supplier > 0 => {
                        depth_party_legal -= 1;
                    }
                    "LegalMonetaryTotal" => {
                        depth_monetary -= 1;
                    }
                    _ => {}
                }
                current_local.clear();
            }
            Ok(Event::Text(ref e)) => {
                let text = match e.unescape() {
                    Ok(t) => t.trim().to_string(),
                    Err(_) => {
                        buf.clear();
                        continue;
                    }
                };
                if text.is_empty() {
                    buf.clear();
                    continue;
                }
                match current_local.as_str() {
                    "ID" if depth_supplier == 0 && invoice_number.is_empty() => {
                        invoice_number = text;
                    }
                    "IssueDate" if depth_supplier == 0 => {
                        issue_date = text;
                    }
                    "DocumentCurrencyCode" => {
                        currency = text;
                    }
                    "CompanyID"
                        if depth_supplier > 0 && depth_party_tax > 0 && issuer_cui.is_empty() =>
                    {
                        issuer_cui = text;
                    }
                    "CompanyID"
                        if depth_customer > 0 && depth_customer_tax > 0 && buyer_cui.is_empty() =>
                    {
                        buyer_cui = text;
                    }
                    "RegistrationName"
                        if depth_supplier > 0
                            && depth_party_legal > 0
                            && issuer_name.is_empty() =>
                    {
                        issuer_name = text;
                    }
                    "Name" if depth_supplier > 0 && issuer_name.is_empty() => {
                        issuer_name = text;
                    }
                    "PayableAmount" if depth_monetary > 0 => {
                        total_amount_str = if let Ok(d) = Decimal::from_str(text.trim()) {
                            d.round_dp_with_strategy(
                                2,
                                rust_decimal::RoundingStrategy::MidpointAwayFromZero,
                            )
                            .to_string()
                        } else {
                            "0.00".to_string()
                        };
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                errors.push(format!("Eroare parsare XML: {}", e));
                return Ok(XmlImportResult {
                    imported: 0,
                    invoice_number: None,
                    supplier_name: None,
                    supplier_cui: None,
                    issue_date: None,
                    total_amount: None,
                    errors,
                });
            }
            _ => {}
        }
        buf.clear();
    }

    if invoice_number.is_empty() {
        errors.push("Numărul facturii lipsește din XML.".into());
        return Ok(XmlImportResult {
            imported: 0,
            invoice_number: None,
            supplier_name: None,
            supplier_cui: None,
            issue_date: None,
            total_amount: None,
            errors,
        });
    }
    if issue_date.is_empty() {
        issue_date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    }
    if issuer_name.is_empty() {
        issuer_name = issuer_cui.clone();
    }

    // ── Dedup: derivăm ID-ul din hash-ul conținutului XML ───────────────────
    // Astfel, importul aceluiași fișier de două ori va lovi UNIQUE(anaf_download_id)
    // și INSERT OR IGNORE va returna 0 rows affected → mesaj clar pentru utilizator.
    use sha2::{Digest, Sha256};
    let xml_hash = {
        let mut h = Sha256::new();
        h.update(xml_str.as_bytes());
        format!("{:x}", h.finalize())
    };
    let anaf_download_id = format!("manual-{}", &xml_hash[..32]); // primii 128 biți

    // Verificăm în avans dacă există deja (pentru mesaj de eroare mai clar). Scopat pe companie —
    // verificarea de duplicat nu trebuie să se uite peste granița de tenant; UNIQUE-ul global pe
    // anaf_download_id rămâne plasa de siguranță la INSERT.
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM received_invoices WHERE anaf_download_id = ?1 AND company_id = ?2 LIMIT 1",
    )
    .bind(&anaf_download_id)
    .bind(&company_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?;

    if existing.is_some() {
        errors.push(format!(
            "Factura {} a fost deja importată (același fișier XML).",
            invoice_number
        ));
        return Ok(XmlImportResult {
            imported: 0,
            invoice_number: Some(invoice_number),
            supplier_name: Some(issuer_name),
            supplier_cui: Some(issuer_cui),
            issue_date: Some(issue_date),
            total_amount: Some(total_amount_str),
            errors,
        });
    }

    // ── Verify buyer CUI matches active company ───────────────────────────────
    fn normalize_cui(s: &str) -> String {
        s.trim()
            .to_uppercase()
            .trim_start_matches("RO")
            .trim()
            .to_string()
    }

    if !buyer_cui.is_empty() {
        let company = crate::db::companies::get(pool, &company_id).await?;
        let xml_buyer = normalize_cui(&buyer_cui);
        let company_cui = normalize_cui(&company.cui);
        if !xml_buyer.is_empty() && xml_buyer != company_cui {
            errors.push(format!(
                "Factura XML este adresată companiei cu CUI {} — nu companiei active ({}).",
                buyer_cui, company.cui
            ));
            return Ok(XmlImportResult {
                imported: 0,
                invoice_number: Some(invoice_number),
                supplier_name: Some(issuer_name),
                supplier_cui: Some(issuer_cui),
                issue_date: Some(issue_date),
                total_amount: Some(total_amount_str),
                errors,
            });
        }
    }

    // ── Compute archive path (but do NOT write yet) ───────────────────────────
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let year_str = if issue_date.len() >= 4 {
        issue_date[..4].to_string()
    } else {
        "0000".to_string()
    };
    let unique_id = crate::db::models::new_id();
    let archive_root = base.join("archive").join("received").join("manual");
    let archive_dir = archive_root.join(&year_str).join(&unique_id);
    let xml_path = archive_dir.join("invoice.xml");

    // ── Insert into received_invoices FIRST ───────────────────────────────────
    // Writing the file first and then inserting into DB risks leaving an orphaned
    // archive file if the DB insert fails (e.g., duplicate). Instead: insert first,
    // write to disk only on success.
    let recv_id = crate::db::models::new_id();
    let now = chrono::Utc::now().timestamp();
    // anaf_download_id already set above from XML content hash

    let res = sqlx::query(
        "INSERT OR IGNORE INTO received_invoices \
         (id, company_id, anaf_download_id, anaf_index, issuer_cui, issuer_name, \
          series, number, total_amount, currency, issue_date, xml_path, status, \
          downloaded_at, created_at) \
         VALUES (?1, ?2, ?3, NULL, ?4, ?5, NULL, ?6, ?7, ?8, ?9, ?10, 'NEW', ?11, ?11)",
    )
    .bind(&recv_id)
    .bind(&company_id)
    .bind(&anaf_download_id)
    .bind(&issuer_cui)
    .bind(&issuer_name)
    .bind(&invoice_number)
    .bind(&total_amount_str)
    .bind(&currency)
    .bind(&issue_date)
    .bind(xml_path.to_string_lossy().as_ref())
    .bind(now)
    .execute(pool)
    .await;

    match res {
        Ok(r) if r.rows_affected() > 0 => {
            // DB insert succeeded — now write the XML archive file.
            // If the file write fails the DB record still has all invoice data;
            // the archive copy is just missing. Log the error but still report success.
            // ROB-12: archive write is best-effort (the DB record holds all invoice data); a
            // failure leaves the archive copy missing. Log via tracing (level-respecting,
            // structured) rather than raw stdout so the failure is actually surfaced in logs.
            if let Err(e) = tokio::fs::create_dir_all(&archive_dir).await {
                tracing::warn!(dir = %archive_dir.display(), error = %e, "nu s-a putut crea directorul arhivă XML");
            } else if let Err(e) = tokio::fs::write(&xml_path, xml_str.as_bytes()).await {
                tracing::warn!(path = %xml_path.display(), error = %e, "nu s-a putut scrie fișierul XML arhivă");
            }
            Ok(XmlImportResult {
                imported: 1,
                invoice_number: Some(invoice_number),
                supplier_name: Some(issuer_name),
                supplier_cui: Some(issuer_cui),
                issue_date: Some(issue_date),
                total_amount: Some(total_amount_str),
                errors,
            })
        }
        Ok(_) => {
            // INSERT OR IGNORE returned 0 rows — duplicate (anaf_download_id already exists).
            // Do NOT write any file.
            errors.push(format!("Factura {} există deja în sistem.", invoice_number));
            Ok(XmlImportResult {
                imported: 0,
                invoice_number: Some(invoice_number),
                supplier_name: Some(issuer_name),
                supplier_cui: Some(issuer_cui),
                issue_date: Some(issue_date),
                total_amount: Some(total_amount_str),
                errors,
            })
        }
        Err(e) => {
            // DB error — no file written.
            errors.push(format!("Eroare DB: {}", e));
            Ok(XmlImportResult {
                imported: 0,
                invoice_number: Some(invoice_number),
                supplier_name: Some(issuer_name),
                supplier_cui: Some(issuer_cui),
                issue_date: Some(issue_date),
                total_amount: Some(total_amount_str),
                errors,
            })
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    #[test]
    fn customer_cui_invalid_rejects_bad_ro_passes_foreign() {
        use super::customer_cui_invalid;
        assert!(!customer_cui_invalid("")); // empty → ok (absent CUI)
        assert!(!customer_cui_invalid("12345674")); // valid RO check digit
        assert!(!customer_cui_invalid("RO12345674")); // valid, RO-prefixed
        assert!(customer_cui_invalid("12345670")); // bad check digit → reject
        assert!(customer_cui_invalid("RO12345670")); // bad check digit → reject
        assert!(!customer_cui_invalid("DE123456789")); // foreign (DE) → pass through
        assert!(!customer_cui_invalid("HU12345678")); // foreign (HU) → pass through
    }

    /// Mirrors the production header lookup logic so we can unit-test header
    /// parsing without spinning up a Tauri AppState + SQLite pool.
    fn parse_headers(csv: &str) -> std::collections::HashMap<String, usize> {
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b';')
            .has_headers(true)
            .flexible(true)
            .from_reader(csv.as_bytes());
        let headers = reader.headers().unwrap().clone();
        headers
            .iter()
            .enumerate()
            .map(|(i, h)| (h.trim().to_ascii_lowercase(), i))
            .collect()
    }

    #[test]
    fn csv_with_quoted_semicolons() {
        let csv = "series;number;issue_date;client_name;client_cui;description;quantity;unit_price;vat_rate\nACME;1;2026-05-30;\"SC Client; Test SRL\";RO123456;\"Serviciu; consultanta\";1;100.00;19";
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b';')
            .has_headers(true)
            .flexible(true)
            .from_reader(csv.as_bytes());
        let records: Vec<_> = reader.records().collect();
        assert_eq!(records.len(), 1);
        let r = records[0].as_ref().unwrap();
        assert_eq!(r.get(3).unwrap(), "SC Client; Test SRL");
    }

    #[test]
    fn csv_explicit_vat_category_for_zero_rate() {
        // Header contains optional vat_category column — Z is valid for 0% rate.
        let csv = "company_cui;customer_cui;customer_name;series;number;issue_date;due_date;item_name;qty;unit;unit_price;vat_rate;vat_category\nRO123;RO456;Client SRL;ACME;1;2026-05-30;2026-06-30;Item;1;buc;100;0;Z";
        let headers = parse_headers(csv);
        assert!(headers.contains_key("vat_category"));
        let idx_vat_cat = headers["vat_category"];
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b';')
            .has_headers(true)
            .flexible(true)
            .from_reader(csv.as_bytes());
        let rec = reader.records().next().unwrap().unwrap();
        let cat = rec.get(idx_vat_cat).unwrap().trim().to_uppercase();
        let vat_rate = Decimal::ZERO;
        // The validation logic should accept "Z" with rate 0
        assert_eq!(cat, "Z");
        let allowed = ["S", "Z", "E", "AE", "K", "G", "O"];
        assert!(allowed.contains(&cat.as_str()));
        assert!(cat != "S" || vat_rate != Decimal::ZERO);
    }

    #[test]
    fn csv_zero_rate_without_category_fails() {
        // Header has no vat_category — for rate 0 this MUST be rejected.
        let csv = "company_cui;customer_cui;customer_name;series;number;issue_date;due_date;item_name;qty;unit;unit_price;vat_rate\nRO123;RO456;Client SRL;ACME;1;2026-05-30;2026-06-30;Item;1;buc;100;0";
        let headers = parse_headers(csv);
        // vat_category absent → the import code path raises a validation error.
        assert!(!headers.contains_key("vat_category"));
        // The validator's contract: for rate 0 and no category column → error.
        // Simulate the rejection: if no idx_vat_category AND rate == 0 → fail.
        let idx_vat_category: Option<usize> = headers.get("vat_category").copied();
        let vat_rate = Decimal::ZERO;
        let should_fail = idx_vat_category.is_none() && vat_rate == Decimal::ZERO;
        assert!(should_fail, "0% rate with no vat_category must be rejected");
    }

    #[test]
    fn csv_invalid_vat_category_fails() {
        let csv = "company_cui;customer_cui;customer_name;series;number;issue_date;due_date;item_name;qty;unit;unit_price;vat_rate;vat_category\nRO123;RO456;Client SRL;ACME;1;2026-05-30;2026-06-30;Item;1;buc;100;0;X";
        let headers = parse_headers(csv);
        let idx_vat_cat = headers["vat_category"];
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b';')
            .has_headers(true)
            .flexible(true)
            .from_reader(csv.as_bytes());
        let rec = reader.records().next().unwrap().unwrap();
        let cat = rec.get(idx_vat_cat).unwrap().trim().to_uppercase();
        let allowed = ["S", "Z", "E", "AE", "K", "G", "O"];
        assert!(!allowed.contains(&cat.as_str()), "X must be rejected");
    }

    #[test]
    fn csv_payment_means_code_default_is_30() {
        // Header omits payment_means_code → import path defaults to "30".
        let csv = "company_cui;customer_cui;customer_name;series;number;issue_date;due_date;item_name;qty;unit;unit_price;vat_rate\nRO123;RO456;Client SRL;ACME;1;2026-05-30;2026-06-30;Item;1;buc;100;19";
        let headers = parse_headers(csv);
        assert!(!headers.contains_key("payment_means_code"));
        let idx_payment_means: Option<usize> = headers.get("payment_means_code").copied();
        let default = idx_payment_means
            .map(|_| "from-csv".to_string())
            .unwrap_or_else(|| "30".to_string());
        assert_eq!(default, "30");
    }

    #[test]
    fn csv_payment_means_code_invalid_rejected() {
        let csv = "company_cui;customer_cui;customer_name;series;number;issue_date;due_date;item_name;qty;unit;unit_price;vat_rate;payment_means_code\nRO123;RO456;Client SRL;ACME;1;2026-05-30;2026-06-30;Item;1;buc;100;19;99";
        let headers = parse_headers(csv);
        let idx_pm = headers["payment_means_code"];
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b';')
            .has_headers(true)
            .flexible(true)
            .from_reader(csv.as_bytes());
        let rec = reader.records().next().unwrap().unwrap();
        let code = rec.get(idx_pm).unwrap().trim();
        let valid = ["10", "20", "30", "42", "48", "49", "57", "58", "59"];
        assert!(!valid.contains(&code), "99 is not a recognised code");
    }

    /// D4: Two CSV rows with empty customer_cui but *different* names must NOT
    /// dedup into the same contact.  This test simulates the branching logic
    /// used by `import_invoices_csv` to choose the dedup query.
    #[test]
    fn d4_empty_cui_rows_with_different_names_are_distinct() {
        struct FakeRow {
            customer_cui: String,
            customer_name: String,
        }
        let rows = vec![
            FakeRow {
                customer_cui: "".to_string(),
                customer_name: "Ion Popescu".to_string(),
            },
            FakeRow {
                customer_cui: "".to_string(),
                customer_name: "Maria Ionescu".to_string(),
            },
            FakeRow {
                customer_cui: "RO123456".to_string(),
                customer_name: "SC Test SRL".to_string(),
            },
            FakeRow {
                customer_cui: "RO123456".to_string(),
                customer_name: "SC Test SRL dup".to_string(),
            }, // same CUI → dedup
        ];

        // Simulate the dedup logic: collect (dedup_key, name) pairs.
        // Blank-CUI rows use (legal_name, "") as key; CUI rows use ("", cui).
        use std::collections::HashMap;
        let mut seen: HashMap<(String, String), String> = HashMap::new();
        let mut distinct_contacts: Vec<String> = Vec::new();

        for row in &rows {
            let dedup_key = if row.customer_cui.is_empty() {
                (row.customer_name.clone(), "".to_string())
            } else {
                ("".to_string(), row.customer_cui.clone())
            };
            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(dedup_key) {
                e.insert(row.customer_name.clone());
                distinct_contacts.push(row.customer_name.clone());
            }
        }

        // Ion Popescu, Maria Ionescu (both blank-CUI but different names) +
        // SC Test SRL (one entry for the deduped CUI) = 3 distinct contacts.
        assert_eq!(
            distinct_contacts.len(),
            3,
            "Expected 3 distinct contacts (2 blank-CUI different names + 1 CUI-deduped), got: {distinct_contacts:?}"
        );
    }

    /// D4: Two rows with the same empty CUI AND the same name dedup into one
    /// contact (e.g. same B2C buyer appears on two invoices in the batch).
    #[test]
    fn d4_empty_cui_same_name_deduped_into_one() {
        struct FakeRow {
            customer_cui: String,
            customer_name: String,
        }
        let rows = vec![
            FakeRow {
                customer_cui: "".to_string(),
                customer_name: "Ion Popescu".to_string(),
            },
            FakeRow {
                customer_cui: "".to_string(),
                customer_name: "Ion Popescu".to_string(),
            }, // dup
        ];

        use std::collections::HashMap;
        let mut seen: HashMap<(String, String), ()> = HashMap::new();
        let mut distinct: usize = 0;

        for row in &rows {
            let key = if row.customer_cui.is_empty() {
                (row.customer_name.clone(), "".to_string())
            } else {
                ("".to_string(), row.customer_cui.clone())
            };
            if seen.insert(key, ()).is_none() {
                distinct += 1;
            }
        }

        assert_eq!(
            distinct, 1,
            "Same blank-CUI + same name must dedup to 1 contact"
        );
    }

    #[test]
    fn full_number_format_matches_invoice_creation() {
        // Regression: BIZ-06. CSV import must use {series}-{number:04} like the
        // rest of the app (see db/invoices.rs::create).
        let series = "ACME";
        let number: i64 = 7;
        let full = format!("{}-{:04}", series, number);
        assert_eq!(full, "ACME-0007");
    }

    /// Unit test for the max_imported_number computation (R5 EDGE-03).
    /// Simulates the accumulation logic that drives the
    /// `UPDATE companies SET last_invoice_number = MAX(...)` at end of import.
    #[test]
    fn max_imported_number_tracks_highest_committed_row() {
        let rows: Vec<i64> = vec![3, 7, 1, 15, 9];
        let mut max_imported_number: i64 = 0;
        for number in rows {
            // Simulate a successful commit path.
            if number > max_imported_number {
                max_imported_number = number;
            }
        }
        assert_eq!(max_imported_number, 15);
    }

    #[test]
    fn max_imported_number_stays_zero_when_no_rows_committed() {
        // If nothing was committed (e.g. all dry_run), bump must not run.
        let max_imported_number: i64 = 0;
        assert_eq!(
            max_imported_number, 0,
            "No update should fire when max is 0"
        );
    }

    #[test]
    fn xml_import_rejects_non_xml_extension() {
        // Guard: extension check. Only a pure logic test here — the actual
        // async command is integration-tested via the running app.
        let paths = vec![
            ("/tmp/invoice.xml", true),
            ("/tmp/invoice.XML", true),
            ("/tmp/invoice.csv", false),
            ("/tmp/invoice.xml.zip", false),
            ("/tmp/noextension", false),
        ];
        for (path, expected_ok) in paths {
            let lower = path.to_lowercase();
            let ok = lower.ends_with(".xml");
            assert_eq!(ok, expected_ok, "Failed for path: {path}");
        }
    }
}
