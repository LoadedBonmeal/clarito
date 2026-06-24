/**
 * NIR (Notă de Intrare Recepție) — formular 14-3-1A, OMFP 2634/2015.
 *
 * Trei view-uri:
 *   list   — tabelul tuturor NIR-urilor pentru compania activă
 *   create — formular de creare (manual sau prefill din factură primită)
 *   detail — vizualizare NIR + buton de finalizare + print
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { NirDocument, NirInput, NirLineInput, NirWithLines, Gestiune } from "@/types";

// ─── Types ────────────────────────────────────────────────────────────────────

type View = "list" | "create" | "detail";
type NirTab = "all" | "draft" | "posted";

// ─── List view ────────────────────────────────────────────────────────────────

function NirList({
  companyId,
  companyName,
  onNew,
  onView,
}: {
  companyId: string;
  companyName: string;
  onNew: () => void;
  onView: (nir: NirDocument) => void;
}) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<NirTab>("all");
  const [search, setSearch] = useState("");

  const { data: nirList = [], isLoading, error } = useQuery({
    queryKey: ["nir", companyId],
    queryFn: () => api.nir.list(companyId),
    enabled: !!companyId,
  });

  const filtered = nirList.filter((n) => {
    if (tab === "draft" && n.status !== "draft") return false;
    if (tab === "posted" && n.status !== "finalized") return false;
    if (search) {
      const q = search.toLowerCase();
      const num = `${n.nirSeries ? n.nirSeries + "-" : ""}${n.nirNumber}`.toLowerCase();
      const sup = (n.supplierName ?? "").toLowerCase();
      if (!num.includes(q) && !sup.includes(q)) return false;
    }
    return true;
  });

  const countAll = nirList.length;
  const countDraft = nirList.filter((n) => n.status === "draft").length;
  const countPosted = nirList.filter((n) => n.status === "finalized").length;

  return (
    <div className="main-inner wide">
      <div className="page-head">
        <div>
          <h1>Nota Intrare Receptie</h1>
          <p className="sub">
            NIR cod 14-3-1A · {companyName}
          </p>
        </div>
        <div className="head-actions">
          <button className="btn-dark" onClick={onNew}>
            <Ic name="plus" /> {t("nir.new")}
          </button>
        </div>
      </div>

      {error && <QueryErrorBanner error={error} label={t("nir.errorLabel")} />}

      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tabs">
            <div
              className={"tab" + (tab === "all" ? " active" : "")}
              onClick={() => setTab("all")}
            >
              Toate<span className="cnt">{countAll}</span>
            </div>
            <div
              className={"tab" + (tab === "draft" ? " active" : "")}
              onClick={() => setTab("draft")}
            >
              Draft<span className="cnt">{countDraft}</span>
            </div>
            <div
              className={"tab" + (tab === "posted" ? " active" : "")}
              onClick={() => setTab("posted")}
            >
              Postate<span className="cnt">{countPosted}</span>
            </div>
          </div>
          <div className="spacer" />
          <div className="scr-search">
            <Ic name="lens" />
            <input
              type="text"
              placeholder="Cauta dupa numar sau furnizor..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        </div>

        <table className="scr-table">
          <thead>
            <tr>
              <th style={{ width: 150 }}>Numar</th>
              <th style={{ width: 130 }}>Data</th>
              <th>Furnizor</th>
              <th className="r" style={{ width: 150 }}>Valoare</th>
              <th style={{ width: 120 }}>Status</th>
            </tr>
          </thead>
          {isLoading ? (
            <tbody>
              <tr>
                <td colSpan={5} style={{ padding: 0 }}>
                  <div className="empty">
                    <div className="ei"><Ic name="inboxIn" /></div>
                    <b>{t("nir.loading")}</b>
                  </div>
                </td>
              </tr>
            </tbody>
          ) : filtered.length === 0 ? (
            <tbody>
              <tr>
                <td colSpan={5} style={{ padding: 0 }}>
                  <div className="empty">
                    <div className="ei"><Ic name="inboxIn" /></div>
                    <b>Niciun NIR.</b>Receptionati marfa de la un furnizor.
                  </div>
                </td>
              </tr>
            </tbody>
          ) : (
            <tbody>
              {filtered.map((n) => {
                const nirLabel = `${n.nirSeries ? n.nirSeries + "-" : ""}${n.nirNumber}`;
                return (
                  <tr key={n.id} style={{ cursor: "pointer" }} onClick={() => onView(n)}>
                    <td>
                      <span className="doc">{nirLabel}</span>
                    </td>
                    <td>{n.nirDate}</td>
                    <td>{n.supplierName ?? "—"}</td>
                    <td className="r">
                      {n.retailMode && (
                        <span className="chip sent" style={{ marginRight: 6 }}>{t("nir.colRetail")}</span>
                      )}
                    </td>
                    <td>
                      <span className={`chip ${n.status === "finalized" ? "sent" : "draft"}`}>
                        {n.status === "finalized" ? t("nir.statusFinalized") : t("nir.statusDraft")}
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          )}
        </table>
      </div>
    </div>
  );
}

// ─── Create form ──────────────────────────────────────────────────────────────

const EMPTY_LINE = (): NirLineInput => ({
  denumire: "",
  qty: "1.000000",
  unitCost: "0.00",
  vatRate: "19",
  lineNo: 1,
});

function NirCreateForm({
  companyId,
  onSaved,
  onCancel,
}: {
  companyId: string;
  onSaved: (doc: NirDocument) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const qc = useQueryClient();

  const today = new Date().toISOString().slice(0, 10);

  const [nirDate, setNirDate] = useState(today);
  const [gestiuneId, setGestiuneId] = useState("");
  const [supplierName, setSupplierName] = useState("");
  const [supplierCui, setSupplierCui] = useState("");
  const [retailMode, setRetailMode] = useState(false);
  const [comisie, setComisie] = useState("");
  const [observatii, setObservatii] = useState("");
  const [lines, setLines] = useState<NirLineInput[]>([{ ...EMPTY_LINE(), lineNo: 1 }]);
  const [prefillInvId, setPrefillInvId] = useState("");
  const [linkedReceivedInvoiceId, setLinkedReceivedInvoiceId] = useState<string | undefined>(undefined);

  const { data: gestiuni = [] } = useQuery<Gestiune[]>({
    queryKey: ["gestiuni", companyId],
    queryFn: () => api.gestiuni.list(companyId),
    enabled: !!companyId,
  });

  const saveMut = useMutation({
    mutationFn: () => {
      const input: NirInput = {
        gestiuneId: gestiuneId,
        receivedInvoiceId: linkedReceivedInvoiceId,
        supplierName: supplierName || undefined,
        supplierCui: supplierCui || undefined,
        nirDate,
        retailMode,
        comisieReceptie: comisie || undefined,
        observatii: observatii || undefined,
        lines: lines.map((l, i) => ({ ...l, lineNo: i + 1 })),
      };
      return api.nir.create(companyId, input);
    },
    onSuccess: (doc) => {
      notify.success(t("nir.saved"));
      void qc.invalidateQueries({ queryKey: ["nir", companyId] });
      onSaved(doc);
    },
    onError: (e) => notify.error(formatError(e, t("nir.saveError"))),
  });

  const prefillMut = useMutation({
    mutationFn: () => api.nir.fromReceivedInvoice(companyId, prefillInvId.trim()),
    onSuccess: (input) => {
      setSupplierName(input.supplierName ?? "");
      setSupplierCui(input.supplierCui ?? "");
      if (input.nirDate) setNirDate(input.nirDate);
      // FIX 3: persist receivedInvoiceId so the GL path (standalone vs linked) is correct
      setLinkedReceivedInvoiceId(input.receivedInvoiceId ?? (prefillInvId.trim() || undefined));
      // Map prefilled lines, preserving productId if the backend returned it
      setLines(input.lines.map((l, i) => ({ ...l, lineNo: i + 1 })));
      notify.success(t("nir.saved")); // reuse for "prefilled"
    },
    onError: (e) => notify.error(formatError(e, t("nir.prefillError"))),
  });

  const updateLine = (i: number, field: keyof NirLineInput, val: string) => {
    setLines((prev) => prev.map((l, idx) => (idx === i ? { ...l, [field]: val } : l)));
  };

  const addLine = () => {
    setLines((prev) => [
      ...prev,
      { ...EMPTY_LINE(), lineNo: prev.length + 1 },
    ]);
  };

  const removeLine = (i: number) => {
    setLines((prev) => prev.filter((_, idx) => idx !== i).map((l, idx) => ({ ...l, lineNo: idx + 1 })));
  };

  return (
    <div className="main-inner wide">
      <div className="page-head">
        <div>
          <h1>{t("nir.new")}</h1>
        </div>
        <div className="head-actions">
          <button className="pill-btn" onClick={onCancel}>{t("nir.cancel")}</button>
        </div>
      </div>

      {/* Prefill from invoice */}
      <div className="scr-card" style={{ marginBottom: 16, padding: "12px 16px" }}>
        <div style={{ fontWeight: 600, marginBottom: 8 }}>{t("nir.fromInvoiceTitle")}</div>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <input
            className="form-input"
            placeholder={t("nir.fromInvoiceId")}
            value={prefillInvId}
            onChange={(e) => setPrefillInvId(e.target.value)}
            style={{ flex: 1 }}
          />
          <button
            className="pill-btn"
            onClick={() => prefillMut.mutate()}
            disabled={!prefillInvId.trim() || prefillMut.isPending}
          >
            {prefillMut.isPending ? "…" : t("nir.prefill")}
          </button>
        </div>
      </div>

      {/* Header */}
      <div className="scr-card" style={{ marginBottom: 16, padding: "16px" }}>
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 12 }}>
          <label className="form-field">
            <span>{t("nir.fieldDate")}</span>
            <input
              type="date"
              className="form-input"
              value={nirDate}
              onChange={(e) => setNirDate(e.target.value)}
            />
          </label>

          <label className="form-field">
            <span>{t("nir.fieldGestiune")}</span>
            <select
              className="form-input"
              value={gestiuneId}
              onChange={(e) => setGestiuneId(e.target.value)}
            >
              <option value="">{t("nir.gestiuneSelect")}</option>
              {gestiuni.map((g) => (
                <option key={g.id} value={g.id}>
                  {g.cod} — {g.denumire}
                </option>
              ))}
            </select>
          </label>

          <label className="form-field" style={{ display: "flex", alignItems: "center", gap: 8, paddingTop: 18 }}>
            <input
              type="checkbox"
              checked={retailMode}
              onChange={(e) => setRetailMode(e.target.checked)}
            />
            <span>{t("nir.fieldRetailMode")}</span>
          </label>

          <label className="form-field">
            <span>{t("nir.fieldSupplier")}</span>
            <input
              className="form-input"
              value={supplierName}
              onChange={(e) => setSupplierName(e.target.value)}
            />
          </label>

          <label className="form-field">
            <span>{t("nir.fieldCui")}</span>
            <input
              className="form-input"
              value={supplierCui}
              onChange={(e) => setSupplierCui(e.target.value)}
            />
          </label>

          <label className="form-field">
            <span>{t("nir.fieldComisie")}</span>
            <input
              className="form-input"
              value={comisie}
              onChange={(e) => setComisie(e.target.value)}
            />
          </label>

          <label className="form-field" style={{ gridColumn: "1 / -1" }}>
            <span>{t("nir.fieldObservatii")}</span>
            <textarea
              className="form-input"
              value={observatii}
              onChange={(e) => setObservatii(e.target.value)}
              rows={2}
            />
          </label>
        </div>

        {retailMode && (
          <div className="chip late" style={{ marginTop: 8, display: "inline-block" }}>
            {t("nir.retailWarning")}
          </div>
        )}
      </div>

      {/* Lines table */}
      <div className="scr-card" style={{ marginBottom: 16, overflowX: "auto" }}>
        <table className="scr-table">
          <thead>
            <tr>
              <th style={{ width: 40 }}>{t("nir.lineNo")}</th>
              <th>{t("nir.lineDenumire")}</th>
              <th style={{ width: 60 }}>{t("nir.lineUm")}</th>
              <th style={{ width: 90 }}>{t("nir.lineQtyRec")}</th>
              <th style={{ width: 90 }}>{t("nir.lineUnitCost")}</th>
              <th style={{ width: 70 }}>{t("nir.lineVatRate")}</th>
              {retailMode && <th style={{ width: 70 }}>{t("nir.lineAdaosPct")}</th>}
              <th style={{ width: 40 }}></th>
            </tr>
          </thead>
          <tbody>
            {lines.map((ln, i) => (
              <tr key={i}>
                <td style={{ textAlign: "center", color: "var(--text-2)" }}>{i + 1}</td>
                <td>
                  <input
                    className="form-input"
                    value={ln.denumire}
                    onChange={(e) => updateLine(i, "denumire", e.target.value)}
                    style={{ width: "100%", minWidth: 160 }}
                  />
                </td>
                <td>
                  <input
                    className="form-input"
                    value={ln.um ?? ""}
                    onChange={(e) => updateLine(i, "um", e.target.value)}
                    style={{ width: 60 }}
                  />
                </td>
                <td>
                  <input
                    className="form-input"
                    value={ln.qty}
                    onChange={(e) => updateLine(i, "qty", e.target.value)}
                    style={{ width: 85, textAlign: "right" }}
                  />
                </td>
                <td>
                  <input
                    className="form-input"
                    value={ln.unitCost}
                    onChange={(e) => updateLine(i, "unitCost", e.target.value)}
                    style={{ width: 85, textAlign: "right" }}
                  />
                </td>
                <td>
                  <input
                    className="form-input"
                    value={ln.vatRate}
                    onChange={(e) => updateLine(i, "vatRate", e.target.value)}
                    style={{ width: 60, textAlign: "right" }}
                  />
                </td>
                {retailMode && (
                  <td>
                    <input
                      className="form-input"
                      value={ln.adaosPct ?? ""}
                      onChange={(e) => updateLine(i, "adaosPct", e.target.value)}
                      style={{ width: 60, textAlign: "right" }}
                    />
                  </td>
                )}
                <td>
                  <button
                    className="pill-btn"
                    onClick={() => removeLine(i)}
                    disabled={lines.length === 1}
                    title={t("nir.removeLine")}
                    style={{ padding: "2px 8px" }}
                  >
                    ✕
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <div style={{ padding: "8px 12px" }}>
          <button className="pill-btn" onClick={addLine}>
            + {t("nir.addLine")}
          </button>
        </div>
      </div>

      {/* Save */}
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
        <button className="pill-btn" onClick={onCancel}>{t("nir.cancel")}</button>
        <button
          className="btn-dark"
          onClick={() => saveMut.mutate()}
          disabled={saveMut.isPending || !gestiuneId}
        >
          {saveMut.isPending ? t("nir.saving") : t("nir.save")}
        </button>
      </div>
    </div>
  );
}

// ─── Detail / print view ──────────────────────────────────────────────────────

function NirDetail({
  companyId,
  nirId,
  onBack,
}: {
  companyId: string;
  nirId: string;
  onBack: () => void;
}) {
  const { t } = useTranslation();
  const qc = useQueryClient();

  const { data, isLoading, error } = useQuery<NirWithLines>({
    queryKey: ["nir", companyId, nirId],
    queryFn: () => api.nir.get(companyId, nirId),
    enabled: !!companyId && !!nirId,
  });

  const finalizeMut = useMutation({
    mutationFn: () => api.nir.finalize(companyId, nirId),
    onSuccess: () => {
      notify.success(t("nir.finalized"));
      void qc.invalidateQueries({ queryKey: ["nir", companyId] });
      void qc.invalidateQueries({ queryKey: ["nir", companyId, nirId] });
    },
    onError: (e) => notify.error(formatError(e, t("nir.finalizeError"))),
  });

  const handlePrint = () => {
    window.print();
  };

  if (isLoading) {
    return (
      <div className="main-inner wide">
        <div className="state-row">{t("nir.loading")}</div>
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="main-inner wide">
        <QueryErrorBanner error={error} label={t("nir.errorLabel")} />
        <button className="pill-btn" onClick={onBack}>{t("nir.backToList")}</button>
      </div>
    );
  }

  const { document: doc, lines } = data;
  const isDraft = doc.status === "draft";
  const nirLabel = `${doc.nirSeries ? doc.nirSeries + "-" : ""}${doc.nirNumber}`;

  // Compute totals
  const totalCost = lines.reduce((s, l) => s + parseFloat(l.valueCost || "0"), 0);
  const totalAdaos = lines.reduce((s, l) => s + parseFloat(l.valueAdaos || "0"), 0);
  const totalTvaNeex = lines.reduce((s, l) => s + parseFloat(l.valueTvaNeex || "0"), 0);
  const totalPret = lines.reduce((s, l) => s + parseFloat(l.pretAmanunt || "0"), 0);

  return (
    <div className="main-inner wide">
      <div className="page-head">
        <div>
          <h1>{t("nir.printTitle", { number: nirLabel })}</h1>
          <p className="sub">Formular 14-3-1A (OMFP 2634/2015) · {doc.nirDate}</p>
        </div>
        <div className="head-actions">
          <button className="pill-btn" onClick={onBack}>{t("nir.backToList")}</button>
          {isDraft && (
            <button
              className="btn-dark"
              onClick={() => {
                if (window.confirm(t("nir.confirmFinalize"))) finalizeMut.mutate();
              }}
              disabled={finalizeMut.isPending}
            >
              {finalizeMut.isPending ? t("nir.finalizing") : t("nir.finalize")}
            </button>
          )}
          <button className="pill-btn" onClick={handlePrint}>
            <Ic name="print" /> {t("nir.print")}
          </button>
        </div>
      </div>

      {/* Formular 14-3-1A layout */}
      <div className="scr-card nir-print-area" style={{ padding: 24 }}>
        {/* Meta */}
        <table style={{ width: "100%", borderCollapse: "collapse", marginBottom: 16, fontSize: 13 }}>
          <tbody>
            <tr>
              <td style={{ padding: "4px 8px", fontWeight: 600, width: 180 }}>{t("nir.printSupplier")}</td>
              <td style={{ padding: "4px 8px" }}>
                {doc.supplierName ?? "—"}
                {doc.supplierCui ? ` (${doc.supplierCui})` : ""}
              </td>
              <td style={{ padding: "4px 8px", fontWeight: 600, width: 180 }}>{t("nir.fieldGestiune")}</td>
              <td style={{ padding: "4px 8px" }}>{doc.gestiuneId}</td>
            </tr>
            {doc.receivedInvoiceId && (
              <tr>
                <td style={{ padding: "4px 8px", fontWeight: 600 }}>{t("nir.printFactura")}</td>
                <td style={{ padding: "4px 8px" }} colSpan={3}>{doc.receivedInvoiceId}</td>
              </tr>
            )}
            {doc.comisieReceptie && (
              <tr>
                <td style={{ padding: "4px 8px", fontWeight: 600 }}>{t("nir.printComisie")}</td>
                <td style={{ padding: "4px 8px" }} colSpan={3}>{doc.comisieReceptie}</td>
              </tr>
            )}
            <tr>
              <td style={{ padding: "4px 8px", fontWeight: 600 }}>Status</td>
              <td style={{ padding: "4px 8px" }}>
                <span className={`chip ${doc.status === "finalized" ? "sent" : "draft"}`}>
                  {doc.status === "finalized" ? t("nir.statusFinalized") : t("nir.statusDraft")}
                </span>
                {doc.retailMode && (
                  <span className="chip sent" style={{ marginLeft: 6 }}>{t("nir.colRetail")}</span>
                )}
              </td>
            </tr>
          </tbody>
        </table>

        {/* Lines table */}
        <div style={{ overflowX: "auto" }}>
          <table className="scr-table" style={{ fontSize: 13 }}>
            <thead>
              <tr>
                <th style={{ width: 36 }}>{t("nir.lineNo")}</th>
                <th>{t("nir.lineDenumire")}</th>
                <th style={{ width: 50 }}>{t("nir.lineUm")}</th>
                <th style={{ width: 80, textAlign: "right" }}>{t("nir.lineQtyRec")}</th>
                <th style={{ width: 80, textAlign: "right" }}>{t("nir.lineUnitCost")}</th>
                <th style={{ width: 80, textAlign: "right" }}>{t("nir.lineValueCost")}</th>
                <th style={{ width: 60, textAlign: "right" }}>{t("nir.lineVatRate")}</th>
                {doc.retailMode && (
                  <>
                    <th style={{ width: 70, textAlign: "right" }}>{t("nir.lineAdaosPct")}</th>
                    <th style={{ width: 80, textAlign: "right" }}>{t("nir.lineValueAdaos")}</th>
                    <th style={{ width: 80, textAlign: "right" }}>{t("nir.lineTvaNeex")}</th>
                    <th style={{ width: 90, textAlign: "right" }}>{t("nir.linePretAmanunt")}</th>
                  </>
                )}
              </tr>
            </thead>
            <tbody>
              {lines.map((ln) => (
                <tr key={ln.id}>
                  <td style={{ textAlign: "center" }}>{ln.lineNo}</td>
                  <td>
                    {ln.denumire}
                    {ln.productId && (
                      <span style={{ fontSize: 11, color: "var(--text-2)", marginLeft: 6 }}>
                        #{ln.productId.slice(0, 8)}
                      </span>
                    )}
                  </td>
                  <td>{ln.um ?? ""}</td>
                  <td style={{ textAlign: "right" }}>{parseFloat(ln.qty).toFixed(4)}</td>
                  <td style={{ textAlign: "right" }}>{parseFloat(ln.unitCost).toFixed(2)}</td>
                  <td style={{ textAlign: "right" }}>{parseFloat(ln.valueCost).toFixed(2)}</td>
                  <td style={{ textAlign: "right" }}>{ln.vatRate}%</td>
                  {doc.retailMode && (
                    <>
                      <td style={{ textAlign: "right" }}>{ln.adaosPct ? `${ln.adaosPct}%` : "—"}</td>
                      <td style={{ textAlign: "right" }}>{parseFloat(ln.valueAdaos).toFixed(2)}</td>
                      <td style={{ textAlign: "right" }}>{parseFloat(ln.valueTvaNeex).toFixed(2)}</td>
                      <td style={{ textAlign: "right" }}>{parseFloat(ln.pretAmanunt).toFixed(2)}</td>
                    </>
                  )}
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr style={{ fontWeight: 700 }}>
                <td colSpan={5} style={{ textAlign: "right", paddingRight: 12 }}>TOTAL</td>
                <td style={{ textAlign: "right" }}>{totalCost.toFixed(2)}</td>
                <td></td>
                {doc.retailMode && (
                  <>
                    <td></td>
                    <td style={{ textAlign: "right" }}>{totalAdaos.toFixed(2)}</td>
                    <td style={{ textAlign: "right" }}>{totalTvaNeex.toFixed(2)}</td>
                    <td style={{ textAlign: "right" }}>{totalPret.toFixed(2)}</td>
                  </>
                )}
              </tr>
            </tfoot>
          </table>
        </div>

        {/* Signatures */}
        <div style={{ display: "flex", justifyContent: "space-between", marginTop: 32, fontSize: 13 }}>
          <div style={{ textAlign: "center", minWidth: 160 }}>
            <div style={{ borderTop: "1px solid var(--border)", paddingTop: 4 }}>
              {t("nir.printComisie")}
            </div>
            {doc.comisieReceptie && (
              <div style={{ marginTop: 4, color: "var(--text-2)" }}>{doc.comisieReceptie}</div>
            )}
          </div>
          <div style={{ textAlign: "center", minWidth: 160 }}>
            <div style={{ borderTop: "1px solid var(--border)", paddingTop: 4 }}>
              {t("nir.printGestionar")}
            </div>
          </div>
          <div style={{ textAlign: "center", minWidth: 160 }}>
            <div style={{ borderTop: "1px solid var(--border)", paddingTop: 4 }}>
              {t("nir.printSemnatura")}
            </div>
            <div style={{ marginTop: 4, color: "var(--text-2)" }}>
              {t("nir.printData")}: {doc.nirDate}
            </div>
          </div>
        </div>

        {doc.observatii && (
          <div style={{ marginTop: 16, fontSize: 13 }}>
            <strong>{t("nir.fieldObservatii")}:</strong> {doc.observatii}
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Page root ────────────────────────────────────────────────────────────────

export function NirPage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const [view, setView] = useState<View>("list");
  const [selectedNirId, setSelectedNirId] = useState<string | null>(null);

  const { data: companies = [] } = useQuery({
    queryKey: ["companies"],
    queryFn: () => api.companies.list(),
  });

  const activeCompany = companies.find((c) => c.id === activeCompanyId) ?? companies[0];
  const companyName = activeCompany?.legalName ?? "";

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head">
          <div>
            <h1>Nota Intrare Receptie</h1>
          </div>
        </div>
        <div className="scr-card" style={{ padding: 24, color: "var(--text-2)" }}>
          {t("nir.selectCompany")}
        </div>
      </div>
    );
  }

  if (view === "create") {
    return (
      <NirCreateForm
        companyId={activeCompanyId}
        onSaved={(doc) => {
          setSelectedNirId(doc.id);
          setView("detail");
        }}
        onCancel={() => setView("list")}
      />
    );
  }

  if (view === "detail" && selectedNirId) {
    return (
      <NirDetail
        companyId={activeCompanyId}
        nirId={selectedNirId}
        onBack={() => setView("list")}
      />
    );
  }

  return (
    <NirList
      companyId={activeCompanyId}
      companyName={companyName}
      onNew={() => setView("create")}
      onView={(n) => {
        setSelectedNirId(n.id);
        setView("detail");
      }}
    />
  );
}
