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
    let type_code = if input.storno_ref.is_some() { "381" } else { "380" };
    {
        let mut tc_elem = BytesStart::new("cbc:InvoiceTypeCode");
        tc_elem.push_attribute(("listID", "UNCL1001"));
        writer.write_event(Event::Start(tc_elem)).map_err(|e| AppError::Xml(e.to_string()))?;
        writer.write_event(Event::Text(BytesText::new(type_code))).map_err(|e| AppError::Xml(e.to_string()))?;
        writer.write_event(Event::End(BytesEnd::new("cbc:InvoiceTypeCode"))).map_err(|e| AppError::Xml(e.to_string()))?;
    }

    write_text(&mut writer, "cbc:DocumentCurrencyCode", currency)?;

    // BillingReference — obligatoriu pentru factura de storno (381)
    if let Some(ref original_number) = input.storno_ref {
        writer.write_event(Event::Start(BytesStart::new("cac:BillingReference"))).map_err(|e| AppError::Xml(e.to_string()))?;
        writer.write_event(Event::Start(BytesStart::new("cac:InvoiceDocumentReference"))).map_err(|e| AppError::Xml(e.to_string()))?;
        write_text(&mut writer, "cbc:ID", original_number)?;
        writer.write_event(Event::End(BytesEnd::new("cac:InvoiceDocumentReference"))).map_err(|e| AppError::Xml(e.to_string()))?;
        writer.write_event(Event::End(BytesEnd::new("cac:BillingReference"))).map_err(|e| AppError::Xml(e.to_string()))?;
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
    let subtotal = fmt_amount(inv.subtotal_amount);
    let total = fmt_amount(inv.total_amount);
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

fn write_supplier_party(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    seller: &Company,
) -> AppResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new(
            "cac:AccountingSupplierParty",
        )))
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
    let seller_cui_digits = seller.cui.trim().trim_start_matches("RO").trim_start_matches("ro");
    write_text(writer, "cbc:CompanyID", &format!("RO{}", seller_cui_digits))?;
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
        .write_event(Event::End(BytesEnd::new(
            "cac:AccountingSupplierParty",
        )))
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
        .write_event(Event::Start(BytesStart::new(
            "cac:AccountingCustomerParty",
        )))
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
        let buyer_cui_digits = cui.trim().trim_start_matches("RO").trim_start_matches("ro");
        write_text(writer, "cbc:CompanyID", &format!("RO{}", buyer_cui_digits))?;
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
        .write_event(Event::End(BytesEnd::new(
            "cac:AccountingCustomerParty",
        )))
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
        let rate_str = format_decimal_2(line.vat_rate);
        let key = (rate_str, line.vat_category.clone());
        let entry = groups.entry(key).or_insert((Decimal::ZERO, Decimal::ZERO));
        entry.0 += Decimal::from_str(&fmt_amount(line.subtotal_amount))
            .unwrap_or(Decimal::ZERO);
        entry.1 += Decimal::from_str(&fmt_amount(line.vat_amount))
            .unwrap_or(Decimal::ZERO);
    }

    let total_vat = fmt_amount(inv.vat_amount);

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
        write_amount(
            writer,
            "cbc:TaxAmount",
            &format!("{:.2}", vat),
            currency,
        )?;
        writer
            .write_event(Event::Start(BytesStart::new("cac:TaxCategory")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        write_text(writer, "cbc:ID", category)?;
        write_text(writer, "cbc:Percent", rate_str)?;
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
    writer
        .write_event(Event::Text(BytesText::new(&format_decimal_2(
            line.quantity,
        ))))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("cbc:InvoicedQuantity")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    write_amount(
        writer,
        "cbc:LineExtensionAmount",
        &fmt_amount(line.subtotal_amount),
        currency,
    )?;

    // <cac:Item>
    writer
        .write_event(Event::Start(BytesStart::new("cac:Item")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

    let description = line
        .description
        .as_deref()
        .unwrap_or(&line.name);
    write_text(writer, "cbc:Description", description)?;
    write_text(writer, "cbc:Name", &line.name)?;

    writer
        .write_event(Event::Start(BytesStart::new(
            "cac:ClassifiedTaxCategory",
        )))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    write_text(writer, "cbc:ID", &line.vat_category)?;
    write_text(
        writer,
        "cbc:Percent",
        &format_decimal_2(line.vat_rate),
    )?;
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
        &fmt_amount(line.unit_price),
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

fn write_text(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    tag: &str,
    value: &str,
) -> AppResult<()> {
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

/// Formatează `f64` ca string cu 2 zecimale.
fn fmt_amount(v: f64) -> String {
    format!("{:.2}", v)
}

/// Formatează un număr `f64` ca string cu 2 zecimale (pentru rate/cantităţi).
fn format_decimal_2(v: f64) -> String {
    format!("{:.2}", v)
}
