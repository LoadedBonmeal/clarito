/**
 * Facturi emise — Polish-Wave 5 restructure.
 *
 * LAYOUT (matches "Claude Design" reference):
 *   Header  : title + count chip + "+ Factură nouă" only
 *   Toolbar : search | "Toate ▾" status-dropdown | period "mai 2026 ▾" |
 *             "Filtre" (popover: Cu erori + Min/Max) | refresh | Export▾ | Import▾
 *   Columns : NUMĂR · DATA · CLIENT · VALOARE NET · TVA · TOTAL · MONEDĂ · STATUS
 *             + hover action cell (eye / pen / "…" RowMenu)
 *
 * ALL wiring preserved:
 *   api.invoices.list, api.contacts.list, api.anaf.submitInvoice,
 *   api.anaf.checkStatus, api.ubl.generatePdf, api.ubl.generateXml,
 *   api.invoices.duplicate, api.invoices.storno,
 *   api.integrations.exportSagaCsv, api.integrations.exportInvoicesXlsx,
 *   api.importData.invoiceXmlFromFile, CsvImportModal,
 *   multi-select bulk-submit + bulk-print, virtualizer, keyboard nav,
 *   ?view=storned deep-link.
 */

import { useMemo, useRef, useState, useEffect } from "react";
import { createPortal } from "react-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useTranslation } from "react-i18next";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { CsvImportModal } from "@/components/shared/CsvImportModal";
import { PageHeader, Btn, IconBtn, Badge, Empty, SearchInput } from "@/components/rf";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuLabel,
} from "@/components/ui/dropdown-menu";
import {
  Popover,
  PopoverTrigger,
  PopoverContent,
} from "@/components/ui/popover";
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

// Month filter value: "YYYY-MM" or "all"
type PeriodFilter = string | "all";

// ── helpers ───────────────────────────────────────────────────────────────────

/** Returns "mai 2026" style Romanian month label from a YYYY-MM string. */
function fmtMonth(ym: string): string {
  const [year, month] = ym.split("-");
  const d = new Date(Number(year), Number(month) - 1, 1);
  return d.toLocaleDateString("ro-RO", { month: "long", year: "numeric" });
}

/** Produce last N month YYYY-MM strings (newest first). */
function recentMonths(n = 12): string[] {
  const result: string[] = [];
  const now = new Date();
  for (let i = 0; i < n; i++) {
    const d = new Date(now.getFullYear(), now.getMonth() - i, 1);
    result.push(`${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`);
  }
  return result;
}

// ── ToolbarBtn — small button styled with rf tokens ───────────────────────────

interface ToolbarBtnProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  active?: boolean;
  dot?: boolean;
  children: React.ReactNode;
}

function ToolbarBtn({ active, dot, children, style, ...rest }: ToolbarBtnProps) {
  return (
    <button
      type="button"
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 5,
        height: 32,
        padding: "0 10px",
        border: `1px solid ${active ? "var(--rf-accent)" : "var(--rf-border-strong)"}`,
        borderRadius: "var(--rf-radius-sm)",
        background: active ? "var(--rf-accent-bg)" : "var(--rf-content)",
        color: active ? "var(--rf-accent)" : "var(--rf-text)",
        fontFamily: "var(--rf-font)",
        fontSize: 13,
        cursor: "pointer",
        whiteSpace: "nowrap",
        position: "relative",
        ...style,
      }}
      {...rest}
    >
      {children}
      {dot && (
        <span
          style={{
            position: "absolute",
            top: 4,
            right: 4,
            width: 6,
            height: 6,
            borderRadius: "50%",
            background: "var(--rf-accent)",
          }}
        />
      )}
    </button>
  );
}

// Chevron down tiny icon (inline SVG — no import needed)
function ChevDown({ size = 10 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 10 10" fill="none" style={{ opacity: 0.6 }}>
      <path d="M2 3.5L5 6.5L8 3.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

// ── RowMenu ───────────────────────────────────────────────────────────────────

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

  // Close on outside click — attach only once, clean up both the timer and the exact handler
  useEffect(() => {
    const h = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest(".rf-row-menu")) onClose();
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
    } catch (e) {
      notify.error(formatError(e, "Eroare stornare."));
    }
    setStornoOpen(false);
    onClose();
  }

  if (stornoOpen) {
    return createPortal(
      <div
        className="rf-row-menu rf-card"
        style={{ ...portalPos(280), padding: 12, boxShadow: "var(--rf-shadow-md)" }}
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
      </div>,
      document.body,
    );
  }

  const items: Array<{ icon: string; label: string; color?: string; action: () => void; show: boolean }> = [
    { icon: "eye", label: "Vizualizează", action: () => { void navigate({ to: "/invoices/$id", params: { id: invoiceId } }); onClose(); }, show: true },
    { icon: "pen", label: "Editează", action: () => { void navigate({ to: "/invoices/$id/edit", params: { id: invoiceId } }); onClose(); }, show: status === "DRAFT" },
    { icon: "cloudUp", label: "Trimite la ANAF", action: handleSubmit, show: (status === "DRAFT" || status === "VALIDATED") && hasXml },
    { icon: "download", label: "Descarcă PDF", action: handlePdf, show: true },
    { icon: "file", label: "Descarcă XML (UBL)", action: handleXml, show: true },
    { icon: "storno", label: "Storno", color: "var(--rf-warning)", action: () => setStornoOpen(true), show: status === "VALIDATED" },
    { icon: "copy", label: "Duplică", action: handleDuplicate, show: true },
    { icon: "refresh", label: "Verifică status ANAF", action: handleCheckStatus, show: status === "SUBMITTED" || status === "QUEUED" },
  ];

  const visible = items.filter((i) => i.show);

  return createPortal(
    <div
      className="rf-row-menu rf-card"
      style={{ ...portalPos(210), padding: 4, boxShadow: "var(--rf-shadow-md)" }}
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
  const { t } = useTranslation();

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
  const [filtersOpen, setFiltersOpen] = useState(false);

  // Fetch invoices
  const { data: paged, isLoading, isError: pagedError, error: pagedErr, refetch: refetchPaged } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.invoices.list({ companyId: activeCompanyId ?? undefined }),
  });

  // Fetch contacts for client name
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
        // Period filter: compare YYYY-MM prefix of issueDate
        if (period !== "all") {
          const ym = i.issueDate.slice(0, 7);
          if (ym !== period) return false;
        }
        return true;
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
  }, [allInvoices, filter, period, errorsOnly, amountMin, amountMax, query, contactMap]);

  // Status counts (of all invoices, not filtered)
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

  // Active filters count (for Filtre dot)
  const activeFilterCount = (errorsOnly ? 1 : 0) + (amountMin ? 1 : 0) + (amountMax ? 1 : 0);

  const toggleOne = (id: string) => {
    const next = new Set(selected);
    next.has(id) ? next.delete(id) : next.add(id);
    setSelected(next);
  };

  // Virtual scrolling (generous 52px row height)
  const tableBodyRef = useRef<HTMLDivElement>(null);
  const rowVirtualizer = useVirtualizer({
    count: list.length,
    getScrollElement: () => tableBodyRef.current,
    estimateSize: () => 52,
    overscan: 10,
  });

  // Bulk submit
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

  // Export handlers
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

  // Import handlers
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

  // Available period months (derived from all invoices, newest first)
  const availableMonths = useMemo(() => {
    const months = new Set<string>();
    for (const inv of allInvoices) {
      const ym = inv.issueDate.slice(0, 7);
      if (ym) months.add(ym);
    }
    // Sort descending
    return Array.from(months).sort((a, b) => b.localeCompare(a));
  }, [allInvoices]);

  // Fallback to recent 12 months if no invoices yet
  const periodOptions = availableMonths.length > 0 ? availableMonths : recentMonths(12);

  // Status dropdown label
  const statusOptions: Array<{ value: StatusFilter; label: string; count: number }> = [
    { value: "all",       label: "Toate",    count: totalCount },
    { value: "VALIDATED", label: "Validate", count: counts.VALIDATED },
    { value: "SUBMITTED", label: "Trimise",  count: counts.SUBMITTED },
    { value: "QUEUED",    label: "În coadă", count: counts.QUEUED },
    { value: "REJECTED",  label: "Respinse", count: counts.REJECTED },
    { value: "DRAFT",     label: "Schițe",   count: counts.DRAFT },
    { value: "STORNED",   label: "Stornate", count: counts.STORNED },
  ];
  const currentStatusLabel = statusOptions.find((o) => o.value === filter)?.label ?? "Toate";

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "var(--rf-app-bg)" }}>

      {/* ── Header ───────────────────────────────────────────────────────── */}
      <PageHeader
        title={t("invoices.title")}
        sub={
          <Badge variant="neutral">
            {list.length !== totalCount
              ? `${list.length} din ${totalCount.toLocaleString("ro-RO")} facturi`
              : `${totalCount.toLocaleString("ro-RO")} facturi`}
          </Badge>
        }
        actions={
          <Btn
            variant="primary"
            icon="plus"
            onClick={() => void navigate({ to: "/invoices/new" })}
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
        }
      />

      {/* ── Toolbar ──────────────────────────────────────────────────────── */}
      <div
        className="rf-toolbar-row"
        style={{
          padding: "10px 32px",
          borderBottom: "1px solid var(--rf-border)",
          background: "var(--rf-content)",
          flexWrap: "nowrap",
          overflowX: "auto",
          gap: 8,
        }}
      >
        {/* Search */}
        <SearchInput
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Caută după număr sau client…"
          style={{ width: 260, flexShrink: 0 }}
        />

        {/* Status dropdown */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <ToolbarBtn>
              {currentStatusLabel}
              <ChevDown />
            </ToolbarBtn>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" style={{ minWidth: 180 }}>
            <DropdownMenuLabel style={{ fontSize: 11, color: "var(--rf-text-dim)", padding: "4px 8px" }}>
              Status factură
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            {statusOptions.map((opt) => (
              <DropdownMenuItem
                key={opt.value}
                onClick={() => { setFilter(opt.value); setErrorsOnly(false); }}
                style={{
                  fontWeight: filter === opt.value ? 600 : 400,
                  color: filter === opt.value ? "var(--rf-accent)" : "var(--rf-text)",
                }}
              >
                <span style={{ flex: 1 }}>{opt.label}</span>
                <span style={{ fontSize: 11, color: "var(--rf-text-dim)", fontVariantNumeric: "tabular-nums" }}>
                  {opt.count}
                </span>
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>

        {/* Period dropdown */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <ToolbarBtn active={period !== "all"}>
              {period === "all" ? "Toate lunile" : fmtMonth(period)}
              <ChevDown />
            </ToolbarBtn>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" style={{ minWidth: 180, maxHeight: 320, overflowY: "auto" }}>
            <DropdownMenuLabel style={{ fontSize: 11, color: "var(--rf-text-dim)", padding: "4px 8px" }}>
              Perioadă
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              onClick={() => setPeriod("all")}
              style={{
                fontWeight: period === "all" ? 600 : 400,
                color: period === "all" ? "var(--rf-accent)" : "var(--rf-text)",
              }}
            >
              Toate lunile
            </DropdownMenuItem>
            {periodOptions.map((ym) => (
              <DropdownMenuItem
                key={ym}
                onClick={() => setPeriod(ym)}
                style={{
                  fontWeight: period === ym ? 600 : 400,
                  color: period === ym ? "var(--rf-accent)" : "var(--rf-text)",
                }}
              >
                {fmtMonth(ym)}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>

        {/* Filtre popover */}
        <Popover open={filtersOpen} onOpenChange={setFiltersOpen}>
          <PopoverTrigger asChild>
            <ToolbarBtn active={activeFilterCount > 0} dot={activeFilterCount > 0} onClick={() => setFiltersOpen((v) => !v)}>
              <Icon name="filter" size={13} />
              Filtre
              {activeFilterCount > 0 && (
                <span
                  style={{
                    marginLeft: 2,
                    background: "var(--rf-accent)",
                    color: "#fff",
                    borderRadius: 10,
                    fontSize: 10,
                    fontWeight: 700,
                    padding: "0 5px",
                    minWidth: 16,
                    textAlign: "center",
                  }}
                >
                  {activeFilterCount}
                </span>
              )}
            </ToolbarBtn>
          </PopoverTrigger>
          <PopoverContent
            align="start"
            sideOffset={6}
            style={{
              width: 260,
              padding: 16,
              background: "var(--rf-content)",
              border: "1px solid var(--rf-border)",
              borderRadius: "var(--rf-radius)",
              boxShadow: "var(--rf-shadow-md)",
            }}
          >
            <div style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-text-muted)", marginBottom: 12, textTransform: "uppercase", letterSpacing: "0.06em" }}>
              Filtre avansate
            </div>

            {/* Cu erori toggle */}
            <label
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                fontSize: 13,
                cursor: "pointer",
                userSelect: "none",
                marginBottom: 14,
                color: errorsOnly ? "var(--rf-error)" : "var(--rf-text)",
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
              Cu erori (respinse ANAF)
            </label>

            {/* Amount range */}
            <div style={{ fontSize: 12, color: "var(--rf-text-muted)", marginBottom: 6 }}>Total factură (RON)</div>
            <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
              <input
                type="number"
                placeholder="Min"
                value={amountMin}
                onChange={(e) => setAmountMin(e.target.value)}
                style={{
                  flex: 1, height: 30, fontSize: 12, padding: "0 8px",
                  border: "1px solid var(--rf-border-strong)", background: "var(--rf-app-bg)",
                  color: "var(--rf-text)", borderRadius: "var(--rf-radius-sm)", fontFamily: "var(--rf-mono)",
                }}
              />
              <span style={{ fontSize: 11, color: "var(--rf-text-dim)" }}>–</span>
              <input
                type="number"
                placeholder="Max"
                value={amountMax}
                onChange={(e) => setAmountMax(e.target.value)}
                style={{
                  flex: 1, height: 30, fontSize: 12, padding: "0 8px",
                  border: "1px solid var(--rf-border-strong)", background: "var(--rf-app-bg)",
                  color: "var(--rf-text)", borderRadius: "var(--rf-radius-sm)", fontFamily: "var(--rf-mono)",
                }}
              />
            </div>

            {/* Reset */}
            {activeFilterCount > 0 && (
              <button
                type="button"
                style={{
                  marginTop: 14, width: "100%", fontSize: 12, padding: "5px 0",
                  border: "1px solid var(--rf-border-strong)", borderRadius: "var(--rf-radius-sm)",
                  background: "transparent", color: "var(--rf-text-muted)", cursor: "pointer",
                  fontFamily: "var(--rf-font)",
                }}
                onClick={() => { setErrorsOnly(false); setAmountMin(""); setAmountMax(""); }}
              >
                Resetează filtrele
              </button>
            )}
          </PopoverContent>
        </Popover>

        {/* Right side: bulk bar or refresh/export/import */}
        <span style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center", flexShrink: 0 }}>
          {selected.size > 0 && (
            <>
              <span style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-text)" }}>
                {selected.size} selectate
              </span>
              <Btn variant="primary" size="sm" icon="cloudUp" onClick={() => void handleBulkSubmit()}>
                Trimite selecția la ANAF
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

          {/* Refresh */}
          <IconBtn icon="refresh" title="Reîncarcă" onClick={() => void refetchPaged()} />

          {/* Export ▾ (Export + Import sections) */}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <ToolbarBtn>
                Export
                <ChevDown />
              </ToolbarBtn>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" style={{ minWidth: 180 }}>
              <DropdownMenuLabel style={{ fontSize: 11, color: "var(--rf-text-dim)", padding: "4px 8px" }}>
                Export
              </DropdownMenuLabel>
              <DropdownMenuItem onClick={() => void handleExportSaga()}>
                <Icon name="download" size={14} />
                SAGA CSV
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => void handleExportXlsx()}>
                <Icon name="table" size={14} />
                {t("invoices.exportXlsx")}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuLabel style={{ fontSize: 11, color: "var(--rf-text-dim)", padding: "4px 8px" }}>
                Import
              </DropdownMenuLabel>
              <DropdownMenuItem onClick={() => void handleImportXml()}>
                <Icon name="upload" size={14} />
                Import XML
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => setShowImportModal(true)}>
                <Icon name="upload" size={14} />
                {t("invoices.importCsv")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </span>
      </div>

      {/* ── Table container ──────────────────────────────────────────────── */}
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
                  <th style={{ width: 36, paddingLeft: 16 }}>
                    {selected.size > 0 && (
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
                    )}
                  </th>
                  <th style={{ width: 134 }} className="sortable sorted">
                    {t("invoices.columns.number")}
                  </th>
                  <th style={{ width: 100 }}>{t("invoices.columns.date")}</th>
                  <th>{t("invoices.columns.customer")}</th>
                  <th className="right" style={{ width: 130 }}>Valoare Net</th>
                  <th className="right" style={{ width: 100 }}>TVA</th>
                  <th className="right" style={{ width: 130 }}>{t("invoices.columns.total")}</th>
                  <th style={{ width: 80 }}>Monedă</th>
                  <th style={{ width: 130 }}>{t("invoices.columns.status")}</th>
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
                        height: 52,
                      }}
                      onClick={() => {
                        setSelectedInvoiceId(inv.id);
                        void navigate({ to: "/invoices/$id", params: { id: inv.id } });
                      }}
                    >
                      <td style={{ width: 36 }} onClick={(e) => e.stopPropagation()}>
                        <input
                          type="checkbox"
                          className="rf-row-check"
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
                      <td style={{ color: "var(--rf-text-muted)", fontSize: 12, fontFamily: "var(--rf-mono)" }}>
                        {inv.currency}
                      </td>
                      <td>
                        <StatusBadge status={inv.status} />
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
                              void navigate({ to: "/invoices/$id", params: { id: inv.id } });
                            }}
                          />
                          {inv.status === "DRAFT" && (
                            <IconBtn
                              icon="pen"
                              ghost
                              title="Editează"
                              onClick={() => void navigate({ to: "/invoices/$id/edit", params: { id: inv.id } })}
                            />
                          )}
                          <IconBtn
                            icon="more"
                            ghost
                            title="Mai multe"
                            onClick={(e) => {
                              if (menuFor === inv.id) { setMenuFor(null); setMenuAnchor(null); }
                              else { setMenuAnchor(e.currentTarget.getBoundingClientRect()); setMenuFor(inv.id); }
                            }}
                          />
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
              <tfoot>
                <tr>
                  <td colSpan={4}>
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
                  <td colSpan={3} style={{ color: "var(--rf-text-dim)", fontSize: 12, fontWeight: 400 }}>RON</td>
                </tr>
              </tfoot>
            </table>
          </div>
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
