/**
 * doc-render/invoice-model — parse a CIUS-RO UBL e-Factura XML into a typed `InvoiceDoc` for the
 * labeled invoice document view. The UBL is element/namespace-based (cac:/cbc:), so we walk by
 * LOCAL NAME (namespace-prefix-agnostic) down explicit direct-child paths — robust to prefix changes.
 * Never throws: returns `null` if the root isn't an Invoice / parse fails.
 *
 * Mirrors the data the Rust invoice PDF (`ubl/pdf.rs`) already renders, so the in-app document and
 * the generated PDF are siblings.
 */

export interface Party {
  name: string;
  vatId: string;
  companyId: string;
  street: string;
  city: string;
  county: string;
  country: string;
}

export interface InvoiceLine {
  id: string;
  name: string;
  description: string;
  quantity: string;
  unit: string;
  unitPrice: string;
  vatCode: string;
  vatPercent: string;
  lineAmount: string;
}

export interface VatSubtotal {
  taxable: string;
  vat: string;
  code: string;
  percent: string;
  exemptionCode: string;
}

export interface InvoiceDoc {
  number: string;
  typeCode: string;
  issueDate: string;
  dueDate: string;
  currency: string;
  notes: string[];
  seller: Party;
  buyer: Party;
  vatSubtotals: VatSubtotal[];
  totalVat: string;
  lineExtension: string;
  taxExclusive: string;
  taxInclusive: string;
  payable: string;
  lines: InvoiceLine[];
}

/** First direct-child element reachable by following `path` of local names. */
function find(el: Element | null, ...path: string[]): Element | null {
  let cur: Element | null = el;
  for (const name of path) {
    if (!cur) return null;
    cur = Array.from(cur.children).find((c) => c.localName === name) ?? null;
  }
  return cur;
}

function findText(el: Element | null, ...path: string[]): string {
  return find(el, ...path)?.textContent?.trim() ?? "";
}

/** All direct children of `el` with the given local name. */
function findAll(el: Element | null, name: string): Element[] {
  return el ? Array.from(el.children).filter((c) => c.localName === name) : [];
}

function parseParty(party: Element | null): Party {
  const addr = find(party, "PostalAddress");
  return {
    name:
      findText(party, "PartyLegalEntity", "RegistrationName") ||
      findText(party, "PartyName", "Name"),
    vatId: findText(party, "PartyTaxScheme", "CompanyID"),
    companyId: findText(party, "PartyLegalEntity", "CompanyID"),
    street: findText(addr, "StreetName"),
    city: findText(addr, "CityName"),
    county: findText(addr, "CountrySubentity"),
    country: findText(addr, "Country", "IdentificationCode"),
  };
}

export function parseInvoice(xml: string): InvoiceDoc | null {
  let root: Element | null;
  try {
    const doc = new DOMParser().parseFromString(xml, "application/xml");
    if (doc.querySelector("parsererror")) return null;
    root = doc.documentElement;
  } catch {
    return null;
  }
  if (!root || root.localName !== "Invoice") return null;

  // The document-currency TaxTotal (the one carrying TaxSubtotals; a 2nd RON-only TaxTotal may exist).
  const taxTotal =
    findAll(root, "TaxTotal").find((t) => findAll(t, "TaxSubtotal").length > 0) ??
    find(root, "TaxTotal");
  const monetary = find(root, "LegalMonetaryTotal");

  return {
    number: findText(root, "ID"),
    typeCode: findText(root, "InvoiceTypeCode"),
    issueDate: findText(root, "IssueDate"),
    dueDate: findText(root, "DueDate"),
    currency: findText(root, "DocumentCurrencyCode"),
    notes: findAll(root, "Note").map((n) => n.textContent?.trim() ?? "").filter(Boolean),
    seller: parseParty(find(root, "AccountingSupplierParty", "Party")),
    buyer: parseParty(find(root, "AccountingCustomerParty", "Party")),
    totalVat: findText(taxTotal, "TaxAmount"),
    vatSubtotals: findAll(taxTotal, "TaxSubtotal").map((ts) => {
      const cat = find(ts, "TaxCategory");
      return {
        taxable: findText(ts, "TaxableAmount"),
        vat: findText(ts, "TaxAmount"),
        code: findText(cat, "ID"),
        percent: findText(cat, "Percent"),
        exemptionCode: findText(cat, "TaxExemptionReasonCode"),
      };
    }),
    lineExtension: findText(monetary, "LineExtensionAmount"),
    taxExclusive: findText(monetary, "TaxExclusiveAmount"),
    taxInclusive: findText(monetary, "TaxInclusiveAmount"),
    payable: findText(monetary, "PayableAmount"),
    lines: findAll(root, "InvoiceLine").map((line) => {
      const qtyEl = find(line, "InvoicedQuantity");
      const item = find(line, "Item");
      const cat = find(item, "ClassifiedTaxCategory");
      return {
        id: findText(line, "ID"),
        name: findText(item, "Name"),
        description: findText(item, "Description"),
        quantity: qtyEl?.textContent?.trim() ?? "",
        unit: qtyEl?.getAttribute("unitCode") ?? "",
        unitPrice: findText(line, "Price", "PriceAmount"),
        vatCode: findText(cat, "ID"),
        vatPercent: findText(cat, "Percent"),
        lineAmount: findText(line, "LineExtensionAmount"),
      };
    }),
  };
}
