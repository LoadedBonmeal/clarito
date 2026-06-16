/**
 * doc-render/saft-model — parse a Ro_SAF-T (D406) `AuditFile` XML into a compact SUMMARY model for a
 * pre-submission cover page (never the full transaction dump). SAF-T files can be very large, so we
 * read ONLY the standalone summary nodes the generator already emits (Header, MasterFiles counts,
 * GeneralLedgerEntries totals, SourceDocuments per-section totals) — we never iterate transaction
 * lines / journals / invoices.
 *
 * Structure is element/namespace-qualified (default xmlns on the root, bare descendant tags, all data
 * in element TEXT), so we walk by LOCAL NAME down direct-child paths — prefix-agnostic. Two profiles:
 * `HeaderComment` = "L" (lunar/periodic: GL + sales/purchases/payments) or "A" (anual D406A: assets).
 * Never throws: returns `null` if the root isn't an `AuditFile` / parse fails.
 */

export interface SaftSection {
  key: string;
  label: string;
  /** The section carries its own summary totals (a populated section, not an empty wrapper). */
  present: boolean;
  count: string;
  /** Section metrics in display order (e.g. Total debit / Total credit, or quantities). */
  metrics: { label: string; value: string }[];
}

export interface SaftSummary {
  version: string;
  country: string;
  dateCreated: string;
  software: string;
  companyName: string;
  cui: string;
  address: string;
  vatNumber: string;
  currency: string;
  basis: string;
  periodStart: string;
  periodEnd: string;
  declType: "L" | "A" | "";
  gl: { present: boolean; entries: string; debit: string; credit: string };
  counts: { key: string; label: string; value: number }[];
  sections: SaftSection[];
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

/** Count direct children of `el` with the given local name (empty/missing container → 0). */
function countChildren(el: Element | null, name: string): number {
  return el ? Array.from(el.children).filter((c) => c.localName === name).length : 0;
}

export function parseSaftSummary(xml: string): SaftSummary | null {
  let root: Element | null;
  try {
    const doc = new DOMParser().parseFromString(xml, "application/xml");
    if (doc.querySelector("parsererror")) return null;
    root = doc.documentElement;
  } catch {
    return null;
  }
  if (!root || root.localName !== "AuditFile") return null;

  const header = find(root, "Header");
  const company = find(header, "Company");
  const addr = find(company, "Address");
  const sel = find(header, "SelectionCriteria");

  const address = [
    findText(addr, "StreetName"),
    findText(addr, "City"),
    findText(addr, "PostalCode"),
    findText(addr, "Region"),
  ]
    .filter(Boolean)
    .join(", ");

  const declRaw = findText(header, "HeaderComment");
  const declType: "L" | "A" | "" = declRaw === "L" || declRaw === "A" ? declRaw : "";

  const sw = [findText(header, "SoftwareID"), findText(header, "SoftwareVersion")]
    .filter(Boolean)
    .join(" ");

  const master = find(root, "MasterFiles");
  const counts = [
    { key: "accounts", label: "Conturi", value: countChildren(find(master, "GeneralLedgerAccounts"), "Account") },
    { key: "customers", label: "Clienți", value: countChildren(find(master, "Customers"), "Customer") },
    { key: "suppliers", label: "Furnizori", value: countChildren(find(master, "Suppliers"), "Supplier") },
    { key: "taxCodes", label: "Coduri TVA", value: countChildren(find(master, "TaxTable"), "TaxTableEntry") },
    { key: "uom", label: "Unități de măsură", value: countChildren(find(master, "UOMTable"), "UOMTableEntry") },
    { key: "products", label: "Produse", value: countChildren(find(master, "Products"), "Product") },
    { key: "assets", label: "Mijloace fixe", value: countChildren(find(master, "Assets"), "Asset") },
  ];

  const gle = find(root, "GeneralLedgerEntries");
  const glEntries = findText(gle, "NumberOfEntries");
  const gl = {
    present: glEntries !== "",
    entries: glEntries,
    debit: findText(gle, "TotalDebit"),
    credit: findText(gle, "TotalCredit"),
  };

  const src = find(root, "SourceDocuments");
  const docSection = (tag: string, label: string): SaftSection => {
    const el = find(src, tag);
    const count = findText(el, "NumberOfEntries");
    return {
      key: tag,
      label,
      present: count !== "",
      count,
      metrics: [
        { label: "Total debit", value: findText(el, "TotalDebit") },
        { label: "Total credit", value: findText(el, "TotalCredit") },
      ],
    };
  };
  const movement = (): SaftSection => {
    const el = find(src, "MovementOfGoods");
    const count = findText(el, "NumberOfMovementLines");
    return {
      key: "MovementOfGoods",
      label: "Mișcări de stoc",
      present: count !== "",
      count,
      metrics: [
        { label: "Cant. intrată", value: findText(el, "TotalQuantityReceived") },
        { label: "Cant. ieșită", value: findText(el, "TotalQuantityIssued") },
      ],
    };
  };
  const assetTx = (): SaftSection => {
    const el = find(src, "AssetTransactions");
    const count = findText(el, "NumberOfAssetTransactions");
    return { key: "AssetTransactions", label: "Tranzacții mijloace fixe", present: count !== "", count, metrics: [] };
  };

  const sections = [
    docSection("SalesInvoices", "Facturi vânzări"),
    docSection("PurchaseInvoices", "Facturi cumpărări"),
    docSection("Payments", "Plăți / încasări"),
    movement(),
    assetTx(),
  ];

  return {
    version: findText(header, "AuditFileVersion"),
    country: findText(header, "AuditFileCountry"),
    dateCreated: findText(header, "AuditFileDateCreated"),
    software: sw,
    companyName: findText(company, "Name"),
    cui: findText(company, "RegistrationNumber"),
    address,
    vatNumber: findText(company, "TaxRegistration", "TaxRegistrationNumber"),
    currency: findText(header, "DefaultCurrencyCode"),
    basis: findText(header, "TaxAccountingBasis"),
    periodStart: findText(sel, "SelectionStartDate"),
    periodEnd: findText(sel, "SelectionEndDate"),
    declType,
    gl,
    counts,
    sections,
  };
}
