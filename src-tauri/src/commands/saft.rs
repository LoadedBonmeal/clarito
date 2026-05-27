//! SAF-T D406 XML export — ANAF mandatory standard audit file.
//!
//! Generates a simplified SAF-T XML following the Romanian ANAF adaptation of
//! the OECD SAF-T schema. Covers SalesInvoices for the selected period.

use serde::Deserialize;
use sqlx::Row;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaftParams {
    pub company_id: String,
    pub year: i32,
    pub month: Option<i32>,
}

#[tauri::command]
pub async fn export_saft_d406(
    state: State<'_, AppState>,
    params: SaftParams,
) -> AppResult<String> {
    let pool = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        rt.block_on(generate_saft(&pool, params))
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

async fn generate_saft(pool: &sqlx::SqlitePool, params: SaftParams) -> AppResult<String> {
    let (date_from, date_to) = if let Some(m) = params.month {
        let last_day = days_in_month(params.year, m as u32);
        (
            format!("{}-{:02}-01", params.year, m),
            format!("{}-{:02}-{:02}", params.year, m, last_day),
        )
    } else {
        (
            format!("{}-01-01", params.year),
            format!("{}-12-31", params.year),
        )
    };

    // Fetch company using dynamic query (no query! macro to avoid compile-time DB requirement)
    let company_row = sqlx::query(
        "SELECT legal_name, cui FROM companies WHERE id = ?1",
    )
    .bind(&params.company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let company_name: String = company_row.try_get("legal_name").unwrap_or_default();
    let company_cui: String = company_row.try_get("cui").unwrap_or_default();

    // Fetch invoices
    let invoice_rows = sqlx::query(
        "SELECT \
            i.series || '-' || i.number AS full_number, \
            i.issue_date, \
            COALESCE(c.legal_name, '') AS client_name, \
            COALESCE(c.cui, '') AS client_cui, \
            COALESCE(i.subtotal_amount, i.total_amount) AS net_amount, \
            COALESCE(i.vat_amount, '0') AS vat_amount, \
            i.total_amount, \
            COALESCE(i.currency, 'RON') AS currency \
         FROM invoices i \
         LEFT JOIN contacts c ON i.contact_id = c.id \
         WHERE i.company_id = ?1 \
           AND i.issue_date >= ?2 \
           AND i.issue_date <= ?3 \
           AND i.status NOT IN ('DRAFT', 'STORNED') \
         ORDER BY i.issue_date ASC",
    )
    .bind(&params.company_id)
    .bind(&date_from)
    .bind(&date_to)
    .fetch_all(pool)
    .await?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let version = env!("CARGO_PKG_VERSION");

    let mut xml = String::with_capacity(16384);
    xml.push('\u{FEFF}'); // UTF-8 BOM
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<AuditFile xmlns=\"urn:StandardAuditFile-Taxation-Financial:RO\"\n");
    xml.push_str("           xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n");

    // Header
    xml.push_str("  <Header>\n");
    xml_elem(&mut xml, 4, "AuditFileVersion", "1.0");
    xml_elem(&mut xml, 4, "AuditFileCountry", "RO");
    xml_elem(&mut xml, 4, "AuditFileDateCreated", &today);
    xml_elem(&mut xml, 4, "SoftwareCompanyName", "Lucaris SRL");
    xml_elem(&mut xml, 4, "SoftwareID", "efactura-desktop");
    xml_elem(&mut xml, 4, "SoftwareVersion", version);
    xml.push_str("    <Company>\n");
    xml_elem(&mut xml, 6, "RegistrationNumber", &company_cui);
    xml_elem(&mut xml, 6, "Name", &escape_xml(&company_name));
    xml.push_str("    </Company>\n");
    xml_elem(&mut xml, 4, "DefaultCurrencyCode", "RON");
    xml.push_str("    <SelectionCriteria>\n");
    xml_elem(&mut xml, 6, "SelectionStartDate", &date_from);
    xml_elem(&mut xml, 6, "SelectionEndDate", &date_to);
    xml.push_str("    </SelectionCriteria>\n");
    xml.push_str("  </Header>\n");

    // SourceDocuments > SalesInvoices
    xml.push_str("  <SourceDocuments>\n");
    xml.push_str("    <SalesInvoices>\n");

    for row in &invoice_rows {
        let full_number: String = row.try_get("full_number").unwrap_or_default();
        let issue_date: String = row.try_get("issue_date").unwrap_or_default();
        let client_name: String = row.try_get("client_name").unwrap_or_default();
        let client_cui: String = row.try_get("client_cui").unwrap_or_default();
        let net_amount: String = row.try_get("net_amount").unwrap_or_else(|_| "0".to_string());
        let vat_amount: String = row.try_get("vat_amount").unwrap_or_else(|_| "0".to_string());
        let total_amount: String = row.try_get("total_amount").unwrap_or_else(|_| "0".to_string());
        let currency: String = row.try_get("currency").unwrap_or_else(|_| "RON".to_string());

        xml.push_str("      <Invoice>\n");
        xml_elem(&mut xml, 8, "InvoiceNo", &escape_xml(&full_number));
        xml_elem(&mut xml, 8, "InvoiceDate", &issue_date);
        xml_elem(&mut xml, 8, "InvoiceType", "380"); // Commercial invoice
        xml_elem(&mut xml, 8, "CustomerName", &escape_xml(&client_name));
        xml_elem(&mut xml, 8, "CustomerTaxID", &client_cui);
        xml_elem(&mut xml, 8, "NetTotal", &format_decimal(&net_amount));
        xml_elem(&mut xml, 8, "VatTotal", &format_decimal(&vat_amount));
        xml_elem(&mut xml, 8, "GrossTotal", &format_decimal(&total_amount));
        xml_elem(&mut xml, 8, "Currency", &currency);
        xml.push_str("      </Invoice>\n");
    }

    xml.push_str("    </SalesInvoices>\n");
    xml.push_str("  </SourceDocuments>\n");
    xml.push_str("</AuditFile>\n");

    Ok(xml)
}

fn xml_elem(out: &mut String, indent: usize, tag: &str, value: &str) {
    let pad = " ".repeat(indent);
    out.push_str(&format!("{pad}<{tag}>{value}</{tag}>\n"));
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn format_decimal(s: &str) -> String {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    match Decimal::from_str(s) {
        Ok(d) => format!("{:.2}", d.round_dp(2)),
        Err(_) => "0.00".to_string(),
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}
