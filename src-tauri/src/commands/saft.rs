//! SAF-T D406 XML export — ANAF mandatory standard audit file.
//!
//! Generates a simplified SAF-T XML following the Romanian ANAF adaptation of
//! the OECD SAF-T schema. Covers SalesInvoices for the selected period.

use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::Row;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::str::FromStr;
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

#[derive(Clone)]
struct SaftLineItem {
    position: i64,
    description: String,
    quantity: Decimal,
    unit_price: Decimal,
    vat_rate: Decimal,
    subtotal_amount: Decimal,
    vat_amount: Decimal,
    total_amount: Decimal,
}

#[tauri::command]
pub async fn export_saft_d406(state: State<'_, AppState>, params: SaftParams) -> AppResult<String> {
    // NOTE: This is a simplified SAF-T export covering SalesInvoices only.
    // It is NOT a complete ANAF D406 submission. Label the UI accordingly.
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
    let company_row = sqlx::query("SELECT legal_name, cui FROM companies WHERE id = ?1")
        .bind(&params.company_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;

    let company_name: String = company_row
        .try_get("legal_name")
        .map_err(AppError::Database)?;
    let company_cui: String = company_row.try_get("cui").map_err(AppError::Database)?;

    // Fetch invoices — include id for line-item correlation
    let invoice_rows = sqlx::query(
        "SELECT \
            i.id, \
            i.series || '-' || printf('%04d', i.number) AS full_number, \
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
           AND i.status = 'VALIDATED' \
         ORDER BY i.issue_date ASC",
    )
    .bind(&params.company_id)
    .bind(&date_from)
    .bind(&date_to)
    .fetch_all(pool)
    .await?;

    // ── Batch-fetch all line items for these invoices ────────────────────────
    let invoice_ids: Vec<String> = invoice_rows
        .iter()
        .map(|r| r.try_get::<String, _>("id").unwrap_or_default())
        .collect();

    let mut lines_by_invoice: HashMap<String, Vec<SaftLineItem>> = HashMap::new();

    if !invoice_ids.is_empty() {
        // Build ?1,?2,... placeholder list (no user data interpolated — safe)
        let placeholders: Vec<String> = (1..=invoice_ids.len()).map(|i| format!("?{i}")).collect();
        let in_clause = placeholders.join(",");
        let sql = format!(
            "SELECT invoice_id, position, name, description, quantity, unit_price, \
                    vat_rate, subtotal_amount, vat_amount, total_amount \
             FROM invoice_line_items \
             WHERE invoice_id IN ({in_clause}) \
             ORDER BY invoice_id, position"
        );
        // NOTE: format! used only to build ?N placeholder list; no user data interpolated

        let mut q = sqlx::query(&sql);
        for id in &invoice_ids {
            q = q.bind(id);
        }
        let rows = q.fetch_all(pool).await.map_err(AppError::Database)?;

        // Helper: TEXT column → Decimal (columns are stored as TEXT after String migration)
        let to_dec = |s: &str| Decimal::from_str(s.trim()).unwrap_or_default();

        for row in rows {
            let invoice_id: String = row.try_get("invoice_id").map_err(AppError::Database)?;
            let name: String = row.try_get("name").unwrap_or_default();
            let description: String = row.try_get("description").unwrap_or_default();
            // Use description if non-empty, otherwise fall back to name
            let desc = if !description.is_empty() {
                description
            } else {
                name
            };

            lines_by_invoice
                .entry(invoice_id.clone())
                .or_default()
                .push(SaftLineItem {
                    position: row.try_get("position").unwrap_or(0),
                    description: desc,
                    quantity: to_dec(
                        &row.try_get::<String, _>("quantity")
                            .unwrap_or_else(|_| "0".to_string()),
                    ),
                    unit_price: to_dec(
                        &row.try_get::<String, _>("unit_price")
                            .unwrap_or_else(|_| "0".to_string()),
                    ),
                    vat_rate: to_dec(
                        &row.try_get::<String, _>("vat_rate")
                            .unwrap_or_else(|_| "0".to_string()),
                    ),
                    subtotal_amount: to_dec(
                        &row.try_get::<String, _>("subtotal_amount")
                            .unwrap_or_else(|_| "0".to_string()),
                    ),
                    vat_amount: to_dec(
                        &row.try_get::<String, _>("vat_amount")
                            .unwrap_or_else(|_| "0".to_string()),
                    ),
                    total_amount: to_dec(
                        &row.try_get::<String, _>("total_amount")
                            .unwrap_or_else(|_| "0".to_string()),
                    ),
                });
        }
    }

    // ── Build XML ─────────────────────────────────────────────────────────────
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let version = env!("CARGO_PKG_VERSION");

    let mut xml = String::with_capacity(32768);
    xml.push('\u{FEFF}'); // UTF-8 BOM
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<AuditFile xmlns=\"urn:StandardAuditFile-Taxation-Financial:RO\"\n");
    xml.push_str("           xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n");

    // Header
    xml.push_str("  <Header>\n");
    xml_elem(&mut xml, 4, "AuditFileVersion", "1.0");
    xml_elem(&mut xml, 4, "AuditFileCountry", "RO");
    xml_elem(&mut xml, 4, "AuditFileDateCreated", &escape_xml(&today));
    xml_elem(&mut xml, 4, "SoftwareCompanyName", "Lucaris SRL");
    xml_elem(&mut xml, 4, "SoftwareID", "efactura-desktop");
    xml_elem(&mut xml, 4, "SoftwareVersion", version);
    xml.push_str("    <Company>\n");
    xml_elem(&mut xml, 6, "RegistrationNumber", &escape_xml(&company_cui));
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
        let invoice_id: String = row.try_get("id").map_err(AppError::Database)?;
        let full_number: String = row.try_get("full_number").map_err(AppError::Database)?;
        let issue_date: String = row.try_get("issue_date").map_err(AppError::Database)?;
        // client_name / client_cui are LEFT JOIN columns — may legitimately be empty
        let client_name: String = row.try_get("client_name").unwrap_or_default();
        let client_cui: String = row.try_get("client_cui").unwrap_or_default();
        let net_amount: String = row
            .try_get("net_amount")
            .unwrap_or_else(|_| "0".to_string());
        let vat_amount: String = row
            .try_get("vat_amount")
            .unwrap_or_else(|_| "0".to_string());
        let total_amount: String = row
            .try_get("total_amount")
            .unwrap_or_else(|_| "0".to_string());
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());

        xml.push_str("      <Invoice>\n");
        xml_elem(&mut xml, 8, "InvoiceNo", &escape_xml(&full_number));
        xml_elem(&mut xml, 8, "InvoiceDate", &issue_date);
        xml_elem(&mut xml, 8, "InvoiceType", "380"); // Commercial invoice
        xml_elem(&mut xml, 8, "CustomerName", &escape_xml(&client_name));
        xml_elem(&mut xml, 8, "CustomerTaxID", &escape_xml(&client_cui));
        xml_elem(&mut xml, 8, "NetTotal", &format_decimal(&net_amount));
        xml_elem(&mut xml, 8, "VatTotal", &format_decimal(&vat_amount));
        xml_elem(&mut xml, 8, "GrossTotal", &format_decimal(&total_amount));
        xml_elem(&mut xml, 8, "Currency", &escape_xml(&currency));

        // ── Lines ─────────────────────────────────────────────────────────────
        let lines = lines_by_invoice
            .get(&invoice_id)
            .cloned()
            .unwrap_or_default();

        xml.push_str("        <Lines>\n");
        for line in &lines {
            xml.push_str("          <Line>\n");
            xml.push_str(&format!(
                "            <LineNumber>{}</LineNumber>\n",
                line.position
            ));
            xml.push_str(&format!(
                "            <Description>{}</Description>\n",
                escape_xml(&line.description)
            ));
            xml.push_str(&format!(
                "            <Quantity>{}</Quantity>\n",
                dec_str(&line.quantity)
            ));
            xml.push_str(&format!(
                "            <UnitPrice>{}</UnitPrice>\n",
                dec_str(&line.unit_price)
            ));
            xml.push_str(&format!(
                "            <TaxCode>{}</TaxCode>\n",
                saft_tax_code(&line.vat_rate)
            ));
            xml.push_str(&format!(
                "            <TaxPercentage>{}</TaxPercentage>\n",
                dec_str(&line.vat_rate)
            ));
            xml.push_str(&format!(
                "            <NetAmount>{}</NetAmount>\n",
                dec_str(&line.subtotal_amount)
            ));
            xml.push_str(&format!(
                "            <TaxAmount>{}</TaxAmount>\n",
                dec_str(&line.vat_amount)
            ));
            xml.push_str(&format!(
                "            <GrossAmount>{}</GrossAmount>\n",
                dec_str(&line.total_amount)
            ));
            xml.push_str("          </Line>\n");
        }
        xml.push_str("        </Lines>\n");

        // ── DocumentTotals — per-VAT-rate tax breakdown ───────────────────────
        // BTreeMap keeps rates in ascending order for deterministic output
        let mut by_rate: BTreeMap<String, (Decimal, Decimal, &'static str)> = BTreeMap::new();
        for line in &lines {
            let rate_key = dec_str(&line.vat_rate);
            let code = saft_tax_code(&line.vat_rate);
            let entry = by_rate
                .entry(rate_key)
                .or_insert((Decimal::ZERO, Decimal::ZERO, code));
            entry.0 += line.subtotal_amount;
            entry.1 += line.vat_amount;
        }

        if !by_rate.is_empty() {
            xml.push_str("        <DocumentTotals>\n");
            for (rate_str, (base, tax, code)) in &by_rate {
                xml.push_str("          <TaxInformation>\n");
                xml.push_str(&format!("            <TaxCode>{}</TaxCode>\n", code));
                xml.push_str(&format!(
                    "            <TaxPercentage>{}</TaxPercentage>\n",
                    rate_str
                ));
                xml.push_str(&format!(
                    "            <TaxBaseAmount>{}</TaxBaseAmount>\n",
                    dec_str(base)
                ));
                xml.push_str(&format!(
                    "            <TaxAmount>{}</TaxAmount>\n",
                    dec_str(tax)
                ));
                xml.push_str("          </TaxInformation>\n");
            }
            xml.push_str("        </DocumentTotals>\n");
        }

        xml.push_str("      </Invoice>\n");
    }

    xml.push_str("    </SalesInvoices>\n");
    xml.push_str("  </SourceDocuments>\n");
    xml.push_str("</AuditFile>\n");

    Ok(xml)
}

// ── SAF-T tax code from VAT rate ────────────────────────────────────────────
// E = exempt (0%), AA = reduced (5%), S = standard (19% or other non-zero)
fn saft_tax_code(rate: &Decimal) -> &'static str {
    if rate.is_zero() {
        "E"
    } else if *rate == Decimal::from(5) {
        "AA"
    } else {
        "S"
    }
}

// ── Format Decimal to 2dp string ────────────────────────────────────────────
fn dec_str(d: &Decimal) -> String {
    format!("{:.2}", d.round_dp(2))
}

// ── XML helpers ─────────────────────────────────────────────────────────────

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
