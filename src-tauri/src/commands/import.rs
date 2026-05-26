//! Importuri CSV pentru facturi și contacte.

use sqlx::Row;
use tauri::State;

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
    let pool = &state.db;
    let mut lines = content.lines();
    // Skip header
    lines.next();

    let mut imported: u32 = 0;
    let mut errors: Vec<String> = Vec::new();

    for (idx, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(';').map(str::trim).collect();
        if fields.len() < 12 {
            errors.push(format!("Linia {}: câmpuri insuficiente ({})", idx + 2, fields.len()));
            continue;
        }

        let customer_cui = fields[1];
        let customer_name = fields[2];
        let series = fields[3];
        let number_str = fields[4];
        let issue_date = fields[5];
        let due_date = fields[6];
        let item_name = fields[7];
        let qty_str = fields[8];
        let unit = fields[9];
        let unit_price_str = fields[10];
        let vat_rate_str = fields[11];

        // Parse number fields
        let number: i64 = match number_str.parse() {
            Ok(n) => n,
            Err(_) => {
                errors.push(format!("Linia {}: număr factură invalid '{}'", idx + 2, number_str));
                continue;
            }
        };
        let qty: f64 = match qty_str.parse() {
            Ok(v) => v,
            Err(_) => {
                errors.push(format!("Linia {}: cantitate invalidă '{}'", idx + 2, qty_str));
                continue;
            }
        };
        let unit_price: f64 = match unit_price_str.replace(',', ".").parse() {
            Ok(v) => v,
            Err(_) => {
                errors.push(format!("Linia {}: preț unitar invalid '{}'", idx + 2, unit_price_str));
                continue;
            }
        };
        let vat_rate: f64 = match vat_rate_str.parse() {
            Ok(v) => v,
            Err(_) => {
                errors.push(format!("Linia {}: cotă TVA invalidă '{}'", idx + 2, vat_rate_str));
                continue;
            }
        };

        // In dry_run mode, stop here — validation passed for this line
        if dry_run {
            imported += 1;
            continue;
        }

        // Find or create contact
        let contact_id: String = {
            let existing = sqlx::query(
                "SELECT id FROM contacts WHERE cui = ?1 AND company_id = ?2 LIMIT 1",
            )
            .bind(customer_cui)
            .bind(&company_id)
            .fetch_optional(pool)
            .await;

            match existing {
                Ok(Some(row)) => row.try_get::<String, _>("id").map_err(AppError::Database)?,
                Ok(None) => {
                    // Create contact
                    let new_id = uuid::Uuid::now_v7().to_string();
                    let now = chrono::Utc::now().timestamp();
                    let res = sqlx::query(
                        "INSERT INTO contacts (id, company_id, contact_type, cui, legal_name, vat_payer, country, created_at, updated_at) \
                         VALUES (?1, ?2, 'CUSTOMER', ?3, ?4, 0, 'RO', ?5, ?5)",
                    )
                    .bind(&new_id)
                    .bind(&company_id)
                    .bind(customer_cui)
                    .bind(customer_name)
                    .bind(now)
                    .execute(pool)
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

        // Calculate amounts
        let subtotal = qty * unit_price;
        let vat_amount = subtotal * vat_rate / 100.0;
        let total = subtotal + vat_amount;
        let full_number = format!("{}{}", series, number);
        let now = chrono::Utc::now().timestamp();
        let invoice_id = uuid::Uuid::now_v7().to_string();
        let line_id = uuid::Uuid::now_v7().to_string();

        // Insert invoice
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
        .bind(subtotal)
        .bind(vat_amount)
        .bind(total)
        .bind(now)
        .execute(pool)
        .await;

        if let Err(e) = inv_res {
            errors.push(format!("Linia {}: eroare inserare factură: {}", idx + 2, e));
            continue;
        }

        // Insert line item
        let vat_cat = if vat_rate == 0.0 { "Z" } else { "S" };
        let _ = sqlx::query(
            "INSERT INTO invoice_line_items \
             (id, invoice_id, position, name, quantity, unit, unit_price, vat_rate, vat_category, \
              subtotal_amount, vat_amount, total_amount) \
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(&line_id)
        .bind(&invoice_id)
        .bind(item_name)
        .bind(qty)
        .bind(unit)
        .bind(unit_price)
        .bind(vat_rate)
        .bind(vat_cat)
        .bind(subtotal)
        .bind(vat_amount)
        .bind(total)
        .execute(pool)
        .await;

        imported += 1;
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
    let mut lines = content.lines();
    // Skip header
    lines.next();

    let mut imported: u32 = 0;
    let mut errors: Vec<String> = Vec::new();

    for (idx, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(';').map(str::trim).collect();
        if fields.len() < 3 {
            errors.push(format!("Linia {}: câmpuri insuficiente (minim 3 necesare)", idx + 2));
            continue;
        }

        let _contact_type = fields[0];
        let cui = if fields.len() > 1 { fields[1] } else { "" };
        let name = fields[2];

        if name.is_empty() {
            errors.push(format!("Linia {}: numele este obligatoriu", idx + 2));
            continue;
        }

        // In dry_run mode, stop here — validation passed
        if dry_run {
            imported += 1;
            continue;
        }

        let contact_type = fields[0];
        let address = fields.get(3).copied().filter(|s| !s.is_empty());
        let city = fields.get(4).copied().filter(|s| !s.is_empty());
        let county = fields.get(5).copied().filter(|s| !s.is_empty());
        let email = fields.get(6).copied().filter(|s| !s.is_empty());
        let phone = fields.get(7).copied().filter(|s| !s.is_empty());

        let new_id = uuid::Uuid::now_v7().to_string();
        let now = chrono::Utc::now().timestamp();

        let res = sqlx::query(
            "INSERT OR IGNORE INTO contacts \
             (id, company_id, contact_type, cui, legal_name, vat_payer, address, city, county, \
              country, email, phone, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, 'RO', ?9, ?10, ?11, ?11)",
        )
        .bind(&new_id)
        .bind(&company_id)
        .bind(contact_type)
        .bind(if cui.is_empty() { None } else { Some(cui) })
        .bind(name)
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
    pub total_amount: Option<f64>,
    pub errors: Vec<String>,
}

/// Importă o factură din XML UBL 2.1 (format e-Factura ANAF) în tabela received_invoices.
/// `xml_content` este conținutul fișierului XML ca string UTF-8.
/// `company_id` este compania destinatară.
/// `app_data_dir` — directorul de date al aplicației (primit din frontend via path.appDataDir()).
#[tauri::command]
pub async fn import_invoice_xml(
    state: State<'_, AppState>,
    xml_content: String,
    company_id: String,
    app_data_dir: String,
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
    let mut current_local = String::new();

    let mut issuer_cui = String::new();
    let mut issuer_name = String::new();
    let mut issue_date = String::new();
    let mut invoice_number = String::new();
    let mut currency = String::from("RON");
    let mut total_amount = 0.0f64;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = std::str::from_utf8(e.local_name().into_inner())
                    .unwrap_or("")
                    .to_string();
                match local.as_str() {
                    "AccountingSupplierParty" => depth_supplier += 1,
                    "PartyTaxScheme" if depth_supplier > 0 => depth_party_tax += 1,
                    "PartyLegalEntity" if depth_supplier > 0 => depth_party_legal += 1,
                    "LegalMonetaryTotal" => depth_monetary += 1,
                    _ => {}
                }
                current_local = local;
            }
            Ok(Event::End(ref e)) => {
                let local = std::str::from_utf8(e.local_name().into_inner()).unwrap_or("");
                match local {
                    "AccountingSupplierParty" => { depth_supplier -= 1; }
                    "PartyTaxScheme" if depth_supplier > 0 => { depth_party_tax -= 1; }
                    "PartyLegalEntity" if depth_supplier > 0 => { depth_party_legal -= 1; }
                    "LegalMonetaryTotal" => { depth_monetary -= 1; }
                    _ => {}
                }
                current_local.clear();
            }
            Ok(Event::Text(ref e)) => {
                let text = match e.unescape() {
                    Ok(t) => t.trim().to_string(),
                    Err(_) => { buf.clear(); continue; }
                };
                if text.is_empty() { buf.clear(); continue; }
                match current_local.as_str() {
                    "ID" if depth_supplier == 0 && invoice_number.is_empty() => {
                        invoice_number = text;
                    }
                    "IssueDate" if depth_supplier == 0 => { issue_date = text; }
                    "DocumentCurrencyCode" => { currency = text; }
                    "CompanyID" if depth_supplier > 0 && depth_party_tax > 0 => {
                        if issuer_cui.is_empty() { issuer_cui = text; }
                    }
                    "RegistrationName" if depth_supplier > 0 && depth_party_legal > 0 => {
                        if issuer_name.is_empty() { issuer_name = text; }
                    }
                    "Name" if depth_supplier > 0 && issuer_name.is_empty() => {
                        issuer_name = text;
                    }
                    "PayableAmount" if depth_monetary > 0 => {
                        total_amount = text.parse().unwrap_or(0.0);
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
            imported: 0, invoice_number: None, supplier_name: None,
            supplier_cui: None, issue_date: None, total_amount: None, errors,
        });
    }
    if issue_date.is_empty() {
        issue_date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    }
    if issuer_name.is_empty() {
        issuer_name = issuer_cui.clone();
    }

    // ── Save XML to disk ─────────────────────────────────────────────────────
    let year = if issue_date.len() >= 4 { &issue_date[..4] } else { "0000" };
    let unique_id = uuid::Uuid::now_v7().to_string();
    let archive_dir = std::path::PathBuf::from(&app_data_dir)
        .join("archive")
        .join("received")
        .join("manual")
        .join(year)
        .join(&unique_id);
    std::fs::create_dir_all(&archive_dir).map_err(AppError::Io)?;
    let xml_path = archive_dir.join("invoice.xml");
    std::fs::write(&xml_path, xml_str.as_bytes()).map_err(AppError::Io)?;

    // ── Insert into received_invoices ─────────────────────────────────────────
    let recv_id = uuid::Uuid::now_v7().to_string();
    let now = chrono::Utc::now().timestamp();
    let anaf_download_id = format!("manual-{}", &unique_id);

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
    .bind(total_amount)
    .bind(&currency)
    .bind(&issue_date)
    .bind(xml_path.to_string_lossy().as_ref())
    .bind(now)
    .execute(pool)
    .await;

    match res {
        Ok(r) if r.rows_affected() > 0 => Ok(XmlImportResult {
            imported: 1,
            invoice_number: Some(invoice_number),
            supplier_name: Some(issuer_name),
            supplier_cui: Some(issuer_cui),
            issue_date: Some(issue_date),
            total_amount: Some(total_amount),
            errors,
        }),
        Ok(_) => {
            errors.push(format!("Factura {} există deja în sistem.", invoice_number));
            Ok(XmlImportResult {
                imported: 0,
                invoice_number: Some(invoice_number),
                supplier_name: Some(issuer_name),
                supplier_cui: Some(issuer_cui),
                issue_date: Some(issue_date),
                total_amount: Some(total_amount),
                errors,
            })
        }
        Err(e) => {
            errors.push(format!("Eroare DB: {}", e));
            Ok(XmlImportResult {
                imported: 0,
                invoice_number: Some(invoice_number),
                supplier_name: Some(issuer_name),
                supplier_cui: Some(issuer_cui),
                issue_date: Some(issue_date),
                total_amount: Some(total_amount),
                errors,
            })
        }
    }
}
