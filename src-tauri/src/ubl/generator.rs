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

use crate::anaf_decl::saft::masterfiles::uom_to_rec20;
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

    // BR-O-02/11..14: an invoice with a category "O" line ("în afara sferei TVA") must NOT
    // carry a Seller/Buyer VAT identifier (PartyTaxScheme[TaxScheme=VAT]) and must not mix O
    // with any other VAT category. Compute once and thread to both party writers.
    let has_o_line = input.lines.iter().any(|l| l.vat_category == "O");

    // ── AccountingSupplierParty ──────────────────────────────────────────────
    write_supplier_party(&mut writer, seller, has_o_line)?;

    // ── AccountingCustomerParty ──────────────────────────────────────────────
    write_customer_party(&mut writer, buyer, currency, has_o_line)?;

    // ── Delivery (EN16931 BR-IC-11/BR-IC-12) ─────────────────────────────────
    // An invoice with any category "K" line (intra-EU supply) must contain the actual
    // delivery date (BT-72) or invoicing period (BG-14), AND the Deliver-to country
    // (BT-80). We emit ActualDeliveryDate = issue date + DeliveryLocation/Country from
    // the buyer's country. UBL element order: cac:Delivery MUST come after
    // AccountingCustomerParty (no PayeeParty/TaxRepresentativeParty in this generator)
    // and before cac:PaymentMeans.
    let has_k_line = input.lines.iter().any(|l| l.vat_category == "K");
    if has_k_line {
        writer
            .write_event(Event::Start(BytesStart::new("cac:Delivery")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        write_text(&mut writer, "cbc:ActualDeliveryDate", &inv.issue_date)?;
        writer
            .write_event(Event::Start(BytesStart::new("cac:DeliveryLocation")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        writer
            .write_event(Event::Start(BytesStart::new("cac:Address")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        writer
            .write_event(Event::Start(BytesStart::new("cac:Country")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        write_text(&mut writer, "cbc:IdentificationCode", &buyer.country)?;
        writer
            .write_event(Event::End(BytesEnd::new("cac:Country")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        writer
            .write_event(Event::End(BytesEnd::new("cac:Address")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        writer
            .write_event(Event::End(BytesEnd::new("cac:DeliveryLocation")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        writer
            .write_event(Event::End(BytesEnd::new("cac:Delivery")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
    }

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

fn write_supplier_party(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    seller: &Company,
    has_o_line: bool,
) -> AppResult<()> {
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
    let subentity = county_to_nuts(&seller.county);
    write_text(
        writer,
        "cbc:CityName",
        &bucharest_city_name(&seller.county, &seller.city, &subentity),
    )?;
    write_text(writer, "cbc:CountrySubentity", &subentity)?;
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

    // PartyTaxScheme — BR-CO-09: only emitted for VAT-registered sellers (CompanyID must carry
    // the ISO country prefix under TaxScheme=VAT). BR-O-02: an invoice with any category "O"
    // line must not carry the seller VAT identifier at all, regardless of vat_payer.
    if seller.vat_payer && !has_o_line {
        writer
            .write_event(Event::Start(BytesStart::new("cac:PartyTaxScheme")))
            .map_err(|e| AppError::Xml(e.to_string()))?;
        let seller_cui_digits = seller
            .cui
            .trim()
            .trim_start_matches("RO")
            .trim_start_matches("ro");
        let supplier_cui_xml = format!("RO{}", seller_cui_digits);
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
    }

    // PartyLegalEntity — BR-CO-26: the seller must carry an identifier (CompanyID) here
    // regardless of VAT-registration status, so this stays unconditional.
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
    has_o_line: bool,
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
    let buyer_subentity = buyer.county.as_deref().map(county_to_nuts);
    if let Some(city) = &buyer.city {
        let city_out = match &buyer_subentity {
            Some(se) => bucharest_city_name(buyer.county.as_deref().unwrap_or(""), city, se),
            None => city.clone(),
        };
        write_text(writer, "cbc:CityName", &city_out)?;
    }
    if let Some(se) = &buyer_subentity {
        write_text(writer, "cbc:CountrySubentity", se)?;
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

    // PartyTaxScheme — BR-CO-09/BR-O-02: only emitted when the buyer is VAT-registered
    // AND the invoice has no category "O" line. Previously this synthesized a "RO"+CUI
    // VAT identifier for ANY Romanian buyer with a CUI on file, which falsely asserted
    // VAT registration for non-VAT-payer buyers (a core e-Factura user class).
    if buyer.vat_payer && !has_o_line {
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
        // BR-O-08 family: category "O" ("în afara sferei TVA") must NOT carry cbc:Percent.
        // Every other category (incl. Z/E/AE/K/G at 0.00) still emits it.
        if category != "O" {
            write_text(writer, "cbc:Percent", rate_str)?;
        }
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
    // CIUS-RO / EN16931 BR-CL-23: unitCode MUST be a UN/ECE Rec 20 code (piece = "H87"), which ANAF
    // enforces — a raw Romanian abbreviation like "buc" is rejected. Normalize the human unit to its
    // Rec 20 code at emit time via the shared mapper (the friendly unit stays in the DB/UI; only the
    // XML is coded). uom_to_rec20 never fails (unknown → H87), so the emitted code is always valid.
    let unit_code = uom_to_rec20(&line.unit);
    let mut qty_elem = BytesStart::new("cbc:InvoicedQuantity");
    qty_elem.push_attribute(("unitCode", unit_code));
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
    // BR-O-05: an "O" line's ClassifiedTaxCategory must NOT contain cbc:Percent.
    if line.vat_category != "O" {
        write_text(writer, "cbc:Percent", &format_decimal_2(&line.vat_rate))?;
    }
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

/// Strip XML-1.0-forbidden control characters — the C0 controls except tab/LF/CR,
/// plus the two non-characters U+FFFE/U+FFFF. quick-xml escapes `&<>` but passes
/// control bytes through verbatim, so a stray control char in user-entered data
/// (contact name, address, item description, notes — including pasted or imported
/// text) would otherwise produce an invalid UBL document that ANAF/SPV rejects.
/// Returns the input borrowed untouched when it is already clean.
fn xml_text_sanitize(s: &str) -> std::borrow::Cow<'_, str> {
    fn forbidden(c: char) -> bool {
        matches!(c as u32, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0xFFFE | 0xFFFF)
    }
    if s.chars().any(forbidden) {
        std::borrow::Cow::Owned(s.chars().filter(|c| !forbidden(*c)).collect())
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

fn write_text(writer: &mut Writer<Cursor<Vec<u8>>>, tag: &str, value: &str) -> AppResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new(tag)))
        .map_err(|e| AppError::Xml(e.to_string()))?;
    let value = xml_text_sanitize(value);
    writer
        .write_event(Event::Text(BytesText::new(&value)))
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
    // Commercial rounding (half away from zero) — consistent with how the amounts were computed.
    let d = Decimal::from_str(s.trim())
        .unwrap_or(Decimal::ZERO)
        .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
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
        .round_dp_with_strategy(n, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
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

/// Extracts a sector digit (1-6) from a free-text county or city string, e.g.
/// "Sector 3", "SECTOR3", "sector  1". Returns `None` when no sector digit is found.
pub(crate) fn extract_sector_digit(s: &str) -> Option<u8> {
    let lower = s.to_lowercase();
    let idx = lower.find("sector")?;
    let rest = &lower[idx + "sector".len()..];
    let digit = rest.chars().find(|c| !c.is_whitespace() && *c != '.')?;
    if digit.is_ascii_digit() {
        let n = digit.to_digit(10).unwrap_or(0) as u8;
        if (1..=6).contains(&n) {
            return Some(n);
        }
    }
    None
}

/// CIUS-RO BR-RO-100/101 (OMF 1366/2021): when the resolved `CountrySubentity` is the
/// Bucharest code "RO-B", `cbc:CityName` MUST be one of "SECTOR1".."SECTOR6" — not the
/// free-text city name. Looks for a sector digit in both the county and city fields
/// (whichever carries it). When no sector digit can be found, the raw city is returned
/// unchanged (the rocius preflight rule surfaces a clear error to the user in that case).
pub(crate) fn bucharest_city_name(county: &str, city: &str, subentity: &str) -> String {
    if subentity != "RO-B" {
        return city.to_string();
    }
    extract_sector_digit(county)
        .or_else(|| extract_sector_digit(city))
        .map(|n| format!("SECTOR{n}"))
        .unwrap_or_else(|| city.to_string())
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
            tax_regime: "micro".into(),
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
            iban: None,
            bank_name: None,
            swift: None,
            payment_term_days: None,
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
            revenue_kind: "goods".into(),
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
    fn unit_code_normalized_to_rec20_br_cl_23() {
        // EN16931/CIUS-RO BR-CL-23: unitCode MUST be a UN/ECE Rec 20 code. A human abbreviation like
        // "buc" emitted raw is rejected by ANAF. The generator must map "buc" → "H87" (the friendly
        // unit stays in the DB/UI; only the XML is coded). Regression guard for the publication blocker.
        let mut input = sample_input();
        input.lines[0].unit = "buc".to_string();
        let xml = generate_ubl(&input).expect("should generate XML");
        assert!(
            xml.contains(r#"unitCode="H87""#),
            "line unit \"buc\" must be emitted as Rec 20 code H87"
        );
        assert!(
            !xml.contains(r#"unitCode="buc""#),
            "raw \"buc\" must NOT appear as a unitCode (BR-CL-23 → ANAF rejection)"
        );
        for (human, code) in [("ora", "HUR"), ("kg", "KGM"), ("l", "LTR"), ("luna", "MON")] {
            let mut inp = sample_input();
            inp.lines[0].unit = human.to_string();
            let x = generate_ubl(&inp).expect("gen");
            assert!(
                x.contains(&format!("unitCode=\"{code}\"")),
                "unit \"{human}\" must map to Rec 20 code {code}"
            );
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
        // BR-CO-09: a non-VAT-registered seller must NOT emit a PartyTaxScheme[VAT] at
        // all (bare-digit CompanyID under TaxScheme=VAT is itself a fatal schematron
        // violation — the whole block must be omitted, not just left un-prefixed).
        let mut input = sample_input();
        input.seller.vat_payer = false;
        input.seller.cui = "12345678".to_string();
        let xml = generate_ubl(&input).expect("should generate XML");

        // Strip BOM for substring search clarity.
        let body = xml.trim_start_matches('\u{FEFF}');
        let supplier_block = body
            .split("</cac:AccountingSupplierParty>")
            .next()
            .unwrap_or(body);
        // No PartyTaxScheme block at all in the supplier party for a non-VAT-payer.
        assert!(
            !supplier_block.contains("cac:PartyTaxScheme"),
            "non-VAT-payer seller must NOT emit any PartyTaxScheme, got: {}",
            supplier_block
        );
        assert!(
            !supplier_block.contains("RO12345678"),
            "supplier party should not contain RO-prefixed CUI for non-VAT-payer"
        );
        // BT-30/BR-CO-26: PartyLegalEntity must still carry the bare CIF.
        assert!(
            supplier_block.contains("<cbc:CompanyID>12345678</cbc:CompanyID>"),
            "expected bare CUI in supplier PartyLegalEntity, got: {}",
            supplier_block
        );
    }

    #[test]
    fn non_vat_seller_ro_buyer_o_line_omits_vat_schemes_and_percent() {
        // Regression for the fatal BR-CO-09 / BR-O-02 / BR-O-05 combination: a
        // non-VAT-payer seller issuing an out-of-scope ("O") line to an RO buyer must
        // produce an XML with NO PartyTaxScheme[VAT] on either party, NO cbc:Percent
        // inside the O line's ClassifiedTaxCategory/TaxSubtotal, but must still carry
        // VATEX-EU-O and both parties' PartyLegalEntity CompanyID.
        let mut input = sample_input();
        input.seller.vat_payer = false;
        input.seller.cui = "12345678".to_string();
        input.buyer.vat_payer = false;
        input.buyer.cui = Some("87654321".to_string());
        input.buyer.country = "RO".to_string();
        input.lines[0].vat_category = "O".to_string();
        input.lines[0].vat_rate = "0.00".to_string();
        input.lines[0].vat_amount = "0.00".to_string();
        input.invoice.vat_amount = "0.00".to_string();
        input.invoice.total_amount = input.invoice.subtotal_amount.clone();

        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');

        // No TaxScheme[ID=VAT] anywhere inside either party's PartyTaxScheme block —
        // simplest robust check: no "cac:PartyTaxScheme" element at all in the party
        // sections (TaxScheme "VAT" only ever appears there or in TaxCategory, which is
        // a different, always-required element and not what BR-O-02 forbids).
        let supplier_block = body
            .split("</cac:AccountingSupplierParty>")
            .next()
            .unwrap_or(body);
        let customer_block = body
            .split("</cac:AccountingCustomerParty>")
            .nth(0)
            .unwrap_or(body);
        assert!(
            !supplier_block.contains("cac:PartyTaxScheme"),
            "seller must have no PartyTaxScheme when non-VAT-payer, got: {}",
            supplier_block
        );
        assert!(
            !customer_block.contains("cac:PartyTaxScheme"),
            "buyer must have no PartyTaxScheme for a non-VAT-payer buyer, got: {}",
            customer_block
        );

        // O line's ClassifiedTaxCategory must not contain cbc:Percent.
        let item_block = body
            .split("<cac:ClassifiedTaxCategory>")
            .nth(1)
            .and_then(|s| s.split("</cac:ClassifiedTaxCategory>").next())
            .unwrap_or("");
        assert!(
            !item_block.contains("cbc:Percent"),
            "O line ClassifiedTaxCategory must not contain cbc:Percent, got: {}",
            item_block
        );

        // O TaxSubtotal/TaxCategory must not contain cbc:Percent either.
        let subtotal_block = body
            .split("<cac:TaxSubtotal>")
            .nth(1)
            .and_then(|s| s.split("</cac:TaxSubtotal>").next())
            .unwrap_or("");
        assert!(
            !subtotal_block.contains("cbc:Percent"),
            "O TaxSubtotal/TaxCategory must not contain cbc:Percent, got: {}",
            subtotal_block
        );

        // VATEX-EU-O must still be present.
        assert!(
            body.contains("VATEX-EU-O"),
            "expected VATEX-EU-O exemption code, got: {}",
            body
        );

        // PartyLegalEntity CompanyID present for both parties (bare CIF).
        assert!(
            supplier_block.contains("<cbc:CompanyID>12345678</cbc:CompanyID>"),
            "seller PartyLegalEntity must carry bare CIF"
        );
        assert!(
            customer_block.contains("<cbc:CompanyID>87654321</cbc:CompanyID>"),
            "buyer PartyLegalEntity must carry bare CIF"
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

    // ── FIX 2: Bucharest sector CityName (CIUS-RO BR-RO-100/101) ─────────────

    #[test]
    fn extract_sector_digit_finds_digit_in_various_forms() {
        assert_eq!(extract_sector_digit("Sector 3"), Some(3));
        assert_eq!(extract_sector_digit("SECTOR3"), Some(3));
        assert_eq!(extract_sector_digit("sector  1"), Some(1));
        assert_eq!(extract_sector_digit("Sector 6"), Some(6));
        assert_eq!(extract_sector_digit("Sector 7"), None);
        assert_eq!(extract_sector_digit("Cluj"), None);
    }

    #[test]
    fn bucharest_city_name_maps_sector_when_ro_b() {
        assert_eq!(
            bucharest_city_name("Sector 3", "București", "RO-B"),
            "SECTOR3"
        );
        // Sector digit found in city instead of county.
        assert_eq!(bucharest_city_name("", "Sector 2", "RO-B"), "SECTOR2");
        // No sector digit extractable — city left unchanged.
        assert_eq!(
            bucharest_city_name("București", "București", "RO-B"),
            "București"
        );
        // Not Bucharest — city untouched.
        assert_eq!(
            bucharest_city_name("Cluj", "Cluj-Napoca", "RO-CJ"),
            "Cluj-Napoca"
        );
    }

    #[test]
    fn seller_bucharest_sector_emitted_as_sector_city_name() {
        // Seller county "Sector 1" (from sample_input) resolves to RO-B; CityName
        // must be normalized to "SECTOR1", not the free-text "București".
        let input = sample_input(); // seller.county = "Sector 1", seller.city = "București"
        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');
        let supplier_block = body
            .split("</cac:AccountingSupplierParty>")
            .next()
            .unwrap_or(body);
        assert!(
            supplier_block.contains("<cbc:CityName>SECTOR1</cbc:CityName>"),
            "seller Bucharest CityName must be 'SECTOR1', got: {}",
            supplier_block
        );
    }

    #[test]
    fn buyer_bucharest_sector_emitted_as_sector_city_name() {
        let mut input = sample_input();
        input.buyer.county = Some("Sector 4".to_string());
        input.buyer.city = Some("București".to_string());
        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');
        let customer_block = body
            .split("</cac:AccountingCustomerParty>")
            .next()
            .unwrap_or(body);
        assert!(
            customer_block.contains("<cbc:CityName>SECTOR4</cbc:CityName>"),
            "buyer Bucharest CityName must be 'SECTOR4', got: {}",
            customer_block
        );
    }

    #[test]
    fn non_bucharest_city_name_unchanged() {
        // Cluj buyer (from sample_input) must keep its free-text city name as-is.
        let input = sample_input(); // buyer.city = Some("Cluj-Napoca")
        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');
        let customer_block = body
            .split("</cac:AccountingCustomerParty>")
            .next()
            .unwrap_or(body);
        assert!(
            customer_block.contains("<cbc:CityName>Cluj-Napoca</cbc:CityName>"),
            "non-Bucharest buyer city must be unchanged, got: {}",
            customer_block
        );
    }

    // ── FIX 3: intra-EU K delivery info (EN16931 BR-IC-11/BR-IC-12) ───────────

    #[test]
    fn k_line_invoice_emits_delivery_with_date_and_buyer_country() {
        let mut input = sample_input();
        input.buyer.country = "DE".to_string();
        input.lines[0].vat_category = "K".to_string();
        input.lines[0].vat_rate = "0.00".to_string();
        input.lines[0].vat_amount = "0.00".to_string();
        input.invoice.vat_amount = "0.00".to_string();
        input.invoice.total_amount = input.invoice.subtotal_amount.clone();

        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');

        assert!(
            body.contains("<cac:Delivery>"),
            "K-line invoice must contain cac:Delivery, got: {}",
            body
        );
        let delivery_block = body
            .split("<cac:Delivery>")
            .nth(1)
            .and_then(|s| s.split("</cac:Delivery>").next())
            .unwrap_or("");
        assert!(
            delivery_block.contains(&format!(
                "<cbc:ActualDeliveryDate>{}</cbc:ActualDeliveryDate>",
                input.invoice.issue_date
            )),
            "Delivery must contain the ActualDeliveryDate, got: {}",
            delivery_block
        );
        assert!(
            delivery_block.contains("<cbc:IdentificationCode>DE</cbc:IdentificationCode>"),
            "Delivery must contain the buyer country code, got: {}",
            delivery_block
        );

        // Element order: cac:Delivery after AccountingCustomerParty, before PaymentMeans.
        let acp_end = body
            .find("</cac:AccountingCustomerParty>")
            .expect("acp end");
        let delivery_start = body.find("<cac:Delivery>").expect("delivery start");
        let payment_start = body.find("<cac:PaymentMeans>").expect("payment start");
        assert!(
            acp_end < delivery_start && delivery_start < payment_start,
            "cac:Delivery must sit between AccountingCustomerParty and PaymentMeans"
        );
    }

    #[test]
    fn non_k_line_invoice_has_no_delivery() {
        let input = sample_input(); // category "S" only
        let xml = generate_ubl(&input).expect("should generate XML");
        let body = xml.trim_start_matches('\u{FEFF}');
        assert!(
            !body.contains("cac:Delivery"),
            "non-K-line invoice must NOT contain cac:Delivery, got: {}",
            body
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

    #[test]
    fn xml_text_sanitize_strips_forbidden_controls() {
        // Forbidden C0 controls removed; tab/LF/CR preserved; clean text borrowed untouched.
        assert_eq!(xml_text_sanitize("clean").as_ref(), "clean");
        assert_eq!(xml_text_sanitize("A\u{0B}B\u{0}C\tD").as_ref(), "ABC\tD");
        assert_eq!(xml_text_sanitize("a\tb\nc\rd").as_ref(), "a\tb\nc\rd");
    }
}
