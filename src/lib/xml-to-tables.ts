/**
 * xmlToTables — flatten declaration XML into LABELED tables for an XLSX export that matches the
 * printable document (XmlDocView).
 *
 * Headers and coded values are resolved through the same doc-render dictionary (descriptors + labels)
 * the PDF uses: attribute names → Romanian labels, coded values → enum text + money/date formatting,
 * table titles → the descriptor's section titles. A document without a descriptor/dictionary falls
 * back gracefully to global labels then the raw attribute name (resolveField/formatValue never throw),
 * so the export is never worse than before and improves wherever a dictionary exists.
 *
 * Parses the XML (DOMParser → attributes + repeating leaf elements) into `{ title, columns, rows }[]`
 * that the Rust `export_declaration_xlsx` command writes to a real spreadsheet. Namespace declarations
 * are dropped. Never throws — returns `[]` on a parse error.
 */
import { pickDescriptor, type DocDescriptor } from "@/lib/doc-render/descriptors";
import { parseInvoice, type InvoiceDoc } from "@/lib/doc-render/invoice-model";
import { formatValue, resolveField, vatCategoryLabel } from "@/lib/doc-render/labels";
import { fmtRON } from "@/lib/utils";

export interface DeclTable {
  title: string;
  columns: string[];
  rows: string[][];
}

// ── e-Factura (UBL) → labeled spreadsheet from the parsed invoice model ─────────────────────────────
const INVOICE_TYPE: Record<string, string> = {
  "380": "Factură",
  "381": "Notă de credit (storno)",
  "384": "Factură corectată",
  "389": "Autofactură",
};
const UNIT: Record<string, string> = {
  C62: "buc", H87: "buc", PCE: "buc", NAR: "buc", XPP: "buc", SET: "set", HUR: "oră",
  DAY: "zi", MON: "lună", ANN: "an", KGM: "kg", MTR: "m", LTR: "l", MTK: "m²", MTQ: "m³",
  KWH: "kWh", TNE: "tonă",
};
const trimZeros = (v: string): string => {
  const s = (v ?? "").trim();
  return /^\d+\.\d+$/.test(s) ? s.replace(/\.?0+$/, "") : s;
};
const money = (v: string, cur: string): string => {
  const n = Number((v ?? "").trim());
  return Number.isFinite(n) && (v ?? "").trim() !== "" ? `${fmtRON(n)}${cur ? ` ${cur}` : ""}` : (v ?? "");
};
function partyRows(p: InvoiceDoc["seller"]): string[][] {
  const cityLine = [p.city, p.county, p.country].filter(Boolean).join(", ");
  return [
    ["Denumire", p.name || "—"],
    ["CIF / CUI", p.vatId || p.companyId || "—"],
    ["Adresă", [p.street, cityLine].filter(Boolean).join(", ") || "—"],
  ];
}
/** Build a labeled, human-friendly XLSX from a parsed CIUS-RO invoice (header/seller/buyer/lines/VAT/
 *  totals). The canonical UBL XML is untouched; this is only the spreadsheet "layout". */
function invoiceTables(doc: InvoiceDoc): DeclTable[] {
  const cur = doc.currency;
  return [
    {
      title: "Factură",
      columns: ["Câmp", "Valoare"],
      rows: [
        ["Nr. factură", doc.number || "—"],
        ["Tip", INVOICE_TYPE[doc.typeCode] ?? doc.typeCode],
        ["Data emiterii", doc.issueDate || "—"],
        ["Data scadenței", doc.dueDate || "—"],
        ["Monedă", cur || "—"],
      ],
    },
    { title: "Vânzător", columns: ["Câmp", "Valoare"], rows: partyRows(doc.seller) },
    { title: "Cumpărător", columns: ["Câmp", "Valoare"], rows: partyRows(doc.buyer) },
    {
      title: "Linii factură",
      columns: ["Nr.", "Denumire", "Cantitate", "UM", "Preț unitar", "Cotă TVA", "Valoare netă"],
      rows: doc.lines.map((l, i) => [
        l.id || String(i + 1),
        l.name,
        trimZeros(l.quantity),
        UNIT[l.unit] ?? l.unit,
        money(l.unitPrice, cur),
        vatCategoryLabel(l.vatCode, l.vatPercent),
        money(l.lineAmount, cur),
      ]),
    },
    {
      title: "Detalierea TVA",
      columns: ["Cotă", "Bază", "TVA"],
      rows: doc.vatSubtotals.map((s) => [
        vatCategoryLabel(s.code, s.percent),
        money(s.taxable, cur),
        money(s.vat, cur),
      ]),
    },
    {
      title: "Totaluri",
      columns: ["Câmp", "Valoare"],
      rows: [
        ["Total fără TVA", money(doc.taxExclusive, cur)],
        ["Total TVA", money(doc.totalVat, cur)],
        ["Total cu TVA", money(doc.taxInclusive, cur)],
        ["De plată", money(doc.payable, cur)],
      ],
    },
  ];
}

function attrsOf(el: Element): [string, string][] {
  return Array.from(el.attributes)
    .filter((a) => a.name !== "xmlns" && !a.name.startsWith("xmlns:"))
    .map((a) => [a.name, a.value]);
}

function childGroups(el: Element): [string, Element[]][] {
  const order: string[] = [];
  const groups = new Map<string, Element[]>();
  for (const child of Array.from(el.children)) {
    if (!groups.has(child.tagName)) {
      groups.set(child.tagName, []);
      order.push(child.tagName);
    }
    groups.get(child.tagName)!.push(child);
  }
  return order.map((tag) => [tag, groups.get(tag)!]);
}

/** Human section title for an element: the descriptor title for the root, else the matching section's
 *  title, else the raw tag (docs without a descriptor). */
function titleFor(el: Element, desc: DocDescriptor | null): string {
  if (!desc) return el.tagName;
  if (el.localName === desc.rootTag) return desc.title;
  return desc.sections.find((s) => s.match === el.localName)?.title ?? el.tagName;
}

function walk(el: Element, desc: DocDescriptor | null, key: string, out: DeclTable[]): void {
  const attrs = attrsOf(el);
  if (attrs.length > 0) {
    // The element's own attributes → a labeled key/value table.
    out.push({
      title: titleFor(el, desc),
      columns: ["Câmp", "Valoare"],
      rows: attrs.map(([k, v]) => {
        const field = resolveField(key, k);
        return [field.label, formatValue(v, field)];
      }),
    });
  }

  for (const [, els] of childGroups(el)) {
    const allLeaf = els.every((e) => e.children.length === 0);
    if (allLeaf) {
      const rawCols = Array.from(new Set(els.flatMap((e) => attrsOf(e).map(([k]) => k))));
      const hasText = els.some((e) => (e.textContent ?? "").trim() !== "" && e.attributes.length === 0);
      // Header row: raw attribute names → human labels; the synthetic text column stays "Valoare".
      const columns = (hasText ? [...rawCols, "valoare"] : rawCols).map((c) =>
        c === "valoare" ? "Valoare" : resolveField(key, c).label,
      );
      const rows = els.map((e) => {
        const a = Object.fromEntries(attrsOf(e));
        const base = rawCols.map((c) => {
          const raw = a[c] ?? "";
          return raw === "" ? "" : formatValue(raw, resolveField(key, c));
        });
        return hasText ? [...base, (e.textContent ?? "").trim()] : base;
      });
      const baseTitle = titleFor(els[0], desc);
      out.push({
        title: els.length > 1 ? `${baseTitle} (×${els.length})` : baseTitle,
        columns,
        rows,
      });
    } else {
      els.forEach((e) => walk(e, desc, key, out));
    }
  }
}

/** `docKey` = the declaration kind (declKind/docKey) used to pick the label dictionary, exactly as
 *  XmlDocView does. Omit it for an unlabeled raw dump (back-compatible behavior via global fallback). */
export function xmlToTables(xml: string, docKey?: string): DeclTable[] {
  let root: Element | null;
  try {
    const doc = new DOMParser().parseFromString(xml, "application/xml");
    root = doc.querySelector("parsererror") ? null : doc.documentElement;
  } catch {
    root = null;
  }
  if (!root) return [];
  // e-Factura (UBL): derive the spreadsheet from the parsed invoice model (header/lines/VAT/totals),
  // not the generic element walker — the canonical UBL XML stays untouched.
  if (root.localName === "Invoice") {
    const inv = parseInvoice(xml);
    if (inv) return invoiceTables(inv);
  }
  // Resolve the descriptor like XmlDocView (docKey/declKind → root tag); `key` drives the labels.
  const desc = pickDescriptor(docKey, root.localName);
  const key = desc?.key ?? docKey ?? "";
  const out: DeclTable[] = [];
  walk(root, desc, key, out);
  return out;
}
