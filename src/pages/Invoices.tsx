/**
 * Facturi emise — re-skinned to rf kit (Wave 2).
 * Preserves 100% of wiring: api.invoices.list, api.contacts.list,
 * @tanstack/react-virtual virtualizer, status tabs, text search,
 * "Cu erori" toggle, amount min/max filter, multi-select + bulk bar,
 * header buttons (new, SAGA CSV, XLSX, XML import, CSV import modal).
 * Adds prototype row hover quick-actions + "…" RowMenu wired to real commands.
 * ?view=storned search param → pre-selects STORNED tab.
 */

import { useMemo, useRef, useState, useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useTranslation } from "react-i18next";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { CsvImportModal } from "@/components/shared/CsvImportModal";
import {
  PageHeader, Btn, IconBtn, Badge, Tabs, Empty, SearchInput,
} from "@/components/rf";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { formatOptionalRon } from "@/lib/formatters";
import { formatError } from "@/lib/error-mapper";
import { fmtShortcut } from "@/lib/platform";
import { notify } from "@/lib/toasts";
import type { InvoiceStatus } from "@/types";

type StatusFilter = InvoiceStatus | "all";

// ── RowMenu ───────────────────────────────────────────────────────────────────

interface RowMenuProps {
  invoiceId: string;
  companyId: string;
  status: InvoiceStatus;
  hasXml: boolean;
  onClose: () => void;
}

function RowMenu({ invoiceId, companyId, status, hasXml, onClose }: RowMenuProps) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [stornoOpen, setStornoOpen] = useState(false);
  const [stornoReason, setStornoReason] = useState("");
  const { data: testModeSetting } = useQuery({
    queryKey: queryKeys.anaf.testMode,
    queryFn: () => api.settings.get("use_anaf_test_env"),
  });
  const testMode = testModeSetting === "1";

  // Close on outside click
  useEffect(() => {
    const h = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest(".rf-row-menu")) onClose();
    };
    setTimeout(() => document.addEventListener("click", h), 0);
    return () => document.removeEventListener("click", h);
  }, [onClose]);

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
    } catch (e) {
      notify.error(formatError(e, "Eroare stornare."));
    }
    setStornoOpen(false);
    onClose();
  }

  if (stornoOpen) {
    return (
      <div
        className="rf-row-menu rf-card"
        style={{ position: "absolute", right: 8, top: 32, zIndex: 50, width: 280, padding: 12, boxShadow: "var(--rf-shadow-md)" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 8, color: "var(--rf-error)" }}>
          Stornare factură
        </div>
        <textarea
          value={stornoReason}
          onChange={(e) => setStornoReason(e.target.value)}
          placeholder="Motivul stornării…"
          className="rf-textarea"
          style={{ minHeight: 56, marginBottom: 8 }}
          autoFocus
        />
        <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
          <button
            className="rf-btn rf-btn--secondary rf-btn--sm"
            onClick={() => { setStornoOpen(false); onClose(); }}
          >
            Anulează
          </button>
          <button
            className="rf-btn rf-btn--danger rf-btn--sm"
            disabled={!stornoReason.trim()}
            onClick={handleStorno}
          >
            Stornează
          </button>
        </div>
      </div>
    );
  }

  const items: Array<{ icon: string; label: string; color?: string; action: () => void; show: boolean }> = [
    { icon: "eye", label: "Vizualizează", action: () => { navigate({ to: "/invoices/$id", params: { id: invoiceId } }); onClose(); }, show: true },
    { icon: "pen", label: "Editează", action: () => { navigate({ to: "/invoices/$id/edit", params: { id: invoiceId } }); onClose(); }, show: status === "DRAFT" },
    { icon: "cloudUp", label: "Trimite la ANAF", action: handleSubmit, show: (status === "DRAFT" || status === "VALIDATED") && hasXml },
    { icon: "download", label: "Descarcă PDF", action: handlePdf, show: true },
    { icon: "file", label: "Descarcă XML (UBL)", action: handleXml, show: true },
    { icon: "storno", label: "Storno", color: "var(--rf-warning)", action: () => setStornoOpen(true), show: status === "VALIDATED" },
    { icon: "copy", label: "Duplică", action: handleDuplicate, show: true },
    { icon: "refresh", label: "Verifică status ANAF", action: handleCheckStatus, show: status === "SUBMITTED" || status === "QUEUED" },
  ];

  const visible = items.filter((i) => i.show);

  return (
    <div
      className="rf-row-menu rf-card"
      style={{ position: "absolute", right: 8, top: 32, zIndex: 50, width: 210, padding: 4, boxShadow: "var(--rf-shadow-md)" }}
      onClick={(e) => e.stopPropagation()}
    >
      {visible.map((item) => (
        <button
          key={item.label}
          type="button"
          onClick={item.action}
          style={{
            display: "flex", width: "100%", gap: 10, alignItems: "center",
            border: "none", background: "transparent", padding: "8px 10px",
            cursor: "pointer", borderRadius: 6, fontSize: 13,
            color: item.color ?? "var(--rf-text)", fontFamily: "var(--rf-font)",
          }}
          onMouseEnter={(e) => (e.currentTarget.style.background = "var(--rf-hover)")}
          onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
        >
          <Icon name={item.icon} size={15} />
          {item.label}
        </button>
      ))}
    </div>
  );
}

// ── InvoicesPage ──────────────────────────────────────────────────────────────

export function InvoicesPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const setSelectedInvoiceId = useAppStore((s) => s.setSelectedInvoiceId);
  const { t } = useTranslation();

  // ?view=storned deep-link
  const { view: viewParam } = useSearch({ from: "/invoices" });
  const [filter, setFilter] = useState<StatusFilter>(viewParam === "storned" ? "STORNED" : "all");
  useEffect(() => {
    if (viewParam === "storned") setFilter("STORNED");
  }, [viewParam]);

  const [query, setQuery] = useState("");
  const [errorsOnly, setErrorsOnly] = useState(false);
  const [amountMin, setAmountMin] = useState("");
  const [amountMax, setAmountMax] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [showImportModal, setShowImportModal] = useState(false);
  const [menuFor, setMenuFor] = useState<string | null>(null);

  // Fetch invoices
  const { data: paged, isLoading, isError: pagedError, error: pagedErr, refetch: refetchPaged } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.invoices.list({ companyId: activeCompanyId ?? undefined }),
  });

  // Fetch contacts for client name / CUI
  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const contactMap = useMemo(() => {
    const m = new Map<string, { legalName: string; cui: string | null }>();
    for (const c of contacts) m.set(c.id, { legalName: c.legalName, cui: c.cui });
    return m;
  }, [contacts]);

  const allInvoices = paged?.items ?? [];
  const totalCount = paged?.total ?? 0;

  // Client-side filter
  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    const minVal = amountMin.trim() ? parseFloat(amountMin) : null;
    const maxVal = amountMax.trim() ? parseFloat(amountMax) : null;
    return allInvoices
      .filter((i) => {
        if (errorsOnly) return i.status === "REJECTED";
        return filter === "all" || i.status === filter;
      })
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
  }, [allInvoices, filter, errorsOnly, amountMin, amountMax, query, contactMap]);

  // Status counts
  const counts = {
    VALIDATED: allInvoices.filter((i) => i.status === "VALIDATED").length,
    SUBMITTED: allInvoices.filter((i) => i.status === "SUBMITTED").length,
    REJECTED:  allInvoices.filter((i) => i.status === "REJECTED").length,
    DRAFT:     allInvoices.filter((i) => i.status === "DRAFT").length,
    QUEUED:    allInvoices.filter((i) => i.status === "QUEUED").length,
    STORNED:   allInvoices.filter((i) => i.status === "STORNED").length,
  };

  // Totals of filtered list
  const totNet   = list.reduce((s, i) => s + parseDec(i.subtotalAmount), 0);
  const totVat   = list.reduce((s, i) => s + parseDec(i.vatAmount), 0);
  const totTotal = list.reduce((s, i) => s + parseDec(i.totalAmount), 0);

  const toggleOne = (id: string) => {
    const next = new Set(selected);
    next.has(id) ? next.delete(id) : next.add(id);
    setSelected(next);
  };

  // Virtual scrolling
  const tableBodyRef = useRef<HTMLDivElement>(null);
  const rowVirtualizer = useVirtualizer({
    count: list.length,
    getScrollElement: () => tableBodyRef.current,
    estimateSize: () => 48,
    overscan: 10,
  });

  // Bulk submit
  async function handleBulkSubmit() {
    if (!activeCompanyId) return;
    const { data: tms } = await Promise.resolve({ data: await api.settings.get("use_anaf_test_env") });
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

  const statusTabs = [
    { value: "all" as StatusFilter, label: "Toate", badge: totalCount },
    { value: "VALIDATED" as StatusFilter, label: "Validate", badge: counts.VALIDATED },
    { value: "SUBMITTED" as StatusFilter, label: "Trimise", badge: counts.SUBMITTED },
    { value: "REJECTED" as StatusFilter, label: "Respinse", badge: counts.REJECTED },
    { value: "DRAFT" as StatusFilter, label: "Schițe", badge: counts.DRAFT },
    { value: "STORNED" as StatusFilter, label: "Stornate", badge: counts.STORNED },
  ];

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "var(--rf-app-bg)" }}>
      <PageHeader
        title={t("invoices.title")}
        sub={
          <Badge variant="neutral">
            {list.length} din {totalCount.toLocaleString("ro-RO")} facturi
          </Badge>
        }
        actions={
          <>
            <Btn
              variant="ghost"
              size="sm"
              icon="download"
              onClick={async () => {
                if (!activeCompanyId) return;
                const { save } = await import("@tauri-apps/plugin-dialog");
                const path = await save({ filters: [{ name: "CSV", extensions: ["csv"] }], defaultPath: "facturi-saga.csv" });
                if (path) {
                  const today = new Date().toISOString().slice(0, 10);
                  const yearStart = `${new Date().getFullYear()}-01-01`;
                  try {
                    await api.integrations.exportSagaCsv(activeCompanyId, yearStart, today, path);
                    notify.success(`Export SAGA salvat: ${path}`);
                  } catch (e) {
                    notify.error(formatError(e, "Eroare export SAGA."));
                  }
                }
              }}
            >
              SAGA CSV
            </Btn>
            <Btn
              variant="ghost"
              size="sm"
              icon="table"
              onClick={async () => {
                if (!activeCompanyId) { notify.warn("Selectați o companie."); return; }
                const { save } = await import("@tauri-apps/plugin-dialog");
                const path = await save({ filters: [{ name: "Excel", extensions: ["xlsx"] }], defaultPath: "facturi.xlsx" });
                if (path) {
                  try {
                    await api.integrations.exportInvoicesXlsx({ companyId: activeCompanyId ?? undefined }, path);
                    notify.success(`Export salvat: ${path}`);
                  } catch (e) {
                    notify.error(formatError(e, "Eroare export XLSX."));
                  }
                }
              }}
            >
              {t("invoices.exportXlsx")}
            </Btn>
            <Btn
              variant="ghost"
              size="sm"
              icon="upload"
              onClick={async () => {
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
              }}
            >
              Import XML
            </Btn>
            <Btn variant="ghost" size="sm" icon="upload" onClick={() => setShowImportModal(true)}>
              {t("invoices.importCsv")}
            </Btn>
            <Btn
              variant="primary"
              icon="plus"
              onClick={() => navigate({ to: "/invoices/new" })}
            >
              {t("invoices.newInvoice")}
              <span
                style={{
                  marginLeft: 6, background: "rgba(255,255,255,0.18)",
                  border: "1px solid rgba(255,255,255,0.3)", color: "#fff",
                  fontSize: 11, padding: "1px 5px", borderRadius: 4,
                }}
              >
                {fmtShortcut("Ctrl N")}
              </span>
            </Btn>
          </>
        }
      />

      {/* Status tabs */}
      <div style={{ padding: "0 32px", background: "var(--rf-app-bg)" }}>
        <Tabs<StatusFilter>
          tabs={statusTabs}
          value={filter}
          onChange={(v) => { setFilter(v); setErrorsOnly(false); }}
        />
      </div>

      {/* Toolbar */}
      <div
        className="rf-toolbar-row"
        style={{
          padding: "10px 32px",
          borderBottom: "1px solid var(--rf-border)",
          background: "var(--rf-content)",
          flexWrap: "nowrap",
          overflowX: "auto",
        }}
      >
        <SearchInput
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Caută după nr., CUI sau denumire…"
          style={{ width: 280, flexShrink: 0 }}
        />

        <div style={{ width: 1, height: 20, background: "var(--rf-border-strong)", flexShrink: 0 }} />

        {/* Cu erori toggle */}
        <label
          style={{
            display: "inline-flex", alignItems: "center", gap: 6,
            fontSize: 12, cursor: "pointer", userSelect: "none", whiteSpace: "nowrap",
            color: errorsOnly ? "var(--rf-error)" : "var(--rf-text-muted)",
          }}
        >
          <input
            type="checkbox"
            checked={errorsOnly}
            onChange={(e) => {
              setErrorsOnly(e.target.checked);
              if (e.target.checked) setFilter("all");
            }}
            style={{ accentColor: "var(--rf-error)", width: 14, height: 14 }}
          />
          Cu erori
        </label>

        <div style={{ width: 1, height: 20, background: "var(--rf-border-strong)", flexShrink: 0 }} />

        <span style={{ fontSize: 12, color: "var(--rf-text-muted)", whiteSpace: "nowrap" }}>Total:</span>
        <input
          type="number"
          placeholder="Min"
          value={amountMin}
          onChange={(e) => setAmountMin(e.target.value)}
          style={{
            width: 72, height: 32, fontSize: 12, padding: "0 8px",
            border: "1px solid var(--rf-border-strong)", background: "var(--rf-content)",
            color: "var(--rf-text)", borderRadius: 6, fontFamily: "var(--rf-mono)",
          }}
        />
        <span style={{ fontSize: 11, color: "var(--rf-text-dim)" }}>–</span>
        <input
          type="number"
          placeholder="Max"
          value={amountMax}
          onChange={(e) => setAmountMax(e.target.value)}
          style={{
            width: 72, height: 32, fontSize: 12, padding: "0 8px",
            border: "1px solid var(--rf-border-strong)", background: "var(--rf-content)",
            color: "var(--rf-text)", borderRadius: 6, fontFamily: "var(--rf-mono)",
          }}
        />

        <span style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center", flexShrink: 0 }}>
          {selected.size > 0 && (
            <>
              <span style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-text)" }}>
                {selected.size} selectate
              </span>
              <Btn variant="primary" size="sm" icon="cloudUp" onClick={handleBulkSubmit}>
                Trimite la ANAF
              </Btn>
              <Btn variant="secondary" size="sm" icon="printer" onClick={() => window.print()}>
                Tipărește
              </Btn>
              <Btn variant="ghost" size="sm" onClick={() => setSelected(new Set())}>
                Deselectează
              </Btn>
              <div style={{ width: 1, height: 20, background: "var(--rf-border-strong)" }} />
            </>
          )}
          <IconBtn icon="refresh" title="Reîncarcă" onClick={() => void refetchPaged()} />
        </span>
      </div>

      {/* Table container */}
      <div ref={tableBodyRef} style={{ flex: 1, overflowY: "auto" }}>
        {isLoading ? (
          <div style={{ padding: 32, fontSize: 13, color: "var(--rf-text-muted)" }}>Se încarcă…</div>
        ) : pagedError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={pagedErr} label="facturile" onRetry={() => void refetchPaged()} />
          </div>
        ) : list.length === 0 ? (
          <Empty
            icon="fileOut"
            title={allInvoices.length === 0 ? "Nicio factură emisă" : "Nicio înregistrare"}
          >
            {allInvoices.length === 0
              ? 'Creați prima factură cu butonul "Factură nouă".'
              : "Nicio înregistrare pentru filtrele aplicate."}
          </Empty>
        ) : (
          <div className="rf-tbl-wrap" style={{ minHeight: "100%" }}>
            <table className="rf-tbl" style={{ width: "100%" }}>
              <thead>
                <tr>
                  <th style={{ width: 40, paddingLeft: 16 }}>
                    <input
                      type="checkbox"
                      checked={selected.size === list.length && list.length > 0}
                      onChange={() =>
                        setSelected(
                          selected.size === list.length ? new Set() : new Set(list.map((i) => i.id))
                        )
                      }
                      style={{ accentColor: "var(--rf-accent)", width: 14, height: 14 }}
                    />
                  </th>
                  <th style={{ width: 134 }} className="sortable sorted">
                    {t("invoices.columns.number")}
                  </th>
                  <th style={{ width: 92 }}>{t("invoices.columns.date")}</th>
                  <th>{t("invoices.columns.customer")}</th>
                  <th style={{ width: 110 }}>CUI</th>
                  <th className="right" style={{ width: 120 }}>Net (RON)</th>
                  <th className="right" style={{ width: 96 }}>TVA</th>
                  <th className="right" style={{ width: 130 }}>{t("invoices.columns.total")}</th>
                  <th style={{ width: 100 }}>Scadență</th>
                  <th style={{ width: 130 }}>{t("invoices.columns.status")}</th>
                  <th style={{ width: 120 }}>Index ANAF</th>
                  <th style={{ width: 90 }}></th>
                </tr>
              </thead>
              <tbody
                style={{
                  height: `${rowVirtualizer.getTotalSize()}px`,
                  position: "relative",
                  display: "block",
                }}
              >
                {rowVirtualizer.getVirtualItems().map((virtualRow) => {
                  const inv = list[virtualRow.index];
                  const client = contactMap.get(inv.contactId);
                  return (
                    <tr
                      key={inv.id}
                      data-index={virtualRow.index}
                      ref={rowVirtualizer.measureElement}
                      className={`clickable${selected.has(inv.id) ? " selected" : ""}`}
                      style={{
                        cursor: "pointer",
                        position: "absolute",
                        top: 0,
                        left: 0,
                        width: "100%",
                        transform: `translateY(${virtualRow.start}px)`,
                      }}
                      onClick={() => {
                        setSelectedInvoiceId(inv.id);
                        navigate({ to: "/invoices/$id", params: { id: inv.id } });
                      }}
                    >
                      <td style={{ width: 40 }} onClick={(e) => e.stopPropagation()}>
                        <input
                          type="checkbox"
                          checked={selected.has(inv.id)}
                          onChange={() => toggleOne(inv.id)}
                          style={{ accentColor: "var(--rf-accent)", width: 14, height: 14 }}
                        />
                      </td>
                      <td style={{ fontFamily: "var(--rf-mono)", fontWeight: 700 }}>
                        {inv.fullNumber}
                      </td>
                      <td style={{ color: "var(--rf-text-muted)" }}>{inv.issueDate}</td>
                      <td style={{ fontWeight: 500 }}>
                        {client?.legalName ?? <span style={{ color: "var(--rf-text-dim)" }}>—</span>}
                      </td>
                      <td style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-text-muted)" }}>
                        {client?.cui ?? "—"}
                      </td>
                      <td
                        className="right"
                        style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-text-muted)", fontVariantNumeric: "tabular-nums" }}
                      >
                        {fmtRON(inv.subtotalAmount)}
                      </td>
                      <td
                        className="right"
                        style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-text-dim)", fontVariantNumeric: "tabular-nums" }}
                      >
                        {fmtRON(inv.vatAmount)}
                      </td>
                      <td
                        className="right"
                        style={{ fontFamily: "var(--rf-mono)", fontWeight: 700, fontVariantNumeric: "tabular-nums" }}
                      >
                        {fmtRON(inv.totalAmount)}
                      </td>
                      <td style={{ color: "var(--rf-text-muted)" }}>{inv.dueDate}</td>
                      <td>
                        <StatusBadge status={inv.status} />
                      </td>
                      <td
                        style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-text-dim)", fontSize: 12 }}
                      >
                        {inv.anafIndex ?? "—"}
                      </td>
                      <td
                        style={{ position: "relative" }}
                        onClick={(e) => e.stopPropagation()}
                      >
                        <div className="rf-cell-actions">
                          <IconBtn
                            icon="eye"
                            ghost
                            title="Vizualizează"
                            onClick={() => {
                              setSelectedInvoiceId(inv.id);
                              navigate({ to: "/invoices/$id", params: { id: inv.id } });
                            }}
                          />
                          {inv.status === "DRAFT" && (
                            <IconBtn
                              icon="pen"
                              ghost
                              title="Editează"
                              onClick={() => navigate({ to: "/invoices/$id/edit", params: { id: inv.id } })}
                            />
                          )}
                          <IconBtn
                            icon="more"
                            ghost
                            title="Mai multe"
                            onClick={() => setMenuFor(menuFor === inv.id ? null : inv.id)}
                          />
                        </div>
                        {menuFor === inv.id && activeCompanyId && (
                          <RowMenu
                            invoiceId={inv.id}
                            companyId={activeCompanyId}
                            status={inv.status}
                            hasXml={!!inv.xmlPath}
                            onClose={() => setMenuFor(null)}
                          />
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
              <tfoot>
                <tr>
                  <td colSpan={5}>
                    {list.length} facturi
                  </td>
                  <td className="right" style={{ fontFamily: "var(--rf-mono)", fontVariantNumeric: "tabular-nums" }}>
                    {fmtRON(totNet)}
                  </td>
                  <td className="right" style={{ fontFamily: "var(--rf-mono)", fontVariantNumeric: "tabular-nums" }}>
                    {fmtRON(totVat)}
                  </td>
                  <td className="right" style={{ fontFamily: "var(--rf-mono)", fontVariantNumeric: "tabular-nums" }}>
                    {fmtRON(totTotal)}
                  </td>
                  <td colSpan={4} style={{ color: "var(--rf-text-dim)", fontSize: 12, fontWeight: 400 }}>RON</td>
                </tr>
              </tfoot>
            </table>
          </div>
        )}
      </div>

      {/* Footer status bar */}
      <div
        style={{
          padding: "6px 32px",
          borderTop: "1px solid var(--rf-border)",
          background: "var(--rf-content)",
          display: "flex",
          gap: 16,
          fontSize: 12,
          color: "var(--rf-text-muted)",
          flexShrink: 0,
        }}
      >
        <span>Validate: <b style={{ color: "var(--rf-success)" }}>{counts.VALIDATED}</b></span>
        <span>Trimise: <b style={{ color: "var(--rf-info)" }}>{counts.SUBMITTED}</b></span>
        <span>Respinse: <b style={{ color: "var(--rf-error)" }}>{counts.REJECTED}</b></span>
        <span>Schițe: <b>{counts.DRAFT}</b></span>
        <span>Stornate: <b>{counts.STORNED}</b></span>
        <span style={{ marginLeft: "auto", color: "var(--rf-text-dim)", fontSize: 11 }}>
          <span style={{ border: "1px solid var(--rf-border-strong)", borderRadius: 4, padding: "1px 4px", fontSize: 10, marginRight: 2 }}>↑↓</span> selectează ·{" "}
          <span style={{ border: "1px solid var(--rf-border-strong)", borderRadius: 4, padding: "1px 4px", fontSize: 10, marginRight: 2 }}>Enter</span> deschide
        </span>
      </div>

      {showImportModal && (
        <CsvImportModal
          type="invoices"
          companyId={activeCompanyId ?? ""}
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
