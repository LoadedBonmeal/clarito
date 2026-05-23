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
