//! SAF-T D406 XML generator — assembles all four mandatory sections.
//!
//! Output structure:
//! ```xml
//! <?xml version="1.0" encoding="UTF-8"?>
//! <AuditFile xmlns="mfp:anaf:dgti:d406:declaratie:v1">
//!   <Header> … </Header>
//!   <MasterFiles> … </MasterFiles>
//!   <GeneralLedgerEntries> NumberOfEntries/TotalDebit/TotalCredit </GeneralLedgerEntries>
//!   <SourceDocuments> … </SourceDocuments>
//! </AuditFile>
//! ```
//!
//! elementFormDefault="qualified" → default namespace on AuditFile; children inherit it,
//! emitted WITHOUT prefix (bare element names).

use std::io::Cursor;

use chrono::Local;
use quick_xml::events::{BytesDecl, BytesStart, Event};
use quick_xml::Writer;
use rust_decimal::Decimal;
use sqlx::Row;

use crate::anaf_decl::saft::masterfiles::{write_amount_structure, write_master_files};
use crate::anaf_decl::saft::source_docs::write_source_documents;
use crate::anaf_decl::xml::{end_elem, start_elem, write_text_elem};
use crate::db::companies::Company;
use crate::error::{AppError, AppResult};

const NAMESPACE: &str = "mfp:anaf:dgti:d406:declaratie:v1";
const AUDIT_FILE_VERSION: &str = "2.4.9";

fn map_qx(e: quick_xml::Error) -> AppError {
    AppError::Other(format!("XML write error: {e}"))
}

/// Write the Header section.
/// Header element order (from XSD HeaderStructure then TaxAccountingBasis/TaxEntity):
///   AuditFileVersion, AuditFileCountry, AuditFileDateCreated,
///   SoftwareCompanyName, SoftwareID, SoftwareVersion,
///   Company (CompanyHeaderStructure),
///   DefaultCurrencyCode, SelectionCriteria, HeaderComment,
///   SegmentIndex, TotalSegmentsInsequence,
///   TaxAccountingBasis [, TaxEntity (minOccurs=0)]
fn write_header(
    w: &mut crate::anaf_decl::xml::XmlWriter,
    company: &Company,
    date_from: &str,
    date_to: &str,
    is_annual: bool,
) -> AppResult<()> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let version = env!("CARGO_PKG_VERSION");

    start_elem(w, "Header")?;

    // HeaderStructure fields (in order)
    write_text_elem(w, "AuditFileVersion", AUDIT_FILE_VERSION)?;
    write_text_elem(w, "AuditFileCountry", "RO")?;
    write_text_elem(w, "AuditFileDateCreated", &today)?;
    write_text_elem(w, "SoftwareCompanyName", "Lucaris SRL")?;
    write_text_elem(w, "SoftwareID", "efactura-desktop")?;
    write_text_elem(w, "SoftwareVersion", version)?;

    // Company — CompanyHeaderStructure
    // Fields: RegistrationNumber, Name, Address+, Contact+ (required: ContactPerson+Telephone),
    //         TaxRegistration* (minOccurs=0), BankAccount+ (required in CompanyHeaderStructure)
    write_header_company(w, company)?;

    write_text_elem(w, "DefaultCurrencyCode", "RON")?;

    // SelectionCriteria — xs:choice: SelectionStartDate + SelectionEndDate
    start_elem(w, "SelectionCriteria")?;
    write_text_elem(w, "SelectionStartDate", date_from)?;
    write_text_elem(w, "SelectionEndDate", date_to)?;
    end_elem(w, "SelectionCriteria")?;

    // HeaderComment: DUK treats this as _tipDeclaratie — "L" = lunară (monthly),
    // "A" = anuală (D406A, Assets). NOTE: a fully DUK-submittable D406A also needs a
    // full-year SelectionCriteria + the A-profile MasterFiles structure (follow-up);
    // here we at least self-identify the declaration type correctly.
    write_text_elem(w, "HeaderComment", if is_annual { "A" } else { "L" })?;

    write_text_elem(w, "SegmentIndex", "1")?;
    write_text_elem(w, "TotalSegmentsInsequence", "1")?;

    // Extension: TaxAccountingBasis (required) — "A" = Accounting
    write_text_elem(w, "TaxAccountingBasis", "A")?;

    end_elem(w, "Header")?;
    Ok(())
}

fn write_header_company(
    w: &mut crate::anaf_decl::xml::XmlWriter,
    company: &Company,
) -> AppResult<()> {
    start_elem(w, "Company")?;

    // RegistrationNumber (required, SAFmiddle1textType max 35) — bare CUI digits,
    // no "RO" prefix (DUK rejects "RO12345678" as an invalid RegistrationNumber).
    let cui_clean = {
        let t = company.cui.trim();
        t.strip_prefix("RO")
            .or_else(|| t.strip_prefix("ro"))
            .unwrap_or(t)
            .trim()
    };
    write_text_elem(w, "RegistrationNumber", &trunc_esc(cui_clean, 35))?;
    write_text_elem(w, "Name", &trunc_esc(&company.legal_name, 256))?;

    // Address (required, maxOccurs=unbounded)
    let street = &company.address;
    let city = if company.city.is_empty() {
        "N/A"
    } else {
        &company.city
    };
    let country_2 = if company.country.len() >= 2 {
        &company.country[..2]
    } else {
        "RO"
    };
    start_elem(w, "Address")?;
    if !street.is_empty() {
        write_text_elem(w, "StreetName", &trunc_esc(street, 70))?;
    }
    write_text_elem(w, "City", &trunc_esc(city, 35))?;
    if let Some(ref pc) = company.postal_code {
        if !pc.is_empty() {
            write_text_elem(w, "PostalCode", &trunc_esc(pc, 18))?;
        }
    }
    if !company.county.is_empty() {
        // DUK rule: Region must be ISO-3166-2 "RO-CJ"/"RO-IF" — prefix with "RO-" unless already prefixed
        let county_upper = company.county.to_uppercase();
        let region = if county_upper.starts_with("RO-") {
            county_upper
        } else {
            format!("RO-{county_upper}")
        };
        write_text_elem(w, "Region", &trunc_esc(&region, 35))?;
    }
    write_text_elem(w, "Country", country_2)?;
    write_text_elem(w, "AddressType", "StreetAddress")?;
    end_elem(w, "Address")?;

    // Contact (required in CompanyHeaderStructure — ContactHeaderStructure:
    //   ContactPerson(required) + Telephone(required))
    // Synthesize: FirstName="Administrator", LastName=truncate(legal_name,70), phone or "0000000000"
    let phone = company
        .phone
        .as_deref()
        .filter(|p| !p.is_empty())
        .unwrap_or("0000000000");
    let last_name_for_contact = trunc_esc(&company.legal_name, 70);
    start_elem(w, "Contact")?;
    start_elem(w, "ContactPerson")?;
    write_text_elem(w, "FirstName", "Administrator")?;
    write_text_elem(w, "LastName", &last_name_for_contact)?;
    end_elem(w, "ContactPerson")?;
    write_text_elem(w, "Telephone", &trunc_esc(phone, 18))?;
    end_elem(w, "Contact")?;

    // TaxRegistration (minOccurs=0 in CompanyHeaderStructure per restriction)
    // Emit it for VAT-registered companies
    if company.vat_payer {
        start_elem(w, "TaxRegistration")?;
        write_text_elem(w, "TaxRegistrationNumber", &trunc_esc(cui_clean, 35))?;
        // Nomenclator_Regim_fiscal: "100010" = Persoană impozabilă înregistrată în
        // scopuri de TVA (VAT-registered taxable person).
        write_text_elem(w, "TaxType", "100010")?;
        write_text_elem(w, "TaxAuthority", "ANAF")?;
        end_elem(w, "TaxRegistration")?;
    }

    // BankAccount (required in CompanyHeaderStructure, maxOccurs=unbounded)
    // Use IBAN if present, else BankAccountNumber with "N/A"
    start_elem(w, "BankAccount")?;
    if let Some(ref iban) = company.iban {
        if !iban.is_empty() {
            write_text_elem(w, "IBANNumber", &trunc_esc(iban, 35))?;
        } else {
            write_text_elem(w, "BankAccountNumber", "N/A")?;
        }
    } else {
        write_text_elem(w, "BankAccountNumber", "N/A")?;
    }
    end_elem(w, "BankAccount")?;

    end_elem(w, "Company")?;
    Ok(())
}

/// Write a fully populated GeneralLedgerEntries section from gl_journal / gl_entry.
///
/// XSD element order (all minOccurs=0 at the top level):
///   NumberOfEntries, TotalDebit, TotalCredit, Journal*
///
/// Journal:  JournalID, Description, Type, Transaction+
/// Transaction (xs:sequence, required unless noted):
///   TransactionID, Period, PeriodYear, TransactionDate,
///   [SourceID min=0], [TransactionType min=0], Description,
///   [BatchID min=0], SystemEntryDate, GLPostingDate,
///   CustomerID (required), SupplierID (required), [SystemID min=0], TransactionLine+
///
/// TransactionLine (xs:sequence, required unless noted):
///   RecordID, AccountID, [Analysis min=0], [ValueDate min=0],
///   [SourceDocumentID min=0], CustomerID (required), SupplierID (required),
///   Description, DebitAmount|CreditAmount, TaxInformation+
async fn write_general_ledger_entries(
    w: &mut crate::anaf_decl::xml::XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<()> {
    // ── 1. Fetch all journals in period ────────────────────────────────────────
    let journal_rows = sqlx::query(
        "SELECT id, journal_id, journal_type, transaction_id, transaction_date, \
                COALESCE(description, '') AS description, \
                COALESCE(customer_id, '') AS customer_id, \
                COALESCE(supplier_id, '') AS supplier_id \
         FROM gl_journal \
         WHERE company_id = ?1 \
           AND transaction_date >= ?2 \
           AND transaction_date <= ?3 \
         ORDER BY journal_id, transaction_date, transaction_id",
    )
    .bind(company_id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    if journal_rows.is_empty() {
        // No GL data — emit empty GLE (minOccurs=0 for all children)
        start_elem(w, "GeneralLedgerEntries")?;
        write_text_elem(w, "NumberOfEntries", "0")?;
        write_text_elem(w, "TotalDebit", "0.00")?;
        write_text_elem(w, "TotalCredit", "0.00")?;
        end_elem(w, "GeneralLedgerEntries")?;
        return Ok(());
    }

    // ── 2. Fetch all entries for those journals ───────────────────────────────
    // Collect PKs from the fetched journals to do a targeted query
    let journal_pks: Vec<String> = journal_rows
        .iter()
        .map(|r| r.try_get::<String, _>("id").unwrap_or_default())
        .collect();

    let placeholders: String = (1..=journal_pks.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");

    let entry_sql = format!(
        "SELECT journal_pk, record_id, account_code, \
                COALESCE(debit, '0') AS debit, COALESCE(credit, '0') AS credit, \
                COALESCE(customer_id, '') AS customer_id, \
                COALESCE(supplier_id, '') AS supplier_id, \
                tax_type, tax_code, \
                tax_percentage, tax_base, tax_amount \
         FROM gl_entry \
         WHERE journal_pk IN ({placeholders}) \
         ORDER BY journal_pk, record_id"
    );
    let mut q = sqlx::query(&entry_sql);
    for pk in &journal_pks {
        q = q.bind(pk);
    }
    let entry_rows = q.fetch_all(pool).await.map_err(AppError::Database)?;

    // ── 3. Compute totals ─────────────────────────────────────────────────────
    let mut total_debit = Decimal::ZERO;
    let mut total_credit = Decimal::ZERO;
    let mut num_entries: u64 = 0;

    for er in &entry_rows {
        let d: String = er.try_get("debit").unwrap_or_else(|_| "0".to_string());
        let c: String = er.try_get("credit").unwrap_or_else(|_| "0".to_string());
        let dv = d.trim().parse::<Decimal>().unwrap_or(Decimal::ZERO);
        let cv = c.trim().parse::<Decimal>().unwrap_or(Decimal::ZERO);
        total_debit += dv;
        total_credit += cv;
        num_entries += 1;
    }

    // Group entries by journal_pk for fast lookup
    use std::collections::HashMap;
    let mut entries_by_journal: HashMap<String, Vec<&sqlx::sqlite::SqliteRow>> = HashMap::new();
    for er in &entry_rows {
        let jpk: String = er.try_get("journal_pk").unwrap_or_default();
        entries_by_journal.entry(jpk).or_default().push(er);
    }

    // ── 4. Group journals by journal_id ───────────────────────────────────────
    // journal_id = "VANZARI" / "CUMPARARI" / "BANCA" — one Journal element per
    // unique journal_id, containing all transactions for that journal.
    use std::collections::BTreeMap;
    // BTreeMap preserves insertion-sorted order by journal_id
    let mut journals_map: BTreeMap<String, Vec<&sqlx::sqlite::SqliteRow>> = BTreeMap::new();
    for jr in &journal_rows {
        let jid: String = jr.try_get("journal_id").unwrap_or_default();
        journals_map.entry(jid).or_default().push(jr);
    }

    // ── 5. Emit XML ───────────────────────────────────────────────────────────
    start_elem(w, "GeneralLedgerEntries")?;
    write_text_elem(w, "NumberOfEntries", &num_entries.to_string())?;
    write_text_elem(w, "TotalDebit", &format!("{:.2}", total_debit))?;
    write_text_elem(w, "TotalCredit", &format!("{:.2}", total_credit))?;

    for (journal_id, txn_rows) in &journals_map {
        // Journal label from journal_type of first transaction
        let journal_type: String = txn_rows
            .first()
            .and_then(|r| r.try_get("journal_type").ok())
            .unwrap_or_else(|| journal_id.clone());

        let journal_label = match journal_id.as_str() {
            "VANZARI" => "Jurnal vanzari",
            "CUMPARARI" => "Jurnal cumparari",
            "BANCA" => "Jurnal banca",
            other => other,
        };

        start_elem(w, "Journal")?;
        write_text_elem(w, "JournalID", &trunc_esc(journal_id, 9))?;
        write_text_elem(w, "Description", &trunc_esc(journal_label, 256))?;
        write_text_elem(w, "Type", &trunc_esc(&journal_type, 9))?;

        for jr in txn_rows {
            let jpk: String = jr.try_get("id").unwrap_or_default();
            let txn_id: String = jr.try_get("transaction_id").unwrap_or_default();
            let txn_date: String = jr.try_get("transaction_date").unwrap_or_default();
            let txn_desc: String = jr.try_get("description").unwrap_or_default();
            let cust_id: String = jr.try_get("customer_id").unwrap_or_default();
            let supp_id: String = jr.try_get("supplier_id").unwrap_or_default();

            // Parse month/year from transaction_date (YYYY-MM-DD)
            let (period, period_year) = parse_period(&txn_date);

            start_elem(w, "Transaction")?;
            write_text_elem(w, "TransactionID", &trunc_esc(&txn_id, 255))?;
            write_text_elem(w, "Period", &period.to_string())?;
            write_text_elem(w, "PeriodYear", &period_year.to_string())?;
            write_text_elem(w, "TransactionDate", &txn_date)?;
            write_text_elem(w, "Description", &trunc_esc(&txn_desc, 256))?;
            write_text_elem(w, "SystemEntryDate", &txn_date)?;
            write_text_elem(w, "GLPostingDate", &txn_date)?;
            // CustomerID and SupplierID are REQUIRED in the XSD (no minOccurs=0),
            // and the DUK rejects empty strings — use "0" as the "no partner" sentinel
            // (the DUK validator explicitly accepts the bare string "0" as a valid ID).
            let cust_id_emit = if cust_id.is_empty() { "0" } else { &cust_id };
            let supp_id_emit = if supp_id.is_empty() { "0" } else { &supp_id };
            write_text_elem(w, "CustomerID", &trunc_esc(cust_id_emit, 35))?;
            write_text_elem(w, "SupplierID", &trunc_esc(supp_id_emit, 35))?;

            // TransactionLines
            let empty_vec: Vec<&sqlx::sqlite::SqliteRow> = Vec::new();
            let lines = entries_by_journal.get(&jpk).unwrap_or(&empty_vec);

            for er in lines {
                let record_id: i64 = er.try_get("record_id").unwrap_or(0);
                let account_code: String = er.try_get("account_code").unwrap_or_default();
                let debit_s: String = er.try_get("debit").unwrap_or_else(|_| "0".to_string());
                let credit_s: String = er.try_get("credit").unwrap_or_else(|_| "0".to_string());
                let entry_cust: String = er.try_get("customer_id").unwrap_or_default();
                let entry_supp: String = er.try_get("supplier_id").unwrap_or_default();
                let tax_type: String = er.try_get("tax_type").unwrap_or_else(|_| "000".to_string());
                let tax_code: String = er
                    .try_get("tax_code")
                    .unwrap_or_else(|_| "000000".to_string());
                let tax_pct: Option<String> = er.try_get("tax_percentage").unwrap_or(None);
                let tax_base: Option<String> = er.try_get("tax_base").unwrap_or(None);
                let tax_amount: Option<String> = er.try_get("tax_amount").unwrap_or(None);

                let dv = debit_s.trim().parse::<Decimal>().unwrap_or(Decimal::ZERO);
                let cv = credit_s.trim().parse::<Decimal>().unwrap_or(Decimal::ZERO);

                // Build line description: "account_code (debit|credit)"
                let line_desc = format!("Cont {account_code}");

                start_elem(w, "TransactionLine")?;
                write_text_elem(w, "RecordID", &record_id.to_string())?;
                write_text_elem(w, "AccountID", &trunc_esc(&account_code, 255))?;
                // CustomerID / SupplierID are REQUIRED.
                // DUK rule: CustomerID and SupplierID cannot BOTH be "0" simultaneously.
                // If neither is set on the line, inherit the transaction-level partner.
                let effective_cust = if !entry_cust.is_empty() {
                    entry_cust.clone()
                } else if !cust_id.is_empty() {
                    cust_id.clone()
                } else {
                    "0".to_string()
                };
                let effective_supp = if !entry_supp.is_empty() {
                    entry_supp.clone()
                } else if !supp_id.is_empty() {
                    supp_id.clone()
                } else {
                    "0".to_string()
                };
                // If still both "0", use transaction customer as fallback for both
                let (line_cust, line_supp) = if effective_cust == "0" && effective_supp == "0" {
                    (cust_id_emit.to_string(), supp_id_emit.to_string())
                } else {
                    (effective_cust, effective_supp)
                };
                write_text_elem(w, "CustomerID", &trunc_esc(&line_cust, 35))?;
                write_text_elem(w, "SupplierID", &trunc_esc(&line_supp, 35))?;
                write_text_elem(w, "Description", &trunc_esc(&line_desc, 256))?;

                // xs:choice — DebitAmount | CreditAmount
                // Emit DebitAmount when debit > 0, otherwise CreditAmount.
                // When both are zero (shouldn't happen in balanced GL) emit CreditAmount(0).
                if dv > Decimal::ZERO {
                    write_amount_structure(w, "DebitAmount", dv)?;
                } else {
                    write_amount_structure(w, "CreditAmount", cv)?;
                }

                // TaxInformation is REQUIRED (minOccurs defaults to 1, maxOccurs=unbounded)
                start_elem(w, "TaxInformation")?;
                write_text_elem(w, "TaxType", &tax_type)?;
                write_text_elem(w, "TaxCode", &tax_code)?;
                if let Some(ref pct) = tax_pct {
                    let pv = pct.trim().parse::<Decimal>().unwrap_or(Decimal::ZERO);
                    write_text_elem(w, "TaxPercentage", &format!("{:.2}", pv))?;
                }
                if let Some(ref base) = tax_base {
                    let bv = base.trim().parse::<Decimal>().unwrap_or(Decimal::ZERO);
                    write_text_elem(w, "TaxBase", &format!("{:.2}", bv))?;
                }
                // TaxAmount is REQUIRED in TaxInformationStructure (no minOccurs=0)
                let tax_amt_val = tax_amount
                    .as_deref()
                    .and_then(|s| s.trim().parse::<Decimal>().ok())
                    .unwrap_or(Decimal::ZERO);
                write_amount_structure(w, "TaxAmount", tax_amt_val)?;
                end_elem(w, "TaxInformation")?;

                end_elem(w, "TransactionLine")?;
            }

            end_elem(w, "Transaction")?;
        }

        end_elem(w, "Journal")?;
    }

    end_elem(w, "GeneralLedgerEntries")?;
    Ok(())
}

/// Parse "YYYY-MM-DD" → (month_u32, year_i32).  Returns (1, 1970) on parse failure.
fn parse_period(date: &str) -> (u32, i32) {
    let parts: Vec<&str> = date.splitn(3, '-').collect();
    if parts.len() == 3 {
        let year = parts[0].parse::<i32>().unwrap_or(1970);
        let month = parts[1].parse::<u32>().unwrap_or(1);
        (month, year)
    } else {
        (1, 1970)
    }
}

// ── String helpers ─────────────────────────────────────────────────────────────
fn trunc_esc(s: &str, max_chars: usize) -> String {
    let t: String = s.chars().take(max_chars).collect();
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ── generate_saft_xml ─────────────────────────────────────────────────────────

/// Generate a complete, schema-conformant SAF-T D406 XML string.
///
/// # Arguments
/// * `pool`      — SQLite pool (for MasterFiles + SourceDocuments queries)
/// * `company`   — the reporting company record
/// * `date_from` — selection start date, `YYYY-MM-DD`
/// * `date_to`   — selection end date, `YYYY-MM-DD`
/// * `is_annual` — `true` for annual D406A (populates Assets); `false` for periodic L (Assets empty)
pub async fn generate_saft_xml(
    pool: &sqlx::SqlitePool,
    company: &Company,
    date_from: &str,
    date_to: &str,
) -> AppResult<String> {
    generate_saft_xml_inner(pool, company, date_from, date_to, false).await
}

/// Annual variant — populates Assets section from fixed_assets table.
pub async fn generate_saft_xml_annual(
    pool: &sqlx::SqlitePool,
    company: &Company,
    date_from: &str,
    date_to: &str,
) -> AppResult<String> {
    generate_saft_xml_inner(pool, company, date_from, date_to, true).await
}

async fn generate_saft_xml_inner(
    pool: &sqlx::SqlitePool,
    company: &Company,
    date_from: &str,
    date_to: &str,
    is_annual: bool,
) -> AppResult<String> {
    // Use new_with_indent for human-readable output (2-space indent)
    let mut w = Writer::new_with_indent(Cursor::new(Vec::<u8>::new()), b' ', 2);

    // <?xml version="1.0" encoding="UTF-8"?>
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(map_qx)?;

    // <AuditFile xmlns="mfp:anaf:dgti:d406:declaratie:v1">
    let mut root = BytesStart::new("AuditFile");
    root.push_attribute(("xmlns", NAMESPACE));
    w.write_event(Event::Start(root)).map_err(map_qx)?;

    // We need a XmlWriter (Writer<Cursor<Vec<u8>>>) from this point.
    // We've built the root start tag directly on `w` which IS a XmlWriter.
    // Continue using `w` via the xml helper functions by wrapping:
    write_header(&mut w, company, date_from, date_to, is_annual)?;
    write_master_files(&mut w, pool, company, date_from, date_to, is_annual).await?;
    write_general_ledger_entries(&mut w, pool, &company.id, date_from, date_to).await?;
    write_source_documents(&mut w, pool, &company.id, date_from, date_to, is_annual).await?;

    // </AuditFile>
    end_elem(&mut w, "AuditFile")?;

    let mut bytes = w.into_inner().into_inner();
    bytes.push(b'\n');
    String::from_utf8(bytes).map_err(|e| AppError::Other(format!("XML utf8 error: {e}")))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anaf_decl::xml::new_writer;
    use crate::db::companies::Company;

    fn test_company() -> Company {
        Company {
            id: "test-co-id".to_string(),
            // Valid Romanian CUI (checksum verified)
            cui: "RO123456789".to_string(),
            legal_name: "CLARITO TEST SRL".to_string(),
            trade_name: None,
            registry_number: Some("J40/1234/2020".to_string()),
            vat_payer: true,
            address: "Calea Victoriei 155".to_string(),
            city: "Bucuresti".to_string(),
            county: "IF".to_string(),
            postal_code: Some("010073".to_string()),
            country: "RO".to_string(),
            email: Some("test@clarito.ro".to_string()),
            phone: Some("0721000000".to_string()),
            iban: Some("RO49AAAA1B31007593840000".to_string()),
            bank_name: Some("Banca Transilvania".to_string()),
            is_active: true,
            spv_enabled: false,
            invoice_series: "F".to_string(),
            last_invoice_number: 10,
            logo_path: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn header_contains_required_elements() {
        let company = test_company();
        let mut w = new_writer().unwrap();
        start_elem(&mut w, "AuditFile").unwrap();
        write_header(&mut w, &company, "2025-01-01", "2025-01-31", false).unwrap();
        end_elem(&mut w, "AuditFile").unwrap();
        let xml = crate::anaf_decl::xml::finish(w).unwrap();

        assert!(
            xml.contains("<AuditFileVersion>2.4.9</AuditFileVersion>"),
            "AuditFileVersion: {xml}"
        );
        assert!(
            xml.contains("<AuditFileCountry>RO</AuditFileCountry>"),
            "AuditFileCountry: {xml}"
        );
        assert!(
            xml.contains("<SoftwareCompanyName>Lucaris SRL</SoftwareCompanyName>"),
            "SoftwareCompanyName: {xml}"
        );
        assert!(
            xml.contains("<SoftwareID>efactura-desktop</SoftwareID>"),
            "SoftwareID: {xml}"
        );
        assert!(
            xml.contains("<DefaultCurrencyCode>RON</DefaultCurrencyCode>"),
            "DefaultCurrencyCode: {xml}"
        );
        assert!(
            xml.contains("<SelectionStartDate>2025-01-01</SelectionStartDate>"),
            "SelectionStartDate: {xml}"
        );
        assert!(
            xml.contains("<SelectionEndDate>2025-01-31</SelectionEndDate>"),
            "SelectionEndDate: {xml}"
        );
        assert!(
            xml.contains("<TaxAccountingBasis>A</TaxAccountingBasis>"),
            "TaxAccountingBasis: {xml}"
        );
        assert!(
            xml.contains("<SegmentIndex>1</SegmentIndex>"),
            "SegmentIndex: {xml}"
        );
        assert!(
            xml.contains("<TotalSegmentsInsequence>1</TotalSegmentsInsequence>"),
            "TotalSegmentsInsequence: {xml}"
        );
    }

    #[test]
    fn header_company_has_contact_and_bank() {
        let company = test_company();
        let mut w = new_writer().unwrap();
        start_elem(&mut w, "AuditFile").unwrap();
        write_header_company(&mut w, &company).unwrap();
        end_elem(&mut w, "AuditFile").unwrap();
        let xml = crate::anaf_decl::xml::finish(w).unwrap();

        assert!(xml.contains("<ContactPerson>"), "ContactPerson: {xml}");
        assert!(
            xml.contains("<FirstName>Administrator</FirstName>"),
            "FirstName: {xml}"
        );
        assert!(
            xml.contains("<Telephone>0721000000</Telephone>"),
            "Telephone: {xml}"
        );
        assert!(
            xml.contains("<IBANNumber>RO49AAAA1B31007593840000</IBANNumber>"),
            "IBAN: {xml}"
        );
        assert!(xml.contains("<TaxRegistration>"), "TaxRegistration: {xml}");
        assert!(
            xml.contains("<TaxAuthority>ANAF</TaxAuthority>"),
            "TaxAuthority: {xml}"
        );
    }

    // NOTE: write_general_ledger_entries is now async and requires a pool; unit-level
    // testing of the GLE emission is covered by the integration test in tests/saft_xsd.rs.
}
