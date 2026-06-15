/**
 * XmlTableView — a read-only, human-readable rendering of declaration XML as tables.
 *
 * ANAF declaration XML keeps all data in ATTRIBUTES on elements, with repeating elements
 * (`<benef>`, `<rand_cod_300>`, `<sect_II>`, …) that map naturally onto tables. This component
 * parses the XML (DOMParser) and renders:
 *   - an element's own attributes  → a key/value header grid,
 *   - a group of same-tag leaf children → one table (rows = elements, columns = their attributes),
 *   - a non-repeating element with its own children → a nested section (recursion).
 * Namespace declarations (xmlns / xmlns:*) are hidden. It never throws — a parse error is shown
 * as a message and the user can switch to the Cod (raw XML) view.
 */
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

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

/** A repeating group of leaf elements (attributes/text only, no element children) → a table. */
function LeafTable({ tag, els }: { tag: string; els: Element[] }) {
  const { t } = useTranslation();
  const cols = Array.from(new Set(els.flatMap((e) => attrsOf(e).map(([k]) => k))));
  const hasText = els.some((e) => (e.textContent ?? "").trim() !== "" && e.attributes.length === 0);
  return (
    <div className="xmlv-tablewrap">
      <div className="xmlv-tabletag">
        {tag}
        {els.length > 1 && <span className="xmlv-count">×{els.length}</span>}
      </div>
      <div className="xmlv-tbl-scroll">
        <table className="xmlv-tbl">
          <thead>
            <tr>
              {cols.map((c) => (
                <th key={c}>{c}</th>
              ))}
              {hasText && <th>{t("shared.xmlViewer.valueCol")}</th>}
            </tr>
          </thead>
          <tbody>
            {els.map((e, i) => {
              const a = Object.fromEntries(attrsOf(e));
              return (
                <tr key={i}>
                  {cols.map((c) => (
                    <td key={c}>{a[c] ?? ""}</td>
                  ))}
                  {hasText && <td>{(e.textContent ?? "").trim()}</td>}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function XmlNode({ el, depth = 0 }: { el: Element; depth?: number }) {
  const attrs = attrsOf(el);
  const groups = childGroups(el);
  const leafText = el.children.length === 0 ? (el.textContent ?? "").trim() : "";

  return (
    <section className="xmlv-node" style={{ marginLeft: depth ? 14 : 0 }}>
      <h4 className="xmlv-node-title">{el.tagName}</h4>

      {attrs.length > 0 && (
        <div className="xmlv-kv">
          {attrs.map(([k, v]) => (
            <div className="xmlv-kv-row" key={k}>
              <span className="xmlv-kv-k">{k}</span>
              <span className="xmlv-kv-v">{v}</span>
            </div>
          ))}
        </div>
      )}

      {leafText && <div className="xmlv-leaftext">{leafText}</div>}

      {groups.map(([tag, els]) => {
        const allLeaf = els.every((e) => e.children.length === 0);
        return allLeaf ? (
          <LeafTable key={tag} tag={tag} els={els} />
        ) : (
          els.map((e, i) => <XmlNode key={tag + i} el={e} depth={depth + 1} />)
        );
      })}
    </section>
  );
}

export function XmlTableView({ xml }: { xml: string }) {
  const { t } = useTranslation();
  const root = useMemo(() => {
    try {
      const doc = new DOMParser().parseFromString(xml, "application/xml");
      if (doc.querySelector("parsererror")) return null;
      return doc.documentElement;
    } catch {
      return null;
    }
  }, [xml]);

  if (!root) {
    return <div className="xmlv-table-msg">{t("shared.xmlViewer.parseError")}</div>;
  }
  return (
    <div className="xmlv-table-scroll">
      <XmlNode el={root} />
    </div>
  );
}
