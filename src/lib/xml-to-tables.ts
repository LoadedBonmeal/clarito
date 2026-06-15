/**
 * xmlToTables — flatten declaration XML into a list of tables for an XLSX export.
 *
 * Mirrors XmlTableView's parsing (DOMParser → attributes + repeating leaf elements), but returns a
 * plain data model `{ title, columns, rows }[]` that the Rust `export_declaration_xlsx` command writes
 * to a real spreadsheet. The root/each element's attributes become a key/value table; each group of
 * same-tag leaf children becomes its own table (columns = union of their attributes). Namespace
 * declarations are dropped. Never throws — returns `[]` on a parse error.
 */
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

function walk(el: Element, out: DeclTable[]): void {
  const attrs = attrsOf(el);
  if (attrs.length > 0) {
    // The element's own attributes → a 2-column key/value table titled by the tag.
    out.push({
      title: el.tagName,
      columns: ["Câmp", "Valoare"],
      rows: attrs.map(([k, v]) => [k, v]),
    });
  }

  for (const [tag, els] of childGroups(el)) {
    const allLeaf = els.every((e) => e.children.length === 0);
    if (allLeaf) {
      const cols = Array.from(new Set(els.flatMap((e) => attrsOf(e).map(([k]) => k))));
      const hasText = els.some((e) => (e.textContent ?? "").trim() !== "" && e.attributes.length === 0);
      const columns = hasText ? [...cols, "valoare"] : cols;
      const rows = els.map((e) => {
        const a = Object.fromEntries(attrsOf(e));
        const base = cols.map((c) => a[c] ?? "");
        return hasText ? [...base, (e.textContent ?? "").trim()] : base;
      });
      out.push({
        title: els.length > 1 ? `${tag} (×${els.length})` : tag,
        columns,
        rows,
      });
    } else {
      els.forEach((e) => walk(e, out));
    }
  }
}

export function xmlToTables(xml: string): DeclTable[] {
  let root: Element | null;
  try {
    const doc = new DOMParser().parseFromString(xml, "application/xml");
    root = doc.querySelector("parsererror") ? null : doc.documentElement;
  } catch {
    root = null;
  }
  if (!root) return [];
  const out: DeclTable[] = [];
  walk(root, out);
  return out;
}
