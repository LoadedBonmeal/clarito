/**
 * XmlDocView — renders ANAF document XML as a clean, human-labeled DOCUMENT (titled sections, real
 * Romanian labels, key/value blocks + tables) instead of raw code or a cryptic attribute grid. This
 * is the primary view of the in-app XML viewer.
 *
 * Two paths: a dedicated typed layout for the UBL e-Factura invoice (seller/buyer/lines/VAT/totals,
 * like the official ANAF visualizer), and a generic descriptor-driven walker for attribute-based
 * declarations (D205, …). Anything without a descriptor falls back to the generic attribute table
 * (`XmlTableView`), so coverage grows by adding descriptors, never by changing this component.
 *
 * Read-only and non-mutating — it only DOMParses `xml`; the viewer's "Salvează" still writes the
 * byte-clean submission XML verbatim. The whole subtree is wrapped in `.docv` so the viewer's
 * "Printează / Salvează PDF" can serialize the same document to a printable HTML.
 */
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { XmlTableView } from "@/components/shared/XmlTableView";
import { pickDescriptor, type DocDescriptor, type SectionSpec } from "@/lib/doc-render/descriptors";
import { parseInvoice, type InvoiceDoc, type Party } from "@/lib/doc-render/invoice-model";
import { parseSaftSummary, type SaftSummary } from "@/lib/doc-render/saft-model";
import { formatValue, resolveField, vatCategoryLabel } from "@/lib/doc-render/labels";
import type { XmlViewerPayload } from "@/lib/xml-viewer-store";
import { fmtRON, formatDate } from "@/lib/utils";

function attrsOf(el: Element): [string, string][] {
  return Array.from(el.attributes)
    .filter((a) => a.name !== "xmlns" && !a.name.startsWith("xmlns:"))
    .map((a) => [a.name, a.value]);
}

/** Elements matching a section: the root itself, or all descendants with that local name. */
function elementsFor(root: Element, match: string): Element[] {
  if (root.localName === match) return [root];
  return Array.from(root.getElementsByTagName("*")).filter((e) => e.localName === match);
}

const isMoney = (fmt?: string) => fmt === "money_lei" || fmt === "money2";

function KvSection({ el, spec, docKey }: { el: Element; spec: SectionSpec; docKey: string }) {
  const all = Object.fromEntries(attrsOf(el));
  // With `attrs`, render only those keys (in order) that are present — lets several sections group
  // one element's attributes (e.g. D300's root). Without it, render all of the element's attributes.
  const attrs: [string, string][] = spec.attrs
    ? spec.attrs.filter((a) => a in all).map((a) => [a, all[a]])
    : attrsOf(el);
  if (attrs.length === 0) return null;
  return (
    <section className="docv-sec">
      <h3 className="docv-sec-title">{spec.title}</h3>
      <div className="docv-kv">
        {attrs.map(([k, v]) => {
          const spec = resolveField(docKey, k);
          return (
            <div className="docv-kv-row" key={k}>
              <span className="docv-kv-k">{spec.label}</span>
              <span className="docv-kv-v">{formatValue(v, spec)}</span>
            </div>
          );
        })}
      </div>
    </section>
  );
}

function TableSection({
  els,
  spec,
  docKey,
}: {
  els: Element[];
  spec: SectionSpec;
  docKey: string;
}) {
  if (els.length === 0) return null;
  const cols =
    spec.columns ?? Array.from(new Set(els.flatMap((e) => attrsOf(e).map(([k]) => k))));
  return (
    <section className="docv-sec">
      <h3 className="docv-sec-title">
        {spec.title}
        {els.length > 1 && <span className="docv-count">{els.length}</span>}
      </h3>
      <div className="docv-tbl-wrap">
        <table className="docv-tbl">
          <thead>
            <tr>
              {cols.map((c) => {
                const f = resolveField(docKey, c);
                return (
                  <th key={c} className={isMoney(f.format) ? "r" : ""}>
                    {f.label}
                  </th>
                );
              })}
            </tr>
          </thead>
          <tbody>
            {els.map((e, i) => {
              const a = Object.fromEntries(attrsOf(e));
              return (
                <tr key={i}>
                  {cols.map((c) => {
                    const f = resolveField(docKey, c);
                    return (
                      <td key={c} className={isMoney(f.format) ? "r" : ""}>
                        {formatValue(a[c] ?? "", f)}
                      </td>
                    );
                  })}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

/** Element groups (by local name) that carry attributes but are NOT consumed by any descriptor
 *  section nor the root — so they're never silently dropped from the printed document. Order =
 *  first occurrence in the XML. (Fixes e.g. D112 medical-leave B-path + certificates + F1/F2.) */
function unconsumedGroups(root: Element, desc: DocDescriptor): [string, Element[]][] {
  const matched = new Set(desc.sections.map((s) => s.match));
  matched.add(desc.rootTag);
  const order: string[] = [];
  const groups = new Map<string, Element[]>();
  for (const el of Array.from(root.getElementsByTagName("*"))) {
    const ln = el.localName;
    if (matched.has(ln) || attrsOf(el).length === 0) continue;
    if (!groups.has(ln)) {
      groups.set(ln, []);
      order.push(ln);
    }
    groups.get(ln)!.push(el);
  }
  return order.map((ln) => [ln, groups.get(ln)!]);
}

function GenericDoc({ root, desc }: { root: Element; desc: DocDescriptor }) {
  const extras = unconsumedGroups(root, desc);
  return (
    <>
      <h2 className="docv-title">{desc.title}</h2>
      {desc.sections.map((s, i) => {
        const els = elementsFor(root, s.match);
        if (s.as === "kv") {
          return els[0] ? <KvSection key={i} el={els[0]} spec={s} docKey={desc.key} /> : null;
        }
        return <TableSection key={i} els={els} spec={s} docKey={desc.key} />;
      })}
      {/* Safety net: render any element group the descriptor didn't name, so nothing is dropped. */}
      {extras.map(([tag, els]) => (
        <TableSection
          key={`extra-${tag}`}
          els={els}
          spec={{ match: tag, title: tag, as: "table" }}
          docKey={desc.key}
        />
      ))}
    </>
  );
}

// ── Invoice (UBL e-Factura) document ──────────────────────────────────────────────────────────────
const INVOICE_TYPE: Record<string, string> = {
  "380": "Factură",
  "381": "Notă de credit (storno)",
  "384": "Factură corectată",
  "389": "Autofactură",
};
const UNIT: Record<string, string> = {
  C62: "buc", H87: "buc", PCE: "buc", NAR: "buc", XPP: "buc", SET: "set",
  HUR: "oră", DAY: "zi", MON: "lună", ANN: "an", KGM: "kg", MTR: "m",
  LTR: "l", MTK: "m²", MTQ: "m³", KWH: "kWh", TNE: "tonă",
};

function money(v: string, currency?: string): string {
  const n = Number((v ?? "").trim());
  if (!Number.isFinite(n) || (v ?? "").trim() === "") return v ?? "";
  return currency ? `${fmtRON(n)} ${currency}` : fmtRON(n);
}

/** Trim trailing zeros from a UBL quantity ("20.000000" → "20", "5.50" → "5.5"). */
function trimNum(v: string): string {
  const s = (v ?? "").trim();
  return /^\d+\.\d+$/.test(s) ? s.replace(/\.?0+$/, "") : s;
}

function PartyBlock({ title, p }: { title: string; p: Party }) {
  const cityLine = [p.city, p.county, p.country].filter(Boolean).join(", ");
  return (
    <div className="docv-party">
      <div className="docv-sec-title">{title}</div>
      <div className="docv-party-name">{p.name || "—"}</div>
      {p.vatId && <div className="docv-party-row">CIF: {p.vatId}</div>}
      {!p.vatId && p.companyId && <div className="docv-party-row">CUI: {p.companyId}</div>}
      {p.street && <div className="docv-party-row">{p.street}</div>}
      {cityLine && <div className="docv-party-row">{cityLine}</div>}
    </div>
  );
}

function InvoiceDocView({ doc }: { doc: InvoiceDoc }) {
  const cur = doc.currency;
  const meta: [string, string][] = [
    ["Nr. factură", doc.number || "—"],
    ["Tip", INVOICE_TYPE[doc.typeCode] ?? doc.typeCode],
    ["Data emiterii", doc.issueDate ? formatDate(doc.issueDate) : "—"],
    ["Data scadenței", doc.dueDate ? formatDate(doc.dueDate) : "—"],
    ["Monedă", cur || "—"],
  ];
  return (
    <>
      <h2 className="docv-title">
        Factură electronică (RO e-Factura)
        {doc.number && <span className="docv-title-sub"> · {doc.number}</span>}
      </h2>

      <div className="docv-parties">
        <PartyBlock title="VÂNZĂTOR" p={doc.seller} />
        <PartyBlock title="CUMPĂRĂTOR" p={doc.buyer} />
      </div>

      <section className="docv-sec">
        <div className="docv-kv">
          {meta.map(([k, v]) => (
            <div className="docv-kv-row" key={k}>
              <span className="docv-kv-k">{k}</span>
              <span className="docv-kv-v">{v}</span>
            </div>
          ))}
        </div>
      </section>

      <section className="docv-sec">
        <h3 className="docv-sec-title">Linii factură</h3>
        <div className="docv-tbl-wrap">
          <table className="docv-tbl">
            <thead>
              <tr>
                <th>Nr.</th>
                <th>Denumire</th>
                <th className="r">Cant.</th>
                <th>UM</th>
                <th className="r">Preț unitar</th>
                <th>Cotă TVA</th>
                <th className="r">Valoare netă</th>
              </tr>
            </thead>
            <tbody>
              {doc.lines.map((l, i) => (
                <tr key={i}>
                  <td>{l.id || i + 1}</td>
                  <td>
                    {l.name}
                    {l.description && l.description !== l.name && (
                      <div className="docv-line-desc">{l.description}</div>
                    )}
                  </td>
                  <td className="r">{trimNum(l.quantity)}</td>
                  <td>{UNIT[l.unit] ?? l.unit}</td>
                  <td className="r">{money(l.unitPrice, cur)}</td>
                  <td>{vatCategoryLabel(l.vatCode, l.vatPercent)}</td>
                  <td className="r">{money(l.lineAmount, cur)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <div className="docv-cols">
        <section className="docv-sec">
          <h3 className="docv-sec-title">Detalierea TVA</h3>
          <div className="docv-tbl-wrap">
            <table className="docv-tbl">
              <thead>
                <tr>
                  <th>Cotă</th>
                  <th className="r">Bază</th>
                  <th className="r">TVA</th>
                </tr>
              </thead>
              <tbody>
                {doc.vatSubtotals.map((s, i) => (
                  <tr key={i}>
                    <td>{vatCategoryLabel(s.code, s.percent)}</td>
                    <td className="r">{money(s.taxable, cur)}</td>
                    <td className="r">{money(s.vat, cur)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>

        <section className="docv-sec docv-totals">
          <div className="docv-tot-row">
            <span>Total fără TVA</span>
            <span>{money(doc.taxExclusive || doc.lineExtension, cur)}</span>
          </div>
          <div className="docv-tot-row">
            <span>TVA</span>
            <span>{money(doc.totalVat, cur)}</span>
          </div>
          <div className="docv-tot-row docv-tot-grand">
            <span>Total de plată</span>
            <span>{money(doc.payable || doc.taxInclusive, cur)}</span>
          </div>
        </section>
      </div>

      {doc.notes.length > 0 && (
        <section className="docv-sec">
          <h3 className="docv-sec-title">Mențiuni</h3>
          {doc.notes.map((n, i) => (
            <div className="docv-note" key={i}>
              {n}
            </div>
          ))}
        </section>
      )}
    </>
  );
}

export function XmlDocView({ payload }: { payload: XmlViewerPayload }) {
  const { t } = useTranslation();
  const parsed = useMemo(() => {
    try {
      const doc = new DOMParser().parseFromString(payload.xml, "application/xml");
      if (doc.querySelector("parsererror")) return null;
      return doc.documentElement;
    } catch {
      return null;
    }
  }, [payload.xml]);

  const desc = useMemo(
    () => (parsed ? pickDescriptor(payload.docKey ?? payload.declKind, parsed.localName) : null),
    [parsed, payload.docKey, payload.declKind],
  );

  if (!parsed) {
    return <div className="xmlv-table-msg">{t("shared.xmlViewer.parseError")}</div>;
  }

  // No descriptor → keep today's generic attribute table (additive rollout).
  if (!desc) {
    return (
      <div className="xmlv-table-scroll">
        <XmlTableView xml={payload.xml} />
      </div>
    );
  }

  return (
    <div className="docv-scroll">
      <article className="docv">
        {desc.key === "INVOICE" ? (
          <InvoiceFromXml xml={payload.xml} />
        ) : desc.key === "D406" ? (
          <SaftFromXml xml={payload.xml} />
        ) : (
          <GenericDoc root={parsed} desc={desc} />
        )}
      </article>
    </div>
  );
}

/** Parse + render the UBL invoice; fall back to the generic table if it isn't a parseable Invoice. */
function InvoiceFromXml({ xml }: { xml: string }) {
  const doc = useMemo(() => parseInvoice(xml), [xml]);
  if (!doc) return <XmlTableView xml={xml} />;
  return <InvoiceDocView doc={doc} />;
}

// ── SAF-T (D406) summary cover page ───────────────────────────────────────────────────────────────
/** Parse + render the SAF-T summary; fall back to the generic table if it isn't a parseable AuditFile. */
function SaftFromXml({ xml }: { xml: string }) {
  const s = useMemo(() => parseSaftSummary(xml), [xml]);
  if (!s) return <XmlTableView xml={xml} />;
  return <SaftDocView s={s} />;
}

const numOf = (v: string): number => {
  const n = Number((v ?? "").trim());
  return Number.isFinite(n) ? n : NaN;
};

function SaftDocView({ s }: { s: SaftSummary }) {
  const typeLabel =
    s.declType === "L"
      ? "D406 lunar/trimestrial"
      : s.declType === "A"
        ? "D406 Active (anual)"
        : "SAF-T";
  const period =
    s.periodStart && s.periodEnd ? `${formatDate(s.periodStart)} – ${formatDate(s.periodEnd)}` : "—";
  const debit = numOf(s.gl.debit);
  const credit = numOf(s.gl.credit);
  const diff = Number.isFinite(debit) && Number.isFinite(credit) ? debit - credit : NaN;
  const balanced = s.gl.present && Number.isFinite(diff) && Math.abs(diff) < 0.01;

  const chips: [string, string, boolean?][] = [
    ["Versiune schemă", s.version || "—"],
    ["Țară", s.country || "—", s.country === "RO"],
    ["Monedă", s.currency || "—", s.currency === "RON"],
    ["Bază contabilă", s.basis || "—"],
    ["Generat la", s.dateCreated ? formatDate(s.dateCreated) : "—"],
    ["Software", s.software || "—"],
  ];

  const presentSections = s.sections.filter((sec) => sec.present);
  const presentCounts = s.counts.filter((c) => c.value > 0);
  const fmtMetric = (label: string, value: string): string => {
    if (!value) return "—";
    return /cant/i.test(label) ? trimNum(value) : fmtRON(numOf(value));
  };

  // Footer verdict (worst state): red blocks, amber warns.
  const periodOk = !!s.periodStart && !!s.periodEnd && s.periodStart <= s.periodEnd;
  const red =
    !s.cui ||
    s.country !== "RO" ||
    s.currency !== "RON" ||
    !periodOk ||
    (s.gl.present && !balanced);
  const amber =
    !red &&
    ((s.declType === "L" && s.gl.present && numOf(s.gl.entries) === 0) ||
      (s.declType === "L" &&
        presentSections.some((x) => x.key === "SalesInvoices" && numOf(x.count) > 0) &&
        s.counts.find((c) => c.key === "taxCodes")?.value === 0));
  const verdict = red
    ? { txt: "Blocant — verificați câmpurile marcate", cls: "xmlv-valid--err" }
    : amber
      ? { txt: "De verificat — vezi avertismentele", cls: "xmlv-valid--na" }
      : { txt: "Gata de depunere", cls: "xmlv-valid--ok" };

  return (
    <>
      <h2 className="docv-title">
        Rezumat pre-depunere SAF-T (D406)
        <span className="docv-title-sub"> · {typeLabel}</span>
      </h2>

      <div className="docv-parties">
        <div className="docv-party">
          <div className="docv-sec-title">FIRMĂ</div>
          <div className="docv-party-name">{s.companyName || "—"}</div>
          <div className="docv-party-row">CUI: {s.cui || "—"}</div>
          <div className="docv-party-row">
            {s.vatNumber ? `Cod TVA: ${s.vatNumber}` : "Neplătitor de TVA"}
          </div>
          {s.address && <div className="docv-party-row">{s.address}</div>}
        </div>
        <div className="docv-party">
          <div className="docv-sec-title">PERIOADĂ</div>
          <div className="docv-party-name">{period}</div>
          <div className="docv-party-row">{typeLabel}</div>
        </div>
      </div>

      <section className="docv-sec">
        <div className="docv-kv">
          {chips.map(([k, v, ok]) => (
            <div className="docv-kv-row" key={k}>
              <span className="docv-kv-k">{k}</span>
              <span className="docv-kv-v">
                {v}
                {ok === false && " ⚠"}
              </span>
            </div>
          ))}
        </div>
      </section>

      <section className="docv-sec">
        <h3 className="docv-sec-title">Balansare registru jurnal (GL)</h3>
        {s.gl.present ? (
          <div className="docv-kv">
            <div className="docv-kv-row">
              <span className="docv-kv-k">Nr. linii GL</span>
              <span className="docv-kv-v">{s.gl.entries || "—"}</span>
            </div>
            <div className="docv-kv-row">
              <span className="docv-kv-k">Total debit</span>
              <span className="docv-kv-v">{fmtRON(debit)}</span>
            </div>
            <div className="docv-kv-row">
              <span className="docv-kv-k">Total credit</span>
              <span className="docv-kv-v">{fmtRON(credit)}</span>
            </div>
            <div className="docv-kv-row">
              <span className="docv-kv-k">Diferență</span>
              <span className="docv-kv-v">
                {balanced
                  ? "Echilibrat ✓"
                  : Number.isNaN(diff)
                    ? "Nevalidă ✗"
                    : `${fmtRON(diff)} ✗`}
              </span>
            </div>
          </div>
        ) : (
          <div className="docv-kv-row">
            <span className="docv-kv-v">Nu se aplică (declarație anuală — fără jurnal GL).</span>
          </div>
        )}
      </section>

      {presentSections.length > 0 && (
        <section className="docv-sec">
          <h3 className="docv-sec-title">Totaluri de control pe secțiuni</h3>
          <div className="docv-tbl-wrap">
            <table className="docv-tbl">
              <thead>
                <tr>
                  <th>Secțiune</th>
                  <th className="r">Nr.</th>
                  <th>Valori</th>
                </tr>
              </thead>
              <tbody>
                {presentSections.map((sec) => (
                  <tr key={sec.key}>
                    <td>{sec.label}</td>
                    <td className="r">{sec.count || "—"}</td>
                    <td>
                      {sec.metrics
                        .filter((m) => m.value)
                        .map((m) => `${m.label}: ${fmtMetric(m.label, m.value)}`)
                        .join(" · ") || "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}

      {presentCounts.length > 0 && (
        <section className="docv-sec">
          <h3 className="docv-sec-title">Nomenclatoare</h3>
          <div className="docv-kv">
            {presentCounts.map((c) => (
              <div className="docv-kv-row" key={c.key}>
                <span className="docv-kv-k">{c.label}</span>
                <span className="docv-kv-v">{c.value}</span>
              </div>
            ))}
          </div>
        </section>
      )}

      <div className={`xmlv-valid ${verdict.cls}`} style={{ marginTop: 12 }}>
        {verdict.txt}
      </div>
    </>
  );
}
