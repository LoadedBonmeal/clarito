import { describe, expect, it } from "vitest";

import { parseSaftSummary } from "./saft-model";

const SAFT_L = `<?xml version="1.0" encoding="UTF-8"?>
<AuditFile xmlns="mfp:anaf:dgti:d406:declaratie:v1">
  <Header>
    <AuditFileVersion>2.4.9</AuditFileVersion>
    <AuditFileCountry>RO</AuditFileCountry>
    <AuditFileDateCreated>2026-07-01</AuditFileDateCreated>
    <SoftwareID>efactura-desktop</SoftwareID>
    <SoftwareVersion>0.7.0</SoftwareVersion>
    <Company>
      <RegistrationNumber>12345678</RegistrationNumber>
      <Name>DEMO Tehnologii SRL</Name>
      <Address><StreetName>Str. Victoriei 10</StreetName><City>București</City></Address>
      <TaxRegistration><TaxRegistrationNumber>12345678</TaxRegistrationNumber></TaxRegistration>
    </Company>
    <DefaultCurrencyCode>RON</DefaultCurrencyCode>
    <SelectionCriteria>
      <SelectionStartDate>2026-06-01</SelectionStartDate>
      <SelectionEndDate>2026-06-30</SelectionEndDate>
    </SelectionCriteria>
    <HeaderComment>L</HeaderComment>
    <TaxAccountingBasis>A</TaxAccountingBasis>
  </Header>
  <MasterFiles>
    <GeneralLedgerAccounts><Account/><Account/></GeneralLedgerAccounts>
    <Customers><Customer/></Customers>
    <Suppliers/>
    <TaxTable><TaxTableEntry/></TaxTable>
    <Products/>
  </MasterFiles>
  <GeneralLedgerEntries>
    <NumberOfEntries>10</NumberOfEntries>
    <TotalDebit>1500.00</TotalDebit>
    <TotalCredit>1500.00</TotalCredit>
    <Journal/>
  </GeneralLedgerEntries>
  <SourceDocuments>
    <SalesInvoices>
      <NumberOfEntries>3</NumberOfEntries>
      <TotalDebit>5000.00</TotalDebit>
      <TotalCredit>4200.00</TotalCredit>
      <Invoice/>
    </SalesInvoices>
    <MovementOfGoods/>
  </SourceDocuments>
</AuditFile>`;

describe("parseSaftSummary", () => {
  it("extracts the SAF-T cover-page summary from an L-profile AuditFile", () => {
    const s = parseSaftSummary(SAFT_L)!;
    expect(s).not.toBeNull();
    // Header / identity
    expect(s.version).toBe("2.4.9");
    expect(s.country).toBe("RO");
    expect(s.currency).toBe("RON");
    expect(s.companyName).toBe("DEMO Tehnologii SRL");
    expect(s.cui).toBe("12345678");
    expect(s.vatNumber).toBe("12345678");
    expect(s.address).toBe("Str. Victoriei 10, București");
    expect(s.declType).toBe("L");
    expect(s.periodStart).toBe("2026-06-01");
    expect(s.periodEnd).toBe("2026-06-30");
    expect(s.software).toBe("efactura-desktop 0.7.0");
    // GL totals — standalone nodes, balanced
    expect(s.gl.present).toBe(true);
    expect(s.gl.entries).toBe("10");
    expect(s.gl.debit).toBe("1500.00");
    expect(s.gl.credit).toBe("1500.00");
    // MasterFiles counts (direct-child item counts)
    const count = (k: string) => s.counts.find((c) => c.key === k)?.value;
    expect(count("accounts")).toBe(2);
    expect(count("customers")).toBe(1);
    expect(count("suppliers")).toBe(0);
    expect(count("taxCodes")).toBe(1);
    expect(count("products")).toBe(0);
    // SourceDocuments — populated SalesInvoices, empty MovementOfGoods
    const sales = s.sections.find((x) => x.key === "SalesInvoices")!;
    expect(sales.present).toBe(true);
    expect(sales.count).toBe("3");
    expect(sales.metrics).toEqual([
      { label: "Total debit", value: "5000.00" },
      { label: "Total credit", value: "4200.00" },
    ]);
    expect(s.sections.find((x) => x.key === "MovementOfGoods")!.present).toBe(false);
  });

  it("returns null for a non-AuditFile root and never throws", () => {
    expect(parseSaftSummary("<declaratie300/>")).toBeNull();
    expect(parseSaftSummary("not xml <<<")).toBeNull();
    expect(() => parseSaftSummary("")).not.toThrow();
  });
});
