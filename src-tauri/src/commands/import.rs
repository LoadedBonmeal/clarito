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

    for (idx, result) in reader.records().enumerate() {
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                errors.push(format!("Linia {}: eroare parsare CSV: {}", idx + 2, e));
                continue;
            }
        };

        if record.len() < 12 {
            errors.push(format!(
                "Linia {}: câmpuri insuficiente ({})",
                idx + 2,
                record.len()
            ));
            continue;
        }

        // Validate that field 0 (company_cui) matches the active company.
        let raw_company_cui = record.get(0).unwrap_or("").trim();
        let row_company_cui = normalize_cui_csv(raw_company_cui);
        if !row_company_cui.is_empty() && row_company_cui != company_cui_norm {
            errors.push(format!(
                "Linia {}: CUI companie din CSV ({}) nu corespunde companiei active ({}).",
                idx + 2,
                raw_company_cui,
                company.cui
            ));
            continue;
        }

        let customer_cui = record.get(1).unwrap_or("").trim().to_string();
        let customer_name = record.get(2).unwrap_or("").trim().to_string();
        let series = record.get(3).unwrap_or("").trim().to_string();
        let number_str = record.get(4).unwrap_or("").trim().to_string();
        let issue_date = record.get(5).unwrap_or("").trim().to_string();
        let due_date = record.get(6).unwrap_or("").trim().to_string();
        let item_name = record.get(7).unwrap_or("").trim().to_string();
        let qty_str = record.get(8).unwrap_or("").trim().to_string();
        let unit = record.get(9).unwrap_or("").trim().to_string();
        let unit_price_str = record.get(10).unwrap_or("").trim().to_string();
        let vat_rate_str = record.get(11).unwrap_or("").trim().to_string();

        // Parse number fields
        let number: i64 = match number_str.parse() {
            Ok(n) => n,
            Err(_) => {
                errors.push(format!(
                    "Linia {}: număr factură invalid '{}'",
                    idx + 2,
                    number_str
                ));
                continue;
            }
        };
        let qty: Decimal = match Decimal::from_str(&qty_str) {
            Ok(v) => v,
            Err(_) => {
                errors.push(format!(
                    "Linia {}: cantitate invalidă '{}'",
                    idx + 2,
                    qty_str
                ));
                continue;
            }
        };
        let unit_price: Decimal = match Decimal::from_str(&unit_price_str.replace(',', ".")) {
            Ok(v) => v,
            Err(_) => {
                errors.push(format!(
                    "Linia {}: preț unitar invalid '{}'",
                    idx + 2,
                    unit_price_str
                ));
                continue;
            }
        };
        let vat_rate: Decimal = match Decimal::from_str(&vat_rate_str) {
            Ok(v) => v,
            Err(_) => {
                errors.push(format!(
                    "Linia {}: cotă TVA invalidă '{}'",
                    idx + 2,
                    vat_rate_str
                ));
                continue;
            }
        };
        let vat_rate_rounded = vat_rate.round_dp(0).to_i64().unwrap_or(-1);
        if !crate::db::models::VALID_VAT_RATES.contains(&vat_rate_rounded) {
            errors.push(format!(
                "Linia {}: cotă TVA invalidă '{}'. Valori permise: 0, 5, 9, 11, 19, 21.",
                idx + 2,
                vat_rate_str
            ));
            continue;
        }

        // In dry_run mode, stop here — validation passed for this line
        if dry_run {
            imported += 1;
            continue;
        }

        // Calculate amounts with Decimal precision
        let subtotal = (qty * unit_price).round_dp(2);
        let vat_amount = (subtotal * vat_rate / Decimal::from(100)).round_dp(2);
        let total = (subtotal + vat_amount).round_dp(2);
        let full_number = format!("{}{}", series, number);
        let now = chrono::Utc::now().timestamp();
        let invoice_id = crate::db::models::new_id();
        let line_id = crate::db::models::new_id();

        // Begin transaction — ensures header + line item are atomic.
        let mut tx = match pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                errors.push(format!("Linia {}: eroare tranzacție DB: {}", idx + 2, e));
                continue;
            }
        };

        // Find or create contact (outside the per-invoice tx — contacts are shared)
        let contact_id: String = {
            let existing =
                sqlx::query("SELECT id FROM contacts WHERE cui = ?1 AND company_id = ?2 LIMIT 1")
                    .bind(&customer_cui)
                    .bind(&company_id)
                    .fetch_optional(&mut *tx)
                    .await;

            match existing {
                Ok(Some(row)) => match row.try_get::<String, _>("id") {
                    Ok(id) => id,
                    Err(e) => {
                        errors.push(format!("Linia {}: eroare DB contact: {}", idx + 2, e));
                        continue;
                    }
                },
                Ok(None) => {
                    // Create contact inside the transaction
                    let new_id = crate::db::models::new_id();
                    let res = sqlx::query(
                        "INSERT INTO contacts (id, company_id, contact_type, cui, legal_name, vat_payer, country, created_at, updated_at) \
                         VALUES (?1, ?2, 'CUSTOMER', ?3, ?4, 0, 'RO', ?5, ?5)",
                    )
                    .bind(&new_id)
                    .bind(&company_id)
                    .bind(&customer_cui)
                    .bind(&customer_name)
                    .bind(now)
                    .execute(&mut *tx)
                    .await;
                    if let Err(e) = res {
                        errors.push(format!("Linia {}: eroare creare contact: {}", idx + 2, e));
                        continue;
                    }
                    new_id
                }
                Err(e) => {
                    errors.push(format!("Linia {}: eroare DB contact: {}", idx + 2, e));
                    continue;
                }
            }
        };

        // Insert invoice header
        let inv_res = sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, issue_date, due_date, \
              currency, subtotal_amount, vat_amount, total_amount, status, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'RON', ?9, ?10, ?11, 'DRAFT', ?12, ?12)",
        )
        .bind(&invoice_id)
        .bind(&company_id)
        .bind(&contact_id)
        .bind(series)
        .bind(number)
        .bind(&full_number)
        .bind(issue_date)
        .bind(due_date)
        .bind(subtotal.to_string())
        .bind(vat_amount.to_string())
        .bind(total.to_string())
        .bind(now)
        .execute(&mut *tx)
        .await;

        if let Err(e) = inv_res {
            errors.push(format!("Linia {}: eroare inserare factură: {}", idx + 2, e));
            // tx is dropped here — rolled back automatically
            continue;
        }

        // Insert line item — error propagates and rolls back the transaction
        let vat_cat = if vat_rate == Decimal::ZERO { "Z" } else { "S" };
        let line_res = sqlx::query(
            "INSERT INTO invoice_line_items \
             (id, invoice_id, position, name, quantity, unit, unit_price, vat_rate, vat_category, \
              subtotal_amount, vat_amount, total_amount) \
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(&line_id)
        .bind(&invoice_id)
        .bind(item_name)
        .bind(qty.to_string())
        .bind(unit)
        .bind(unit_price.to_string())
        .bind(vat_rate.to_string())
        .bind(vat_cat)
        .bind(subtotal.to_string())
        .bind(vat_amount.to_string())
        .bind(total.to_string())
        .execute(&mut *tx)
        .await;

        if let Err(e) = line_res {
            errors.push(format!(
                "Linia {}: eroare inserare linie factură: {}",
                idx + 2,
                e
            ));
            // tx dropped — rolled back automatically
            continue;
        }

        // Commit — only now count as imported
        match tx.commit().await {
            Ok(_) => imported += 1,
            Err(e) => errors.push(format!(
                "Linia {}: eroare commit tranzacție: {}",
                idx + 2,
                e
            )),
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
/// Citim fișierul în Rust (nu prin plugin-fs din frontend) pentru a ocoli restricțiile
/// de scope ale plugin-ului FS (care permit doar $APPDATA/**).
/// Orice cale returnată de dialog::open este validă pentru tokio::fs::read_to_string.
#[tauri::command]
pub async fn import_invoice_xml_from_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_path: String,
    company_id: String,
) -> AppResult<XmlImportResult> {
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
                            d.round_dp(2).to_string()
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

    // Verificăm în avans dacă există deja (pentru mesaj de eroare mai clar)
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM received_invoices WHERE anaf_download_id = ?1 LIMIT 1")
            .bind(&anaf_download_id)
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
            if let Err(e) = tokio::fs::create_dir_all(&archive_dir).await {
                eprintln!(
                    "WARN: impossibil de creat directorul arhivă {}: {}",
                    archive_dir.display(),
                    e
                );
            } else if let Err(e) = tokio::fs::write(&xml_path, xml_str.as_bytes()).await {
                eprintln!(
                    "WARN: imposibil de scris fișierul XML arhivă {}: {}",
                    xml_path.display(),
                    e
                );
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
}
