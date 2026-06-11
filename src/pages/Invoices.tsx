/**
 * Facturi emise — verbatim port of the design "Facturi emise.html":
 *   .page-head (title + count sub + btn-dark "Factură nouă" ⌘N)
 *   .scr-card → .scr-toolbar (.scr-search · .tabs status counts · period pill ·
 *   Filtre pill · refresh sq-btn · Export/Import pop) → .bulkbar → .scr-table
 *   (cbx · doc · date · client · net/TVA/total · monedă · status/plată chips ·
 *   .row-acts eye + "··· " pop) → .tot-foot totals.
 *
 * ALL wiring preserved: api.invoices.list, api.contacts.list,
 * api.anaf.submitInvoice/checkStatus, api.ubl.generatePdf/generateXml,
 * api.invoices.duplicate/storno, api.payments.listSummaries,
 * api.integrations.exportSagaCsv/exportInvoicesXlsx,
 * api.importData.invoiceXmlFromFile, CsvImportModal, bulk submit/print,
 * ?view=storned deep-link.
 */

import { useMemo, useState, useEffect } from "react";
import { createPortal } from "react-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { CsvImportModal } from "@/components/shared/CsvImportModal";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { formatOptionalRon } from "@/lib/formatters";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { InvoiceStatus } from "@/types";

type StatusFilter = InvoiceStatus | "all";
type PeriodFilter = string | "all";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

function fmtMonth(ym: string): string {
  const [year, month] = ym.split("-");
  const d = new Date(Number(year), Number(month) - 1, 1);
  return d.toLocaleDateString("ro-RO", { month: "long", year: "numeric" });
}

/** Render at most this many rows (plain table, no virtualizer — design parity). */
const MAX_ROWS = 1000;

// Status → design chip (.chip variants + icon + label).
const STATUS_CHIP: Record<InvoiceStatus, { cls: string; icon: string; label: string }> = {
  DRAFT:     { cls: "sent", icon: "docText", label: "Schiță" },
  QUEUED:    { cls: "wait", icon: "clock",   label: "În coadă" },
  SUBMITTED: { cls: "sent", icon: "send",    label: "Trimisă" },
  VALIDATED: { cls: "paid", icon: "check",   label: "Validată" },
  REJECTED:  { cls: "late", icon: "xMark",   label: "Respinsă" },
  STORNED:   { cls: "wait", icon: "undo",    label: "Stornată" },
};

// ── RowMenu — design .pop with .pop-item rows (portal-anchored) ───────────────

interface RowMenuProps {
  invoiceId: string;
  companyId: string;
  status: InvoiceStatus;
  hasXml: boolean;
  onClose: () => void;
  anchor: DOMRect | null;
}

function RowMenu({ invoiceId, companyId, status, hasXml, onClose, anchor }: RowMenuProps) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [stornoOpen, setStornoOpen] = useState(false);
  const [stornoReason, setStornoReason] = useState("");
  const { data: testModeSetting } = useQuery({
    queryKey: queryKeys.anaf.testMode,
    queryFn: () => api.settings.get("use_anaf_test_env"),
  });
  const testMode = testModeSetting === "1";

  useEffect(() => {
    const h = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest(".row-menu-pop")) onClose();
    };
    const tid = setTimeout(() => document.addEventListener("click", h), 0);
    window.addEventListener("scroll", onClose, true);
    return () => {
      clearTimeout(tid);
      document.removeEventListener("click", h);
      window.removeEventListener("scroll", onClose, true);
    };
  }, [onClose]);

  const portalPos = (width: number): React.CSSProperties => {
    const GAP = 4;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    if (!anchor) return { position: "fixed", top: 64, right: 16, zIndex: 100, width };
    const left = Math.min(Math.max(8, anchor.right - width), vw - width - 8);
    const openUp = anchor.bottom > vh - 340;
    return {
      position: "fixed",
      left,
      ...(openUp ? { bottom: vh - anchor.top + GAP } : { top: anchor.bottom + GAP }),
      zIndex: 100,
      width,
      maxHeight: "min(360px, calc(100vh - 24px))",
      overflowY: "auto",
    };
  };

  async function handleSubmit() {
    try {
      await api.anaf.submitInvoice(companyId, invoiceId, testMode);
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success("Factură trimisă la ANAF.");
    } catch (e) {
      notify.error(formatError(e, "Eroare trimitere ANAF."));
    }
    onClose();
  }

  async function handlePdf() {
    try {
      const { openPath } = await import("@tauri-apps/plugin-opener");
      const path = await api.ubl.generatePdf(invoiceId, companyId);
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      if (path) await openPath(path);
      notify.success("PDF generat.");
    } catch (e) {
      notify.error(formatError(e, "Eroare generare PDF."));
    }
    onClose();
  }

  async function handleXml() {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({ filters: [{ name: "XML", extensions: ["xml"] }], defaultPath: `${invoiceId}.xml` });
      if (!path) { onClose(); return; }
      await api.ubl.generateXml(invoiceId, companyId);
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success(`XML salvat: ${path}`);
    } catch (e) {
      notify.error(formatError(e, "Eroare generare XML."));
    }
    onClose();
  }

  async function handleDuplicate() {
    try {
      const newId = await api.invoices.duplicate(invoiceId, companyId);
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success("Factură duplicată.");
      void navigate({ to: "/invoices/$id", params: { id: newId } });
    } catch (e) {
      notify.error(formatError(e, "Eroare duplicare."));
    }
    onClose();
  }

  async function handleCheckStatus() {
    try {
      const stare = await api.anaf.checkStatus(companyId, invoiceId, testMode);
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success(`Status ANAF: ${stare}`);
    } catch (e) {
      notify.error(formatError(e, "Eroare verificare status."));
    }
    onClose();
  }

  async function handleStorno() {
    if (!stornoReason.trim()) { notify.warn("Introduceți motivul stornării."); return; }
    try {
      const stornoInv = await api.invoices.storno(invoiceId, companyId, stornoReason.trim());
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success(`Factură storno creată: ${stornoInv.fullNumber}`);
      void navigate({ to: "/invoices/$id", params: { id: stornoInv.id } });
    } catch (e) {
      notify.error(formatError(e, "Eroare stornare."));
    }
    setStornoOpen(false);
    onClose();
  }

  if (stornoOpen) {
    return createPortal(
      <div
        className="row-menu-pop pop show"
        style={{ ...portalPos(280), padding: 12 }}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 8, color: "var(--red)" }}>
          Stornare factură
        </div>
        <textarea
          value={stornoReason}
          onChange={(e) => setStornoReason(e.target.value)}
          placeholder="Motivul stornării…"
          style={{
            width: "100%", minHeight: 56, marginBottom: 8, padding: "8px 10px",
            border: "1px solid var(--line)", borderRadius: 8, font: "inherit",
            fontSize: 12.5, resize: "vertical",
          }}
          autoFocus
        />
        <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
          <button className="pill-btn" onClick={() => { setStornoOpen(false); onClose(); }}>
            Anulează
          </button>
          <button
            className="btn-dark"
            style={{ height: 34, opacity: stornoReason.trim() ? 1 : 0.5 }}
            disabled={!stornoReason.trim()}
            onClick={handleStorno}
          >
            Stornează
          </button>
        </div>
      </div>,
      document.body,
    );
  }

  const items: Array<{ icon: string; label: string; danger?: boolean; action: () => void; show: boolean }> = [
    { icon: "eye", label: "Vizualizează", action: () => { void navigate({ to: "/invoices/$id", params: { id: invoiceId } }); onClose(); }, show: true },
    { icon: "pen", label: "Editează", action: () => { void navigate({ to: "/invoices/$id/edit", params: { id: invoiceId } }); onClose(); }, show: status === "DRAFT" },
    { icon: "send", label: "Trimite la ANAF", action: handleSubmit, show: (status === "DRAFT" || status === "VALIDATED") && hasXml },
    { icon: "dl", label: "Descarcă PDF", action: handlePdf, show: true },
    { icon: "code", label: "Descarcă XML (UBL)", action: handleXml, show: true },
    { icon: "undo", label: "Storno", danger: true, action: () => setStornoOpen(true), show: status === "VALIDATED" },
    { icon: "copy", label: "Duplică", action: handleDuplicate, show: true },
    { icon: "sync", label: "Verifică status ANAF", action: handleCheckStatus, show: status === "SUBMITTED" || status === "QUEUED" },
  ];

  return createPortal(
    <div
      className="row-menu-pop pop show"
      style={portalPos(210)}
      onClick={(e) => e.stopPropagation()}
    >
      {items.filter((i) => i.show).map((item) => (
        <button
          key={item.label}
          type="button"
          className={`pop-item${item.danger ? " danger" : ""}`}
          onClick={item.action}
        >
          <Ic name={item.icon} />
          {item.label}
        </button>
      ))}
    </div>,
    document.body,
  );
}

// ── InvoicesPage ──────────────────────────────────────────────────────────────

export function InvoicesPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const setSelectedInvoiceId = useAppStore((s) => s.setSelectedInvoiceId);

  // ?view=storned deep-link
  const { view: viewParam } = useSearch({ from: "/invoices" });
  const [filter, setFilter] = useState<StatusFilter>(viewParam === "storned" ? "STORNED" : "all");
  useEffect(() => {
    if (viewParam === "storned") setFilter("STORNED");
  }, [viewParam]);

  const [query, setQuery] = useState("");
  const [period, setPeriod] = useState<PeriodFilter>("all");
  const [errorsOnly, setErrorsOnly] = useState(false);
  const [amountMin, setAmountMin] = useState("");
  const [amountMax, setAmountMax] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [showImportModal, setShowImportModal] = useState(false);
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const [menuAnchor, setMenuAnchor] = useState<DOMRect | null>(null);
  const [openPop, setOpenPop] = useState<"" | "period" | "filters" | "export">("");

  // Close toolbar pops on outside click
  useEffect(() => {
    if (!openPop) return;
    const h = () => setOpenPop("");
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [openPop]);

  const { data: paged, isLoading, isError: pagedError, error: pagedErr, refetch: refetchPaged } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    queryFn: () => api.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    enabled: !!activeCompanyId,
  });

  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });
  const activeCompany = companies.find((c) => c.id === activeCompanyId);

  const contactMap = useMemo(() => {
    const m = new Map<string, { legalName: string; cui: string | null }>();
    for (const c of contacts) m.set(c.id, { legalName: c.legalName, cui: c.cui });
    return m;
  }, [contacts]);

  const { data: paymentSummaries = [] } = useQuery({
    queryKey: ["payments", "summaries", activeCompanyId ?? ""],
    queryFn: () => api.payments.listSummaries(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const paymentStatusMap = useMemo(() => {
    const m = new Map<string, "UNPAID" | "PARTIAL" | "PAID">();
    for (const s of paymentSummaries) m.set(s.invoiceId, s.paymentStatus);
    return m;
  }, [paymentSummaries]);

  const allInvoices = paged?.items ?? [];
  const totalCount = paged?.total ?? 0;

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    const minVal = amountMin.trim() ? parseFloat(amountMin) : null;
    const maxVal = amountMax.trim() ? parseFloat(amountMax) : null;
    return allInvoices
      .filter((i) => {
        if (errorsOnly) return i.status === "REJECTED";
        return filter === "all" || i.status === filter;
      })
      .filter((i) => period === "all" || i.issueDate.slice(0, 7) === period)
      .filter((i) => {
        if (minVal !== null && !isNaN(minVal) && parseDec(i.totalAmount) < minVal) return false;
        if (maxVal !== null && !isNaN(maxVal) && parseDec(i.totalAmount) > maxVal) return false;
        return true;
      })
      .filter((i) => {
        if (!q) return true;
        const client = contactMap.get(i.contactId);
        return (
          i.fullNumber.toLowerCase().includes(q) ||
          (client?.legalName.toLowerCase().includes(q) ?? false) ||
          (client?.cui?.toLowerCase().includes(q) ?? false)
        );
      });
  }, [allInvoices, filter, period, errorsOnly, amountMin, amountMax, query, contactMap]);

  const counts = {
    VALIDATED: allInvoices.filter((i) => i.status === "VALIDATED").length,
    SUBMITTED: allInvoices.filter((i) => i.status === "SUBMITTED").length,
    REJECTED:  allInvoices.filter((i) => i.status === "REJECTED").length,
    DRAFT:     allInvoices.filter((i) => i.status === "DRAFT").length,
    QUEUED:    allInvoices.filter((i) => i.status === "QUEUED").length,
    STORNED:   allInvoices.filter((i) => i.status === "STORNED").length,
  };

  const ronList = list.filter((i) => i.currency === "RON");
  const nonRonCount = list.length - ronList.length;
  const totNet   = ronList.reduce((s, i) => s + parseDec(i.subtotalAmount), 0);
  const totVat   = ronList.reduce((s, i) => s + parseDec(i.vatAmount), 0);
  const totTotal = ronList.reduce((s, i) => s + parseDec(i.totalAmount), 0);

  const activeFilterCount = (errorsOnly ? 1 : 0) + (amountMin ? 1 : 0) + (amountMax ? 1 : 0);

  const toggleOne = (id: string) => {
    const next = new Set(selected);
    next.has(id) ? next.delete(id) : next.add(id);
    setSelected(next);
  };

  async function handleBulkSubmit() {
    if (!activeCompanyId) return;
    const tms = await api.settings.get("use_anaf_test_env");
    const testMode = tms === "1";
    const ids = Array.from(selected);
    let ok = 0; const errs: string[] = [];
    for (const id of ids) {
      try { await api.anaf.submitInvoice(activeCompanyId, id, testMode); ok++; }
      catch (e) { errs.push(formatError(e, "Trimitere eșuată.")); }
    }
    void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
    setSelected(new Set());
    if (errs.length) notify.error(`${ok} trimise, ${errs.length} erori: ${errs.slice(0, 3).join("; ")}`);
    else notify.success(`${ok} facturi trimise la ANAF`);
  }

  async function handleExportSaga() {
    if (!activeCompanyId) return;
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({ filters: [{ name: "CSV", extensions: ["csv"] }], defaultPath: "facturi-saga.csv" });
    if (!path) return;
    const today = new Date().toISOString().slice(0, 10);
    const yearStart = `${new Date().getFullYear()}-01-01`;
    try {
      await api.integrations.exportSagaCsv(activeCompanyId, yearStart, today, path);
      notify.success(`Export SAGA salvat: ${path}`);
    } catch (e) {
      notify.error(formatError(e, "Eroare export SAGA."));
    }
  }

  async function handleExportXlsx() {
    if (!activeCompanyId) { notify.warn("Selectați o companie."); return; }
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({ filters: [{ name: "Excel", extensions: ["xlsx"] }], defaultPath: "facturi.xlsx" });
    if (!path) return;
    try {
      await api.integrations.exportInvoicesXlsx({ companyId: activeCompanyId }, path);
      notify.success(`Export salvat: ${path}`);
    } catch (e) {
      notify.error(formatError(e, "Eroare export XLSX."));
    }
  }

  async function handleImportXml() {
    if (!activeCompanyId) { notify.warn("Selectați o companie."); return; }
    const { open } = await import("@tauri-apps/plugin-dialog");
    const filePath = await open({ filters: [{ name: "XML e-Factura", extensions: ["xml"] }] });
    if (!filePath || typeof filePath !== "string") return;
    try {
      const result = await api.importData.invoiceXmlFromFile(filePath, activeCompanyId);
      if (result.imported > 0) {
        notify.success(
          `Factură importată: ${result.invoiceNumber ?? "?"} — ${result.supplierName ?? "?"} · ${formatOptionalRon(result.totalAmount)}`,
        );
        void queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      } else {
        notify.error(`Import eșuat: ${result.errors.join("; ")}`);
      }
    } catch (e) {
      notify.error(formatError(e, "Eroare import XML."));
    }
  }

  const availableMonths = useMemo(() => {
    const months = new Set<string>();
    for (const inv of allInvoices) months.add(inv.issueDate.slice(0, 7));
    return Array.from(months).filter(Boolean).sort((a, b) => b.localeCompare(a));
  }, [allInvoices]);

  const tabs: Array<{ value: StatusFilter; label: string; count: number }> = [
    { value: "all",       label: "Toate",    count: totalCount },
    { value: "VALIDATED", label: "Validate", count: counts.VALIDATED },
    { value: "SUBMITTED", label: "Trimise",  count: counts.SUBMITTED },
    { value: "QUEUED",    label: "În coadă", count: counts.QUEUED },
    { value: "REJECTED",  label: "Respinse", count: counts.REJECTED },
    { value: "DRAFT",     label: "Schițe",   count: counts.DRAFT },
    { value: "STORNED",   label: "Stornate", count: counts.STORNED },
  ];

  const visibleRows = list.slice(0, MAX_ROWS);

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>Facturi emise</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Selectați o companie activă pentru a vedea facturile emise.
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Facturi emise</h1>
          <p className="sub">
            {list.length !== totalCount
              ? `${list.length} din ${totalCount.toLocaleString("ro-RO")} facturi`
              : `${totalCount.toLocaleString("ro-RO")} facturi`}
            {activeCompany ? ` · ${activeCompany.legalName}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="btn-dark" onClick={() => void navigate({ to: "/invoices/new" })}>
            <Ic name="plus" />Factură nouă
            <span className="kbd" style={{ background: "rgba(255,255,255,.15)", borderColor: "rgba(255,255,255,.3)", color: "#fff" }}>⌘ N</span>
          </button>
        </div>
      </div>

      <div className="scr-card">
        {/* toolbar */}
        <div className="scr-toolbar">
          <div className="scr-search">
            <Ic name="lens" />
            <input
              type="text"
              placeholder="Caută după număr sau client…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <div className="tabs">
            {tabs.map((t) => (
              <div
                key={t.value}
                className={`tab${filter === t.value && !errorsOnly ? " active" : ""}`}
                onClick={() => { setFilter(t.value); setErrorsOnly(false); }}
              >
                {t.label}<span className="cnt num">{t.count}</span>
              </div>
            ))}
          </div>
          <div className="spacer" />

          {/* period pill */}
          <div className="nou-wrap" style={{ position: "relative" }}>
            <button
              className="pill-btn"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => setOpenPop(openPop === "period" ? "" : "period")}
            >
              <Ic name="calendar" />
              {period === "all" ? "Toate lunile" : fmtMonth(period)}
              <Ic name="chevD" cls="ic" />
            </button>
            {openPop === "period" && (
              <div className="pop show" style={{ right: 0, top: 40, width: 210, maxHeight: 300, overflowY: "auto" }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="col-title">Perioadă</div>
                <button className="pop-item" onClick={() => { setPeriod("all"); setOpenPop(""); }}>
                  <span style={{ flex: 1 }}>Toate lunile</span>
                  {period === "all" && <Ic name="check" cls="co-check" />}
                </button>
                {availableMonths.map((ym) => (
                  <button key={ym} className="pop-item" onClick={() => { setPeriod(ym); setOpenPop(""); }}>
                    <span style={{ flex: 1 }}>{fmtMonth(ym)}</span>
                    {period === ym && <Ic name="check" cls="co-check" />}
                  </button>
                ))}
              </div>
            )}
          </div>

          {/* filters pill */}
          <div className="nou-wrap" style={{ position: "relative" }}>
            <button
              className="pill-btn"
              style={activeFilterCount > 0 ? { borderColor: "var(--black)", color: "var(--text)" } : undefined}
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => setOpenPop(openPop === "filters" ? "" : "filters")}
            >
              <Ic name="funnel" />Filtre
              {activeFilterCount > 0 && (
                <span className="pill-new" style={{ background: "var(--black)" }}>{activeFilterCount}</span>
              )}
            </button>
            {openPop === "filters" && (
              <div className="pop show" style={{ right: 0, top: 40, width: 250, padding: 10 }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="col-title">Filtre avansate</div>
                <label
                  style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, cursor: "pointer", userSelect: "none", padding: "6px 10px 10px", color: errorsOnly ? "var(--red)" : "var(--text)" }}
                >
                  <button
                    className={`cbx${errorsOnly ? " on" : ""}`}
                    onClick={(e) => { e.preventDefault(); setErrorsOnly(!errorsOnly); if (!errorsOnly) setFilter("all"); }}
                  />
                  Cu erori (respinse ANAF)
                </label>
                <div style={{ fontSize: 12, color: "var(--text-2)", padding: "0 10px 6px" }}>Total factură (RON)</div>
                <div style={{ display: "flex", gap: 6, alignItems: "center", padding: "0 10px 8px" }}>
                  <input
                    type="number" placeholder="Min" value={amountMin}
                    onChange={(e) => setAmountMin(e.target.value)}
                    style={{ flex: 1, height: 30, fontSize: 12, padding: "0 8px", border: "1px solid var(--line)", borderRadius: 8, fontFamily: "var(--mono)" }}
                  />
                  <span style={{ fontSize: 11, color: "var(--dim)" }}>–</span>
                  <input
                    type="number" placeholder="Max" value={amountMax}
                    onChange={(e) => setAmountMax(e.target.value)}
                    style={{ flex: 1, height: 30, fontSize: 12, padding: "0 8px", border: "1px solid var(--line)", borderRadius: 8, fontFamily: "var(--mono)" }}
                  />
                </div>
                {activeFilterCount > 0 && (
                  <button
                    className="pill-btn"
                    style={{ margin: "0 10px 8px", width: "calc(100% - 20px)", justifyContent: "center" }}
                    onClick={() => { setErrorsOnly(false); setAmountMin(""); setAmountMax(""); }}
                  >
                    Resetează filtrele
                  </button>
                )}
              </div>
            )}
          </div>

          {/* refresh */}
          <button className="sq-btn spin-btn" title="Reîncarcă" onClick={() => void refetchPaged()}>
            <Ic name="sync" />
          </button>

          {/* export/import pop */}
          <div className="nou-wrap" style={{ position: "relative" }}>
            <button
              className="pill-btn"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => setOpenPop(openPop === "export" ? "" : "export")}
            >
              Export<Ic name="chevD" cls="ic" />
            </button>
            {openPop === "export" && (
              <div className="pop show" style={{ right: 0, top: 40, width: 210 }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="col-title">Export</div>
                <button className="pop-item" onClick={() => { setOpenPop(""); void handleExportSaga(); }}>
                  <Ic name="dl" />SAGA CSV
                </button>
                <button className="pop-item" onClick={() => { setOpenPop(""); void handleExportXlsx(); }}>
                  <Ic name="dl" />Export XLSX
                </button>
                <div className="pop-div" />
                <div className="col-title">Import</div>
                <button className="pop-item" onClick={() => { setOpenPop(""); void handleImportXml(); }}>
                  <Ic name="docUp" />Import XML
                </button>
                <button className="pop-item" onClick={() => { setOpenPop(""); setShowImportModal(true); }}>
                  <Ic name="docUp" />Import CSV
                </button>
              </div>
            )}
          </div>
        </div>

        {/* bulk bar */}
        <div className={`bulkbar${selected.size > 0 ? " show" : ""}`}>
          <b>{selected.size} selectate</b>
          <span className="spacer" />
          <button className="pill-btn send-btn" onClick={() => void handleBulkSubmit()}>
            <Ic name="send" />Trimite selecția la ANAF
          </button>
          <button className="pill-btn" onClick={() => window.print()}>
            <Ic name="printer" />Tipărește
          </button>
          <button className="pill-btn" onClick={() => setSelected(new Set())}>Deselectează</button>
        </div>

        {/* truncation note */}
        {paged && paged.total > paged.items.length && (
          <div style={{ padding: "6px 16px", borderBottom: "1px solid var(--line)", fontSize: 12, color: "var(--amber)" }}>
            Afișate primele {paged.items.length.toLocaleString("ro-RO")} din {paged.total.toLocaleString("ro-RO")} facturi.
          </div>
        )}

        {/* table */}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
        ) : pagedError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={pagedErr} label="facturile" onRetry={() => void refetchPaged()} />
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {allInvoices.length === 0
              ? "Nicio factură emisă. Creați prima factură cu butonul „Factură nouă”."
              : "Nicio înregistrare pentru filtrele aplicate."}
          </div>
        ) : (
          <>
            <table className="scr-table">
              <thead>
                <tr>
                  <th style={{ width: 36 }}>
                    <button
                      className={`cbx${selected.size === list.length && list.length > 0 ? " on" : ""}`}
                      aria-label="Selectează tot"
                      onClick={() =>
                        setSelected(selected.size === list.length ? new Set() : new Set(list.map((i) => i.id)))
                      }
                    />
                  </th>
                  <th>Număr</th>
                  <th>Data</th>
                  <th>Client</th>
                  <th className="r">Valoare net</th>
                  <th className="r">TVA</th>
                  <th className="r">Total</th>
                  <th>Monedă</th>
                  <th>Status</th>
                  <th>Plată</th>
                  <th className="r" style={{ width: 64 }}></th>
                </tr>
              </thead>
              <tbody>
                {visibleRows.map((inv) => {
                  const client = contactMap.get(inv.contactId);
                  const chip = STATUS_CHIP[inv.status] ?? STATUS_CHIP.DRAFT;
                  const payApplicable = inv.status !== "DRAFT" && inv.status !== "STORNED";
                  const payStatus = paymentStatusMap.get(inv.id) ?? "UNPAID";
                  const payCfg = payStatus === "PAID"
                    ? { cls: "paid", icon: "check", label: "Încasată" }
                    : payStatus === "PARTIAL"
                      ? { cls: "wait", icon: "clock", label: "Parțial" }
                      : { cls: "sent", icon: "dot", label: "Neîncasată" };
                  return (
                    <tr
                      key={inv.id}
                      className={`clickable${selected.has(inv.id) ? " selected" : ""}`}
                      onClick={() => {
                        setSelectedInvoiceId(inv.id);
                        void navigate({ to: "/invoices/$id", params: { id: inv.id } });
                      }}
                    >
                      <td onClick={(e) => e.stopPropagation()}>
                        <button
                          className={`cbx row-cbx${selected.has(inv.id) ? " on" : ""}`}
                          onClick={() => toggleOne(inv.id)}
                        />
                      </td>
                      <td><span className="doc" style={{ fontWeight: 700, color: "var(--text)" }}>{inv.fullNumber}</span></td>
                      <td className="num">{fmtRoDate(inv.issueDate)}</td>
                      <td><div className="cli">{client?.legalName ?? "—"}</div></td>
                      <td className="r num">{fmtRON(inv.subtotalAmount)}</td>
                      <td className="r num">{fmtRON(inv.vatAmount)}</td>
                      <td className="r num"><b>{fmtRON(inv.totalAmount)}</b></td>
                      <td>{inv.currency}</td>
                      <td>
                        <span className={`chip ${chip.cls}`}><Ic name={chip.icon} cls="sic" />{chip.label}</span>
                      </td>
                      <td>
                        {payApplicable
                          ? <span className={`chip ${payCfg.cls}`}><Ic name={payCfg.icon} cls="sic" />{payCfg.label}</span>
                          : <span className="muted">—</span>}
                      </td>
                      <td onClick={(e) => e.stopPropagation()}>
                        <div className="row-acts">
                          <button
                            className="mini-btn"
                            title="Vizualizează"
                            onClick={() => {
                              setSelectedInvoiceId(inv.id);
                              void navigate({ to: "/invoices/$id", params: { id: inv.id } });
                            }}
                          >
                            <Ic name="eye" />
                          </button>
                          <button
                            className="mini-btn"
                            title="Mai multe"
                            onClick={(e) => {
                              if (menuFor === inv.id) { setMenuFor(null); setMenuAnchor(null); }
                              else { setMenuAnchor(e.currentTarget.getBoundingClientRect()); setMenuFor(inv.id); }
                            }}
                          >
                            <Ic name="dots" />
                          </button>
                        </div>
                        {menuFor === inv.id && activeCompanyId && (
                          <RowMenu
                            invoiceId={inv.id}
                            companyId={activeCompanyId}
                            status={inv.status}
                            hasXml={!!inv.xmlPath}
                            anchor={menuAnchor}
                            onClose={() => { setMenuFor(null); setMenuAnchor(null); }}
                          />
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>

            {/* totals footer */}
            <div className="tot-foot">
              <span>Totaluri RON (filtrate): net <b className="num">{fmtRON(totNet)}</b></span>
              <span>TVA <b className="num">{fmtRON(totVat)}</b></span>
              <span>total <b className="num">{fmtRON(totTotal)}</b></span>
              <span className="spacer" style={{ flex: 1 }} />
              {list.length > MAX_ROWS && (
                <span className="muted">afișate primele {MAX_ROWS.toLocaleString("ro-RO")} din {list.length.toLocaleString("ro-RO")}</span>
              )}
              {nonRonCount > 0 && (
                <span className="muted">{nonRonCount === 1 ? "1 factură în altă monedă exclusă din totaluri" : `${nonRonCount} facturi în altă monedă excluse din totaluri`}</span>
              )}
            </div>
          </>
        )}
      </div>

      {showImportModal && activeCompanyId && (
        <CsvImportModal
          type="invoices"
          companyId={activeCompanyId}
          onClose={() => setShowImportModal(false)}
          onSuccess={() => {
            void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
            setShowImportModal(false);
          }}
        />
      )}
    </div>
  );
}
