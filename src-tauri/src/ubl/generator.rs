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
use crate::ubl::fx;

pub struct GeneratorInput {
    pub invoice: Invoice,
    pub lines: Vec<LineItem>,
    pub seller: Company,
    pub buyer: Contact,
    /// Dacă este o factură de storno (credit note 381), conține numărul facturii originale.
    pub storno_ref: Option<String>,
}

/// Mențiunea obligatorie "TVA la încasare" (Cod fiscal art. 319 alin. (20) lit. r) se aplică
/// atunci când furnizorul aplică regimul TVA la încasare ŞI factura conţine cel puţin o
/// livrare taxabilă standard ("S") — operaţiunile excluse (taxare inversă / scutite / intra-UE
/// per art. 282 alin. (6)) nu declanşează mențiunea.
pub fn invoice_under_cash_vat(seller: &Company, lines: &[LineItem]) -> bool {
    lines.iter().any(|l| {
        crate::anaf_decl::cash_vat::sales_status(seller.cash_vat, &l.vat_category).applies()
    })
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

    // BT-22 — mențiunea obligatorie "TVA la încasare" (art. 319 alin. (20) lit. r). UBL impune
    // cbc:Note după InvoiceTypeCode şi înainte de DocumentCurrencyCode.
    if invoice_under_cash_vat(seller, &input.lines) {
        write_text(&mut writer, "cbc:Note", "TVA la încasare")?;
    }

    write_text(&mut writer, "cbc:DocumentCurrencyCode", currency)?;

    // BT-6 / EN16931 BR-53: when the invoice currency differs from RON (the
    // accounting currency for Romanian e-Factura), emit TaxCurrencyCode = "RON"
    // immediately after DocumentCurrencyCode (UBL element order matters).
    if !currency.eq_ignore_ascii_case("RON") {
        write_text(&mut writer, "cbc:TaxCurrencyCode", "RON")?;
    }

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

    // ── TaxTotal (document currency, with subtotals) ────────────────────────
    let doc_vat = write_tax_total(&mut writer, &input.lines, inv, currency)?;

    // BT-111 / EN16931 BR-53: for non-RON invoices emit a second TaxTotal
    // containing ONLY the TaxAmount in RON (no TaxSubtotals — per EN16931 the
    // accounting-currency TaxTotal carries only the aggregate TaxAmount).
    if !currency.eq_ignore_ascii_case("RON") {
        let ron_vat = fx::amount_to_ron(doc_vat, currency, fx::parse_rate(inv.exchange_rate));
        let ron_vat_str = format!("{:.2}", ron_vat);
        writer
            .write_event(Event::Start(BytesStart::new("cac:TaxTotal")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        write_amount(&mut writer, "cbc:TaxAmount", &ron_vat_str, "RON")?;
        writer
            .write_event(Event::End(BytesEnd::new("cac:TaxTotal")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
    }

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
    write_text(
        writer,
        "cbc:CountrySubentity",
        &county_to_nuts(&seller.county),
    )?;
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
        write_text(writer, "cbc:CountrySubentity", &county_to_nuts(county))?;
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
            // Non-RO buyer with no country prefix in the stored value: synthesize it
            // from the buyer's country so the EU VAT ID (BT-48) is well-formed
            // (e.g. "DE123456789") — CIUS/Schematron rejects an unprefixed foreign VAT ID.
            format!("{country_code}{trimmed}")
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
    } else if buyer.is_individual {
        // B2C: unidentified consumer (persoană fizică) — ANAF convention is the
        // placeholder buyer CompanyID "0000000000000" (13 zeros).
        write_text(writer, "cbc:CompanyID", "0000000000000")?;
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
/// Returnează valoarea totală a TVA (în moneda documentului) pentru a fi reutilizată
/// de apelant (ex. conversie în RON pentru al doilea TaxTotal).
fn write_tax_total(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    lines: &[LineItem],
    inv: &Invoice,
    currency: &str,
) -> AppResult<Decimal> {
    // group by (vat_rate_str, vat_category)
    let mut groups: HashMap<(String, String), (Decimal, Decimal)> = HashMap::new();
    for line in lines {
        let rate_str = format_decimal_2(&line.vat_rate);
        let key = (rate_str, line.vat_category.clone());
        let entry = groups.entry(key).or_insert((Decimal::ZERO, Decimal::ZERO));
        entry.0 += Decimal::from_str(&fmt_amount(&line.subtotal_amount)).unwrap_or(Decimal::ZERO);
        entry.1 += Decimal::from_str(&fmt_amount(&line.vat_amount)).unwrap_or(Decimal::ZERO);
    }

    let total_vat_dec = Decimal::from_str(&fmt_amount(&inv.vat_amount)).unwrap_or(Decimal::ZERO);
    let total_vat = format!("{:.2}", total_vat_dec);

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
    Ok(total_vat_dec)
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
///
/// Nota: categoria "Z" (cotă zero domestică) NU emite un cod VATEX-EU-G
/// (care este rezervat exclusiv exporturilor extra-UE, categoria "G").
/// Pentru Z, în conformitate cu EN 16931 / CIUS-RO, nu se emite nici un cod
/// de excepție — cota zero se exprimă prin `<cbc:Percent>0</cbc:Percent>`
/// în <cac:TaxCategory>, fără TaxExemptionReasonCode.
fn write_tax_exemption(writer: &mut Writer<Cursor<Vec<u8>>>, category: &str) -> AppResult<()> {
    let (code, reason) = match category {
        "E" => ("VATEX-EU-132", "Scutire fără drept de deducere"),
        // "Z" — cotă zero domestică: NU emite cod VATEX-EU-G (care este
        //        rezervat pentru exporturi extra-UE, categoria "G").
        "Z" => return Ok(()),
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

// ─── County name → ISO 3166-2:RO code ────────────────────────────────────────

/// Converts a free-text Romanian county name (or abbreviation) to the ISO
/// 3166-2:RO subdivision code required by CIUS-RO for `cbc:CountrySubentity`.
///
/// Rules (in priority order):
/// 1. If `raw` already matches `^RO-[A-Z]{1,2}$` → return as-is.
/// 2. Try a case-insensitive name lookup (handles diacritic and plain spellings).
/// 3. Fallback → return `raw` unchanged so unknown values don't break generation.
pub(crate) fn county_to_nuts(raw: &str) -> String {
    let trimmed = raw.trim();

    // Rule 1: already a valid ISO 3166-2:RO code.
    let upper = trimmed.to_ascii_uppercase();
    if let Some(rest) = upper.strip_prefix("RO-") {
        if !rest.is_empty() && rest.len() <= 2 && rest.chars().all(|c| c.is_ascii_alphabetic()) {
            return upper;
        }
    }

    // Rule 2: name lookup (normalise by lowercasing; diacritics stripped below).
    let lower = trimmed.to_lowercase();
    // Normalise by stripping common Romanian diacritics so both "Cluj" and
    // "Ilfov" match regardless of whether the caller used ș/ț or s/t.
    let norm = lower
        .replace(['ș', 'ş'], "s")
        .replace(['ț', 'ţ'], "t")
        .replace(['ă'], "a")
        .replace(['â', 'î'], "i");

    // Static map: (lowercase normalised name or abbreviation) → ISO code.
    // 41 județe + București (B).
    let code: Option<&'static str> = match norm.as_str() {
        // By ISO code (short form users sometimes type)
        "ab" | "alba" => Some("RO-AB"),
        "ar" | "arad" => Some("RO-AR"),
        "ag" | "arges" | "argeș" | "argeş" => Some("RO-AG"),
        "bc" | "bacau" | "bacău" => Some("RO-BC"),
        "bh" | "bihor" => Some("RO-BH"),
        "bn" | "bistrita-nasaud" | "bistrita nasaud" | "bistrița-năsăud" | "bistrita" => {
            Some("RO-BN")
        }
        "bt" | "botosani" | "botoșani" | "botoşani" => Some("RO-BT"),
        "bv" | "brasov" | "brașov" | "braşov" => Some("RO-BV"),
        "br" | "braila" | "brăila" | "brăilа" => Some("RO-BR"),
        // București — code is "B" (not "BU")
        "b"
        | "bucuresti"
        | "bucurești"
        | "municipiul bucuresti"
        | "municipiul bucurești"
        | "sector 1"
        | "sector 2"
        | "sector 3"
        | "sector 4"
        | "sector 5"
        | "sector 6"
        | "ilfov-bucuresti" => Some("RO-B"),
        "bz" | "buzau" | "buzău" => Some("RO-BZ"),
        "cs" | "caras-severin" | "caraș-severin" | "caras severin" => Some("RO-CS"),
        "cl" | "calarasi" | "călărași" | "calarași" => Some("RO-CL"),
        "cj" | "cluj" => Some("RO-CJ"),
        "ct" | "constanta" | "constanța" | "constanţa" => Some("RO-CT"),
        "cv" | "covasna" => Some("RO-CV"),
        "db" | "dambovita" | "dâmbovița" | "damboviţa" | "dambovița" => Some("RO-DB"),
        "dj" | "dolj" => Some("RO-DJ"),
        "gl" | "galati" | "galați" | "galaţi" => Some("RO-GL"),
        "gr" | "giurgiu" => Some("RO-GR"),
        "gj" | "gorj" => Some("RO-GJ"),
        "hr" | "harghita" => Some("RO-HR"),
        "hd" | "hunedoara" => Some("RO-HD"),
        "il" | "ialomita" | "ialomița" | "ialomiţa" => Some("RO-IL"),
        "is" | "iasi" | "iași" | "iaşi" => Some("RO-IS"),
        "if" | "ilfov" => Some("RO-IF"),
        "mm" | "maramures" | "maramureș" | "maramureş" => Some("RO-MM"),
        "mh" | "mehedinti" | "mehedinți" | "mehedinţi" => Some("RO-MH"),
        "ms" | "mures" | "mureș" | "mureş" => Some("RO-MS"),
        "nt" | "neamt" | "neamț" | "neamţ" => Some("RO-NT"),
        "ot" | "olt" => Some("RO-OT"),
        "ph" | "prahova" => Some("RO-PH"),
        "sm" | "satu mare" | "satu-mare" => Some("RO-SM"),
        "sj" | "salaj" | "sălaj" | "sălаj" => Some("RO-SJ"),
        "sb" | "sibiu" => Some("RO-SB"),
        "sv" | "suceava" => Some("RO-SV"),
        "tr" | "teleorman" => Some("RO-TR"),
        "tm" | "timis" | "timiș" | "timiş" => Some("RO-TM"),
        "tl" | "tulcea" => Some("RO-TL"),
        "vs" | "vaslui" => Some("RO-VS"),
        "vl" | "valcea" | "vâlcea" | "vâlcеa" => Some("RO-VL"),
        "vn" | "vrancea" => Some("RO-VN"),
        _ => None,
    };

    match code {
        Some(c) => c.to_string(),
        // Rule 3: unknown value — return unchanged.
        None => trimmed.to_string(),
    }
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
            cash_vat: false,
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
            cash_vat: false,
            is_individual: false,
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
            art331_code: None,
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
    fn cash_vat_mention_present_for_cash_vat_seller_with_s_line() {
        let mut input = sample_input();
        input.seller.cash_vat = true; // sample line is category "S"
        let xml = generate_ubl(&input).expect("should generate XML");
        assert!(
            xml.contains("<cbc:Note>TVA la încasare</cbc:Note>"),
            "BT-22 cash-VAT mention must be present"
        );
        // Order: Note must sit between InvoiceTypeCode and DocumentCurrencyCode.
        let note = xml.find("cbc:Note").expect("note");
        let itc = xml.find("cbc:InvoiceTypeCode").expect("type code");
        let dcc = xml.find("cbc:DocumentCurrencyCode").expect("currency");
        assert!(
            itc < note && note < dcc,
            "Note must follow InvoiceTypeCode, precede currency"
        );
    }

    #[test]
    fn cash_vat_mention_absent_for_non_cash_vat_seller() {
        let input = sample_input(); // cash_vat = false
        let xml = generate_ubl(&input).expect("should generate XML");
        assert!(
            !xml.contains("TVA la încasare"),
            "no mention when the seller is not on cash VAT"
        );
    }

    #[test]
    fn cash_vat_mention_absent_when_only_excluded_lines() {
        let mut input = sample_input();
        input.seller.cash_vat = true;
        input.lines[0].vat_category = "E".to_string(); // exempt — excluded (art. 282(6))
        let xml = generate_ubl(&input).expect("should generate XML");
        assert!(
            !xml.contains("TVA la încasare"),
            "an exempt-only invoice gets no cash-VAT mention"
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
    fn category_z_does_not_emit_vatex_eu_g() {
        // Fix #3: category "Z" (domestic zero-rated) must NOT emit VATEX-EU-G.
        // VATEX-EU-G is reserved exclusively for category "G" (export outside EU).
        let mut input = sample_input();
        input.lines[0].vat_category = "Z".to_string();
        input.lines[0].vat_rate = "0.00".to_string();
        input.lines[0].vat_amount = "0.00".to_string();
        input.invoice.vat_amount = "0.00".to_string();
        input.invoice.total_amount = input.invoice.subtotal_amount.clone();
        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');
        assert!(
            !body.contains("VATEX-EU-G"),
            "category Z must NOT emit VATEX-EU-G (export code); got: {}",
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

    // ── U5: county_to_nuts ────────────────────────────────────────────────────

    #[test]
    fn county_to_nuts_maps_cluj_to_ro_cj() {
        assert_eq!(county_to_nuts("Cluj"), "RO-CJ");
    }

    #[test]
    fn county_to_nuts_passthrough_existing_code() {
        // "RO-B" is already a valid ISO code — must be returned as-is.
        assert_eq!(county_to_nuts("RO-B"), "RO-B");
    }

    #[test]
    fn county_to_nuts_unknown_unchanged() {
        // Unknown values must not be altered.
        assert_eq!(county_to_nuts("Sector 7"), "Sector 7");
        assert_eq!(county_to_nuts("UNKNOWN"), "UNKNOWN");
    }

    #[test]
    fn county_to_nuts_bucuresti_to_ro_b() {
        assert_eq!(county_to_nuts("București"), "RO-B");
        assert_eq!(county_to_nuts("Bucuresti"), "RO-B");
        // Sector names map to RO-B
        assert_eq!(county_to_nuts("Sector 1"), "RO-B");
    }

    #[test]
    fn county_to_nuts_timis_to_ro_tm() {
        assert_eq!(county_to_nuts("Timiș"), "RO-TM");
        assert_eq!(county_to_nuts("Timis"), "RO-TM");
    }

    #[test]
    fn county_to_nuts_seller_county_emitted_as_iso_code() {
        // U5: seller county "Cluj" must appear as RO-CJ in XML, not "Cluj".
        let mut input = sample_input();
        input.seller.county = "Cluj".to_string();
        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');
        // Find the supplier block and check the CountrySubentity value.
        let supplier_block = body
            .split("</cac:AccountingSupplierParty>")
            .next()
            .unwrap_or(body);
        assert!(
            supplier_block.contains("<cbc:CountrySubentity>RO-CJ</cbc:CountrySubentity>"),
            "seller county 'Cluj' must be emitted as 'RO-CJ', got: {}",
            supplier_block
        );
        assert!(
            !supplier_block.contains("<cbc:CountrySubentity>Cluj</cbc:CountrySubentity>"),
            "seller county must NOT be emitted as raw name 'Cluj'"
        );
    }

    #[test]
    fn county_to_nuts_buyer_county_emitted_as_iso_code() {
        // U5: buyer county "Cluj" (from sample_input) must appear as RO-CJ in XML.
        let input = sample_input(); // buyer.county = Some("Cluj")
        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');
        let customer_block = body
            .split("</cac:AccountingCustomerParty>")
            .next()
            .unwrap_or(body);
        assert!(
            customer_block.contains("<cbc:CountrySubentity>RO-CJ</cbc:CountrySubentity>"),
            "buyer county 'Cluj' must be emitted as 'RO-CJ', got: {}",
            customer_block
        );
    }

    // ── R17 Wave 1: multi-currency / FX ──────────────────────────────────────

    /// R17-W1-EUR: a EUR invoice must emit TaxCurrencyCode=RON, DocumentCurrencyCode=EUR,
    /// and a second TaxTotal with TaxAmount in RON (190.00 EUR * 5.0 = 950.00 RON).
    #[test]
    fn eur_invoice_emits_tax_currency_code_and_ron_tax_total() {
        let mut input = sample_input();
        // Override to a EUR invoice: 1000.00 EUR net, 19% VAT = 190.00 EUR, total 1190.00 EUR
        input.invoice.currency = "EUR".to_string();
        input.invoice.exchange_rate = Some(5.0);
        input.invoice.subtotal_amount = "1000.00".to_string();
        input.invoice.vat_amount = "190.00".to_string();
        input.invoice.total_amount = "1190.00".to_string();
        input.lines[0].unit_price = "1000.00".to_string();
        input.lines[0].subtotal_amount = "1000.00".to_string();
        input.lines[0].vat_amount = "190.00".to_string();
        input.lines[0].total_amount = "1190.00".to_string();

        let xml = generate_ubl(&input).expect("should generate EUR XML");
        let body = xml.trim_start_matches('\u{FEFF}');

        // DocumentCurrencyCode must be EUR
        assert!(
            body.contains("<cbc:DocumentCurrencyCode>EUR</cbc:DocumentCurrencyCode>"),
            "EUR invoice must emit DocumentCurrencyCode=EUR, got: {}",
            body
        );
        // TaxCurrencyCode must be RON and must follow DocumentCurrencyCode
        assert!(
            body.contains("<cbc:TaxCurrencyCode>RON</cbc:TaxCurrencyCode>"),
            "EUR invoice must emit TaxCurrencyCode=RON, got: {}",
            body
        );
        let doc_pos = body
            .find("<cbc:DocumentCurrencyCode>EUR</cbc:DocumentCurrencyCode>")
            .expect("DocumentCurrencyCode not found");
        let tax_pos = body
            .find("<cbc:TaxCurrencyCode>RON</cbc:TaxCurrencyCode>")
            .expect("TaxCurrencyCode not found");
        assert!(
            tax_pos > doc_pos,
            "TaxCurrencyCode must appear after DocumentCurrencyCode"
        );
        // Second TaxTotal must contain RON TaxAmount = 950.00
        assert!(
            body.contains("<cbc:TaxAmount currencyID=\"RON\">950.00</cbc:TaxAmount>"),
            "EUR invoice must emit second TaxTotal with RON TaxAmount=950.00, got: {}",
            body
        );
        // Must have two TaxTotal blocks (first in EUR with subtotals, second in RON only)
        let tax_total_count = body.matches("<cac:TaxTotal>").count();
        assert_eq!(
            tax_total_count, 2,
            "EUR invoice must have exactly 2 TaxTotal elements, got {}: {}",
            tax_total_count, body
        );
    }

    /// R17-W1-RON: a RON invoice must NOT emit TaxCurrencyCode and must have
    /// exactly one TaxTotal (behavior identical to before this wave).
    #[test]
    fn ron_invoice_has_no_tax_currency_code_and_single_tax_total() {
        let input = sample_input(); // currency = "RON"
        let xml = generate_ubl(&input).expect("should generate RON XML");
        let body = xml.trim_start_matches('\u{FEFF}');

        assert!(
            !body.contains("TaxCurrencyCode"),
            "RON invoice must NOT emit TaxCurrencyCode, got: {}",
            body
        );
        let tax_total_count = body.matches("<cac:TaxTotal>").count();
        assert_eq!(
            tax_total_count, 1,
            "RON invoice must have exactly 1 TaxTotal, got {}: {}",
            tax_total_count, body
        );
    }
}
