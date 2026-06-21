//! SAF-T D406 SourceDocuments section builder.
//!
//! Builds:
//!   SalesInvoices    — from invoices + invoice_line_items (company_id, period)
//!   PurchaseInvoices — from received_invoices (company_id, period)
//!   Payments         — from payments (company_id, period)
//!   MovementOfGoods  — from stock_movements + stock_movement_lines (company_id, period)

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use rust_decimal::Decimal;
use sqlx::Row;

use crate::anaf_decl::saft::masterfiles::{
    canonical_partner_id, saft_tax_code_dir, write_amount_structure, TaxDirection,
};
use crate::anaf_decl::xml::{end_elem, start_elem, trunc, write_text_elem, XmlWriter};
use crate::error::{AppError, AppResult};
use crate::ubl::fx::{amount_to_ron, parse_rate};

// ── XML escaping ───────────────────────────────────────────────────────────────
use crate::anaf_decl::xml_esc as esc;

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
// Nom_Mecanisme_plati (PaymentMethod column): 01=Cash, 02=Compensare, 03=Fără numerar,
// 98=Definit de comun acord, 99=Instrument nedefinit.
fn map_payment_method(method: &str) -> &'static str {
    match method.to_lowercase().as_str() {
        "cash" | "numerar" => "01",
        "transfer" | "bank" | "virament" | "op" | "bancar" => "03",
        "card" => "03",
        "offset" | "compensare" | "netting" | "comp" => "02",
        "cec" | "check" | "cheque" => "03", // non-cash instrument
        _ => "99",                          // undefined instrument
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
    revenue_kind: String,
}

/// SAF-T revenue AccountID for a sales line by its kind — must match db::gl::revenue_account.
fn saft_revenue_account(revenue_kind: &str) -> &'static str {
    match revenue_kind.trim() {
        "product" => "701",
        "service" => "704",
        "reduction" => "709",
        _ => "707",
    }
}

pub async fn write_sales_invoices(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<()> {
    // Fetch sales invoices (also fetch contact CUI for canonical ID derivation)
    let inv_rows = sqlx::query(
        "SELECT i.id, \
                i.series || '-' || printf('%04d', i.number) AS full_number, \
                i.issue_date, \
                COALESCE(c.id, '') AS contact_id, \
                COALESCE(c.cui, '') AS contact_cui, \
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
                    COALESCE(unit, 'buc') AS unit, COALESCE(revenue_kind,'goods') AS revenue_kind \
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
            let revenue_kind: String = row
                .try_get("revenue_kind")
                .unwrap_or_else(|_| "goods".to_string());

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
                revenue_kind,
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
        let contact_id_raw: String = row.try_get("contact_id").unwrap_or_default();
        // Derive canonical ID from contact; we need the CUI for this — fetch from query result
        let contact_cui: String = row.try_get("contact_cui").unwrap_or_default();
        let contact_id = canonical_partner_id(&contact_id_raw, &contact_cui);
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
        // SelfBillingIndicator: "0" = not self-billed, "389" = self-billed
        write_text_elem(w, "SelfBillingIndicator", "0")?;

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
            let fallback_code = saft_tax_code_dir("S", Decimal::ZERO, TaxDirection::Sales);
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
                fallback_code,
                inv_net_ron, // DEFECT 3 FIX: TaxBase = net (not 0)
                inv_vat_ron,
                "C",
                "H87",
                "707", // synthetic fallback line → default revenue account
            )?;
        } else {
            for line in &lines_ron {
                let code =
                    saft_tax_code_dir(&line.vat_category, line.vat_rate, TaxDirection::Sales);
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
                    saft_revenue_account(&line.revenue_kind),
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
    account_id: &str, // revenue account 701/704/707/709 (must match the GL)
) -> AppResult<()> {
    start_elem(w, "InvoiceLine")?;
    write_text_elem(w, "LineNumber", &line_no.to_string())?;
    write_text_elem(w, "AccountID", account_id)?;
    write_text_elem(w, "Quantity", &dec6(quantity))?;
    // UnitPrice is SAFmonetaryType (simple decimal), NOT AmountStructure
    write_text_elem(w, "UnitPrice", &dec2(unit_price))?;
    write_text_elem(w, "TaxPointDate", tax_point_date)?;
    write_text_elem(w, "Description", &esc(&trunc(description, 256)))?;
    write_amount_structure(w, "InvoiceLineAmount", line_amount)?;
    write_text_elem(w, "DebitCreditIndicator", dci)?;
    write_tax_information(w, "300", tax_code, tax_pct, tax_base, tax_amount)?;
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
        (d * Decimal::from(100))
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
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

        // Supplier ID = canonical CUI-based ID (RO-stripped), or sanitized uuid
        let supplier_canon_id = canonical_partner_id(&id, &issuer_cui);
        let supplier_id = &supplier_canon_id;

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
        // SelfBillingIndicator: "0" = not self-billed
        write_text_elem(w, "SelfBillingIndicator", "0")?;

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
                let tax_code =
                    saft_tax_code_dir(&vl.vat_category, vl.vat_rate, TaxDirection::Purchase);
                start_elem(w, "InvoiceLine")?;
                write_text_elem(w, "LineNumber", &(line_no as i64 + 1).to_string())?;
                write_text_elem(w, "AccountID", "607")?;
                write_text_elem(w, "Quantity", "1.000000")?;
                write_text_elem(w, "UnitPrice", &dec2(base_ron))?;
                write_text_elem(w, "TaxPointDate", &issue_date)?;
                write_text_elem(w, "Description", &esc(&trunc("Achiziție", 256)))?;
                write_amount_structure(w, "InvoiceLineAmount", base_ron)?;
                write_text_elem(w, "DebitCreditIndicator", "D")?;
                write_tax_information(w, "300", tax_code, vl.vat_rate, base_ron, vat_ron_line)?;
                end_elem(w, "InvoiceLine")?;
            }
        } else {
            // Fallback: no parsed vat_lines — derive percentage from header totals.
            // DEFECT 1 FIX: never hard-code 19%; compute from actual amounts.
            let tax_pct = if net_ron > Decimal::ZERO {
                ((vat_ron / net_ron) * Decimal::from(100))
                    .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
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
            let fallback_code = saft_tax_code_dir("S", tax_pct, TaxDirection::Purchase);
            write_tax_information(w, "300", fallback_code, tax_pct, net_ron, vat_ron)?;
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
                COALESCE(c.cui, '') AS contact_cui, \
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
        let contact_id_raw: String = row.try_get("contact_id").unwrap_or_default();
        let contact_cui: String = row.try_get("contact_cui").unwrap_or_default();
        // Use canonical partner ID so it matches MasterFiles and GL
        let contact_canon_id = canonical_partner_id(&contact_id_raw, &contact_cui);

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
        write_text_elem(w, "CustomerID", &esc(&trunc(&contact_canon_id, 35)))?;
        write_text_elem(w, "SupplierID", &esc(&trunc(&contact_canon_id, 35)))?;
        write_text_elem(w, "DebitCreditIndicator", "D")?; // bank = debit (money in)
        write_amount_structure(w, "PaymentLineAmount", amount_ron)?;
        // TaxInformation required (maxOccurs=unbounded)
        write_tax_information(w, "000", "000000", Decimal::ZERO, amount_ron, Decimal::ZERO)?;
        end_elem(w, "PaymentLine")?;

        end_elem(w, "Payment")?;
    }

    end_elem(w, "Payments")?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────────
// MovementOfGoods — populated from stock_movements + stock_movement_lines
// ────────────────────────────────────────────────────────────────────────────────

pub async fn write_movement_of_goods(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
    is_annual: bool,
) -> AppResult<()> {
    // DUK production validator (L-type): treats all MovementOfGoods sub-elements as max:0.
    // Only populate for annual (D406A) declarations; periodic must emit the empty wrapper.
    if !is_annual {
        start_elem(w, "MovementOfGoods")?;
        end_elem(w, "MovementOfGoods")?;
        return Ok(());
    }

    // Query stock movements in the period.
    let movement_rows = sqlx::query(
        "SELECT id, movement_ref, movement_date, posting_date, movement_type, \
                direction, document_type, document_number \
         FROM stock_movements \
         WHERE company_id = ?1 \
           AND movement_date >= ?2 \
           AND movement_date <= ?3 \
         ORDER BY movement_date ASC, movement_ref ASC",
    )
    .bind(company_id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(pool)
    .await?;

    // When there are no movements, emit the empty mandatory wrapper (no children).
    // DUK rule: NumberOfMovementLines=0 on an empty section triggers an error.
    if movement_rows.is_empty() {
        start_elem(w, "MovementOfGoods")?;
        end_elem(w, "MovementOfGoods")?;
        return Ok(());
    }

    // Collect movement PKs for batch line fetch.
    use sqlx::Row;
    let movement_ids: Vec<String> = movement_rows
        .iter()
        .map(|r| r.try_get::<String, _>("id").unwrap_or_default())
        .collect();

    // Batch-fetch all lines for these movements.
    let placeholders: String = (1..=movement_ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let line_sql = format!(
        "SELECT movement_id, line_number, product_code, account_id, \
                customer_id, supplier_id, quantity, unit_of_measure, \
                uom_conv_factor, book_value, movement_subtype, comments \
         FROM stock_movement_lines \
         WHERE movement_id IN ({placeholders}) \
         ORDER BY movement_id, line_number"
    );
    let mut q = sqlx::query(&line_sql);
    for id in &movement_ids {
        q = q.bind(id);
    }
    let line_rows = q.fetch_all(pool).await.map_err(AppError::Database)?;

    use std::collections::HashMap;
    // Group lines by movement_id.
    struct LineData {
        line_number: i64,
        product_code: String,
        account_id: String,
        customer_id: String,
        supplier_id: String,
        quantity: Decimal,
        unit_of_measure: String,
        uom_conv_factor: String,
        book_value: Decimal,
        movement_subtype: String,
        comments: Option<String>,
    }

    let mut lines_by_movement: HashMap<String, Vec<LineData>> = HashMap::new();
    let mut total_received = Decimal::ZERO;
    let mut total_issued = Decimal::ZERO;
    let mut total_line_count: u64 = 0;

    for lr in &line_rows {
        let mid: String = lr.try_get("movement_id").unwrap_or_default();
        let qty = dec(&lr
            .try_get::<String, _>("quantity")
            .unwrap_or_else(|_| "0".to_string()));
        let line_number: i64 = lr.try_get("line_number").unwrap_or(1);
        let product_code: String = lr.try_get("product_code").unwrap_or_default();
        let account_id: String = lr
            .try_get("account_id")
            .unwrap_or_else(|_| "371".to_string());
        let customer_id: String = lr
            .try_get("customer_id")
            .unwrap_or_else(|_| "0".to_string());
        let supplier_id: String = lr
            .try_get("supplier_id")
            .unwrap_or_else(|_| "0".to_string());
        let uom: String = lr
            .try_get("unit_of_measure")
            .unwrap_or_else(|_| "H87".to_string());
        let uom_cf: String = lr
            .try_get("uom_conv_factor")
            .unwrap_or_else(|_| "1".to_string());
        let bv = dec(&lr
            .try_get::<String, _>("book_value")
            .unwrap_or_else(|_| "0".to_string()));
        let subtype: String = lr
            .try_get("movement_subtype")
            .unwrap_or_else(|_| "10".to_string());
        let comments: Option<String> = lr.try_get("comments").unwrap_or(None);

        total_line_count += 1;
        lines_by_movement.entry(mid).or_default().push(LineData {
            line_number,
            product_code,
            account_id,
            customer_id,
            supplier_id,
            quantity: qty,
            unit_of_measure: uom,
            uom_conv_factor: uom_cf,
            book_value: bv,
            movement_subtype: subtype,
            comments,
        });
    }

    // Compute received / issued totals by scanning movement direction.
    for mr in &movement_rows {
        let mid: String = mr.try_get("id").unwrap_or_default();
        let direction: String = mr.try_get("direction").unwrap_or_else(|_| "IN".to_string());
        if let Some(lines) = lines_by_movement.get(&mid) {
            let qty_sum: Decimal = lines.iter().map(|l| l.quantity).sum();
            if direction.eq_ignore_ascii_case("IN") {
                total_received += qty_sum;
            } else {
                total_issued += qty_sum;
            }
        }
    }

    start_elem(w, "MovementOfGoods")?;
    write_text_elem(w, "NumberOfMovementLines", &total_line_count.to_string())?;
    write_text_elem(w, "TotalQuantityReceived", &dec6(total_received))?;
    write_text_elem(w, "TotalQuantityIssued", &dec6(total_issued))?;

    for mr in &movement_rows {
        let mid: String = mr.try_get("id").map_err(AppError::Database)?;
        let movement_ref: String = mr.try_get("movement_ref").map_err(AppError::Database)?;
        let movement_date: String = mr.try_get("movement_date").map_err(AppError::Database)?;
        let posting_date: Option<String> = mr.try_get("posting_date").unwrap_or(None);
        let movement_type: String = mr
            .try_get("movement_type")
            .unwrap_or_else(|_| "10".to_string());
        let doc_type: Option<String> = mr.try_get("document_type").unwrap_or(None);
        let doc_number: Option<String> = mr.try_get("document_number").unwrap_or(None);

        let lines = lines_by_movement.get(&mid);
        if lines.is_none() {
            continue; // skip movements without lines (shouldn't happen, but be defensive)
        }
        let lines = lines.unwrap();

        start_elem(w, "StockMovement")?;
        write_text_elem(w, "MovementReference", &esc(&trunc(&movement_ref, 35)))?;
        write_text_elem(w, "MovementDate", &movement_date)?;
        if let Some(ref pd) = posting_date {
            if !pd.is_empty() && pd != &movement_date {
                write_text_elem(w, "MovementPostingDate", pd)?;
            }
        }
        write_text_elem(w, "MovementType", &movement_type)?;
        if let (Some(dt), Some(dn)) = (doc_type.as_deref(), doc_number.as_deref()) {
            if !dt.is_empty() || !dn.is_empty() {
                start_elem(w, "DocumentReference")?;
                write_text_elem(w, "DocumentType", &esc(&trunc(dt, 9)))?;
                write_text_elem(w, "DocumentNumber", &esc(&trunc(dn, 35)))?;
                end_elem(w, "DocumentReference")?;
            }
        }

        for line in lines {
            start_elem(w, "StockMovementLine")?;
            write_text_elem(w, "LineNumber", &line.line_number.to_string())?;
            write_text_elem(w, "AccountID", &esc(&trunc(&line.account_id, 35)))?;
            // CustomerID + SupplierID are required in StockMovementLine — canonicalize
            // the stored value to the DUK ID format ("00"+CUI / "0"), like every other
            // SAF-T section, treating the stored id as the partner CUI.
            let cid = canonical_partner_id("", &line.customer_id);
            let sid = canonical_partner_id("", &line.supplier_id);
            write_text_elem(w, "CustomerID", &esc(&trunc(&cid, 35)))?;
            write_text_elem(w, "SupplierID", &esc(&trunc(&sid, 35)))?;
            write_text_elem(w, "ProductCode", &esc(&trunc(&line.product_code, 70)))?;
            write_text_elem(w, "Quantity", &dec6(line.quantity))?;
            // UnitOfMeasure + UOMToUOMPhysicalStockConversionFactor (xs:sequence, both required)
            write_text_elem(w, "UnitOfMeasure", &trunc(&line.unit_of_measure, 9))?;
            write_text_elem(
                w,
                "UOMToUOMPhysicalStockConversionFactor",
                &line.uom_conv_factor,
            )?;
            if line.book_value != Decimal::ZERO {
                write_text_elem(w, "BookValue", &dec2(line.book_value))?;
            }
            write_text_elem(w, "MovementSubType", &line.movement_subtype)?;
            if let Some(ref c) = line.comments {
                if !c.is_empty() {
                    write_text_elem(w, "MovementComments", &esc(&trunc(c, 256)))?;
                }
            }
            end_elem(w, "StockMovementLine")?;
        }

        end_elem(w, "StockMovement")?;
    }

    end_elem(w, "MovementOfGoods")?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────────
// AssetTransactions (SourceDocuments sub-section; A-profile only)
// ────────────────────────────────────────────────────────────────────────────────

/// Map the stored transaction_type string to the DUK AssetTransactionType numeric code.
/// Valid DUK codes: {10,20,30,40,50,60,70,80,90,100,110,120,130}
fn map_asset_txn_type(raw: &str) -> &'static str {
    // Raw may already be a numeric string (stored as "10","30",...) or a keyword.
    match raw.trim() {
        // Already-numeric codes — pass through if valid
        "10" => "10",
        "20" => "20",
        "30" => "30",
        "40" => "40",
        "50" => "50",
        "60" => "60",
        "70" => "70",
        "80" => "80",
        "90" => "90",
        "100" => "100",
        "110" => "110",
        "120" => "120",
        "130" => "130",
        // Keyword aliases
        "acquisition" | "achizitie" | "achizitii" => "10",
        "sale" | "vanzare" | "vânzare" => "20",
        "depreciation" | "amortizare" | "amortissement" => "30",
        "transfer" => "40",
        "scrap" | "casare" => "50",
        // Anything else → 130 (other)
        _ => "130",
    }
}

pub async fn write_asset_transactions(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<()> {
    let txn_rows = sqlx::query(
        "SELECT id, asset_id, transaction_code, transaction_type, transaction_date, \
                description, gl_transaction_id, acq_prod_cost, book_value, amount \
         FROM asset_transactions \
         WHERE company_id = ?1 \
           AND transaction_date >= ?2 \
           AND transaction_date <= ?3 \
         ORDER BY transaction_date ASC, transaction_code ASC",
    )
    .bind(company_id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(pool)
    .await?;

    let count = txn_rows.len();

    start_elem(w, "AssetTransactions")?;
    write_text_elem(w, "NumberOfAssetTransactions", &count.to_string())?;

    for row in &txn_rows {
        let txn_id: String = row.try_get("transaction_code").unwrap_or_default();
        let asset_id: String = row.try_get("asset_id").unwrap_or_default();
        let txn_type_raw: String = row
            .try_get("transaction_type")
            .unwrap_or_else(|_| "130".to_string());
        let txn_type = map_asset_txn_type(&txn_type_raw);
        let description: String = row.try_get("description").unwrap_or_default();
        let txn_date: String = row.try_get("transaction_date").unwrap_or_default();
        let gl_txn_id: Option<String> = row.try_get("gl_transaction_id").unwrap_or(None);
        let acq_cost_raw: String = row
            .try_get("acq_prod_cost")
            .unwrap_or_else(|_| "0.00".to_string());
        let book_val_raw: String = row
            .try_get("book_value")
            .unwrap_or_else(|_| "0.00".to_string());
        let amount_raw: String = row.try_get("amount").unwrap_or_else(|_| "0.00".to_string());

        let acq_cost = dec(&acq_cost_raw);
        let book_val = dec(&book_val_raw);
        let amount = dec(&amount_raw);

        // TransactionID: use gl_transaction_id if present, otherwise synthesize
        let gl_ref = gl_txn_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&txn_id);

        start_elem(w, "AssetTransaction")?;
        write_text_elem(w, "AssetTransactionID", &esc(&trunc(&txn_id, 35)))?;
        write_text_elem(w, "AssetID", &esc(&trunc(&asset_id, 35)))?;
        write_text_elem(w, "AssetTransactionType", txn_type)?;
        if !description.is_empty() {
            write_text_elem(w, "Description", &esc(&trunc(&description, 256)))?;
        }
        write_text_elem(
            w,
            "AssetTransactionDate",
            &txn_date[..txn_date.len().min(10)],
        )?;
        write_text_elem(w, "TransactionID", &esc(&trunc(gl_ref, 255)))?;

        // AssetTransactionValuations → AssetTransactionValuation (1..N)
        start_elem(w, "AssetTransactionValuations")?;
        start_elem(w, "AssetTransactionValuation")?;
        // AssetValuationType is optional — emit "fiscal"
        write_text_elem(w, "AssetValuationType", "fiscal")?;
        write_text_elem(
            w,
            "AcquisitionAndProductionCostsOnTransaction",
            &dec2(acq_cost),
        )?;
        write_text_elem(w, "BookValueOnTransaction", &dec2(book_val))?;
        write_text_elem(w, "AssetTransactionAmount", &dec2(amount))?;
        end_elem(w, "AssetTransactionValuation")?;
        end_elem(w, "AssetTransactionValuations")?;

        end_elem(w, "AssetTransaction")?;
    }

    end_elem(w, "AssetTransactions")?;
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
    is_annual: bool,
) -> AppResult<()> {
    start_elem(w, "SourceDocuments")?;

    if is_annual {
        // A-profile: SalesInvoices/PurchaseInvoices/Payments/MovementOfGoods are
        // forbidden (max:0 children) — emit empty wrappers only.
        // AssetTransactions is REQUIRED and POPULATED.
        start_elem(w, "SalesInvoices")?;
        end_elem(w, "SalesInvoices")?;
        start_elem(w, "PurchaseInvoices")?;
        end_elem(w, "PurchaseInvoices")?;
        start_elem(w, "Payments")?;
        end_elem(w, "Payments")?;
        start_elem(w, "MovementOfGoods")?;
        end_elem(w, "MovementOfGoods")?;
        write_asset_transactions(w, pool, company_id, date_from, date_to).await?;
    } else {
        // L-profile: SalesInvoices/PurchaseInvoices/Payments/MovementOfGoods are
        // populated; AssetTransactions is forbidden — do NOT emit it.
        write_sales_invoices(w, pool, company_id, date_from, date_to).await?;
        write_purchase_invoices(w, pool, company_id, date_from, date_to).await?;
        write_payments(w, pool, company_id, date_from, date_to).await?;
        write_movement_of_goods(w, pool, company_id, date_from, date_to, false).await?;
    }

    end_elem(w, "SourceDocuments")?;
    Ok(())
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payment_method_mapping() {
        // Nom_Mecanisme_plati codes: 01=Cash, 02=Compensare, 03=Fără numerar, 99=nedefinit
        assert_eq!(map_payment_method("transfer"), "03");
        assert_eq!(map_payment_method("cash"), "01");
        assert_eq!(map_payment_method("numerar"), "01");
        assert_eq!(map_payment_method("card"), "03");
        assert_eq!(map_payment_method("cec"), "03");
        assert_eq!(map_payment_method("offset"), "02");
        assert_eq!(map_payment_method("compensare"), "02");
        assert_eq!(map_payment_method("TRANSFER"), "03");
        assert_eq!(map_payment_method("unknown_xyz"), "99"); // undefined instrument
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

    #[test]
    fn saft_revenue_account_maps_every_kind() {
        // SAF-T revenue lines must hit the right RO chart account (wrong account = wrong D406).
        assert_eq!(saft_revenue_account("product"), "701");
        assert_eq!(saft_revenue_account("service"), "704");
        assert_eq!(saft_revenue_account("reduction"), "709");
        assert_eq!(saft_revenue_account("goods"), "707"); // default
        assert_eq!(saft_revenue_account("anything_else"), "707");
        assert_eq!(saft_revenue_account(" service "), "704"); // trimmed
    }

    /// DEFECT 1 FIX: a purchase invoice with TWO vat_lines (21% + 11%) must
    /// produce TWO InvoiceLines with correct per-line TaxCode/TaxPercentage/TaxBase/TaxAmount,
    /// not a single 19% line.
    #[tokio::test]
    async fn purchase_multi_rate_vat_lines_produce_two_invoice_lines() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory DB");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");

        // Seed company required by received_invoices.company_id FK.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, vat_payer, address, city, county, country) \
             VALUES ('co-1', 'RO999', 'Firma Test SRL', 1, 'Str 1', 'Buc', 'B', 'RO')",
        )
        .execute(&pool)
        .await
        .expect("seed company");

        // Seed: one invoice with net=1100, vat=221 (1000@21% + 100@11%).
        // Use named columns — real schema has anaf_download_id NOT NULL UNIQUE,
        // issuer_cui/issuer_name NOT NULL, xml_path NOT NULL.
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, \
              series, number, total_amount, currency, issue_date, xml_path, status, \
              net_amount, vat_amount) \
             VALUES ('ri-1','co-1','dl-001','RO111','Furnizor Test',\
             'FC','100','1321.00','RON','2025-03-01','/tmp/ri-1.xml','APPROVED',\
             '1100.00','221.00')",
        )
        .execute(&pool)
        .await
        .expect("seed received invoice");

        // VAT line 1: 21%  — base 1000, vat 210
        sqlx::query(
            "INSERT INTO received_invoice_vat_lines \
             (id, received_invoice_id, vat_category, vat_rate, base_amount, vat_amount) \
             VALUES ('vl-1','ri-1','S','21','1000.00','210.00')",
        )
        .execute(&pool)
        .await
        .expect("seed vat line 1");

        // VAT line 2: 11%  — base 100, vat 11
        sqlx::query(
            "INSERT INTO received_invoice_vat_lines \
             (id, received_invoice_id, vat_category, vat_rate, base_amount, vat_amount) \
             VALUES ('vl-2','ri-1','S','11','100.00','11.00')",
        )
        .execute(&pool)
        .await
        .expect("seed vat line 2");

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

        // First line: 21% → TaxCode "301104", TaxPercentage "21.00"
        assert!(
            xml.contains("<TaxPercentage>21.00</TaxPercentage>"),
            "Expected TaxPercentage 21.00 for 21% VAT line. XML:\n{xml}"
        );
        // Second line: 11% → TaxCode "301105" (domestic purchase 11%)
        assert!(
            xml.contains("<TaxPercentage>11.00</TaxPercentage>"),
            "Expected TaxPercentage 11.00 for 11% VAT line. XML:\n{xml}"
        );
        assert!(
            xml.contains("<TaxCode>301105</TaxCode>"),
            "Expected TaxCode 301105 for 11% domestic purchase rate. XML:\n{xml}"
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
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory DB");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");

        // Seed company required by received_invoices.company_id FK.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, vat_payer, address, city, county, country) \
             VALUES ('co-2', 'RO888', 'Firma 2 SRL', 1, 'Str 2', 'Buc', 'B', 'RO')",
        )
        .execute(&pool)
        .await
        .expect("seed company");

        // Seed: one invoice with net=1000, vat=50 (a genuine 5% rate) — NO vat_lines.
        // Chosen NON-19% so the test proves the fallback COMPUTES the rate from the
        // header amounts (round(50/1000*100)=5) rather than hard-coding 19%.
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, \
              series, number, total_amount, currency, issue_date, xml_path, status, \
              net_amount, vat_amount) \
             VALUES ('ri-2','co-2','dl-002','RO222','Furnizor 2',\
             'FACT','002','1050.00','RON','2025-03-01','/tmp/ri-2.xml','APPROVED',\
             '1000.00','50.00')",
        )
        .execute(&pool)
        .await
        .expect("seed received invoice");

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
