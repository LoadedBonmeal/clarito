import { describe, expect, it } from "vitest";

import { parseInvoice } from "./invoice-model";

const UBL = `<?xml version="1.0" encoding="UTF-8"?>
<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
         xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
         xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:ID>FAC 1</cbc:ID>
  <cbc:IssueDate>2022-06-14</cbc:IssueDate>
  <cbc:DueDate>2022-06-28</cbc:DueDate>
  <cbc:InvoiceTypeCode>380</cbc:InvoiceTypeCode>
  <cbc:Note>Test note</cbc:Note>
  <cbc:DocumentCurrencyCode>RON</cbc:DocumentCurrencyCode>
  <cac:AccountingSupplierParty><cac:Party>
    <cac:PartyName><cbc:Name>FACTURIS ONLINE SRL</cbc:Name></cac:PartyName>
    <cac:PostalAddress>
      <cbc:StreetName>B-DUL IULIU MANIU NR.6E</cbc:StreetName>
      <cbc:CityName>SECTOR 6</cbc:CityName>
      <cbc:CountrySubentity>RO-B</cbc:CountrySubentity>
      <cac:Country><cbc:IdentificationCode>RO</cbc:IdentificationCode></cac:Country>
    </cac:PostalAddress>
    <cac:PartyTaxScheme><cbc:CompanyID>RO34283300</cbc:CompanyID><cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme></cac:PartyTaxScheme>
    <cac:PartyLegalEntity><cbc:RegistrationName>FACTURIS ONLINE SRL</cbc:RegistrationName><cbc:CompanyID>34283300</cbc:CompanyID></cac:PartyLegalEntity>
  </cac:Party></cac:AccountingSupplierParty>
  <cac:AccountingCustomerParty><cac:Party>
    <cac:PartyName><cbc:Name>MIDSOFT IT GROUP SRL</cbc:Name></cac:PartyName>
    <cac:PartyLegalEntity><cbc:RegistrationName>MIDSOFT IT GROUP SRL</cbc:RegistrationName><cbc:CompanyID>19211548</cbc:CompanyID></cac:PartyLegalEntity>
  </cac:Party></cac:AccountingCustomerParty>
  <cac:TaxTotal>
    <cbc:TaxAmount currencyID="RON">0.00</cbc:TaxAmount>
    <cac:TaxSubtotal>
      <cbc:TaxableAmount currencyID="RON">5000.00</cbc:TaxableAmount>
      <cbc:TaxAmount currencyID="RON">0.00</cbc:TaxAmount>
      <cac:TaxCategory><cbc:ID>O</cbc:ID><cbc:TaxExemptionReasonCode>VATEX-EU-O</cbc:TaxExemptionReasonCode><cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme></cac:TaxCategory>
    </cac:TaxSubtotal>
  </cac:TaxTotal>
  <cac:LegalMonetaryTotal>
    <cbc:LineExtensionAmount currencyID="RON">5000.00</cbc:LineExtensionAmount>
    <cbc:TaxExclusiveAmount currencyID="RON">5000.00</cbc:TaxExclusiveAmount>
    <cbc:TaxInclusiveAmount currencyID="RON">5000.00</cbc:TaxInclusiveAmount>
    <cbc:PayableAmount currencyID="RON">5000.00</cbc:PayableAmount>
  </cac:LegalMonetaryTotal>
  <cac:InvoiceLine>
    <cbc:ID>1</cbc:ID>
    <cbc:InvoicedQuantity unitCode="H87">5</cbc:InvoicedQuantity>
    <cbc:LineExtensionAmount currencyID="RON">5000.00</cbc:LineExtensionAmount>
    <cac:Item><cbc:Name>Test e-factura</cbc:Name><cac:ClassifiedTaxCategory><cbc:ID>O</cbc:ID><cbc:Percent>0</cbc:Percent><cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme></cac:ClassifiedTaxCategory></cac:Item>
    <cac:Price><cbc:PriceAmount currencyID="RON">1000.00</cbc:PriceAmount></cac:Price>
  </cac:InvoiceLine>
</Invoice>`;

describe("parseInvoice", () => {
  it("extracts header, parties, totals and lines from CIUS-RO UBL", () => {
    const d = parseInvoice(UBL);
    expect(d).not.toBeNull();
    expect(d!.number).toBe("FAC 1");
    expect(d!.issueDate).toBe("2022-06-14");
    expect(d!.currency).toBe("RON");
    expect(d!.typeCode).toBe("380");
    expect(d!.seller.name).toBe("FACTURIS ONLINE SRL");
    expect(d!.seller.vatId).toBe("RO34283300");
    expect(d!.seller.city).toBe("SECTOR 6");
    expect(d!.seller.country).toBe("RO");
    expect(d!.buyer.name).toBe("MIDSOFT IT GROUP SRL");
    expect(d!.buyer.companyId).toBe("19211548");
    expect(d!.payable).toBe("5000.00");
    expect(d!.totalVat).toBe("0.00");
    expect(d!.lines).toHaveLength(1);
    expect(d!.lines[0]).toMatchObject({
      name: "Test e-factura",
      quantity: "5",
      unit: "H87",
      unitPrice: "1000.00",
      vatCode: "O",
      lineAmount: "5000.00",
    });
    expect(d!.vatSubtotals[0]).toMatchObject({ taxable: "5000.00", code: "O", exemptionCode: "VATEX-EU-O" });
    expect(d!.notes).toEqual(["Test note"]);
  });

  it("returns null for non-invoice or unparseable XML", () => {
    expect(parseInvoice('<declaratie205 xmlns="x"/>')).toBeNull();
    expect(parseInvoice("")).toBeNull();
  });
});
