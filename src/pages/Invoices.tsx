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
import { useTranslation } from "react-i18next";

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

function fmtMonth(ym: string, locale: string): string {
  const [year, month] = ym.split("-");
  const d = new Date(Number(year), Number(month) - 1, 1);
  return d.toLocaleDateString(locale, { month: "long", year: "numeric" });
}

/** Render at most this many rows (plain table, no virtualizer — design parity). */
const MAX_ROWS = 1000;

// Status → design chip (.chip variants + icon + i18n label key).
const STATUS_CHIP: Record<InvoiceStatus, { cls: string; icon: string; labelKey: string }> = {
  DRAFT:     { cls: "sent", icon: "docText", labelKey: "invoices.status.draft" },
  QUEUED:    { cls: "wait", icon: "clock",   labelKey: "invoices.status.queued" },
  SUBMITTED: { cls: "sent", icon: "send",    labelKey: "invoices.status.submitted" },
  VALIDATED: { cls: "paid", icon: "check",   labelKey: "invoices.status.validated" },
  REJECTED:  { cls: "late", icon: "xMark",   labelKey: "invoices.status.rejected" },
  STORNED:   { cls: "wait", icon: "undo",    labelKey: "invoices.status.storned" },
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
  const { t } = useTranslation();
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
      notify.success(t("invoices.notify.sent"));
    } catch (e) {
      notify.error(formatError(e, t("invoices.notify.sendError")));
    }
    onClose();
  }

  async function handlePdf() {
    try {
      const { openPath } = await import("@tauri-apps/plugin-opener");
      const path = await api.ubl.generatePdf(invoiceId, companyId);
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      if (path) await openPath(path);
      notify.success(t("invoices.notify.pdfDone"));
    } catch (e) {
      notify.error(formatError(e, t("invoices.notify.pdfError")));
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
      notify.success(t("invoices.notify.xmlSaved", { path }));
    } catch (e) {
      notify.error(formatError(e, t("invoices.notify.xmlError")));
    }
    onClose();
  }

  async function handleDuplicate() {
    try {
      const newId = await api.invoices.duplicate(invoiceId, companyId);
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success(t("invoices.notify.duplicated"));
      void navigate({ to: "/invoices/$id", params: { id: newId } });
    } catch (e) {
      notify.error(formatError(e, t("invoices.notify.duplicateError")));
    }
    onClose();
  }

  async function handleCheckStatus() {
    try {
      const stare = await api.anaf.checkStatus(companyId, invoiceId, testMode);
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success(t("invoices.notify.anafStatus", { status: stare }));
    } catch (e) {
      notify.error(formatError(e, t("invoices.notify.statusError")));
    }
    onClose();
  }

  async function handleStorno() {
    if (!stornoReason.trim()) { notify.warn(t("invoices.notify.stornoReasonRequired")); return; }
    try {
      const stornoInv = await api.invoices.storno(invoiceId, companyId, stornoReason.trim());
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success(t("invoices.notify.stornoCreated", { number: stornoInv.fullNumber }));
      void navigate({ to: "/invoices/$id", params: { id: stornoInv.id } });
    } catch (e) {
      notify.error(formatError(e, t("invoices.notify.stornoError")));
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
          {t("invoices.stornoModal.title")}
        </div>
        <textarea
          value={stornoReason}
          onChange={(e) => setStornoReason(e.target.value)}
          placeholder={t("invoices.stornoModal.reasonPlaceholder")}
          style={{
            width: "100%", minHeight: 56, marginBottom: 8, padding: "8px 10px",
            border: "1px solid var(--line)", borderRadius: 8, font: "inherit",
            fontSize: 12.5, resize: "vertical",
          }}
          autoFocus
        />
        <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
          <button className="pill-btn" onClick={() => { setStornoOpen(false); onClose(); }}>
            {t("invoices.stornoModal.cancel")}
          </button>
          <button
            className="btn-dark"
            style={{ height: 34, opacity: stornoReason.trim() ? 1 : 0.5 }}
            disabled={!stornoReason.trim()}
            onClick={handleStorno}
          >
            {t("invoices.stornoModal.confirm")}
          </button>
        </div>
      </div>,
      document.body,
    );
  }

  const items: Array<{ icon: string; label: string; danger?: boolean; action: () => void; show: boolean }> = [
    { icon: "eye", label: t("invoices.rowActions.view"), action: () => { void navigate({ to: "/invoices/$id", params: { id: invoiceId } }); onClose(); }, show: true },
    { icon: "pen", label: t("invoices.rowActions.edit"), action: () => { void navigate({ to: "/invoices/$id/edit", params: { id: invoiceId } }); onClose(); }, show: status === "DRAFT" },
    { icon: "send", label: t("invoices.rowActions.sendAnaf"), action: handleSubmit, show: (status === "DRAFT" || status === "VALIDATED") && hasXml },
    { icon: "dl", label: t("invoices.rowActions.downloadPdf"), action: handlePdf, show: true },
    { icon: "code", label: t("invoices.rowActions.downloadXml"), action: handleXml, show: true },
    { icon: "undo", label: t("invoices.rowActions.storno"), danger: true, action: () => setStornoOpen(true), show: status === "VALIDATED" },
    { icon: "copy", label: t("invoices.rowActions.duplicate"), action: handleDuplicate, show: true },
    { icon: "sync", label: t("invoices.rowActions.checkStatus"), action: handleCheckStatus, show: status === "SUBMITTED" || status === "QUEUED" },
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
  const { t, i18n } = useTranslation();
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
      catch (e) { errs.push(formatError(e, t("invoices.notify.submitFailed"))); }
    }
    void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
    setSelected(new Set());
    if (errs.length) notify.error(t("invoices.notify.bulkPartial", { ok, errors: errs.length, details: errs.slice(0, 3).join("; ") }));
    else notify.success(t("invoices.notify.bulkSent", { n: ok }));
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
      notify.success(t("invoices.notify.sagaSaved", { path }));
    } catch (e) {
      notify.error(formatError(e, t("invoices.notify.sagaError")));
    }
  }

  async function handleExportXlsx() {
    if (!activeCompanyId) { notify.warn(t("invoices.notify.selectCompany")); return; }
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({ filters: [{ name: "Excel", extensions: ["xlsx"] }], defaultPath: "facturi.xlsx" });
    if (!path) return;
    try {
      await api.integrations.exportInvoicesXlsx({ companyId: activeCompanyId }, path);
      notify.success(t("invoices.notify.exportSaved", { path }));
    } catch (e) {
      notify.error(formatError(e, t("invoices.notify.xlsxError")));
    }
  }

  async function handleImportXml() {
    if (!activeCompanyId) { notify.warn(t("invoices.notify.selectCompany")); return; }
    const { open } = await import("@tauri-apps/plugin-dialog");
    const filePath = await open({ filters: [{ name: "XML e-Factura", extensions: ["xml"] }] });
    if (!filePath || typeof filePath !== "string") return;
    try {
      const result = await api.importData.invoiceXmlFromFile(filePath, activeCompanyId);
      if (result.imported > 0) {
        notify.success(
          t("invoices.notify.imported", {
            number: result.invoiceNumber ?? "?",
            supplier: result.supplierName ?? "?",
            total: formatOptionalRon(result.totalAmount),
          }),
        );
        void queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      } else {
        notify.error(t("invoices.notify.importFailed", { errors: result.errors.join("; ") }));
      }
    } catch (e) {
      notify.error(formatError(e, t("invoices.notify.importXmlError")));
    }
  }

  const availableMonths = useMemo(() => {
    const months = new Set<string>();
    for (const inv of allInvoices) months.add(inv.issueDate.slice(0, 7));
    return Array.from(months).filter(Boolean).sort((a, b) => b.localeCompare(a));
  }, [allInvoices]);

  const tabs: Array<{ value: StatusFilter; label: string; count: number }> = [
    { value: "all",       label: t("invoices.tabs.all"),       count: totalCount },
    { value: "VALIDATED", label: t("invoices.tabs.validated"), count: counts.VALIDATED },
    { value: "SUBMITTED", label: t("invoices.tabs.submitted"), count: counts.SUBMITTED },
    { value: "QUEUED",    label: t("invoices.tabs.queued"),    count: counts.QUEUED },
    { value: "REJECTED",  label: t("invoices.tabs.rejected"),  count: counts.REJECTED },
    { value: "DRAFT",     label: t("invoices.tabs.drafts"),    count: counts.DRAFT },
    { value: "STORNED",   label: t("invoices.tabs.storned"),   count: counts.STORNED },
  ];

  const visibleRows = list.slice(0, MAX_ROWS);

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("invoices.title")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("invoices.states.selectCompany")}
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("invoices.title")}</h1>
          <p className="sub">
            {list.length !== totalCount
              ? t("invoices.countFiltered", { shown: list.length, total: totalCount.toLocaleString(i18n.language) })
              : t("invoices.countTotal", { n: totalCount.toLocaleString(i18n.language) })}
            {activeCompany ? ` · ${activeCompany.legalName}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="btn-dark" onClick={() => void navigate({ to: "/invoices/new" })}>
            <Ic name="plus" />{t("invoices.newInvoice")}
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
              placeholder={t("invoices.toolbar.searchPlaceholder")}
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
              {period === "all" ? t("invoices.toolbar.allMonths") : fmtMonth(period, i18n.language)}
              <Ic name="chevD" cls="ic" />
            </button>
            {openPop === "period" && (
              <div className="pop show" style={{ right: 0, top: 40, width: 210, maxHeight: 300, overflowY: "auto" }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="col-title">{t("invoices.toolbar.period")}</div>
                <button className="pop-item" onClick={() => { setPeriod("all"); setOpenPop(""); }}>
                  <span style={{ flex: 1 }}>{t("invoices.toolbar.allMonths")}</span>
                  {period === "all" && <Ic name="check" cls="co-check" />}
                </button>
                {availableMonths.map((ym) => (
                  <button key={ym} className="pop-item" onClick={() => { setPeriod(ym); setOpenPop(""); }}>
                    <span style={{ flex: 1 }}>{fmtMonth(ym, i18n.language)}</span>
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
              <Ic name="funnel" />{t("invoices.toolbar.filters")}
              {activeFilterCount > 0 && (
                <span className="pill-new" style={{ background: "var(--black)" }}>{activeFilterCount}</span>
              )}
            </button>
            {openPop === "filters" && (
              <div className="pop show" style={{ right: 0, top: 40, width: 250, padding: 10 }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="col-title">{t("invoices.toolbar.advancedFilters")}</div>
                <label
                  style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, cursor: "pointer", userSelect: "none", padding: "6px 10px 10px", color: errorsOnly ? "var(--red)" : "var(--text)" }}
                >
                  <button
                    className={`cbx${errorsOnly ? " on" : ""}`}
                    onClick={(e) => { e.preventDefault(); setErrorsOnly(!errorsOnly); if (!errorsOnly) setFilter("all"); }}
                  />
                  {t("invoices.toolbar.errorsOnly")}
                </label>
                <div style={{ fontSize: 12, color: "var(--text-2)", padding: "0 10px 6px" }}>{t("invoices.toolbar.totalRon")}</div>
                <div style={{ display: "flex", gap: 6, alignItems: "center", padding: "0 10px 8px" }}>
                  <input
                    type="number" placeholder={t("invoices.toolbar.min")} value={amountMin}
                    onChange={(e) => setAmountMin(e.target.value)}
                    style={{ flex: 1, height: 30, fontSize: 12, padding: "0 8px", border: "1px solid var(--line)", borderRadius: 8, fontFamily: "var(--mono)" }}
                  />
                  <span style={{ fontSize: 11, color: "var(--dim)" }}>–</span>
                  <input
                    type="number" placeholder={t("invoices.toolbar.max")} value={amountMax}
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
                    {t("invoices.toolbar.resetFilters")}
                  </button>
                )}
              </div>
            )}
          </div>

          {/* refresh */}
          <button className="sq-btn spin-btn" title={t("invoices.toolbar.refresh")} onClick={() => void refetchPaged()}>
            <Ic name="sync" />
          </button>

          {/* export/import pop */}
          <div className="nou-wrap" style={{ position: "relative" }}>
            <button
              className="pill-btn"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => setOpenPop(openPop === "export" ? "" : "export")}
            >
              {t("invoices.toolbar.export")}<Ic name="chevD" cls="ic" />
            </button>
            {openPop === "export" && (
              <div className="pop show" style={{ right: 0, top: 40, width: 210 }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="col-title">{t("invoices.toolbar.export")}</div>
                <button className="pop-item" onClick={() => { setOpenPop(""); void handleExportSaga(); }}>
                  <Ic name="dl" />{t("invoices.toolbar.exportSaga")}
                </button>
                <button className="pop-item" onClick={() => { setOpenPop(""); void handleExportXlsx(); }}>
                  <Ic name="dl" />{t("invoices.toolbar.exportXlsx")}
                </button>
                <div className="pop-div" />
                <div className="col-title">{t("invoices.toolbar.import")}</div>
                <button className="pop-item" onClick={() => { setOpenPop(""); void handleImportXml(); }}>
                  <Ic name="docUp" />{t("invoices.toolbar.importXml")}
                </button>
                <button className="pop-item" onClick={() => { setOpenPop(""); setShowImportModal(true); }}>
                  <Ic name="docUp" />{t("invoices.toolbar.importCsv")}
                </button>
              </div>
            )}
          </div>
        </div>

        {/* bulk bar */}
        <div className={`bulkbar${selected.size > 0 ? " show" : ""}`}>
          <b>{t("invoices.bulk.selected", { n: selected.size })}</b>
          <span className="spacer" />
          <button className="pill-btn send-btn" onClick={() => void handleBulkSubmit()}>
            <Ic name="send" />{t("invoices.bulk.sendSelection")}
          </button>
          <button className="pill-btn" onClick={() => window.print()}>
            <Ic name="printer" />{t("invoices.bulk.print")}
          </button>
          <button className="pill-btn" onClick={() => setSelected(new Set())}>{t("invoices.bulk.deselect")}</button>
        </div>

        {/* truncation note */}
        {paged && paged.total > paged.items.length && (
          <div style={{ padding: "6px 16px", borderBottom: "1px solid var(--line)", fontSize: 12, color: "var(--amber)" }}>
            {t("invoices.states.truncated", { shown: paged.items.length.toLocaleString(i18n.language), total: paged.total.toLocaleString(i18n.language) })}
          </div>
        )}

        {/* table */}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("invoices.states.loading")}</div>
        ) : pagedError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={pagedErr} label={t("invoices.states.errorLabel")} onRetry={() => void refetchPaged()} />
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {allInvoices.length === 0
              ? t("invoices.states.emptyAll")
              : t("invoices.states.emptyFiltered")}
          </div>
        ) : (
          <>
            <table className="scr-table">
              <thead>
                <tr>
                  <th style={{ width: 36 }}>
                    <button
                      className={`cbx${selected.size === list.length && list.length > 0 ? " on" : ""}`}
                      aria-label={t("invoices.table.selectAll")}
                      onClick={() =>
                        setSelected(selected.size === list.length ? new Set() : new Set(list.map((i) => i.id)))
                      }
                    />
                  </th>
                  <th>{t("invoices.table.number")}</th>
                  <th>{t("invoices.table.date")}</th>
                  <th>{t("invoices.table.client")}</th>
                  <th className="r">{t("invoices.table.net")}</th>
                  <th className="r">{t("invoices.table.vat")}</th>
                  <th className="r">{t("invoices.table.total")}</th>
                  <th>{t("invoices.table.currency")}</th>
                  <th>{t("invoices.table.status")}</th>
                  <th>{t("invoices.table.payment")}</th>
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
                    ? { cls: "paid", icon: "check", label: t("invoices.pay.paid") }
                    : payStatus === "PARTIAL"
                      ? { cls: "wait", icon: "clock", label: t("invoices.pay.partial") }
                      : { cls: "sent", icon: "dot", label: t("invoices.pay.unpaid") };
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
                        <span className={`chip ${chip.cls}`}><Ic name={chip.icon} cls="sic" />{t(chip.labelKey)}</span>
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
                            title={t("invoices.rowActions.view")}
                            onClick={() => {
                              setSelectedInvoiceId(inv.id);
                              void navigate({ to: "/invoices/$id", params: { id: inv.id } });
                            }}
                          >
                            <Ic name="eye" />
                          </button>
                          <button
                            className="mini-btn"
                            title={t("invoices.rowActions.more")}
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
              <span>{t("invoices.foot.totalsNet")} <b className="num">{fmtRON(totNet)}</b></span>
              <span>{t("invoices.foot.vat")} <b className="num">{fmtRON(totVat)}</b></span>
              <span>{t("invoices.foot.total")} <b className="num">{fmtRON(totTotal)}</b></span>
              <span className="spacer" style={{ flex: 1 }} />
              {list.length > MAX_ROWS && (
                <span className="muted">{t("invoices.foot.shownFirst", { shown: MAX_ROWS.toLocaleString(i18n.language), total: list.length.toLocaleString(i18n.language) })}</span>
              )}
              {nonRonCount > 0 && (
                <span className="muted">{t("invoices.foot.nonRonExcluded", { count: nonRonCount })}</span>
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
