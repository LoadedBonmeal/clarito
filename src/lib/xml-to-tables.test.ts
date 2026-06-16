import { describe, expect, it } from "vitest";

import { formatValue, resolveField } from "./doc-render/labels";
import { xmlToTables } from "./xml-to-tables";

const D205_XML = `<?xml version="1.0" encoding="UTF-8"?>
<declaratie205 xmlns="mfp:anaf:dgti:d205:declaratie:v3" luna="12" an="2026" cui="40">
  <sect_II tip_venit="08" nrben="1" Tbaza="40000"/>
  <benef id_inreg="1" cifR="1960101410019" den1="Popescu Andrei" baza1="40000"/>
  <benef id_inreg="2" cifR="2960101410010" den1="Ana Pop" baza1="10000"/>
</declaratie205>`;

describe("xmlToTables", () => {
  it("labels a D205 declaration like the printed document (titles + headers + formatted values)", () => {
    const tables = xmlToTables(D205_XML, "D205");

    // Root → the descriptor's document title; attributes resolved to human labels + formatted values.
    const header = tables.find((t) => t.title.startsWith("Declarația 205"));
    expect(header).toBeTruthy();
    expect(header!.columns).toEqual(["Câmp", "Valoare"]);
    const luna = resolveField("D205", "luna");
    expect(header!.rows).toContainEqual([luna.label, formatValue("12", luna)]);
    expect(header!.rows).toContainEqual([resolveField("D205", "cui").label, "40"]);

    // benef group → titled "Beneficiari", headers mapped to labels, money values formatted.
    const benef = tables.find((t) => t.title.startsWith("Beneficiari"));
    expect(benef!.title).toBe("Beneficiari (×2)");
    expect(benef!.columns).toEqual([
      resolveField("D205", "id_inreg").label,
      resolveField("D205", "cifR").label,
      resolveField("D205", "den1").label,
      resolveField("D205", "baza1").label,
    ]);
    const baza1 = resolveField("D205", "baza1");
    expect(baza1.format).toBe("money_lei"); // guards the labeling premise
    expect(benef!.rows[0]).toEqual([
      "1",
      "1960101410019",
      "Popescu Andrei",
      formatValue("40000", baza1), // money_lei → "40.000 lei", NOT the raw "40000"
    ]);
    expect(benef!.rows[0][3]).not.toBe("40000");
  });

  it("falls back to raw tag/name for a document without a descriptor", () => {
    const tables = xmlToTables(`<oarecare camp_x="1"/>`); // no docKey, unknown root → no dictionary
    const t = tables.find((tbl) => tbl.title === "oarecare");
    expect(t).toBeTruthy();
    expect(t!.rows).toEqual([["camp_x", "1"]]); // raw name + raw value
  });

  it("labels e-Transport via the ETRANSPORT dictionary (attribute → RO label)", () => {
    const xml = `<eTransport xmlns="mfp:anaf:dgti:eTransport:declaratie:v2" codDeclarant="12345678">
      <notificare codTipOperatiune="30">
        <bunuriTransportate codScopOperatiune="101" denumireMarfa="Roșii" cantitate="1000" codUnitateMasura="KGM" greutateBruta="1050"/>
      </notificare>
    </eTransport>`;
    const tables = xmlToTables(xml, "ETRANSPORT");
    const goods = tables.find((t) => t.title === "Bunuri transportate");
    expect(goods).toBeTruthy();
    expect(goods!.columns).toContain(resolveField("ETRANSPORT", "denumireMarfa").label); // "Denumire marfă"
    expect(goods!.columns).not.toContain("denumireMarfa"); // not the raw attr name
  });

  it("builds a labeled spreadsheet from a CIUS-RO invoice (header + lines + VAT + totals)", () => {
    const xml = `<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2" xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2" xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2">
      <cbc:ID>FCT-0001</cbc:ID>
      <cbc:IssueDate>2026-06-10</cbc:IssueDate>
      <cbc:InvoiceTypeCode>380</cbc:InvoiceTypeCode>
      <cbc:DocumentCurrencyCode>RON</cbc:DocumentCurrencyCode>
      <cac:AccountingSupplierParty><cac:Party><cac:PartyLegalEntity><cbc:RegistrationName>VANZATOR SRL</cbc:RegistrationName></cac:PartyLegalEntity></cac:Party></cac:AccountingSupplierParty>
      <cac:AccountingCustomerParty><cac:Party><cac:PartyLegalEntity><cbc:RegistrationName>CUMPARATOR SRL</cbc:RegistrationName></cac:PartyLegalEntity></cac:Party></cac:AccountingCustomerParty>
      <cac:LegalMonetaryTotal><cbc:TaxExclusiveAmount>1000.00</cbc:TaxExclusiveAmount><cbc:TaxInclusiveAmount>1210.00</cbc:TaxInclusiveAmount><cbc:PayableAmount>1210.00</cbc:PayableAmount></cac:LegalMonetaryTotal>
      <cac:InvoiceLine><cbc:ID>1</cbc:ID><cbc:InvoicedQuantity unitCode="C62">2</cbc:InvoicedQuantity><cbc:LineExtensionAmount>1000.00</cbc:LineExtensionAmount><cac:Item><cbc:Name>Consultanță</cbc:Name><cac:ClassifiedTaxCategory><cbc:ID>S</cbc:ID><cbc:Percent>21</cbc:Percent></cac:ClassifiedTaxCategory></cac:Item><cac:Price><cbc:PriceAmount>500.00</cbc:PriceAmount></cac:Price></cac:InvoiceLine>
    </Invoice>`;
    const tables = xmlToTables(xml, "INVOICE");
    // Human sections, not raw UBL element names.
    expect(tables.map((t) => t.title)).toEqual([
      "Factură",
      "Vânzător",
      "Cumpărător",
      "Linii factură",
      "Detalierea TVA",
      "Totaluri",
    ]);
    const header = tables[0];
    expect(header.rows).toContainEqual(["Nr. factură", "FCT-0001"]);
    expect(tables[1].rows).toContainEqual(["Denumire", "VANZATOR SRL"]);
    const lines = tables.find((t) => t.title === "Linii factură")!;
    expect(lines.columns[1]).toBe("Denumire");
    expect(lines.rows[0][1]).toBe("Consultanță");
    expect(lines.rows[0][5]).toBe("21%"); // vat category label, not "S"/raw
  });

  it("returns [] for invalid XML and never throws", () => {
    expect(xmlToTables("not xml <<<")).toEqual([]);
    expect(() => xmlToTables("")).not.toThrow();
  });
});
