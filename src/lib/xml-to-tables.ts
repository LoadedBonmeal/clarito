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
import { formatValue, resolveField } from "@/lib/doc-render/labels";

export interface DeclTable {
  title: string;
  columns: string[];
  rows: string[][];
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
  // Resolve the descriptor like XmlDocView (docKey/declKind → root tag); `key` drives the labels.
  const desc = pickDescriptor(docKey, root.localName);
  const key = desc?.key ?? docKey ?? "";
  const out: DeclTable[] = [];
  walk(root, desc, key, out);
  return out;
}
