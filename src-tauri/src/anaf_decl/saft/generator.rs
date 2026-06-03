//! SAF-T D406 XML generator — assembles all four mandatory sections.
//!
//! Output structure:
//! ```xml
//! <?xml version="1.0" encoding="UTF-8"?>
//! <AuditFile xmlns="mfp:anaf:dgti:d406t:declaratie:v1">
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

use crate::anaf_decl::saft::masterfiles::write_master_files;
use crate::anaf_decl::saft::source_docs::write_source_documents;
use crate::anaf_decl::xml::{end_elem, start_elem, write_text_elem};
use crate::db::companies::Company;
use crate::error::{AppError, AppResult};

const NAMESPACE: &str = "mfp:anaf:dgti:d406t:declaratie:v1";
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

    // HeaderComment (required in HeaderStructure per XSD)
    write_text_elem(w, "HeaderComment", "Generat de efactura-desktop")?;

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

    // RegistrationNumber (required, SAFmiddle1textType max 35)
    let cui_clean = company.cui.trim();
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
        write_text_elem(w, "Region", &trunc_esc(&company.county, 35))?;
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
        write_text_elem(w, "TaxType", "TVA")?;
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

/// Write the empty GeneralLedgerEntries mandatory wrapper (Phase 4).
/// All children (NumberOfEntries, TotalDebit, TotalCredit, Journal) are minOccurs=0.
fn write_general_ledger_entries(w: &mut crate::anaf_decl::xml::XmlWriter) -> AppResult<()> {
    start_elem(w, "GeneralLedgerEntries")?;
    write_text_elem(w, "NumberOfEntries", "0")?;
    write_text_elem(w, "TotalDebit", "0.00")?;
    write_text_elem(w, "TotalCredit", "0.00")?;
    end_elem(w, "GeneralLedgerEntries")?;
    Ok(())
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
pub async fn generate_saft_xml(
    pool: &sqlx::SqlitePool,
    company: &Company,
    date_from: &str,
    date_to: &str,
) -> AppResult<String> {
    // Use new_with_indent for human-readable output (2-space indent)
    let mut w = Writer::new_with_indent(Cursor::new(Vec::<u8>::new()), b' ', 2);

    // <?xml version="1.0" encoding="UTF-8"?>
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(map_qx)?;

    // <AuditFile xmlns="mfp:anaf:dgti:d406t:declaratie:v1">
    let mut root = BytesStart::new("AuditFile");
    root.push_attribute(("xmlns", NAMESPACE));
    w.write_event(Event::Start(root)).map_err(map_qx)?;

    // We need a XmlWriter (Writer<Cursor<Vec<u8>>>) from this point.
    // We've built the root start tag directly on `w` which IS a XmlWriter.
    // Continue using `w` via the xml helper functions by wrapping:
    write_header(&mut w, company, date_from, date_to)?;
    write_master_files(&mut w, pool, company, date_from, date_to).await?;
    write_general_ledger_entries(&mut w)?;
    write_source_documents(&mut w, pool, &company.id, date_from, date_to).await?;

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
            cui: "RO12345678".to_string(),
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
        write_header(&mut w, &company, "2025-01-01", "2025-01-31").unwrap();
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

    #[test]
    fn empty_gl_entries_has_zero_totals() {
        let mut w = new_writer().unwrap();
        write_general_ledger_entries(&mut w).unwrap();
        let xml = crate::anaf_decl::xml::finish(w).unwrap();
        assert!(
            xml.contains("<NumberOfEntries>0</NumberOfEntries>"),
            "NumberOfEntries: {xml}"
        );
        assert!(
            xml.contains("<TotalDebit>0.00</TotalDebit>"),
            "TotalDebit: {xml}"
        );
        assert!(
            xml.contains("<TotalCredit>0.00</TotalCredit>"),
            "TotalCredit: {xml}"
        );
        assert!(
            !xml.contains("<Journal>"),
            "must have no Journal children: {xml}"
        );
    }
}
