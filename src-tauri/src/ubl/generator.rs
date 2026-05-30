//! Generare UBL 2.1 XML conform standardului CIUS-RO.
//!
//! Foloseşte `quick-xml` Writer API (nu serde serialize) pentru a construi
//! XML-ul direct ca string.

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::io::Cursor;
use std::str::FromStr;

use crate::db::companies::Company;
use crate::db::contacts::Contact;
use crate::db::invoices::{Invoice, LineItem};
use crate::error::{AppError, AppResult};

pub struct GeneratorInput {
    pub invoice: Invoice,
    pub lines: Vec<LineItem>,
    pub seller: Company,
    pub buyer: Contact,
    /// Dacă este o factură de storno (credit note 381), conține numărul facturii originale.
    pub storno_ref: Option<String>,
}

/// Generează un string XML UBL 2.1 valid CIUS-RO pentru factura dată.
pub fn generate_ubl(input: &GeneratorInput) -> AppResult<String> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    // XML declaration
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    // <Invoice ...>
    let mut invoice_elem = BytesStart::new("Invoice");
    invoice_elem.push_attribute((
        "xmlns",
        "urn:oasis:names:specification:ubl:schema:xsd:Invoice-2",
    ));
    invoice_elem.push_attribute((
        "xmlns:cac",
        "urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2",
    ));
    invoice_elem.push_attribute((
        "xmlns:cbc",
        "urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2",
    ));
    writer
        .write_event(Event::Start(invoice_elem))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    let inv = &input.invoice;
    let seller = &input.seller;
    let buyer = &input.buyer;
    let currency = &inv.currency;

    // ── Header fields ────────────────────────────────────────────────────────
    write_text(
        &mut writer,
        "cbc:CustomizationID",
        "urn:cen.eu:en16931:2017#compliant#urn:efactura.mfinante.ro:CIUS-RO:1.0.1",
    )?;
    write_text(
        &mut writer,
        "cbc:ProfileID",
        "urn:fdc:peppol.eu:2017:poacc:billing:01:1.0",
    )?;
    write_text(&mut writer, "cbc:ID", &inv.full_number)?;
    write_text(&mut writer, "cbc:IssueDate", &inv.issue_date)?;
    write_text(&mut writer, "cbc:DueDate", &inv.due_date)?;

    // InvoiceTypeCode: 380 = factură normală, 381 = notă de credit (storno)
    // listID="UNCL1001" este obligatoriu per EN 16931 / CIUS-RO
    let type_code = if input.storno_ref.is_some() {
        "381"
    } else {
        "380"
    };
    {
        let mut tc_elem = BytesStart::new("cbc:InvoiceTypeCode");
        tc_elem.push_attribute(("listID", "UNCL1001"));
        writer
            .write_event(Event::Start(tc_elem))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        writer
            .write_event(Event::Text(BytesText::new(type_code)))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        writer
            .write_event(Event::End(BytesEnd::new("cbc:InvoiceTypeCode")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
    }

    write_text(&mut writer, "cbc:DocumentCurrencyCode", currency)?;

    // BillingReference — obligatoriu pentru factura de storno (381)
    if let Some(ref original_number) = input.storno_ref {
        writer
            .write_event(Event::Start(BytesStart::new("cac:BillingReference")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        writer
            .write_event(Event::Start(BytesStart::new(
                "cac:InvoiceDocumentReference",
            )))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        write_text(&mut writer, "cbc:ID", original_number)?;
        writer
            .write_event(Event::End(BytesEnd::new("cac:InvoiceDocumentReference")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        writer
            .write_event(Event::End(BytesEnd::new("cac:BillingReference")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
    }

    // ── AccountingSupplierParty ──────────────────────────────────────────────
    write_supplier_party(&mut writer, seller)?;

    // ── AccountingCustomerParty ──────────────────────────────────────────────
    write_customer_party(&mut writer, buyer, currency)?;

    // ── PaymentMeans (BR-RO-100) ─────────────────────────────────────────────
    writer
        .write_event(Event::Start(BytesStart::new("cac:PaymentMeans")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::Start(BytesStart::new("cbc:PaymentMeansCode")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(inv.payment_means_code.as_str())))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("cbc:PaymentMeansCode")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    if let Some(ref iban) = seller.iban {
        if !iban.is_empty() {
            writer
                .write_event(Event::Start(BytesStart::new("cac:PayeeFinancialAccount")))
                .map_err(|e| AppError::Xml(e.to_string()))?;
            writer
                .write_event(Event::Start(BytesStart::new("cbc:ID")))
                .map_err(|e| AppError::Xml(e.to_string()))?;
            writer
                .write_event(Event::Text(BytesText::new(iban)))
                .map_err(|e| AppError::Xml(e.to_string()))?;
            writer
                .write_event(Event::End(BytesEnd::new("cbc:ID")))
                .map_err(|e| AppError::Xml(e.to_string()))?;
            writer
                .write_event(Event::End(BytesEnd::new("cac:PayeeFinancialAccount")))
                .map_err(|e| AppError::Xml(e.to_string()))?;
        }
    }
    writer
        .write_event(Event::End(BytesEnd::new("cac:PaymentMeans")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    // ── TaxTotal ────────────────────────────────────────────────────────────
    write_tax_total(&mut writer, &input.lines, inv, currency)?;

    // ── LegalMonetaryTotal ───────────────────────────────────────────────────
    let subtotal = fmt_amount(&inv.subtotal_amount);
    let total = fmt_amount(&inv.total_amount);
    writer
        .write_event(Event::Start(BytesStart::new("cac:LegalMonetaryTotal")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_amount(&mut writer, "cbc:LineExtensionAmount", &subtotal, currency)?;
    write_amount(&mut writer, "cbc:TaxExclusiveAmount", &subtotal, currency)?;
    write_amount(&mut writer, "cbc:TaxInclusiveAmount", &total, currency)?;
    write_amount(&mut writer, "cbc:PayableAmount", &total, currency)?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:LegalMonetaryTotal")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    // ── InvoiceLine per linie ────────────────────────────────────────────────
    for line in &input.lines {
        write_invoice_line(&mut writer, line, currency)?;
    }

    // </Invoice>
    writer
        .write_event(Event::End(BytesEnd::new("Invoice")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    let bytes = writer.into_inner().into_inner();
    let xml_string = String::from_utf8(bytes).map_err(|e| AppError::Xml(e.to_string()))?;
    // Prepend UTF-8 BOM (U+FEFF) required by ANAF
    let mut with_bom = String::from("\u{FEFF}");
    with_bom.push_str(&xml_string);
    Ok(with_bom)
}

// ─── Supplier party ──────────────────────────────────────────────────────────

fn write_supplier_party(writer: &mut Writer<Cursor<Vec<u8>>>, seller: &Company) -> AppResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("cac:AccountingSupplierParty")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::Start(BytesStart::new("cac:Party")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    // PartyName
    writer
        .write_event(Event::Start(BytesStart::new("cac:PartyName")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_text(writer, "cbc:Name", &seller.legal_name)?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:PartyName")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    // PostalAddress
    writer
        .write_event(Event::Start(BytesStart::new("cac:PostalAddress")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_text(writer, "cbc:StreetName", &seller.address)?;
    write_text(writer, "cbc:CityName", &seller.city)?;
    write_text(writer, "cbc:CountrySubentity", &seller.county)?;
    writer
        .write_event(Event::Start(BytesStart::new("cac:Country")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_text(writer, "cbc:IdentificationCode", &seller.country)?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:Country")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:PostalAddress")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    // PartyTaxScheme
    writer
        .write_event(Event::Start(BytesStart::new("cac:PartyTaxScheme")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    let seller_cui_digits = seller
        .cui
        .trim()
        .trim_start_matches("RO")
        .trim_start_matches("ro");
    // BIZ-02: only VAT-registered sellers get the "RO" prefix in PartyTaxScheme/CompanyID
    let supplier_cui_xml = if seller.vat_payer {
        format!("RO{}", seller_cui_digits)
    } else {
        seller_cui_digits.to_string()
    };
    write_text(writer, "cbc:CompanyID", &supplier_cui_xml)?;
    writer
        .write_event(Event::Start(BytesStart::new("cac:TaxScheme")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_text(writer, "cbc:ID", "VAT")?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:TaxScheme")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:PartyTaxScheme")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    // PartyLegalEntity
    writer
        .write_event(Event::Start(BytesStart::new("cac:PartyLegalEntity")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_text(writer, "cbc:RegistrationName", &seller.legal_name)?;
    write_text(writer, "cbc:CompanyID", &seller.cui)?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:PartyLegalEntity")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("cac:Party")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:AccountingSupplierParty")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    Ok(())
}

// ─── Customer party ──────────────────────────────────────────────────────────

fn write_customer_party(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    buyer: &Contact,
    _currency: &str,
) -> AppResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("cac:AccountingCustomerParty")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::Start(BytesStart::new("cac:Party")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    // PartyName
    writer
        .write_event(Event::Start(BytesStart::new("cac:PartyName")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_text(writer, "cbc:Name", &buyer.legal_name)?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:PartyName")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    // PostalAddress (optional fields)
    writer
        .write_event(Event::Start(BytesStart::new("cac:PostalAddress")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    if let Some(addr) = &buyer.address {
        write_text(writer, "cbc:StreetName", addr)?;
    }
    if let Some(city) = &buyer.city {
        write_text(writer, "cbc:CityName", city)?;
    }
    if let Some(county) = &buyer.county {
        write_text(writer, "cbc:CountrySubentity", county)?;
    }
    writer
        .write_event(Event::Start(BytesStart::new("cac:Country")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_text(writer, "cbc:IdentificationCode", &buyer.country)?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:Country")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:PostalAddress")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    // PartyTaxScheme — doar dacă buyerul are CUI
    if let Some(cui) = &buyer.cui {
        writer
            .write_event(Event::Start(BytesStart::new("cac:PartyTaxScheme")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        // BIZ-03: emit the original VAT ID for non-RO buyers; only RO buyers get the
        // "RO" prefix normalization. If the stored CUI already begins with a
        // two-letter ISO country code (e.g. "DE123456789"), keep it as-is.
        let trimmed = cui.trim();
        let country_code = buyer.country.trim().to_ascii_uppercase();
        let starts_with_country_prefix = trimmed.len() >= 2
            && trimmed.as_bytes()[0].is_ascii_alphabetic()
            && trimmed.as_bytes()[1].is_ascii_alphabetic();
        let buyer_cui_xml = if starts_with_country_prefix {
            // Caller already provided VAT-ID with country prefix — trust it.
            trimmed.to_string()
        } else if country_code.is_empty() || country_code == "RO" {
            format!("RO{}", trimmed)
        } else {
            // Non-RO buyer, no prefix in stored value — leave digits as-is
            // (we cannot safely synthesize a foreign VAT-ID prefix here).
            trimmed.to_string()
        };
        write_text(writer, "cbc:CompanyID", &buyer_cui_xml)?;
        writer
            .write_event(Event::Start(BytesStart::new("cac:TaxScheme")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        write_text(writer, "cbc:ID", "VAT")?;
        writer
            .write_event(Event::End(BytesEnd::new("cac:TaxScheme")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        writer
            .write_event(Event::End(BytesEnd::new("cac:PartyTaxScheme")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
    }

    // PartyLegalEntity
    writer
        .write_event(Event::Start(BytesStart::new("cac:PartyLegalEntity")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_text(writer, "cbc:RegistrationName", &buyer.legal_name)?;
    if let Some(cui) = &buyer.cui {
        write_text(writer, "cbc:CompanyID", cui)?;
    }
    writer
        .write_event(Event::End(BytesEnd::new("cac:PartyLegalEntity")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("cac:Party")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:AccountingCustomerParty")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    Ok(())
}

// ─── TaxTotal ─────────────────────────────────────────────────────────────────

/// Grupează liniile pe (vat_rate, vat_category) şi emite TaxTotal cu subtotaluri.
fn write_tax_total(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    lines: &[LineItem],
    inv: &Invoice,
    currency: &str,
) -> AppResult<()> {
    // group by (vat_rate_str, vat_category)
    let mut groups: HashMap<(String, String), (Decimal, Decimal)> = HashMap::new();
    for line in lines {
        let rate_str = format_decimal_2(&line.vat_rate);
        let key = (rate_str, line.vat_category.clone());
        let entry = groups.entry(key).or_insert((Decimal::ZERO, Decimal::ZERO));
        entry.0 += Decimal::from_str(&fmt_amount(&line.subtotal_amount)).unwrap_or(Decimal::ZERO);
        entry.1 += Decimal::from_str(&fmt_amount(&line.vat_amount)).unwrap_or(Decimal::ZERO);
    }

    let total_vat = fmt_amount(&inv.vat_amount);

    writer
        .write_event(Event::Start(BytesStart::new("cac:TaxTotal")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_amount(writer, "cbc:TaxAmount", &total_vat, currency)?;

    // Sort keys for deterministic output
    let mut sorted_keys: Vec<(String, String)> = groups.keys().cloned().collect();
    sorted_keys.sort();

    for key in sorted_keys {
        let (taxable, vat) = groups[&key];
        let (rate_str, category) = &key;
        writer
            .write_event(Event::Start(BytesStart::new("cac:TaxSubtotal")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        write_amount(
            writer,
            "cbc:TaxableAmount",
            &format!("{:.2}", taxable),
            currency,
        )?;
        write_amount(writer, "cbc:TaxAmount", &format!("{:.2}", vat), currency)?;
        writer
            .write_event(Event::Start(BytesStart::new("cac:TaxCategory")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        write_text(writer, "cbc:ID", category)?;
        write_text(writer, "cbc:Percent", rate_str)?;
        // BIZ-05: emit exemption code for non-standard categories
        write_tax_exemption(writer, category)?;
        writer
            .write_event(Event::Start(BytesStart::new("cac:TaxScheme")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        write_text(writer, "cbc:ID", "VAT")?;
        writer
            .write_event(Event::End(BytesEnd::new("cac:TaxScheme")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        writer
            .write_event(Event::End(BytesEnd::new("cac:TaxCategory")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        writer
            .write_event(Event::End(BytesEnd::new("cac:TaxSubtotal")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("cac:TaxTotal")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    Ok(())
}

// ─── InvoiceLine ─────────────────────────────────────────────────────────────

fn write_invoice_line(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    line: &LineItem,
    currency: &str,
) -> AppResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("cac:InvoiceLine")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    write_text(writer, "cbc:ID", &line.position.to_string())?;

    // <cbc:InvoicedQuantity unitCode="...">
    let mut qty_elem = BytesStart::new("cbc:InvoicedQuantity");
    qty_elem.push_attribute(("unitCode", line.unit.as_str()));
    writer
        .write_event(Event::Start(qty_elem))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    // BIZ-09: quantity uses 6-decimal precision (CIUS-RO allows it; avoids
    // truncating fractional units like grams or per-hour billing).
    writer
        .write_event(Event::Text(BytesText::new(&format_decimal_n(
            &line.quantity,
            6,
        ))))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("cbc:InvoicedQuantity")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    write_amount(
        writer,
        "cbc:LineExtensionAmount",
        &fmt_amount(&line.subtotal_amount),
        currency,
    )?;

    // <cac:Item>
    writer
        .write_event(Event::Start(BytesStart::new("cac:Item")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    let description = line.description.as_deref().unwrap_or(&line.name);
    write_text(writer, "cbc:Description", description)?;
    write_text(writer, "cbc:Name", &line.name)?;

    writer
        .write_event(Event::Start(BytesStart::new("cac:ClassifiedTaxCategory")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_text(writer, "cbc:ID", &line.vat_category)?;
    write_text(writer, "cbc:Percent", &format_decimal_2(&line.vat_rate))?;
    // BIZ-05: line-level exemption code (mirrors TaxSubtotal/TaxCategory)
    write_tax_exemption(writer, &line.vat_category)?;
    writer
        .write_event(Event::Start(BytesStart::new("cac:TaxScheme")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_text(writer, "cbc:ID", "VAT")?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:TaxScheme")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:ClassifiedTaxCategory")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("cac:Item")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    // <cac:Price>
    writer
        .write_event(Event::Start(BytesStart::new("cac:Price")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_amount(
        writer,
        "cbc:PriceAmount",
        &fmt_amount(&line.unit_price),
        currency,
    )?;
    writer
        .write_event(Event::End(BytesEnd::new("cac:Price")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("cac:InvoiceLine")))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    Ok(())
}

// ─── Low-level helpers ────────────────────────────────────────────────────────

fn write_text(writer: &mut Writer<Cursor<Vec<u8>>>, tag: &str, value: &str) -> AppResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new(tag)))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(value)))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new(tag)))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    Ok(())
}

/// Emite un element cu atribut `currencyID`.
fn write_amount(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    tag: &str,
    value: &str,
    currency: &str,
) -> AppResult<()> {
    let mut elem = BytesStart::new(tag);
    elem.push_attribute(("currencyID", currency));
    writer
        .write_event(Event::Start(elem))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(value)))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new(tag)))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    Ok(())
}

/// Formatează un `&str` numeric ca string cu 2 zecimale, rutând prin `Decimal`
/// pentru rotunjire zecimală corectă (evită artefactele binary-float).
fn fmt_amount(s: &str) -> String {
    let d = Decimal::from_str(s.trim())
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);
    format!("{:.2}", d)
}

/// Formatează un `&str` numeric ca string cu 2 zecimale (pentru rate/cantităţi).
/// Identic cu `fmt_amount` — ambele rutează prin `Decimal`.
fn format_decimal_2(s: &str) -> String {
    fmt_amount(s)
}

/// Formatează un `&str` numeric ca string cu `n` zecimale, rutând prin `Decimal`.
/// Folosit pentru cantităţi unde CIUS-RO permite până la 6 zecimale.
fn format_decimal_n(s: &str, n: u32) -> String {
    let d = Decimal::from_str(s.trim())
        .unwrap_or(Decimal::ZERO)
        .round_dp(n);
    format!("{:.*}", n as usize, d)
}

/// BIZ-05: emite `<cbc:TaxExemptionReasonCode>` + `<cbc:TaxExemptionReason>`
/// pentru categoriile TVA care nu sunt cota standard ("S"). Codurile urmează
/// lista VATEX-EU publicată de CEF (EN 16931 / CIUS-RO).
fn write_tax_exemption(writer: &mut Writer<Cursor<Vec<u8>>>, category: &str) -> AppResult<()> {
    let (code, reason) = match category {
        "E" => ("VATEX-EU-132", "Scutire fără drept de deducere"),
        "Z" => ("VATEX-EU-G", "Cota zero"),
        "AE" => ("VATEX-EU-AE", "Taxare inversă"),
        "K" => ("VATEX-EU-IC", "Livrare intracomunitară"),
        "G" => ("VATEX-EU-G", "Export în afara UE"),
        "O" => ("VATEX-EU-O", "În afara sferei TVA"),
        _ => return Ok(()), // "S" standard rate — no exemption emitted
    };
    write_text(writer, "cbc:TaxExemptionReasonCode", code)?;
    write_text(writer, "cbc:TaxExemptionReason", reason)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input() -> GeneratorInput {
        use crate::db::companies::Company;
        use crate::db::contacts::Contact;
        use crate::db::invoices::{Invoice, LineItem};

        let seller = Company {
            id: "company-1".to_string(),
            cui: "RO12345678".to_string(),
            legal_name: "Test SRL".to_string(),
            trade_name: None,
            registry_number: None,
            vat_payer: true,
            address: "Str. Exemplu nr. 1".to_string(),
            city: "București".to_string(),
            county: "Sector 1".to_string(),
            postal_code: None,
            country: "RO".to_string(),
            email: None,
            phone: None,
            iban: None,
            bank_name: None,
            is_active: true,
            spv_enabled: false,
            invoice_series: "FAC".to_string(),
            last_invoice_number: 1,
            logo_path: None,
            created_at: 0,
            updated_at: 0,
        };

        let buyer = Contact {
            id: "contact-1".to_string(),
            company_id: "company-1".to_string(),
            contact_type: "CUSTOMER".to_string(),
            cui: Some("RO87654321".to_string()),
            legal_name: "Client SRL".to_string(),
            vat_payer: true,
            address: Some("Str. Client nr. 2".to_string()),
            city: Some("Cluj-Napoca".to_string()),
            county: Some("Cluj".to_string()),
            country: "RO".to_string(),
            email: None,
            phone: None,
            currency: None,
            created_at: 0,
            updated_at: 0,
        };

        let invoice = Invoice {
            id: "invoice-1".to_string(),
            company_id: "company-1".to_string(),
            contact_id: "contact-1".to_string(),
            series: "FAC".to_string(),
            number: 1,
            full_number: "FAC-2024-0001".to_string(),
            issue_date: "2024-01-15".to_string(),
            due_date: "2024-02-15".to_string(),
            currency: "RON".to_string(),
            exchange_rate: None,
            subtotal_amount: "100.00".to_string(),
            vat_amount: "19.00".to_string(),
            total_amount: "119.00".to_string(),
            status: "DRAFT".to_string(),
            anaf_upload_id: None,
            anaf_index: None,
            anaf_submitted_at: None,
            anaf_validated_at: None,
            anaf_rejected_at: None,
            xml_path: None,
            pdf_path: None,
            signature_xml_path: None,
            rejection_reason: None,
            rejection_code: None,
            notes: None,
            payment_means_code: "30".to_string(),
            storno_of_invoice_id: None,
            created_at: 0,
            updated_at: 0,
        };

        let line = LineItem {
            id: "line-1".to_string(),
            invoice_id: "invoice-1".to_string(),
            position: 1,
            name: "Serviciu consultanță".to_string(),
            description: None,
            quantity: "1.00".to_string(),
            unit: "H87".to_string(),
            unit_price: "100.00".to_string(),
            vat_rate: "19.00".to_string(),
            vat_category: "S".to_string(),
            subtotal_amount: "100.00".to_string(),
            vat_amount: "19.00".to_string(),
            total_amount: "119.00".to_string(),
            cpv_code: None,
        };

        GeneratorInput {
            invoice,
            lines: vec![line],
            seller,
            buyer,
            storno_ref: None,
        }
    }

    #[test]
    fn xml_starts_with_utf8_bom() {
        let input = sample_input();
        let xml = generate_ubl(&input).expect("should generate XML");
        let bytes = xml.as_bytes();
        assert!(bytes.len() >= 3, "XML too short");
        assert_eq!(
            &bytes[..3],
            &[0xEF, 0xBB, 0xBF],
            "XML must start with UTF-8 BOM (EF BB BF)"
        );
    }

    #[test]
    fn non_vat_payer_seller_omits_ro_prefix() {
        // BIZ-02: a non-VAT-registered seller must NOT have the "RO" prefix on
        // its PartyTaxScheme/CompanyID.
        let mut input = sample_input();
        input.seller.vat_payer = false;
        input.seller.cui = "12345678".to_string();
        let xml = generate_ubl(&input).expect("should generate XML");

        // Strip BOM for substring search clarity.
        let body = xml.trim_start_matches('\u{FEFF}');
        // Supplier PartyTaxScheme block must contain the bare digits, not "RO…".
        assert!(
            body.contains("<cbc:CompanyID>12345678</cbc:CompanyID>"),
            "expected bare CUI in supplier PartyTaxScheme, got: {}",
            body
        );
        // And specifically should not produce "RO12345678" in the supplier party.
        // (PartyLegalEntity may still carry the raw stored value.)
        let supplier_block = body
            .split("</cac:AccountingSupplierParty>")
            .next()
            .unwrap_or(body);
        assert!(
            !supplier_block.contains("RO12345678"),
            "supplier party should not contain RO-prefixed CUI for non-VAT-payer"
        );
    }

    #[test]
    fn eu_buyer_keeps_original_vat_id() {
        // BIZ-03: a German buyer with VAT-ID "DE123456789" must keep that
        // identifier verbatim in the customer PartyTaxScheme.
        let mut input = sample_input();
        input.buyer.country = "DE".to_string();
        input.buyer.cui = Some("DE123456789".to_string());
        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');

        let customer_block = body
            .split("</cac:AccountingCustomerParty>")
            .next()
            .unwrap_or(body);
        // Must contain the original DE VAT-ID verbatim.
        assert!(
            customer_block.contains("<cbc:CompanyID>DE123456789</cbc:CompanyID>"),
            "expected DE VAT-ID kept as-is, got: {}",
            customer_block
        );
        // Must NOT have synthesized "RODE123456789" or "RO123456789".
        assert!(
            !customer_block.contains("RODE123456789"),
            "must not double-prefix DE VAT-ID"
        );
    }

    #[test]
    fn ro_buyer_gets_ro_prefix() {
        // BIZ-03: a Romanian buyer with raw digits stored gets the "RO" prefix
        // synthesized in PartyTaxScheme/CompanyID.
        let mut input = sample_input();
        input.buyer.country = "RO".to_string();
        input.buyer.cui = Some("87654321".to_string());
        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');

        let customer_block = body
            .split("</cac:AccountingCustomerParty>")
            .next()
            .unwrap_or(body);
        assert!(
            customer_block.contains("<cbc:CompanyID>RO87654321</cbc:CompanyID>"),
            "expected RO-prefixed buyer CUI, got: {}",
            customer_block
        );
    }

    #[test]
    fn exemption_code_emitted_for_reverse_charge() {
        // BIZ-05: vat_category "AE" (reverse charge) must emit
        // <cbc:TaxExemptionReasonCode>VATEX-EU-AE</cbc:TaxExemptionReasonCode>
        // in both the TaxSubtotal/TaxCategory and the line ClassifiedTaxCategory.
        let mut input = sample_input();
        input.lines[0].vat_category = "AE".to_string();
        input.lines[0].vat_rate = "0.00".to_string();
        input.lines[0].vat_amount = "0.00".to_string();
        input.invoice.vat_amount = "0.00".to_string();
        input.invoice.total_amount = input.invoice.subtotal_amount.clone();
        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');
        let occurrences = body.matches("VATEX-EU-AE").count();
        assert!(
            occurrences >= 2,
            "expected VATEX-EU-AE in both TaxCategory and ClassifiedTaxCategory, found {}: {}",
            occurrences,
            body
        );
    }

    #[test]
    fn quantity_uses_six_decimal_precision() {
        // BIZ-09: quantities must serialize with 6 decimals, not 2.
        let mut input = sample_input();
        input.lines[0].quantity = "1.234567".to_string();
        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');
        assert!(
            body.contains(">1.234567<"),
            "expected 6-decimal quantity in XML, got: {}",
            body
        );
    }
}
