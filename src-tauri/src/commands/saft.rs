//! SAF-T D406 XML export — ANAF mandatory standard audit file.
//!
//! Two commands:
//!   `export_saft_d406`     — legacy simplified preview (SalesInvoices only); kept untouched.
//!   `export_saft_official` — Phase 4 official D406 with Header + MasterFiles +
//!                            empty GeneralLedgerEntries + SourceDocuments; validated against
//!                            the official Ro_SAFT_Schema_v249.xsd via xmllint.

use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::Row;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::str::FromStr;
use tauri::State;

use crate::anaf_decl::saft::generator::{generate_saft_xml, generate_saft_xml_annual};
use crate::db::companies;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::fx::{amount_to_ron, parse_rate};

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
    /// CIUS VAT category code stored on the line (S/AE/E/Z/O/K/G).
    /// Used to derive the SAF-T TaxCode directly — more reliable than
    /// inferring from the numeric rate, which cannot distinguish E from Z
    /// or AE/O/K/G at 0%.
    vat_category: String,
    subtotal_amount: Decimal,
    vat_amount: Decimal,
    total_amount: Decimal,
}

// ─── Official D406 export ──────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaftOfficialParams {
    pub company_id: String,
    pub year: i32,
    pub month: Option<i32>,
    pub dest_path: String,
}

/// Generate a complete, schema-conformant SAF-T D406 XML and save it to `dest_path`.
/// The file is validated by structure (element order, types, enums) when xmllint is
/// available — see `anaf_decl::validation::validate_with_xsd`.
#[tauri::command]
pub async fn export_saft_official(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: SaftOfficialParams,
    skip_duk_override: bool,
) -> AppResult<crate::commands::declarations::OfficialExportResult> {
    use crate::anaf_decl::DeclKind;
    use crate::commands::declarations::duk_gate_allows_write;
    use crate::commands::integrations::validate_export_path;

    let dest = validate_export_path(&params.dest_path)?;

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

    let company = companies::get(&state.db, &params.company_id).await?;

    // Determine if this is an annual (A-profile) declaration.
    // Annual = month is None (full-year range was already set above).
    let is_annual = params.month.is_none();

    let xml = if is_annual {
        // Annual A-profile: skip GL auto-post (GLE is empty in A),
        // call the annual generator which applies A-profile section rules.
        generate_saft_xml_annual(&state.db, &company, &date_from, &date_to).await?
    } else {
        // Periodic L-profile: auto-post GL entries (idempotent) before generating
        // so that GeneralLedgerEntries is populated with current period data.
        crate::db::gl::generate_gl_entries(&state.db, &params.company_id, &date_from, &date_to)
            .await?;
        generate_saft_xml(&state.db, &company, &date_from, &date_to).await?
    };

    // Layer D: validate with the bundled DUK before writing. Graceful: no runtime → proceed.
    let tmp =
        std::env::temp_dir().join(format!("d406_official_check_{}.xml", uuid::Uuid::now_v7()));
    std::fs::write(&tmp, xml.as_bytes())
        .map_err(|e| AppError::Other(format!("Nu s-a putut scrie temp D406: {e}")))?;
    let provider = crate::anaf_decl::duk::BundledProvider::new(&app);
    let duk = crate::anaf_decl::duk::run_duk(&provider, DeclKind::D406, &tmp)?;
    let _ = std::fs::remove_file(&tmp);
    let (duk_available, duk_passed, issues) = match &duk {
        Some(o) => (true, o.passed, o.errors.clone()),
        None => (false, false, Vec::new()),
    };
    if !duk_gate_allows_write(duk_available, duk_passed, skip_duk_override) {
        return Ok(crate::commands::declarations::OfficialExportResult {
            path: String::new(),
            written: false,
            duk_available,
            duk_passed,
            issues,
        });
    }

    std::fs::write(&dest, xml.as_bytes())
        .map_err(|e| AppError::Other(format!("Nu s-a putut scrie fișierul D406: {e}")))?;

    Ok(crate::commands::declarations::OfficialExportResult {
        path: dest.to_string_lossy().to_string(),
        written: true,
        duk_available,
        duk_passed,
        issues,
    })
}

// ─── Legacy preview command (unchanged) ───────────────────────────────────────

#[tauri::command]
pub async fn export_saft_d406(state: State<'_, AppState>, params: SaftParams) -> AppResult<String> {
    // NOTE: This is a simplified SAF-T export covering SalesInvoices only.
    // It is NOT a complete ANAF D406 submission. Label the UI accordingly.
    // generate_saft is fully async (it awaits sqlx queries), so call it directly.
    // The previous spawn_blocking + Handle::block_on nested an async future inside a
    // blocking-pool thread, which can deadlock or panic on a single-threaded runtime.
    generate_saft(&state.db, params).await
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

    // Fetch invoices — include id and storno_of_invoice_id for line-item correlation
    // and credit-note type detection (381 vs 380).
    // Wave 4: also fetch exchange_rate for RON normalisation of fiscal amounts.
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
            COALESCE(i.currency, 'RON') AS currency, \
            i.exchange_rate, \
            i.storno_of_invoice_id \
         FROM invoices i \
         LEFT JOIN contacts c ON i.contact_id = c.id \
         WHERE i.company_id = ?1 \
           AND i.issue_date >= ?2 \
           AND i.issue_date <= ?3 \
           AND i.status IN ('VALIDATED', 'STORNED') \
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
                    vat_rate, vat_category, subtotal_amount, vat_amount, total_amount \
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
                    vat_category: row
                        .try_get::<String, _>("vat_category")
                        .unwrap_or_else(|_| "S".to_string()),
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
        let fx_rate = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );

        // InvoiceType: 381 = credit note (storno), 380 = commercial invoice
        let storno_of: Option<String> = row.try_get("storno_of_invoice_id").unwrap_or(None);
        let invoice_type = if storno_of.is_some() { "381" } else { "380" };

        // Wave 4: convert invoice-level fiscal amounts to RON for SAF-T.
        let parse_dec = |s: &str| Decimal::from_str(s.trim()).unwrap_or_default();
        let net_ron = amount_to_ron(parse_dec(&net_amount), &currency, fx_rate);
        let vat_ron = amount_to_ron(parse_dec(&vat_amount), &currency, fx_rate);
        let gross_ron = amount_to_ron(parse_dec(&total_amount), &currency, fx_rate);

        xml.push_str("      <Invoice>\n");
        xml_elem(&mut xml, 8, "InvoiceNo", &escape_xml(&full_number));
        xml_elem(&mut xml, 8, "InvoiceDate", &issue_date);
        xml_elem(&mut xml, 8, "InvoiceType", invoice_type);
        xml_elem(&mut xml, 8, "CustomerName", &escape_xml(&client_name));
        xml_elem(&mut xml, 8, "CustomerTaxID", &escape_xml(&client_cui));
        xml_elem(&mut xml, 8, "NetTotal", &dec_str(&net_ron));
        xml_elem(&mut xml, 8, "VatTotal", &dec_str(&vat_ron));
        xml_elem(&mut xml, 8, "GrossTotal", &dec_str(&gross_ron));
        xml_elem(&mut xml, 8, "Currency", &escape_xml(&currency));

        // ── Lines ─────────────────────────────────────────────────────────────
        let lines = lines_by_invoice
            .get(&invoice_id)
            .cloned()
            .unwrap_or_default();

        xml.push_str("        <Lines>\n");
        for line in &lines {
            // Wave 4: convert line-level amounts to RON for SAF-T fiscal reporting.
            let line_net_ron = amount_to_ron(line.subtotal_amount, &currency, fx_rate);
            let line_vat_ron = amount_to_ron(line.vat_amount, &currency, fx_rate);
            let line_gross_ron = amount_to_ron(line.total_amount, &currency, fx_rate);
            let unit_price_ron = amount_to_ron(line.unit_price, &currency, fx_rate);
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
                dec_str(&unit_price_ron)
            ));
            xml.push_str(&format!(
                "            <TaxCode>{}</TaxCode>\n",
                saft_tax_code_from_category(&line.vat_category, &line.vat_rate)
            ));
            xml.push_str(&format!(
                "            <TaxPercentage>{}</TaxPercentage>\n",
                dec_str(&line.vat_rate)
            ));
            xml.push_str(&format!(
                "            <NetAmount>{}</NetAmount>\n",
                dec_str(&line_net_ron)
            ));
            xml.push_str(&format!(
                "            <TaxAmount>{}</TaxAmount>\n",
                dec_str(&line_vat_ron)
            ));
            xml.push_str(&format!(
                "            <GrossAmount>{}</GrossAmount>\n",
                dec_str(&line_gross_ron)
            ));
            xml.push_str("          </Line>\n");
        }
        xml.push_str("        </Lines>\n");

        // ── DocumentTotals — per-(VAT-rate, category) tax breakdown ─────────
        // BTreeMap keeps entries in deterministic ascending order.
        // Key = (rate_str, vat_category) so AE/E/Z/O/K/G stay separate rows
        // even when their numeric rate collides (e.g. multiple 0% categories).
        let mut by_rate: BTreeMap<(String, String), (Decimal, Decimal)> = BTreeMap::new();
        for line in &lines {
            let rate_key = dec_str(&line.vat_rate);
            let entry = by_rate
                .entry((rate_key, line.vat_category.clone()))
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            // Wave 4: accumulate RON-converted amounts in DocumentTotals.
            entry.0 += amount_to_ron(line.subtotal_amount, &currency, fx_rate);
            entry.1 += amount_to_ron(line.vat_amount, &currency, fx_rate);
        }

        if !by_rate.is_empty() {
            xml.push_str("        <DocumentTotals>\n");
            for ((rate_str, category), (base, tax)) in &by_rate {
                let rate_dec = Decimal::from_str(rate_str).unwrap_or(Decimal::ZERO);
                let code = saft_tax_code_from_category(category, &rate_dec);
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

// ── SAF-T tax code from CIUS VAT category ───────────────────────────────────
// Maps the stored vat_category (S/AE/E/Z/O/K/G) to the SAF-T D406 TaxCode.
// Per ANAF SAF-T D406 nomenclature, the category IS the CIUS code with one
// exception: "S" lines with a reduced rate (5%, 9%, 11%) map to "AA".
// For all other categories the code is the category itself.
//
// vat_category values stored in invoice_line_items:
//   S  — standard rate (19%, 21%)           → S
//   AE — autolichidare / reverse charge      → AE
//   E  — scutit / exempt                     → E
//   Z  — zero-rated (intra-EU / export)      → Z
//   O  — în afara TVA / outside scope        → O
//   K  — livrare intracomunitară scutită     → K
//   G  — livrare stat / governmental         → G
//
// NOTE: lines with vat_category="S" but a reduced rate (5%, 9%, 11%) were
// stored before the vat_category field was differentiated — map them to "AA".
fn saft_tax_code_from_category(category: &str, rate: &Decimal) -> &'static str {
    match category {
        // Reduced rates (5%, 9%, 11%) must map to "AA" per ANAF D406.
        "S" if *rate == Decimal::from(5)
            || *rate == Decimal::from(9)
            || *rate == Decimal::from(11) =>
        {
            "AA"
        }
        // Standard rates (19%, 21%) — and 0% for legacy S lines — map to "S".
        "S" => "S",
        "AE" => "AE",
        "E" => "E",
        "Z" => "Z",
        "O" => "O",
        "K" => "K",
        "G" => "G",
        _ => "S", // fallback for unknown / legacy values
    }
}

// ── SAF-T tax code from VAT rate (legacy, kept for tests) ───────────────────
// Per ANAF SAF-T D406 nomenclature:
//   E  = exempt / scutit (0%)
//   AA = reduced / cotă redusă (5%, 9%, 11%)
//   S  = standard / cotă standard (19%, 21%)
// Rates 9% (≤2025-07-31) and 11% (from 2025-08-01) are reduced, NOT standard.
// This function is retained only for the existing rate-based unit tests.
#[cfg(test)]
fn saft_tax_code(rate: &Decimal) -> &'static str {
    if rate.is_zero() {
        "E"
    } else if *rate == Decimal::from(5) || *rate == Decimal::from(9) || *rate == Decimal::from(11) {
        "AA" // reduced rates
    } else {
        "S" // standard rates (19%, 21%)
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Fix #3: saft_tax_code maps rates correctly ───────────────────────────

    #[test]
    fn saft_tax_code_zero_is_exempt() {
        assert_eq!(saft_tax_code(&Decimal::ZERO), "E");
    }

    #[test]
    fn saft_tax_code_reduced_rates_map_to_aa() {
        // 5%, 9%, 11% are all reduced rates and must produce "AA"
        assert_eq!(saft_tax_code(&Decimal::from(5)), "AA", "5% must be AA");
        assert_eq!(
            saft_tax_code(&Decimal::from(9)),
            "AA",
            "9% must be AA (not S)"
        );
        assert_eq!(
            saft_tax_code(&Decimal::from(11)),
            "AA",
            "11% must be AA (not S)"
        );
    }

    #[test]
    fn saft_tax_code_standard_rates_map_to_s() {
        // 19% and 21% are standard rates
        assert_eq!(saft_tax_code(&Decimal::from(19)), "S", "19% must be S");
        assert_eq!(saft_tax_code(&Decimal::from(21)), "S", "21% must be S");
    }

    // ── Wave 4: FX normalisation ──────────────────────────────────────────────

    /// Wave 4: EUR line (base=1000, vat=190, rate=5.0) → 5000/950 RON in SAF-T.
    #[test]
    fn saft_eur_line_converted_to_ron() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        let base_ron = amount_to_ron(
            Decimal::from_str("1000.00").unwrap(),
            "EUR",
            parse_rate(Some(5.0)),
        );
        let vat_ron = amount_to_ron(
            Decimal::from_str("190.00").unwrap(),
            "EUR",
            parse_rate(Some(5.0)),
        );
        assert_eq!(
            base_ron,
            Decimal::from_str("5000.00").unwrap(),
            "EUR 1000 * 5.0 must equal RON 5000"
        );
        assert_eq!(
            vat_ron,
            Decimal::from_str("950.00").unwrap(),
            "EUR 190 * 5.0 must equal RON 950"
        );
    }

    /// Wave 4: RON line is unchanged in SAF-T (identity path).
    #[test]
    fn saft_ron_line_unchanged() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        let base = Decimal::from_str("1000.00").unwrap();
        let vat = Decimal::from_str("190.00").unwrap();
        assert_eq!(
            amount_to_ron(base, "RON", parse_rate(Some(5.0))),
            base,
            "RON base must be unchanged"
        );
        assert_eq!(
            amount_to_ron(vat, "RON", parse_rate(Some(5.0))),
            vat,
            "RON vat must be unchanged"
        );
    }

    /// Wave 4: DocumentTotals accumulation with EUR line → RON aggregate.
    #[test]
    fn saft_document_totals_eur_accumulation() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};
        use std::collections::BTreeMap;

        // Two lines on a EUR invoice (rate=5.0):
        // Line 1: base=1000, vat=190 → 5000/950 RON
        // Line 2: base=500, vat=95 → 2500/475 RON
        // Aggregate at 19%: base=7500, vat=1425
        let lines = [
            (
                Decimal::from_str("1000.00").unwrap(),
                Decimal::from_str("190.00").unwrap(),
                Decimal::from_str("0.19").unwrap(),
            ),
            (
                Decimal::from_str("500.00").unwrap(),
                Decimal::from_str("95.00").unwrap(),
                Decimal::from_str("0.19").unwrap(),
            ),
        ];
        let fx_rate = parse_rate(Some(5.0));
        let currency = "EUR";

        let mut by_rate: BTreeMap<String, (Decimal, Decimal)> = BTreeMap::new();
        for (sub, vat, rate) in &lines {
            let key = format!("{:.2}", rate);
            let entry = by_rate.entry(key).or_insert((Decimal::ZERO, Decimal::ZERO));
            entry.0 += amount_to_ron(*sub, currency, fx_rate);
            entry.1 += amount_to_ron(*vat, currency, fx_rate);
        }

        let totals = &by_rate["0.19"];
        assert_eq!(
            totals.0,
            Decimal::from_str("7500.00").unwrap(),
            "EUR line aggregate base must be 5000+2500=7500 RON"
        );
        assert_eq!(
            totals.1,
            Decimal::from_str("1425.00").unwrap(),
            "EUR line aggregate vat must be 950+475=1425 RON"
        );
    }

    // ── Fix #3 (category-based): saft_tax_code_from_category ────────────────

    /// Fix #3: every CIUS category maps to itself (or "S" fallback).
    /// "S" at standard rate 19% must produce "S".
    #[test]
    fn saft_tax_code_from_category_all_categories() {
        assert_eq!(saft_tax_code_from_category("S", &Decimal::from(19)), "S");
        assert_eq!(saft_tax_code_from_category("AE", &Decimal::ZERO), "AE");
        assert_eq!(saft_tax_code_from_category("E", &Decimal::ZERO), "E");
        assert_eq!(saft_tax_code_from_category("Z", &Decimal::ZERO), "Z");
        assert_eq!(saft_tax_code_from_category("O", &Decimal::ZERO), "O");
        assert_eq!(saft_tax_code_from_category("K", &Decimal::ZERO), "K");
        assert_eq!(saft_tax_code_from_category("G", &Decimal::ZERO), "G");
    }

    /// Fix #3: unknown / legacy category falls back to "S" (safe default).
    #[test]
    fn saft_tax_code_from_category_unknown_fallback() {
        assert_eq!(saft_tax_code_from_category("", &Decimal::from(19)), "S");
        assert_eq!(
            saft_tax_code_from_category("UNKNOWN", &Decimal::from(19)),
            "S"
        );
    }

    /// Fix #3: AE/K/G/O/Z at 0% do NOT collapse to "E" (the rate-based bug).
    #[test]
    fn saft_tax_code_category_preserves_zero_rate_distinctions() {
        // All these categories may have 0% rate, but they must NOT all become "E"
        assert_ne!(
            saft_tax_code_from_category("AE", &Decimal::ZERO),
            "E",
            "AE must not be E"
        );
        assert_ne!(
            saft_tax_code_from_category("K", &Decimal::ZERO),
            "E",
            "K must not be E"
        );
        assert_ne!(
            saft_tax_code_from_category("G", &Decimal::ZERO),
            "E",
            "G must not be E"
        );
        assert_ne!(
            saft_tax_code_from_category("O", &Decimal::ZERO),
            "E",
            "O must not be E"
        );
        assert_ne!(
            saft_tax_code_from_category("Z", &Decimal::ZERO),
            "E",
            "Z must not be E"
        );
    }

    /// Regression fix: S + reduced rate must map to "AA"; S + standard rate must map to "S".
    /// AE/K/G/O/Z + 0% must still return their own code (NOT "E").
    #[test]
    fn saft_tax_code_from_category_s_reduced_rate_maps_to_aa() {
        // Reduced S rates → "AA"
        assert_eq!(
            saft_tax_code_from_category("S", &Decimal::from(9)),
            "AA",
            "S + 9% must be AA (reduced rate)"
        );
        assert_eq!(
            saft_tax_code_from_category("S", &Decimal::from(5)),
            "AA",
            "S + 5% must be AA (reduced rate)"
        );
        assert_eq!(
            saft_tax_code_from_category("S", &Decimal::from(11)),
            "AA",
            "S + 11% must be AA (reduced rate)"
        );
        // Standard S rates → "S"
        assert_eq!(
            saft_tax_code_from_category("S", &Decimal::from(19)),
            "S",
            "S + 19% must be S (standard rate)"
        );
        assert_eq!(
            saft_tax_code_from_category("S", &Decimal::from(21)),
            "S",
            "S + 21% must be S (standard rate)"
        );
        // Non-S categories at 0% return their own code, not "E"
        assert_eq!(
            saft_tax_code_from_category("AE", &Decimal::ZERO),
            "AE",
            "AE + 0% must be AE"
        );
        assert_eq!(
            saft_tax_code_from_category("K", &Decimal::ZERO),
            "K",
            "K + 0% must be K"
        );
        assert_eq!(
            saft_tax_code_from_category("G", &Decimal::ZERO),
            "G",
            "G + 0% must be G"
        );
        assert_eq!(
            saft_tax_code_from_category("O", &Decimal::ZERO),
            "O",
            "O + 0% must be O"
        );
        assert_eq!(
            saft_tax_code_from_category("Z", &Decimal::ZERO),
            "Z",
            "Z + 0% must be Z"
        );
    }
}
