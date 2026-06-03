//! SAF-T D406 SourceDocuments section builder.
//!
//! Builds:
//!   SalesInvoices    — from invoices + invoice_line_items (company_id, period)
//!   PurchaseInvoices — from received_invoices (company_id, period)
//!   Payments         — from payments (company_id, period)
//!   MovementOfGoods  — empty mandatory wrapper

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use rust_decimal::Decimal;
use sqlx::Row;

use crate::anaf_decl::saft::masterfiles::{saft_tax_code, write_amount_structure};
use crate::anaf_decl::xml::{end_elem, start_elem, write_text_elem, XmlWriter};
use crate::error::{AppError, AppResult};
use crate::ubl::fx::{amount_to_ron, parse_rate};

// ── XML escaping ───────────────────────────────────────────────────────────────
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn trunc(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or_default()
}

fn dec2(d: Decimal) -> String {
    format!("{:.2}", d)
}

fn dec6(d: Decimal) -> String {
    format!("{:.6}", d)
}

// ── BillingAddress helper (required in CustomerInfo/SupplierInfo) ──────────────
fn write_billing_address(w: &mut XmlWriter, city: &str, country_2: &str) -> AppResult<()> {
    start_elem(w, "BillingAddress")?;
    let city_val = if city.is_empty() { "N/A" } else { city };
    write_text_elem(w, "City", &esc(&trunc(city_val, 35)))?;
    write_text_elem(w, "Country", country_2)?;
    write_text_elem(w, "AddressType", "BillingAddress")?;
    end_elem(w, "BillingAddress")?;
    Ok(())
}

// ── TaxInformationStructure ───────────────────────────────────────────────────
fn write_tax_information(
    w: &mut XmlWriter,
    tax_type: &str,
    tax_code: &str,
    tax_percentage: Decimal,
    tax_base: Decimal,
    tax_amount: Decimal,
) -> AppResult<()> {
    start_elem(w, "TaxInformation")?;
    write_text_elem(w, "TaxType", tax_type)?;
    write_text_elem(w, "TaxCode", tax_code)?;
    write_text_elem(w, "TaxPercentage", &dec2(tax_percentage))?;
    write_text_elem(w, "TaxBase", &dec2(tax_base))?;
    write_amount_structure(w, "TaxAmount", tax_amount)?;
    end_elem(w, "TaxInformation")?;
    Ok(())
}

// ── Payment method mapping ────────────────────────────────────────────────────
fn map_payment_method(method: &str) -> &'static str {
    match method.to_lowercase().as_str() {
        "cash" | "numerar" => "Cash",
        "transfer" | "bank" | "virament" | "op" => "Transfer",
        "card" => "Card",
        "cec" | "check" | "cheque" => "Cheque",
        "bilet" | "biletordin" => "Other",
        _ => "Transfer", // safe default
    }
}

// ────────────────────────────────────────────────────────────────────────────────
// SalesInvoices
// ────────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct SaftLine {
    position: i64,
    description: String,
    quantity: Decimal,
    unit_price: Decimal,
    vat_rate: Decimal,
    vat_category: String,
    subtotal_ron: Decimal,
    vat_ron: Decimal,
    total_ron: Decimal,
    unit: String,
}

pub async fn write_sales_invoices(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<()> {
    // Fetch sales invoices
    let inv_rows = sqlx::query(
        "SELECT i.id, \
                i.series || '-' || printf('%04d', i.number) AS full_number, \
                i.issue_date, \
                COALESCE(c.id, '') AS contact_id, \
                COALESCE(c.legal_name, '') AS client_name, \
                COALESCE(c.city, '') AS client_city, \
                COALESCE(c.country, 'RO') AS client_country, \
                COALESCE(i.subtotal_amount, i.total_amount) AS net_amount, \
                COALESCE(i.vat_amount, '0') AS vat_amount, \
                i.total_amount, \
                COALESCE(i.currency, 'RON') AS currency, \
                i.exchange_rate, \
                i.storno_of_invoice_id \
         FROM invoices i \
         LEFT JOIN contacts c ON i.contact_id = c.id \
         WHERE i.company_id = ?1 \
           AND i.issue_date >= ?2 AND i.issue_date <= ?3 \
           AND i.status IN ('VALIDATED','STORNED') \
         ORDER BY i.issue_date ASC",
    )
    .bind(company_id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(pool)
    .await?;

    let count = inv_rows.len();

    // Batch-fetch line items
    let invoice_ids: Vec<String> = inv_rows
        .iter()
        .map(|r| r.try_get::<String, _>("id").unwrap_or_default())
        .collect();

    let mut lines_by_invoice: HashMap<String, Vec<SaftLine>> = HashMap::new();

    if !invoice_ids.is_empty() {
        let placeholders: String = (1..=invoice_ids.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT invoice_id, position, name, description, quantity, unit_price, \
                    vat_rate, vat_category, subtotal_amount, vat_amount, total_amount, \
                    COALESCE(unit, 'buc') AS unit \
             FROM invoice_line_items \
             WHERE invoice_id IN ({placeholders}) \
             ORDER BY invoice_id, position"
        );
        let mut q = sqlx::query(&sql);
        for id in &invoice_ids {
            q = q.bind(id);
        }
        let line_rows = q.fetch_all(pool).await.map_err(AppError::Database)?;

        for row in line_rows {
            let inv_id: String = row.try_get("invoice_id").unwrap_or_default();
            let name: String = row.try_get("name").unwrap_or_default();
            let desc: String = row.try_get("description").unwrap_or_default();
            let description = if !desc.is_empty() { desc } else { name };
            let quantity = dec(&row
                .try_get::<String, _>("quantity")
                .unwrap_or_else(|_| "0".to_string()));
            let unit_price = dec(&row
                .try_get::<String, _>("unit_price")
                .unwrap_or_else(|_| "0".to_string()));
            let vat_rate = dec(&row
                .try_get::<String, _>("vat_rate")
                .unwrap_or_else(|_| "0".to_string()));
            let vat_category = row
                .try_get::<String, _>("vat_category")
                .unwrap_or_else(|_| "S".to_string());
            let subtotal = dec(&row
                .try_get::<String, _>("subtotal_amount")
                .unwrap_or_else(|_| "0".to_string()));
            let vat = dec(&row
                .try_get::<String, _>("vat_amount")
                .unwrap_or_else(|_| "0".to_string()));
            let total = dec(&row
                .try_get::<String, _>("total_amount")
                .unwrap_or_else(|_| "0".to_string()));
            let unit: String = row.try_get("unit").unwrap_or_else(|_| "buc".to_string());
            let position: i64 = row.try_get("position").unwrap_or(0);

            lines_by_invoice.entry(inv_id).or_default().push(SaftLine {
                position,
                description,
                quantity,
                unit_price,
                vat_rate,
                vat_category,
                subtotal_ron: subtotal, // will be FX-converted below per invoice
                vat_ron: vat,
                total_ron: total,
                unit,
            });
        }
    }

    // Compute totals for NumberOfEntries, TotalDebit (customer debit), TotalCredit (revenue credit)
    let mut total_debit = Decimal::ZERO;
    let mut total_credit = Decimal::ZERO;

    // Pre-scan to compute totals
    for row in &inv_rows {
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx_rate = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        let gross_ron = amount_to_ron(
            dec(&row
                .try_get::<String, _>("total_amount")
                .unwrap_or_else(|_| "0".to_string())),
            &currency,
            fx_rate,
        );
        total_debit += gross_ron;
        total_credit += amount_to_ron(
            dec(&row
                .try_get::<String, _>("net_amount")
                .unwrap_or_else(|_| "0".to_string())),
            &currency,
            fx_rate,
        );
    }

    start_elem(w, "SalesInvoices")?;
    write_text_elem(w, "NumberOfEntries", &count.to_string())?;
    write_text_elem(w, "TotalDebit", &dec2(total_debit))?;
    write_text_elem(w, "TotalCredit", &dec2(total_credit))?;

    for row in &inv_rows {
        let inv_id: String = row.try_get("id").map_err(AppError::Database)?;
        let full_number: String = row.try_get("full_number").map_err(AppError::Database)?;
        let issue_date: String = row.try_get("issue_date").map_err(AppError::Database)?;
        let contact_id: String = row.try_get("contact_id").unwrap_or_default();
        let client_city: String = row.try_get("client_city").unwrap_or_default();
        let client_country: String = row
            .try_get("client_country")
            .unwrap_or_else(|_| "RO".to_string());
        let country_2 = if client_country.len() >= 2 {
            &client_country[..2]
        } else {
            "RO"
        };
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx_rate = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        let storno_of: Option<String> = row.try_get("storno_of_invoice_id").unwrap_or(None);
        let invoice_type = if storno_of.is_some() { "381" } else { "380" };

        let lines = lines_by_invoice.get(&inv_id).cloned().unwrap_or_default();

        // Convert line amounts to RON
        let lines_ron: Vec<SaftLine> = lines
            .into_iter()
            .map(|l| SaftLine {
                subtotal_ron: amount_to_ron(l.subtotal_ron, &currency, fx_rate),
                vat_ron: amount_to_ron(l.vat_ron, &currency, fx_rate),
                total_ron: amount_to_ron(l.total_ron, &currency, fx_rate),
                unit_price: amount_to_ron(l.unit_price, &currency, fx_rate),
                ..l
            })
            .collect();

        // Compute invoice-level totals
        let inv_net_ron = amount_to_ron(
            dec(&row
                .try_get::<String, _>("net_amount")
                .unwrap_or_else(|_| "0".to_string())),
            &currency,
            fx_rate,
        );
        let inv_gross_ron = amount_to_ron(
            dec(&row
                .try_get::<String, _>("total_amount")
                .unwrap_or_else(|_| "0".to_string())),
            &currency,
            fx_rate,
        );

        start_elem(w, "Invoice")?;
        write_text_elem(w, "InvoiceNo", &esc(&trunc(&full_number, 70)))?;

        // CustomerInfo with CustomerID (contact_id) + BillingAddress
        start_elem(w, "CustomerInfo")?;
        if !contact_id.is_empty() {
            write_text_elem(w, "CustomerID", &esc(&trunc(&contact_id, 35)))?;
        } else {
            // CustomerInfo requires either CustomerID or Name
            write_text_elem(w, "Name", "PERSOANA FIZICA")?;
        }
        write_billing_address(w, &client_city, country_2)?;
        end_elem(w, "CustomerInfo")?;

        write_text_elem(w, "AccountID", "4111")?;
        write_text_elem(w, "InvoiceDate", &issue_date)?;
        write_text_elem(w, "InvoiceType", invoice_type)?;
        write_text_elem(w, "SelfBillingIndicator", "false")?;

        // InvoiceLine — at least one required
        if lines_ron.is_empty() {
            // Emit a synthetic line to satisfy minOccurs=1 on InvoiceLine
            let inv_vat_ron = amount_to_ron(
                dec(&row
                    .try_get::<String, _>("vat_amount")
                    .unwrap_or_else(|_| "0".to_string())),
                &currency,
                fx_rate,
            );
            write_invoice_line(
                w,
                1,
                "Serviciu",
                Decimal::ONE,
                inv_net_ron, // UnitPrice (simple decimal)
                inv_net_ron,
                &issue_date,
                "Servicii",
                Decimal::ZERO,
                "S",
                inv_net_ron, // DEFECT 3 FIX: TaxBase = net (not 0)
                inv_vat_ron,
                "C",
                "buc",
            )?;
        } else {
            for line in &lines_ron {
                let code = saft_tax_code(&line.vat_category, line.vat_rate);
                write_invoice_line(
                    w,
                    line.position,
                    &line.description,
                    line.quantity,
                    line.unit_price,
                    line.subtotal_ron,
                    &issue_date,
                    &line.description,
                    line.vat_rate,
                    code,
                    line.subtotal_ron,
                    line.vat_ron,
                    "C", // revenue = credit
                    &line.unit,
                )?;
            }
        }

        // InvoiceDocumentTotals (optional but good practice)
        start_elem(w, "InvoiceDocumentTotals")?;
        write_text_elem(w, "NetTotal", &dec2(inv_net_ron))?;
        write_text_elem(w, "GrossTotal", &dec2(inv_gross_ron))?;
        end_elem(w, "InvoiceDocumentTotals")?;

        end_elem(w, "Invoice")?;
    }

    end_elem(w, "SalesInvoices")?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_invoice_line(
    w: &mut XmlWriter,
    line_no: i64,
    _name: &str,
    quantity: Decimal,
    unit_price: Decimal,
    line_amount: Decimal,
    tax_point_date: &str,
    description: &str,
    tax_pct: Decimal,
    tax_code: &str,
    tax_base: Decimal,
    tax_amount: Decimal,
    dci: &str, // "C" or "D"
    _uom: &str,
) -> AppResult<()> {
    start_elem(w, "InvoiceLine")?;
    write_text_elem(w, "LineNumber", &line_no.to_string())?;
    write_text_elem(w, "AccountID", "707")?;
    write_text_elem(w, "Quantity", &dec6(quantity))?;
    // UnitPrice is SAFmonetaryType (simple decimal), NOT AmountStructure
    write_text_elem(w, "UnitPrice", &dec2(unit_price))?;
    write_text_elem(w, "TaxPointDate", tax_point_date)?;
    write_text_elem(w, "Description", &esc(&trunc(description, 256)))?;
    write_amount_structure(w, "InvoiceLineAmount", line_amount)?;
    write_text_elem(w, "DebitCreditIndicator", dci)?;
    write_tax_information(w, "TVA", tax_code, tax_pct, tax_base, tax_amount)?;
    end_elem(w, "InvoiceLine")?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────────
// PurchaseInvoices
// ────────────────────────────────────────────────────────────────────────────────

/// One per-rate VAT breakdown line fetched from received_invoice_vat_lines.
#[derive(Clone)]
struct PurchaseVatLine {
    vat_category: String,
    vat_rate: Decimal,
    base_amount: Decimal,
    vat_amount: Decimal,
}

/// Normalise a DB-stored vat_rate string to a canonical percent Decimal.
/// The DB may store "0.19" (fraction) or "19" (integer percent).
fn normalize_vat_rate_dec(raw: &str) -> Decimal {
    let s = raw.trim();
    let d = Decimal::from_str(s).unwrap_or(Decimal::ZERO);
    if d < Decimal::ONE && d > Decimal::ZERO {
        // stored as fraction — multiply to percent
        (d * Decimal::from(100)).round_dp(2)
    } else {
        d
    }
}

pub async fn write_purchase_invoices(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<()> {
    let inv_rows = sqlx::query(
        "SELECT id, issuer_cui, issuer_name, series, number, \
                COALESCE(net_amount, total_amount) AS net_amount, \
                COALESCE(vat_amount, '0') AS vat_amount, \
                total_amount, currency, exchange_rate, issue_date \
         FROM received_invoices \
         WHERE company_id = ?1 \
           AND issue_date >= ?2 AND issue_date <= ?3 \
         ORDER BY issue_date ASC",
    )
    .bind(company_id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(pool)
    .await?;

    let count = inv_rows.len();

    // Batch-fetch all VAT lines for the invoices in this period.
    // DEFECT 1 FIX: mirror the D394 approach — query received_invoice_vat_lines.
    let invoice_ids: Vec<String> = inv_rows
        .iter()
        .map(|r| r.try_get::<String, _>("id").unwrap_or_default())
        .collect();

    // Map: invoice_id → Vec<PurchaseVatLine> (in original DB order).
    // We need this before computing totals so the per-line RON values are available.
    let mut vat_lines_by_invoice: HashMap<String, Vec<PurchaseVatLine>> = HashMap::new();
    // Track which invoices have at least one parsed vat_line.
    let mut invoices_with_vat_lines: HashSet<String> = HashSet::new();

    if !invoice_ids.is_empty() {
        let placeholders: String = (1..=invoice_ids.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT received_invoice_id, vat_category, vat_rate, base_amount, vat_amount \
             FROM received_invoice_vat_lines \
             WHERE received_invoice_id IN ({placeholders}) \
             ORDER BY received_invoice_id"
        );
        let mut q = sqlx::query(&sql);
        for id in &invoice_ids {
            q = q.bind(id);
        }
        let vat_rows = q.fetch_all(pool).await.map_err(AppError::Database)?;

        for vr in vat_rows {
            let inv_id: String = vr.try_get("received_invoice_id").unwrap_or_default();
            let vat_category: String = vr
                .try_get("vat_category")
                .unwrap_or_else(|_| "S".to_string());
            let raw_rate: String = vr.try_get("vat_rate").unwrap_or_else(|_| "0".to_string());
            let vat_rate = normalize_vat_rate_dec(&raw_rate);
            let base_amount = dec(&vr.try_get::<String, _>("base_amount").unwrap_or_default());
            let vat_amount = dec(&vr.try_get::<String, _>("vat_amount").unwrap_or_default());

            invoices_with_vat_lines.insert(inv_id.clone());
            vat_lines_by_invoice
                .entry(inv_id)
                .or_default()
                .push(PurchaseVatLine {
                    vat_category,
                    vat_rate,
                    base_amount,
                    vat_amount,
                });
        }
    }

    let mut total_debit = Decimal::ZERO;
    let mut total_credit = Decimal::ZERO;

    for row in &inv_rows {
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx_rate = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        let gross_ron = amount_to_ron(
            dec(&row
                .try_get::<String, _>("total_amount")
                .unwrap_or_else(|_| "0".to_string())),
            &currency,
            fx_rate,
        );
        let net_ron = amount_to_ron(
            dec(&row
                .try_get::<String, _>("net_amount")
                .unwrap_or_else(|_| "0".to_string())),
            &currency,
            fx_rate,
        );
        total_debit += net_ron;
        total_credit += gross_ron;
    }

    start_elem(w, "PurchaseInvoices")?;
    write_text_elem(w, "NumberOfEntries", &count.to_string())?;
    write_text_elem(w, "TotalDebit", &dec2(total_debit))?;
    write_text_elem(w, "TotalCredit", &dec2(total_credit))?;

    for row in &inv_rows {
        let id: String = row.try_get("id").map_err(AppError::Database)?;
        let issuer_cui: String = row.try_get("issuer_cui").unwrap_or_default();
        // Supplier name lives in MasterFiles/Suppliers (keyed by SupplierID); the
        // invoice only needs the SupplierID, so issuer_name is not emitted here.
        let series: Option<String> = row.try_get("series").unwrap_or(None);
        let number: Option<String> = row.try_get("number").unwrap_or(None);
        let issue_date: String = row.try_get("issue_date").map_err(AppError::Database)?;
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx_rate = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );

        let full_number = match (series.as_deref(), number.as_deref()) {
            (Some(s), Some(n)) if !s.is_empty() && !n.is_empty() => format!("{s}-{n}"),
            (Some(s), None) if !s.is_empty() => s.to_string(),
            (None, Some(n)) if !n.is_empty() => n.to_string(),
            _ => id.clone(),
        };

        // Supplier ID = issuer_cui (or id if no CUI)
        let supplier_id = if issuer_cui.is_empty() {
            &id
        } else {
            &issuer_cui
        };

        let net_ron = amount_to_ron(
            dec(&row
                .try_get::<String, _>("net_amount")
                .unwrap_or_else(|_| "0".to_string())),
            &currency,
            fx_rate,
        );
        let vat_ron = amount_to_ron(
            dec(&row
                .try_get::<String, _>("vat_amount")
                .unwrap_or_else(|_| "0".to_string())),
            &currency,
            fx_rate,
        );
        let gross_ron = amount_to_ron(
            dec(&row
                .try_get::<String, _>("total_amount")
                .unwrap_or_else(|_| "0".to_string())),
            &currency,
            fx_rate,
        );

        start_elem(w, "Invoice")?;
        write_text_elem(w, "InvoiceNo", &esc(&trunc(&full_number, 70)))?;

        // SupplierInfo
        start_elem(w, "SupplierInfo")?;
        write_text_elem(w, "SupplierID", &esc(&trunc(supplier_id, 35)))?;
        // DEFECT 2 FIX: BillingAddress — use neutral placeholder, never the supplier name in City.
        start_elem(w, "BillingAddress")?;
        write_text_elem(w, "City", "N/A")?;
        write_text_elem(w, "Country", "RO")?;
        write_text_elem(w, "AddressType", "BillingAddress")?;
        end_elem(w, "BillingAddress")?;
        end_elem(w, "SupplierInfo")?;

        write_text_elem(w, "AccountID", "401")?;
        write_text_elem(w, "InvoiceDate", &issue_date)?;
        write_text_elem(w, "InvoiceType", "380")?;
        write_text_elem(w, "SelfBillingIndicator", "false")?;

        // DEFECT 1 FIX: emit one InvoiceLine per VAT breakdown line.
        // If no vat_lines exist (unparsed invoice), emit a single fallback line
        // using the header totals with a computed rate — never hard-code 19%.
        let has_vat_lines = invoices_with_vat_lines.contains(&id);
        if has_vat_lines {
            // One InvoiceLine per received_invoice_vat_lines entry.
            let vl_vec = vat_lines_by_invoice.get(&id).cloned().unwrap_or_default();
            for (line_no, vl) in vl_vec.iter().enumerate() {
                let base_ron = amount_to_ron(vl.base_amount, &currency, fx_rate);
                let vat_ron_line = amount_to_ron(vl.vat_amount, &currency, fx_rate);
                let tax_code = saft_tax_code(&vl.vat_category, vl.vat_rate);
                start_elem(w, "InvoiceLine")?;
                write_text_elem(w, "LineNumber", &(line_no as i64 + 1).to_string())?;
                write_text_elem(w, "AccountID", "607")?;
                write_text_elem(w, "Quantity", "1.000000")?;
                write_text_elem(w, "UnitPrice", &dec2(base_ron))?;
                write_text_elem(w, "TaxPointDate", &issue_date)?;
                write_text_elem(w, "Description", &esc(&trunc("Achiziție", 256)))?;
                write_amount_structure(w, "InvoiceLineAmount", base_ron)?;
                write_text_elem(w, "DebitCreditIndicator", "D")?;
                write_tax_information(w, "TVA", tax_code, vl.vat_rate, base_ron, vat_ron_line)?;
                end_elem(w, "InvoiceLine")?;
            }
        } else {
            // Fallback: no parsed vat_lines — derive percentage from header totals.
            // DEFECT 1 FIX: never hard-code 19%; compute from actual amounts.
            let tax_pct = if net_ron > Decimal::ZERO {
                ((vat_ron / net_ron) * Decimal::from(100)).round_dp(0)
            } else {
                Decimal::ZERO
            };
            start_elem(w, "InvoiceLine")?;
            write_text_elem(w, "LineNumber", "1")?;
            write_text_elem(w, "AccountID", "607")?;
            write_text_elem(w, "Quantity", "1.000000")?;
            write_text_elem(w, "UnitPrice", &dec2(net_ron))?;
            write_text_elem(w, "TaxPointDate", &issue_date)?;
            write_text_elem(w, "Description", &esc(&trunc("Achiziție", 256)))?;
            write_amount_structure(w, "InvoiceLineAmount", net_ron)?;
            write_text_elem(w, "DebitCreditIndicator", "D")?;
            write_tax_information(w, "TVA", "S", tax_pct, net_ron, vat_ron)?;
            end_elem(w, "InvoiceLine")?;
        }

        // InvoiceDocumentTotals
        start_elem(w, "InvoiceDocumentTotals")?;
        write_text_elem(w, "NetTotal", &dec2(net_ron))?;
        write_text_elem(w, "GrossTotal", &dec2(gross_ron))?;
        end_elem(w, "InvoiceDocumentTotals")?;

        end_elem(w, "Invoice")?;
    }

    end_elem(w, "PurchaseInvoices")?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────────
// Payments
// ────────────────────────────────────────────────────────────────────────────────

pub async fn write_payments(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<()> {
    let pay_rows = sqlx::query(
        "SELECT p.id, p.invoice_id, p.amount, p.currency, p.paid_at, p.method, p.reference, \
                COALESCE(c.id, '') AS contact_id, \
                COALESCE(c.city, '') AS contact_city, \
                COALESCE(c.country, 'RO') AS contact_country \
         FROM payments p \
         JOIN invoices i ON p.invoice_id = i.id \
         LEFT JOIN contacts c ON i.contact_id = c.id \
         WHERE p.company_id = ?1 \
           AND p.paid_at >= ?2 AND p.paid_at <= ?3 \
         ORDER BY p.paid_at ASC",
    )
    .bind(company_id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(pool)
    .await?;

    let count = pay_rows.len();
    let mut total_debit = Decimal::ZERO;
    let mut total_credit = Decimal::ZERO;

    for row in &pay_rows {
        let amount = dec(&row
            .try_get::<String, _>("amount")
            .unwrap_or_else(|_| "0".to_string()));
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        // Payments are already in RON or local currency; convert if needed
        let amount_ron = amount_to_ron(amount, &currency, None); // no FX for payments (stored net)
        total_debit += amount_ron;
        total_credit += amount_ron;
    }

    start_elem(w, "Payments")?;
    write_text_elem(w, "NumberOfEntries", &count.to_string())?;
    write_text_elem(w, "TotalDebit", &dec2(total_debit))?;
    write_text_elem(w, "TotalCredit", &dec2(total_credit))?;

    for row in &pay_rows {
        let id: String = row.try_get("id").map_err(AppError::Database)?;
        let amount = dec(&row
            .try_get::<String, _>("amount")
            .unwrap_or_else(|_| "0".to_string()));
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let amount_ron = amount_to_ron(amount, &currency, None);
        let paid_at: String = row.try_get("paid_at").map_err(AppError::Database)?;
        let method: String = row
            .try_get("method")
            .unwrap_or_else(|_| "transfer".to_string());
        let reference: Option<String> = row.try_get("reference").unwrap_or(None);
        let contact_id: String = row.try_get("contact_id").unwrap_or_default();

        let pay_method = map_payment_method(&method);
        let pay_ref = reference.as_deref().unwrap_or(&id);
        // Truncate paid_at date to 10 chars (YYYY-MM-DD)
        let paid_date = &paid_at[..paid_at.len().min(10)];

        start_elem(w, "Payment")?;
        write_text_elem(w, "PaymentRefNo", &esc(&trunc(pay_ref, 35)))?;
        write_text_elem(w, "TransactionDate", paid_date)?;
        write_text_elem(w, "PaymentMethod", pay_method)?;
        write_text_elem(
            w,
            "Description",
            &esc(&trunc(&format!("Plată {pay_method} {pay_ref}"), 256)),
        )?;

        // PaymentLine (required, maxOccurs=unbounded)
        start_elem(w, "PaymentLine")?;
        write_text_elem(w, "AccountID", "5121")?; // bank account
        write_text_elem(w, "CustomerID", &esc(&trunc(&contact_id, 35)))?;
        write_text_elem(w, "SupplierID", &esc(&trunc(&contact_id, 35)))?;
        write_text_elem(w, "DebitCreditIndicator", "D")?; // bank = debit (money in)
        write_amount_structure(w, "PaymentLineAmount", amount_ron)?;
        // TaxInformation required (maxOccurs=unbounded)
        write_tax_information(w, "TVA", "S", Decimal::ZERO, amount_ron, Decimal::ZERO)?;
        end_elem(w, "PaymentLine")?;

        end_elem(w, "Payment")?;
    }

    end_elem(w, "Payments")?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────────
// MovementOfGoods — empty mandatory wrapper
// ────────────────────────────────────────────────────────────────────────────────

pub fn write_movement_of_goods(w: &mut XmlWriter) -> AppResult<()> {
    start_elem(w, "MovementOfGoods")?;
    write_text_elem(w, "NumberOfMovementLines", "0")?;
    write_text_elem(w, "TotalQuantityReceived", "0.000000")?;
    write_text_elem(w, "TotalQuantityIssued", "0.000000")?;
    end_elem(w, "MovementOfGoods")?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────────
// SourceDocuments top-level
// ────────────────────────────────────────────────────────────────────────────────

pub async fn write_source_documents(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<()> {
    start_elem(w, "SourceDocuments")?;
    write_sales_invoices(w, pool, company_id, date_from, date_to).await?;
    write_purchase_invoices(w, pool, company_id, date_from, date_to).await?;
    write_payments(w, pool, company_id, date_from, date_to).await?;
    write_movement_of_goods(w)?;
    end_elem(w, "SourceDocuments")?;
    Ok(())
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payment_method_mapping() {
        assert_eq!(map_payment_method("transfer"), "Transfer");
        assert_eq!(map_payment_method("cash"), "Cash");
        assert_eq!(map_payment_method("card"), "Card");
        assert_eq!(map_payment_method("cec"), "Cheque");
        assert_eq!(map_payment_method("TRANSFER"), "Transfer");
        assert_eq!(map_payment_method("unknown_xyz"), "Transfer"); // safe default
    }

    #[test]
    fn normalize_vat_rate_dec_handles_fractions_and_integers() {
        assert_eq!(normalize_vat_rate_dec("0.19"), Decimal::from(19));
        assert_eq!(normalize_vat_rate_dec("0.09"), Decimal::from(9));
        assert_eq!(normalize_vat_rate_dec("0.05"), Decimal::from(5));
        assert_eq!(normalize_vat_rate_dec("0.11"), Decimal::from(11));
        assert_eq!(normalize_vat_rate_dec("19"), Decimal::from(19));
        assert_eq!(normalize_vat_rate_dec("9"), Decimal::from(9));
        assert_eq!(normalize_vat_rate_dec("21"), Decimal::from(21));
        assert_eq!(normalize_vat_rate_dec("0"), Decimal::ZERO);
        assert_eq!(normalize_vat_rate_dec(""), Decimal::ZERO);
    }

    /// DEFECT 1 FIX: a purchase invoice with TWO vat_lines (21% + 11%) must
    /// produce TWO InvoiceLines with correct per-line TaxCode/TaxPercentage/TaxBase/TaxAmount,
    /// not a single 19% line.
    #[tokio::test]
    async fn purchase_multi_rate_vat_lines_produce_two_invoice_lines() {
        use sqlx::sqlite::SqlitePoolOptions;
        use sqlx::Executor;

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        pool.execute(sqlx::query(
            "CREATE TABLE received_invoices (
                id TEXT PRIMARY KEY, company_id TEXT,
                anaf_download_id TEXT, anaf_index TEXT,
                issuer_cui TEXT, issuer_name TEXT,
                series TEXT, number TEXT,
                total_amount TEXT, net_amount TEXT, vat_amount TEXT,
                currency TEXT, exchange_rate REAL, issue_date TEXT,
                xml_path TEXT, pdf_path TEXT, status TEXT,
                downloaded_at INTEGER, created_at INTEGER
            )",
        ))
        .await
        .unwrap();

        pool.execute(sqlx::query(
            "CREATE TABLE received_invoice_vat_lines (
                id TEXT PRIMARY KEY,
                received_invoice_id TEXT,
                vat_category TEXT,
                vat_rate TEXT,
                base_amount TEXT,
                vat_amount TEXT
            )",
        ))
        .await
        .unwrap();

        // Seed: one invoice with net=1100, vat=251 (1000*21%=210 + 100*11%=11 +
        //       — simplified: 1000@21% + 100@11%)
        sqlx::query(
            "INSERT INTO received_invoices VALUES ('ri-1','co-1',NULL,NULL,'RO111','Furnizor Test',\
             'FC','100','1321.00','1100.00','221.00','RON',NULL,'2025-03-01',NULL,NULL,'APPROVED',0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // VAT line 1: 21%  — base 1000, vat 210
        sqlx::query(
            "INSERT INTO received_invoice_vat_lines VALUES ('vl-1','ri-1','S','21','1000.00','210.00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // VAT line 2: 11%  — base 100, vat 11
        sqlx::query(
            "INSERT INTO received_invoice_vat_lines VALUES ('vl-2','ri-1','S','11','100.00','11.00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut w = crate::anaf_decl::xml::new_writer().unwrap();
        write_purchase_invoices(&mut w, &pool, "co-1", "2025-01-01", "2025-12-31")
            .await
            .expect("write_purchase_invoices must succeed");
        let xml = crate::anaf_decl::xml::finish(w).unwrap();

        // Must have exactly 2 InvoiceLine elements
        let line_count = xml.matches("<InvoiceLine>").count();
        assert_eq!(
            line_count, 2,
            "Expected 2 InvoiceLines for multi-rate purchase; got {line_count}. XML:\n{xml}"
        );

        // First line: 21% → TaxCode "S", TaxPercentage "21.00"
        assert!(
            xml.contains("<TaxPercentage>21.00</TaxPercentage>"),
            "Expected TaxPercentage 21.00 for 21% VAT line. XML:\n{xml}"
        );
        // Second line: 11% → TaxCode "AA" (reduced rate S+11)
        assert!(
            xml.contains("<TaxPercentage>11.00</TaxPercentage>"),
            "Expected TaxPercentage 11.00 for 11% VAT line. XML:\n{xml}"
        );
        assert!(
            xml.contains("<TaxCode>AA</TaxCode>"),
            "Expected TaxCode AA for 11% reduced rate. XML:\n{xml}"
        );

        // TaxBase must equal the per-line base (1000 and 100), NOT 0
        assert!(
            xml.contains("<TaxBase>1000.00</TaxBase>"),
            "Expected TaxBase 1000.00 for first line. XML:\n{xml}"
        );
        assert!(
            xml.contains("<TaxBase>100.00</TaxBase>"),
            "Expected TaxBase 100.00 for second line. XML:\n{xml}"
        );

        // Must NOT contain the old hard-coded 19%
        assert!(
            !xml.contains("<TaxPercentage>19.00</TaxPercentage>"),
            "Must not hard-code 19% for multi-rate invoice. XML:\n{xml}"
        );
    }

    /// DEFECT 1 FIX: purchase invoice with NO vat_lines → one fallback line
    /// with TaxBase = header net (not 0).
    #[tokio::test]
    async fn purchase_no_vat_lines_fallback_uses_header_net() {
        use sqlx::sqlite::SqlitePoolOptions;
        use sqlx::Executor;

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        pool.execute(sqlx::query(
            "CREATE TABLE received_invoices (
                id TEXT PRIMARY KEY, company_id TEXT,
                anaf_download_id TEXT, anaf_index TEXT,
                issuer_cui TEXT, issuer_name TEXT,
                series TEXT, number TEXT,
                total_amount TEXT, net_amount TEXT, vat_amount TEXT,
                currency TEXT, exchange_rate REAL, issue_date TEXT,
                xml_path TEXT, pdf_path TEXT, status TEXT,
                downloaded_at INTEGER, created_at INTEGER
            )",
        ))
        .await
        .unwrap();

        pool.execute(sqlx::query(
            "CREATE TABLE received_invoice_vat_lines (
                id TEXT PRIMARY KEY,
                received_invoice_id TEXT,
                vat_category TEXT,
                vat_rate TEXT,
                base_amount TEXT,
                vat_amount TEXT
            )",
        ))
        .await
        .unwrap();

        // Seed: one invoice with net=1000, vat=50 (a genuine 5% rate) — NO vat_lines.
        // Chosen NON-19% so the test proves the fallback COMPUTES the rate from the
        // header amounts (round(50/1000*100)=5) rather than hard-coding 19%.
        sqlx::query(
            "INSERT INTO received_invoices VALUES ('ri-2','co-2',NULL,NULL,'RO222','Furnizor 2',\
             'FACT','002','1050.00','1000.00','50.00','RON',NULL,'2025-03-01',NULL,NULL,'APPROVED',0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut w = crate::anaf_decl::xml::new_writer().unwrap();
        write_purchase_invoices(&mut w, &pool, "co-2", "2025-01-01", "2025-12-31")
            .await
            .expect("write_purchase_invoices must succeed");
        let xml = crate::anaf_decl::xml::finish(w).unwrap();

        // Must have exactly 1 InvoiceLine
        let line_count = xml.matches("<InvoiceLine>").count();
        assert_eq!(
            line_count, 1,
            "Expected 1 fallback InvoiceLine; got {line_count}. XML:\n{xml}"
        );

        // TaxBase must be 1000.00 (header net), NOT 0
        assert!(
            xml.contains("<TaxBase>1000.00</TaxBase>"),
            "Fallback line TaxBase must equal header net (1000.00). XML:\n{xml}"
        );

        // TaxAmount must be 50.00 (header vat)
        assert!(
            xml.contains(">50.00<"),
            "Fallback line must carry vat_amount 50.00. XML:\n{xml}"
        );

        // Rate must be COMPUTED from the amounts (5%), proving it is NOT hard-coded 19%.
        assert!(
            xml.contains("<TaxPercentage>5.00</TaxPercentage>"),
            "Fallback rate must be computed from amounts (5%). XML:\n{xml}"
        );
        assert!(
            !xml.contains("<TaxPercentage>19.00</TaxPercentage>"),
            "Fallback must not hard-code 19%. XML:\n{xml}"
        );
    }
}
