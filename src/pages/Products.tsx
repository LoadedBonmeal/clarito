/**
 * Articole & stocuri — verbatim port of the design "Articole si stocuri.html":
 *   .page-head (title + sub "N articole · evaluare stoc FIFO/CMP per articol · firmă"
 *   + sq-btn refresh + btn-dark "Articol nou") · .banner.danger stoc negativ ·
 *   .scr-card → .scr-toolbar (.tabs Toate/Active · .scr-search) → .scr-table
 *   (denumire cu .cli-ava + chip art. 331 · cod .doc · UM · preț r/num · TVA % ·
 *   metodă chip FIFO/CMP · cont stoc .doc · stoc r/num cu chip late pe negativ) →
 *   .pager real (client-side) · al doilea .scr-card "Fișa de magazie (gestiune)"
 *   cu ledger-ul real al articolului selectat.
 *
 * ALL wiring preserved: api.products.list/create/update/delete,
 * api.stockValuation.ledger/recordReceipt/recordIssue/setValuation,
 * filtre Toate/Active, căutare, modal creare/editare (cod/denumire/UM/preț/
 * cotă TVA/categorie TVA/cod art. 331/stoc/activ), confirmare ștergere,
 * guard "selectați o companie".
 */

import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { Product, ProductInput, UpdateProductInput } from "@/types";
import { VAT_RATES, VAT_CATEGORIES, VAT_CATEGORY_LABELS } from "@/lib/constants";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

/** Cantități: ro-RO, fără zecimale inutile (24, nu 24,000). */
const fmtQty = (s: string | number | null | undefined) =>
  parseDec(s).toLocaleString("ro-RO", { maximumFractionDigits: 3 });

/** Inițiale pentru .cli-ava (LP ← „Laptop Pro 14""). */
const initials = (name: string) => {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length >= 2) return (parts[0][0] + parts[1][0]).toUpperCase();
  return name.slice(0, 2).toUpperCase() || "—";
};

const PAGE_SIZE = 50;

// Inline icons absent from Ic (verbatim from the prototype / heroicons outline).
const SVG_WARN = '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';
const SVG_REVERSE = '<path d="M9 15 3 9m0 0 6-6M3 9h12a6 6 0 0 1 0 12h-3"/>';
const SVG_CHEV_L = '<path d="M15.75 19.5 8.25 12l7.5-7.5"/>';
const SVG_CHEV_R = '<path d="m8.25 4.5 7.5 7.5-7.5 7.5"/>';
const SVG_TRASH = '<path d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>';

// Art. 331 product categories (codPR) — Parameters_v7._listaCodPR
// Shown only when vatCategory="AE". For tip_partener=1 (default use-case).
const ART331_CODES: { value: string; label: string }[] = [
  { value: "22", label: "22 — Deșeuri feroase/neferoase" },
  { value: "23", label: "23 — Masă lemnoasă și materiale lemnoase" },
  { value: "24", label: "24 — Certificate CO₂/gaze cu efect de seră" },
  { value: "25", label: "25 — Energie electrică" },
  { value: "26", label: "26 — Certificate verzi" },
  { value: "27", label: "27 — Construcții/terenuri" },
  { value: "28", label: "28 — Aur de investiții" },
  { value: "29", label: "29 — Telefoane mobile" },
  { value: "30", label: "30 — Microprocesoare (circuite integrate)" },
  { value: "31", label: "31 — Console/tablete/laptopuri" },
  { value: "36", label: "36 — Gaze naturale" },
  { value: "1001", label: "1001 — Grâu comun/alac" },
  { value: "1002", label: "1002 — Secară" },
  { value: "1003", label: "1003 — Orz" },
  { value: "1004", label: "1004 — Ovăz" },
  { value: "1005", label: "1005 — Porumb" },
  { value: "1201", label: "1201 — Soia" },
  { value: "1205", label: "1205 — Rapiță" },
  { value: "120600", label: "120600 — Floarea-soarelui" },
  { value: "121291", label: "121291 — Sfeclă de zahăr" },
  { value: "10086000", label: "10086000 — Orez" },
  { value: "120400", label: "120400 — Semințe de in" },
];

const art331Label = (code: string) =>
  ART331_CODES.find((c) => c.value === code)?.label ?? code;

// ─── ProductsPage ─────────────────────────────────────────────────────────────

export function ProductsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<"all" | "active">("all");
  const [modal, setModal] = useState<"create" | { edit: Product } | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [pageRaw, setPageRaw] = useState(1);

  const {
    data: allProducts = [],
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: queryKeys.products.list(activeCompanyId ?? "", undefined),
    queryFn: () => api.products.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });
  const activeCompany = companies.find((c) => c.id === activeCompanyId);

  const list = useMemo(() => {
    const base =
      filter === "active" ? allProducts.filter((p) => p.active) : allProducts;
    const q = query.trim().toLowerCase();
    if (!q) return base;
    return base.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        (p.code ?? "").toLowerCase().includes(q),
    );
  }, [allProducts, query, filter]);

  const activeCount = allProducts.filter((p) => p.active).length;

  // Stoc negativ (FIFO nu poate descărca pe stoc negativ) → banner danger.
  const negativeStock = useMemo(
    () => allProducts.filter((p) => p.stockQty != null && parseDec(p.stockQty) < 0),
    [allProducts],
  );

  // Paginare reală client-side (design .pager).
  useEffect(() => { setPageRaw(1); }, [query, filter]);
  const totalPages = Math.max(1, Math.ceil(list.length / PAGE_SIZE));
  const page = Math.min(pageRaw, totalPages);
  const visibleRows = list.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE);
  const pageWindow = useMemo(() => {
    const start = Math.max(1, Math.min(page - 2, totalPages - 4));
    const end = Math.min(totalPages, start + 4);
    const out: number[] = [];
    for (let i = start; i <= end; i++) out.push(i);
    return out;
  }, [page, totalPages]);

  // Articolul selectat pentru fișa de magazie (rândul evidențiat din prototip).
  const selected =
    list.find((p) => p.id === selectedId) ??
    visibleRows.find((p) => p.stockQty != null) ??
    visibleRows[0] ??
    null;

  const deleteMutation = useMutation({
    mutationFn: (id: string) => {
      if (!activeCompanyId)
        return Promise.reject(new Error("Nicio companie activă."));
      return api.products.delete(id, activeCompanyId);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.products.all });
      notify.success("Articol șters.");
    },
    onError: (e) =>
      notify.error(formatError(e, "Nu s-a putut șterge articolul.")),
  });

  const handleDelete = async (p: Product) => {
    if (!activeCompanyId) return;
    const ok = await confirm(
      `Șterge articolul "${p.name}"? Această acțiune nu poate fi anulată.`,
      { title: "Confirmare ștergere", kind: "warning" },
    );
    if (!ok) return;
    deleteMutation.mutate(p.id);
  };

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>Articole</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Selectați o companie activă pentru a vedea catalogul de articole.
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Articole</h1>
          <p className="sub">
            {list.length !== allProducts.length
              ? `${list.length} din ${allProducts.length.toLocaleString("ro-RO")} articole`
              : `${allProducts.length.toLocaleString("ro-RO")} articole`}
            {" · evaluare stoc FIFO/CMP per articol"}
            {activeCompany ? ` · ${activeCompany.legalName}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="sq-btn spin-btn" title="Reîmprospătează" onClick={() => void refetch()}>
            <Ic name="sync" />
          </button>
          <button className="btn-dark" onClick={() => setModal("create")}>
            <Ic name="plus" />Articol nou
          </button>
        </div>
      </div>

      {/* banner stoc negativ */}
      {negativeStock.length > 0 && (
        <div className="banner danger">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN }} />
          <span>
            <b>
              Stoc negativ: {negativeStock.slice(0, 3).map((p) => `${p.name} (${fmtQty(p.stockQty)} ${p.unit})`).join(", ")}
              {negativeStock.length > 3 ? ` și încă ${negativeStock.length - 3}` : ""}.
            </b>{" "}
            Ieșirile depășesc intrările înregistrate — la metoda FIFO descărcarea de gestiune nu se
            poate calcula pe stoc negativ. Înregistrați recepția lipsă din Gestiune, apoi reluați
            descărcarea.
          </span>
        </div>
      )}

      <div className="scr-card" style={{ marginBottom: 14 }}>
        {/* toolbar */}
        <div className="scr-toolbar">
          <div className="tabs">
            <div
              className={`tab${filter === "all" ? " active" : ""}`}
              onClick={() => setFilter("all")}
            >
              Toate<span className="cnt num">{allProducts.length}</span>
            </div>
            <div
              className={`tab${filter === "active" ? " active" : ""}`}
              onClick={() => setFilter("active")}
            >
              Active<span className="cnt num">{activeCount}</span>
            </div>
          </div>
          <div className="spacer" />
          <div className="scr-search">
            <Ic name="lens" />
            <input
              type="text"
              placeholder="Caută articol sau cod…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
        </div>

        {/* table */}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
        ) : isError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={error} label="articolele" onRetry={() => void refetch()} />
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {allProducts.length === 0
              ? "Niciun articol. Adăugați produse sau servicii cu butonul „Articol nou” — vor apărea ca linii de factură."
              : "Niciun rezultat pentru filtrele aplicate."}
          </div>
        ) : (
          <>
            <table className="scr-table">
              <thead>
                <tr>
                  <th>Denumire</th>
                  <th>Cod</th>
                  <th>UM</th>
                  <th className="r">Preț unitar</th>
                  <th className="r">TVA %</th>
                  <th>Metodă</th>
                  <th>Cont stoc</th>
                  <th className="r">Stoc</th>
                  <th className="r" style={{ width: 92 }}></th>
                </tr>
              </thead>
              <tbody>
                {visibleRows.map((p) => {
                  const tracked = p.stockQty != null;
                  const stock = tracked ? parseDec(p.stockQty) : null;
                  const method = p.valuationMethod === "FIFO" ? "FIFO" : "CMP";
                  const isSel = selected?.id === p.id;
                  return (
                    <tr
                      key={p.id}
                      style={isSel ? { background: "#FCFCFD" } : undefined}
                      onClick={() => setSelectedId(p.id)}
                    >
                      <td>
                        <div className="cli">
                          <span className="cli-ava">{initials(p.name)}</span>
                          {isSel ? <b>{p.name}</b> : p.name}
                          {p.vatCategory === "AE" && (
                            <span className="chip wait" style={{ marginLeft: 6 }}>
                              <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_REVERSE }} />
                              art. 331 · {art331Label(p.art331Code || "22")}
                            </span>
                          )}
                          {!p.active && (
                            <span className="chip sent" style={{ marginLeft: 6 }}>Inactiv</span>
                          )}
                        </div>
                      </td>
                      <td>{p.code ? <span className="doc">{p.code}</span> : <span className="muted">—</span>}</td>
                      <td>{p.unit}</td>
                      <td className="r num">{fmtRON(p.unitPrice)}</td>
                      <td className="num">{p.vatRate}%</td>
                      <td>
                        {tracked
                          ? <span className="chip sent">{method}</span>
                          : <span className="muted">—</span>}
                      </td>
                      <td>
                        {tracked
                          ? <span className="doc">{p.stockAccount || "371"}</span>
                          : <span className="muted">—</span>}
                      </td>
                      <td className={`r num${tracked ? "" : " muted"}`}>
                        {!tracked ? (
                          "—"
                        ) : stock !== null && stock < 0 ? (
                          <span className="chip late">
                            <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN }} />
                            {fmtQty(p.stockQty)}
                          </span>
                        ) : (
                          fmtQty(p.stockQty)
                        )}
                      </td>
                      <td onClick={(e) => e.stopPropagation()}>
                        <div className="row-acts">
                          <button
                            className="mini-btn"
                            title="Editează"
                            onClick={() => setModal({ edit: p })}
                          >
                            <Ic name="pen" />
                          </button>
                          <button
                            className="mini-btn"
                            title="Gestiune (fișa de magazie)"
                            onClick={() => setSelectedId(p.id)}
                          >
                            <Ic name="cube" />
                          </button>
                          <button
                            className="mini-btn"
                            title="Șterge"
                            onClick={() => void handleDelete(p)}
                          >
                            <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_TRASH }} />
                          </button>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>

            {/* pager */}
            <div className="pager">
              <span>
                Afișezi <b>{((page - 1) * PAGE_SIZE + 1).toLocaleString("ro-RO")}–{Math.min(page * PAGE_SIZE, list.length).toLocaleString("ro-RO")}</b> din <b>{list.length.toLocaleString("ro-RO")}</b> articole
              </span>
              <div className="pg-btns">
                <button
                  className="pg-btn"
                  disabled={page <= 1}
                  onClick={() => setPageRaw(page - 1)}
                  aria-label="Pagina anterioară"
                >
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_CHEV_L }} />
                </button>
                {pageWindow.map((n) => (
                  <button
                    key={n}
                    className={`pg-btn${n === page ? " cur" : ""}`}
                    onClick={() => setPageRaw(n)}
                  >
                    {n}
                  </button>
                ))}
                <button
                  className="pg-btn"
                  disabled={page >= totalPages}
                  onClick={() => setPageRaw(page + 1)}
                  aria-label="Pagina următoare"
                >
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_CHEV_R }} />
                </button>
              </div>
            </div>
          </>
        )}
      </div>

      {/* fișa de magazie */}
      {selected && activeCompanyId && (
        <FisaMagazieCard key={selected.id} companyId={activeCompanyId} product={selected} />
      )}

      {/* product modal */}
      {modal !== null && (
        <ProductModal
          companyId={activeCompanyId}
          product={modal === "create" ? null : modal.edit}
          onClose={() => setModal(null)}
          onSaved={() => {
            void queryClient.invalidateQueries({ queryKey: queryKeys.products.all });
            setModal(null);
          }}
        />
      )}
    </div>
  );
}

// ─── Fișa de magazie (gestiune: recepție / descărcare / ledger) ───────────────

function FisaMagazieCard({ companyId, product }: { companyId: string; product: Product }) {
  const qc = useQueryClient();
  const [tab, setTab] = useState<"in" | "out">("in");
  const [date, setDate] = useState(new Date().toISOString().slice(0, 10));
  const [qty, setQty] = useState("");
  const [cost, setCost] = useState("");
  const [docRef, setDocRef] = useState("");
  const [method, setMethod] = useState(product.valuationMethod === "FIFO" ? "FIFO" : "CMP");
  const [stockAcct, setStockAcct] = useState(product.stockAccount || "371");

  const { data: ledger = [], refetch } = useQuery({
    queryKey: ["stock-ledger", product.id],
    queryFn: () => api.stockValuation.ledger(companyId, product.id),
  });

  const valMut = useMutation({
    mutationFn: (m: string) =>
      api.stockValuation.setValuation(companyId, product.id, m, stockAcct.trim() || "371"),
    onSuccess: () => {
      notify.success("Metodă de evaluare actualizată — stocul a fost reevaluat.");
      void refetch();
      void qc.invalidateQueries({ queryKey: queryKeys.products.all });
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut schimba metoda de evaluare.")),
  });

  const mut = useMutation({
    mutationFn: () => {
      if (!/^\d+(\.\d+)?$/.test(qty.trim())) throw new Error("Cantitate invalidă.");
      const input = {
        companyId, productId: product.id, entryDate: date, qty: qty.trim(),
        unitCost: tab === "in" ? (cost.trim() || "0") : undefined,
        docType: tab === "in" ? "NIR" : "BC", docRef: docRef.trim() || undefined,
      };
      return tab === "in" ? api.stockValuation.recordReceipt(input) : api.stockValuation.recordIssue(input);
    },
    onSuccess: (warning) => {
      notify.success(tab === "in" ? "Recepție înregistrată." : "Descărcare gestiune înregistrată.");
      if (warning) notify.warn(warning);
      setQty(""); setCost(""); setDocRef("");
      void refetch();
      void qc.invalidateQueries({ queryKey: queryKeys.products.all });
    },
    onError: (e) => notify.error(formatError(e, "Eroare la mișcarea de stoc.")),
  });

  const inputStyle: React.CSSProperties = { height: 32, fontSize: 12.5 };

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">Fișa de magazie (gestiune) — {product.name}</div>
        <span className="chip sent">{method} · cont stoc {stockAcct.trim() || "371"}</span>
        <div className="spacer" />
        <select
          className="select num"
          style={{ width: 150, height: 32, fontSize: 12.5 }}
          value={method}
          onChange={(e) => { setMethod(e.target.value); valMut.mutate(e.target.value); }}
          disabled={valMut.isPending}
          title="Metodă de evaluare (OMFP 1802/2014)"
        >
          <option value="CMP">CMP (cost mediu)</option>
          <option value="FIFO">FIFO</option>
        </select>
        <input
          className="input num"
          style={{ width: 76, height: 32, fontSize: 12.5 }}
          value={stockAcct}
          onChange={(e) => setStockAcct(e.target.value)}
          onBlur={() => valMut.mutate(method)}
          placeholder="371"
          title="Cont stoc (371/301/345…)"
        />
        {/* propunere — neimplementat: export fișa de magazie */}
        <button className="pill-btn" onClick={() => notify.info("În curând.")}>
          <Ic name="dl" />Export
        </button>
      </div>

      {/* mișcare nouă: recepție / descărcare (funcționalitate reală, restilizată) */}
      <div
        style={{
          display: "flex", alignItems: "flex-end", gap: 10, flexWrap: "wrap",
          padding: "12px 16px", borderBottom: "1px solid var(--line)",
        }}
      >
        <div className="tabs">
          <div className={`tab${tab === "in" ? " active" : ""}`} onClick={() => setTab("in")}>
            Recepție (intrare)
          </div>
          <div className={`tab${tab === "out" ? " active" : ""}`} onClick={() => setTab("out")}>
            Descărcare (ieșire)
          </div>
        </div>
        <div className="field" style={{ width: 140 }}>
          <label>Data</label>
          <input className="input num" style={inputStyle} type="date" value={date} onChange={(e) => setDate(e.target.value)} />
        </div>
        <div className="field" style={{ width: 100 }}>
          <label>Cantitate</label>
          <input className="input num" style={inputStyle} inputMode="decimal" value={qty} onChange={(e) => setQty(e.target.value)} placeholder="10" />
        </div>
        {tab === "in" && (
          <div className="field" style={{ width: 120 }}>
            <label>Cost unitar (lei)</label>
            <input className="input num" style={inputStyle} inputMode="decimal" value={cost} onChange={(e) => setCost(e.target.value)} placeholder="5.00" />
          </div>
        )}
        <div className="field" style={{ width: 130 }}>
          <label>Document</label>
          <input className="input" style={inputStyle} value={docRef} onChange={(e) => setDocRef(e.target.value)} placeholder={tab === "in" ? "NIR 123" : "BC 45"} />
        </div>
        <button
          className="btn-dark"
          style={{ height: 32 }}
          disabled={mut.isPending}
          onClick={() => mut.mutate()}
        >
          <Ic name="check" />{mut.isPending ? "Se salvează…" : "Înregistrează"}
        </button>
        <span style={{ flexBasis: "100%", fontSize: 11.5, color: "var(--dim)", lineHeight: 1.45 }}>
          Evaluare stoc OMFP 1802/2014 (FIFO/CMP). Recepția intră la cost; descărcarea e evaluată
          automat. Nota contabilă (D 371 / C 607 la intrare — reclasă din cheltuiala facturii;
          D 345 / C 711 la producție; D 6xx / C 371 la ieșire) se postează în jurnal.
        </span>
      </div>

      {/* ledger */}
      <table className="scr-table">
        <thead>
          <tr>
            <th>Data</th>
            <th>Tip</th>
            <th className="r">Cant.</th>
            <th className="r">Cost unit.</th>
            <th className="r">Valoare</th>
            <th className="r">Sold cant.</th>
            <th className="r">Sold valoare</th>
          </tr>
        </thead>
        <tbody>
          {ledger.length === 0 ? (
            <tr>
              <td colSpan={7} style={{ textAlign: "center", color: "var(--text-2)", padding: "24px 16px" }}>
                Nicio mișcare de stoc pentru acest articol.
              </td>
            </tr>
          ) : (
            ledger.map((r, idx) => {
              const isIn = r.direction === "IN";
              const last = idx === ledger.length - 1;
              return (
                <tr key={r.id}>
                  <td className="num">{fmtRoDate(r.entryDate)}</td>
                  <td>
                    {isIn ? "Recepție" : "Ieșire"}
                    {r.docRef ? <> · <span className="doc">{r.docRef}</span></> : null}
                  </td>
                  <td className={`r num ${isIn ? "pos" : "neg"}`}>
                    {isIn ? "+" : "-"}{fmtQty(r.qty)}
                  </td>
                  <td className="r num">
                    {fmtRON(r.unitCost)}
                    {!isIn && method === "FIFO" && <> <span className="muted">(FIFO)</span></>}
                  </td>
                  <td className="r num">{fmtRON(r.value)}</td>
                  <td className="r num">{last ? <b>{fmtQty(r.runQty)}</b> : fmtQty(r.runQty)}</td>
                  <td className="r num">{last ? <b>{fmtRON(r.runValue)}</b> : fmtRON(r.runValue)}</td>
                </tr>
              );
            })
          )}
        </tbody>
      </table>
    </div>
  );
}

// ─── ProductModal (creare / editare articol) ──────────────────────────────────

function ProductModal({
  companyId,
  product,
  onClose,
  onSaved,
}: {
  companyId: string;
  product: Product | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const isEdit = product !== null;

  const [form, setForm] = useState<ProductInput>({
    name: product?.name ?? "",
    unit: product?.unit ?? "buc",
    unitPrice: product?.unitPrice ?? "0.00",
    vatRate: product?.vatRate ?? "21", // 2026 standard (Legea 141/2025); editing preserves the existing rate
    vatCategory: product?.vatCategory ?? "S",
    code: product?.code ?? "",
    stockQty: product?.stockQty ?? "",
    art331Code: product?.art331Code ?? "",
    active: product?.active ?? true,
  });
  const [error, setError] = useState<string | null>(null);

  const create = useMutation({
    mutationFn: (input: ProductInput) => api.products.create(companyId, input),
    onSuccess: () => {
      notify.success("Articol adăugat.");
      onSaved();
    },
    onError: (e) => setError(formatError(e, "Eroare la adăugare.")),
  });

  const updateMut = useMutation({
    mutationFn: (input: UpdateProductInput) =>
      api.products.update(product!.id, companyId, input),
    onSuccess: () => {
      notify.success("Articol salvat.");
      onSaved();
    },
    onError: (e) => setError(formatError(e, "Eroare la salvare.")),
  });

  const isPending = create.isPending || updateMut.isPending;

  const field = (key: keyof ProductInput) => ({
    value: (form[key] as string) ?? "",
    onChange: (
      e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>,
    ) => setForm((f) => ({ ...f, [key]: e.target.value })),
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (isPending) return;
    setError(null);
    if (!form.name?.trim()) {
      setError("Denumirea este obligatorie.");
      return;
    }
    const input: ProductInput = {
      ...form,
      name: form.name.trim(),
      code: form.code?.trim() || undefined,
      stockQty: (form.stockQty as string)?.trim() || undefined,
      art331Code: (form.art331Code as string)?.trim() || undefined,
      unit: form.unit || "buc",
      unitPrice: form.unitPrice || "0.00",
      vatRate: form.vatRate || "21",
      vatCategory: form.vatCategory || "S",
    };
    if (isEdit) {
      const { active, ...rest } = input;
      updateMut.mutate({ ...rest, active });
    } else {
      create.mutate(input);
    }
  };

  return (
    <div
      className="modal-back show"
      style={{ position: "fixed", zIndex: 80 }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="modal">
        <div className="modal-head">
          <div>
            <div className="mt">{isEdit ? `Editează: ${product.name}` : "Articol nou"}</div>
            <div className="ms">
              Articolele apar ca linii de factură; stocul se gestionează din fișa de magazie.
            </div>
          </div>
          <button className="modal-x" onClick={onClose} aria-label="Închide">
            <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: '<path d="M6 18 18 6M6 6l12 12"/>' }} />
          </button>
        </div>
        <form onSubmit={handleSubmit} style={{ display: "contents" }}>
          <div className="modal-body">
            <div className="fgrid">
              <div className="field span2">
                <label>Denumire <span className="req">*</span></label>
                <input
                  className={`input${error && !form.name?.trim() ? " invalid" : ""}`}
                  placeholder="ex. Servicii consultanță"
                  autoFocus
                  {...field("name")}
                />
                {error && !form.name?.trim() && (
                  <span className="err">
                    <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN }} />
                    {error}
                  </span>
                )}
              </div>
              <div className="field">
                <label>Cod articol</label>
                <input className="input num" placeholder="ex. SVC-001" {...field("code")} />
              </div>
              <div className="field">
                <label>UM</label>
                <input className="input" placeholder="buc / oră / kg…" {...field("unit")} />
              </div>
              <div className="field">
                <label>Preț unitar (fără TVA)</label>
                <input
                  className="input num"
                  type="number"
                  step="0.01"
                  min="0"
                  placeholder="0.00"
                  {...field("unitPrice")}
                />
              </div>
              <div className="field">
                <label>Cotă TVA %</label>
                <select className="select num" {...field("vatRate")}>
                  {VAT_RATES.map((r) => (
                    <option key={r} value={String(r)}>{r}%</option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label>Categorie TVA</label>
                <select className="select" {...field("vatCategory")}>
                  {VAT_CATEGORIES.map((cat) => (
                    <option key={cat} value={cat}>
                      {cat} — {VAT_CATEGORY_LABELS[cat]}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label>Stoc (cantitate)</label>
                <input
                  className="input num"
                  type="number"
                  step="0.001"
                  min="0"
                  placeholder="—"
                  {...field("stockQty")}
                />
                <span className="hint">gol = articol fără gestiune (serviciu)</span>
              </div>
              {form.vatCategory === "AE" && (
                <div className="field span2">
                  <label>Cod art. 331 (taxare inversă — D394 codPR)</label>
                  <select
                    className="select"
                    value={(form.art331Code as string) ?? ""}
                    onChange={(e) =>
                      setForm((f) => ({ ...f, art331Code: e.target.value || undefined }))
                    }
                  >
                    <option value="">— implicit 22 (Deșeuri feroase/neferoase) —</option>
                    {ART331_CODES.map((c) => (
                      <option key={c.value} value={c.value}>{c.label}</option>
                    ))}
                  </select>
                </div>
              )}
              <label
                className="span2"
                style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, cursor: "pointer", userSelect: "none" }}
              >
                <button
                  type="button"
                  className={`cbx${form.active ? " on" : ""}`}
                  onClick={() => setForm((f) => ({ ...f, active: !f.active }))}
                  aria-label="Articol activ"
                />
                Articol activ
              </label>
            </div>
            {error && form.name?.trim() && (
              <div className="banner danger" style={{ marginTop: 12, marginBottom: 0 }}>
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN }} />
                <span>{error}</span>
              </div>
            )}
          </div>
          <div className="modal-foot">
            <button type="button" className="pill-btn" onClick={onClose} disabled={isPending}>
              Anulează
            </button>
            <button type="submit" className="btn-dark" disabled={isPending}>
              <Ic name="check" />
              {isPending ? "Se salvează…" : isEdit ? "Salvează" : "Adaugă"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
