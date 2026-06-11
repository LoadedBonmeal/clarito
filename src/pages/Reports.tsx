/**
 * Rapoarte — verbatim port of the design "Rapoarte.html":
 *   .page-head (title + sub + period pill-btn with month/year pop) ·
 *   .rep-grid with 6 .rep-card (jurnal vânzări / jurnal cumpărări / sumar TVA /
 *   export contabilitate Saga+WinMentor / export XLSX / arhivă ZIP) ·
 *   .scr-card "Verificare integritate arhivă" wired to api.archive.verifyIntegrity.
 *
 * ALL wiring preserved: ?view= deep-link tabs (tva/etva/d390/d394/d101/d100/
 * salariu/saft/sales-journal/purchase-journal/accounting-export) with the
 * embedded sub-view components kept as-is, api.reports.generateVatReport,
 * api.reports.exportReport (TVA CSV), api.integrations.exportSagaCsv /
 * exportWinmentorCsv / exportInvoicesXlsx, api.archive.exportZip / getSize /
 * verifyIntegrity, invoices + contacts + received queries.
 */

import { useMemo, useState, useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { useSearch, useNavigate } from "@tanstack/react-router";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec, MONTHS_RO } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Contact, InvoiceStatus } from "@/types";
import type { ReportView } from "@/router";

import { D390View }            from "./reports/D390View";
import { D394View }            from "./reports/D394View";
import { D101View }            from "./reports/D101View";
import { D100View }            from "./reports/D100View";
import { SalaryView }          from "./reports/SalaryView";
import { EtvaView }            from "./reports/EtvaView";
import { SaftView }            from "./reports/SaftView";
import { SalesJournalView }    from "./reports/SalesJournalView";
import { PurchaseJournalView } from "./reports/PurchaseJournalView";
import { AccountingExportView } from "./reports/AccountingExportView";

// ─── helpers ─────────────────────────────────────────────────────────────────

const MONTHS = MONTHS_RO;

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

function fmtBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function buildYearOptions(): number[] {
  const current = new Date().getFullYear();
  const years: number[] = [];
  for (let y = current; y >= current - 5; y--) years.push(y);
  return years;
}

function periodPrefix(year: number, month: number): string {
  const mm = String(month).padStart(2, "0");
  return `${year}-${mm}`;
}

function periodDateRange(year: number, month: number): { dateFrom: string; dateTo: string } {
  const mm      = String(month).padStart(2, "0");
  const lastDay = new Date(year, month, 0).getDate();
  return {
    dateFrom: `${year}-${mm}-01`,
    dateTo:   `${year}-${mm}-${String(lastDay).padStart(2, "0")}`,
  };
}

function vatCategoryLabel(cat: string): string {
  switch (cat) {
    case "S":  return "Standard";
    case "Z":  return "Zero-rated";
    case "E":  return "Scutit";
    case "AE": return "Autolichidare";
    case "K":  return "Intracomunitar";
    case "G":  return "Guvernamental";
    case "O":  return "În afara TVA";
    default:   return cat;
  }
}

// Status → design chip (.chip variants + icon + label) — for the invoice list.
const STATUS_CHIP: Record<InvoiceStatus, { cls: string; icon: string; label: string }> = {
  DRAFT:     { cls: "sent", icon: "docText", label: "Schiță" },
  QUEUED:    { cls: "wait", icon: "clock",   label: "În coadă" },
  SUBMITTED: { cls: "sent", icon: "send",    label: "Trimisă" },
  VALIDATED: { cls: "paid", icon: "check",   label: "Validată" },
  REJECTED:  { cls: "late", icon: "xMark",   label: "Respinsă" },
  STORNED:   { cls: "wait", icon: "undo",    label: "Stornată" },
};

// Icons present in the prototype but absent from the Ic set — inlined verbatim.
const IC_ARROWS_LR = '<path d="M7.5 21 3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5"/>';
const IC_ARCHIVE   = '<path d="m20.25 7.5-.625 10.632a2.25 2.25 0 0 1-2.247 2.118H6.622a2.25 2.25 0 0 1-2.247-2.118L3.75 7.5M10 11.25h4M3.375 7.5h17.25c.621 0 1.125-.504 1.125-1.125v-1.5c0-.621-.504-1.125-1.125-1.125H3.375c-.621 0-1.125.504-1.125 1.125v1.5c0 .621.504 1.125 1.125 1.125Z"/>';
const IC_WARN      = '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

// ─── Tab definitions ─────────────────────────────────────────────────────────

const TABS: { value: ReportView; label: string }[] = [
  { value: "tva",               label: "Sumar TVA"          },
  { value: "etva",              label: "e-TVA"               },
  { value: "d390",              label: "D390"                },
  { value: "d394",              label: "D394"                },
  { value: "d101",              label: "D101"                },
  { value: "d100",              label: "D100"                },
  { value: "salariu",           label: "Salariu (D112)"      },
  { value: "saft",              label: "D406 SAF-T"          },
  { value: "sales-journal",     label: "Jurnal vânzări"      },
  { value: "purchase-journal",  label: "Jurnal cumpărări"    },
  { value: "accounting-export", label: "Export contabil"     },
];

// ─── component ───────────────────────────────────────────────────────────────

export function ReportsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const navigate = useNavigate();

  const { view: viewParam } = useSearch({ from: "/reports" });
  const view: ReportView = viewParam ?? "tva";

  const now = new Date();
  const [selectedYear, setSelectedYear]   = useState(now.getFullYear());
  const [selectedMonth, setSelectedMonth] = useState(now.getMonth() + 1);
  const [exportingVat, setExportingVat]   = useState(false);
  const [exportingSaga, setExportingSaga] = useState(false);
  const [exportingWinmentor, setExportingWinmentor] = useState(false);
  const [exportingXlsx, setExportingXlsx] = useState(false);
  const [exportingZip, setExportingZip]   = useState(false);
  const [openPop, setOpenPop] = useState<"" | "period">("");

  // Close toolbar pops on outside click
  useEffect(() => {
    if (!openPop) return;
    const h = () => setOpenPop("");
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [openPop]);

  const yearOptions = buildYearOptions();
  const { dateFrom, dateTo } = periodDateRange(selectedYear, selectedMonth);

  // ── Queries ──────────────────────────────────────────────────────────────

  const {
    data:    vatReport,
    isLoading: vatLoading,
    isError: vatError,
    error:   vatErr,
    refetch: refetchVat,
  } = useQuery({
    queryKey: queryKeys.vatReport.get(selectedYear, selectedMonth, activeCompanyId ?? ""),
    queryFn:  () =>
      api.reports.generateVatReport(dateFrom, dateTo, activeCompanyId ?? undefined),
    enabled:   !!activeCompanyId,
    staleTime: 60_000,
  });

  const {
    data:    paged,
    isLoading: invoicesLoading,
    isError: invoicesError,
    error:   invoicesErr,
    refetch: refetchInvoices,
  } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    queryFn:  () =>
      api.invoices.list({
        companyId: activeCompanyId ?? undefined,
        page: { offset: 0, limit: 10000 },
      }),
    enabled: !!activeCompanyId,
  });

  const allInvoices       = paged?.items ?? [];
  const validatedInvoices = allInvoices.filter((inv) => inv.status === "VALIDATED");

  const { data: contactList = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn:  () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled:  !!activeCompanyId,
  });

  const contactMap = useMemo(
    () => new Map(contactList.map((c: Contact) => [c.id, c.legalName])),
    [contactList],
  );

  // Received invoices — for the "Jurnal de cumpărări" card count (same query
  // key as PurchaseJournalView so the cache is shared).
  const { data: receivedPaged } = useQuery({
    queryKey: queryKeys.received.list({ companyId: activeCompanyId ?? undefined }),
    queryFn:  () => api.received.list({ companyId: activeCompanyId ?? undefined }),
    enabled:  !!activeCompanyId,
    staleTime: 60_000,
  });
  const periodReceivedCount = useMemo(
    () =>
      (receivedPaged?.items ?? []).filter(
        (inv) => inv.issueDate >= dateFrom && inv.issueDate <= dateTo,
      ).length,
    [receivedPaged, dateFrom, dateTo],
  );

  // Archive size — for the "Arhivă facturi XML + PDF" card.
  const { data: archiveSize } = useQuery({
    queryKey: queryKeys.system.archiveSize,
    queryFn:  () => api.archive.getSize(),
    staleTime: 60_000,
  });

  // Archive integrity — the "Verificare integritate arhivă" card.
  const {
    data:       integrity,
    isLoading:  integrityLoading,
    isFetching: integrityFetching,
    isError:    integrityError,
    error:      integrityErr,
    refetch:    refetchIntegrity,
  } = useQuery({
    queryKey: ["archive-integrity", activeCompanyId ?? ""],
    queryFn:  () => api.archive.verifyIntegrity(activeCompanyId!),
    enabled:   !!activeCompanyId,
    staleTime: 60_000,
  });

  const prefix  = periodPrefix(selectedYear, selectedMonth);
  const periodInvoices = useMemo(
    () => allInvoices.filter((inv) => inv.issueDate.startsWith(prefix)),
    [allInvoices, prefix],
  );

  // REG-STORNO: fiscal set for the Sales Journal = VALIDATED + STORNED.
  // STORNED originals are positive fiscal events in the period they were issued.
  // The negative credit note (VALIDATED) offsets them in its own period.
  // DRAFT / SUBMITTED / QUEUED / REJECTED are not fiscal events yet.
  const periodFiscalInvoices = useMemo(
    () => periodInvoices.filter((inv) => inv.status === "VALIDATED" || inv.status === "STORNED"),
    [periodInvoices],
  );

  const yearValidatedInvoices = useMemo(
    () => validatedInvoices.filter((inv) => inv.issueDate.startsWith(String(selectedYear))),
    [validatedInvoices, selectedYear],
  );

  // ── TVA header / footer stats ─────────────────────────────────────────────
  // Drive the Stats strip (Total net / TVA / cu TVA) from the SAME authoritative
  // `vatReport` totals the "TVA pe cote" table uses — both use VALIDATED+STORNED
  // and RON-converted amounts from the Rust backend.  Using a separate client-side
  // recompute from VALIDATED-only raw-currency invoices produced contradictory
  // totals on the same screen (P2 fix).
  const vatGroups = vatReport?.vatGroups ?? [];
  const vatTotals = vatReport
    ? { base: parseDec(vatReport.totalBase), vat: parseDec(vatReport.totalVat), total: parseDec(vatReport.totalAmount) }
    : { base: 0, vat: 0, total: 0 };

  // `totalCount` is kept as a client-side count since vatReport doesn't expose
  // an invoice count (it's a grouping report).  It uses the fiscal set
  // (VALIDATED + STORNED) to mirror the Rust backend.
  const stats = useMemo(() => {
    const totalCount = periodFiscalInvoices.length;
    return {
      totalCount,
      totalNet:   vatTotals.base,
      totalVat:   vatTotals.vat,
      totalGross: vatTotals.total,
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [periodFiscalInvoices.length, vatTotals.base, vatTotals.vat, vatTotals.total]);

  const isLoading = invoicesLoading || vatLoading;

  // ── Export TVA CSV ────────────────────────────────────────────────────────

  const handleExportVatCsv = async () => {
    if (periodInvoices.length === 0 && vatGroups.length === 0) {
      notify.info("Nu există date pentru perioada selectată.");
      return;
    }
    const outputPath = await saveDialog({
      title: "Salvează raport TVA",
      defaultPath: `raport-tva-${selectedYear}-${String(selectedMonth).padStart(2, "0")}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!outputPath) return;
    setExportingVat(true);
    try {
      const saved = await api.reports.exportReport(
        "vat",
        { dateFrom, dateTo, companyId: activeCompanyId ?? undefined },
        "csv",
        outputPath,
      );
      notify.success(`Raport TVA salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta raportul TVA."));
    } finally {
      setExportingVat(false);
    }
  };

  // ── Export contabilitate (SAGA / WinMentor) — same wiring as the
  //    accounting-export tab, exposed as quick links on the report card. ─────

  const handleExportSaga = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (periodInvoices.length === 0) {
      notify.info("Nu există date pentru perioada selectată.");
      return;
    }
    const savePath = await saveDialog({
      title:       "Salvează export SAGA",
      defaultPath: `facturi-saga-${dateFrom}-${dateTo}.csv`,
      filters:     [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!savePath) return;
    setExportingSaga(true);
    try {
      const saved = await api.integrations.exportSagaCsv(activeCompanyId, dateFrom, dateTo, savePath);
      notify.success(`Export SAGA salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta în SAGA."));
    } finally {
      setExportingSaga(false);
    }
  };

  const handleExportWinmentor = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (periodInvoices.length === 0) {
      notify.info("Nu există date pentru perioada selectată.");
      return;
    }
    const savePath = await saveDialog({
      title:       "Salvează export WinMentor",
      defaultPath: `facturi-winmentor-${dateFrom}-${dateTo}.csv`,
      filters:     [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!savePath) return;
    setExportingWinmentor(true);
    try {
      const saved = await api.integrations.exportWinmentorCsv(activeCompanyId, dateFrom, dateTo, savePath);
      notify.success(`Export WinMentor salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta în WinMentor."));
    } finally {
      setExportingWinmentor(false);
    }
  };

  // ── Export XLSX general ───────────────────────────────────────────────────

  const handleExportXlsx = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    const savePath = await saveDialog({
      title:       "Salvează export XLSX",
      defaultPath: "facturi.xlsx",
      filters:     [{ name: "Excel", extensions: ["xlsx"] }],
    });
    if (!savePath) return;
    setExportingXlsx(true);
    try {
      await api.integrations.exportInvoicesXlsx({ companyId: activeCompanyId }, savePath);
      notify.success(`Export salvat: ${savePath}`);
      try { await openPath(savePath); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta XLSX."));
    } finally {
      setExportingXlsx(false);
    }
  };

  // ── Arhivă ZIP ────────────────────────────────────────────────────────────

  const handleArchiveZip = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setExportingZip(true);
    try {
      const path = await api.archive.exportZip(activeCompanyId);
      notify.success(`Arhivă exportată: ${path}`);
      try { await openPath(path); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Exportul arhivei a eșuat."));
    } finally {
      setExportingZip(false);
    }
  };

  // ── Tab navigation ────────────────────────────────────────────────────────

  function goToView(v: ReportView) {
    void navigate({ to: "/reports", search: { view: v } });
  }

  // ── Empty state ───────────────────────────────────────────────────────────

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>Rapoarte</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Selectați o companie activă pentru a vedea rapoartele.
        </div>
      </div>
    );
  }

  const integrityChecked        = integrity?.checked ?? 0;
  const integrityMissing        = integrity?.missing ?? [];
  const integrityOk             = integrity?.ok ?? true;
  const missingUnderRetention   = integrity?.missingUnderRetention ?? 0;

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Rapoarte</h1>
          <p className="sub">
            Sumar TVA · jurnale · exporturi contabile · arhivă — {periodInvoices.length} facturi emise în {MONTHS[selectedMonth - 1]} {selectedYear}
          </p>
        </div>
        <div className="head-actions">
          {/* period pill */}
          <div className="nou-wrap" style={{ position: "relative" }}>
            <button
              className="pill-btn"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => setOpenPop(openPop === "period" ? "" : "period")}
            >
              <Ic name="calendar" />
              Perioada: {MONTHS[selectedMonth - 1]} {selectedYear}
              <Ic name="chevD" cls="ic" />
            </button>
            {openPop === "period" && (
              <div
                className="pop show"
                style={{ right: 0, top: 40, width: 210, maxHeight: 320, overflowY: "auto" }}
                onMouseDown={(e) => e.stopPropagation()}
              >
                <div className="col-title">Lună</div>
                {MONTHS.map((label, idx) => (
                  <button
                    key={label}
                    className="pop-item"
                    onClick={() => { setSelectedMonth(idx + 1); setOpenPop(""); }}
                  >
                    <span style={{ flex: 1 }}>{label}</span>
                    {selectedMonth === idx + 1 && <Ic name="check" cls="co-check" />}
                  </button>
                ))}
                <div className="pop-div" />
                <div className="col-title">An</div>
                {yearOptions.map((y) => (
                  <button
                    key={y}
                    className="pop-item"
                    onClick={() => { setSelectedYear(y); setOpenPop(""); }}
                  >
                    <span style={{ flex: 1 }} className="num">{y}</span>
                    {selectedYear === y && <Ic name="check" cls="co-check" />}
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>

      {/* tab bar */}
      <div style={{ marginBottom: 16, overflowX: "auto" }}>
        <div className="tabs" style={{ width: "max-content" }}>
          {TABS.map((t) => (
            <div
              key={t.value}
              className={`tab${view === t.value ? " active" : ""}`}
              onClick={() => goToView(t.value)}
            >
              {t.label}
            </div>
          ))}
        </div>
      </div>

      {/* truncation warning */}
      {paged && paged.total > paged.items.length && (
        <div style={{ marginBottom: 12, fontSize: 12, color: "var(--amber)" }}>
          Afișate primele {paged.items.length.toLocaleString("ro-RO")} din {paged.total.toLocaleString("ro-RO")} facturi — restrânge filtrele pentru a vedea toate înregistrările.
        </div>
      )}

      {/* ── Sumar TVA (default) ─────────────────────────────────────────── */}
      {view === "tva" && (
        <>
          {/* report cards */}
          <div className="rep-grid" style={{ marginBottom: 16 }}>
            <div className="rep-card" onClick={() => goToView("sales-journal")}>
              <div className="rep-ic"><Ic name="chart" /></div>
              <div className="rep-t">Jurnal de vânzări</div>
              <div className="rep-s">Toate facturile emise pe perioadă, defalcat pe cote TVA — export CSV</div>
              <div className="rep-foot">
                <span className="rep-link">Generează →</span>
                <span className="muted" style={{ fontSize: 11.5 }}>{periodFiscalInvoices.length} documente</span>
              </div>
            </div>
            <div className="rep-card" onClick={() => goToView("purchase-journal")}>
              <div className="rep-ic"><Ic name="docDown" /></div>
              <div className="rep-t">Jurnal de cumpărări</div>
              <div className="rep-s">Facturile primite cu TVA deductibilă, doar documente cu defalcare completă</div>
              <div className="rep-foot">
                <span className="rep-link">Generează →</span>
                <span className="muted" style={{ fontSize: 11.5 }}>{periodReceivedCount} documente</span>
              </div>
            </div>
            <div className="rep-card" onClick={() => void handleExportVatCsv()}>
              <div className="rep-ic"><Ic name="chart" /></div>
              <div className="rep-t">Sumar TVA pe cote</div>
              <div className="rep-s">Bază și TVA pe fiecare cotă și categorie (S/AE/E/Z), pentru perioada selectată</div>
              <div className="rep-foot">
                <span className="rep-link">{exportingVat ? "Export…" : "Generează →"}</span>
                <span className="muted" style={{ fontSize: 11.5 }}>CSV</span>
              </div>
            </div>
            <div className="rep-card" onClick={() => goToView("accounting-export")}>
              <div className="rep-ic">
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_ARROWS_LR }} />
              </div>
              <div className="rep-t">Export contabilitate</div>
              <div className="rep-s">Note contabile și parteneri pentru import în alt soft de contabilitate</div>
              <div className="rep-foot">
                <span
                  className="rep-link"
                  onClick={(e) => { e.stopPropagation(); void handleExportSaga(); }}
                >
                  {exportingSaga ? "Export…" : "Saga CSV →"}
                </span>
                <span
                  className="rep-link"
                  onClick={(e) => { e.stopPropagation(); void handleExportWinmentor(); }}
                >
                  {exportingWinmentor ? "Export…" : "WinMentor →"}
                </span>
              </div>
            </div>
            <div className="rep-card" onClick={() => void handleExportXlsx()}>
              <div className="rep-ic"><Ic name="dl" /></div>
              <div className="rep-t">Export XLSX general</div>
              <div className="rep-s">Facturi cu net, TVA și total în foaie de calcul, gata de pivot</div>
              <div className="rep-foot">
                <span className="rep-link">{exportingXlsx ? "Export…" : "Descarcă →"}</span>
                <span className="muted" style={{ fontSize: 11.5 }}>XLSX</span>
              </div>
            </div>
            <div className="rep-card" onClick={() => void handleArchiveZip()}>
              <div className="rep-ic">
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_ARCHIVE }} />
              </div>
              <div className="rep-t">Arhivă facturi XML + PDF</div>
              <div className="rep-s">Toate e-Facturile cu recipisele semnate ANAF, organizate pe luni — ZIP</div>
              <div className="rep-foot">
                <span className="rep-link">{exportingZip ? "Export…" : "Descarcă arhiva →"}</span>
                <span className="muted" style={{ fontSize: 11.5 }}>
                  {archiveSize != null ? fmtBytes(archiveSize) : "—"}
                </span>
              </div>
            </div>
          </div>

          {/* stats strip */}
          <div className="kpis">
            <div className="kpi">
              <div className="top"><span className="klabel">Total facturi emise</span></div>
              <div className="val num">{stats.totalCount}</div>
            </div>
            <div className="kpi">
              <div className="top"><span className="klabel">Total net (RON)</span></div>
              <div className="val num">{fmtRON(stats.totalNet)}</div>
            </div>
            <div className="kpi">
              <div className="top"><span className="klabel">Total TVA (RON)</span></div>
              <div className="val num">{fmtRON(stats.totalVat)}</div>
            </div>
            <div className="kpi">
              <div className="top"><span className="klabel">Total cu TVA (RON)</span></div>
              <div className="val num">{fmtRON(stats.totalGross)}</div>
            </div>
          </div>

          {/* TVA table + bar chart */}
          <div
            style={{
              display: "grid",
              gridTemplateColumns: vatGroups.length > 0 ? "1fr 320px" : "1fr",
              gap: 16,
              alignItems: "start",
              marginBottom: 16,
            }}
          >
            <div className="scr-card">
              <div className="scr-toolbar">
                <div className="tt">TVA colectată pe cote — {MONTHS[selectedMonth - 1]} {selectedYear}</div>
                <div className="spacer" />
                <button
                  className="pill-btn"
                  disabled={exportingVat}
                  onClick={() => void handleExportVatCsv()}
                >
                  <Ic name="dl" />{exportingVat ? "Export…" : "Export TVA CSV"}
                </button>
              </div>
              {isLoading ? (
                <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
              ) : vatError ? (
                <div style={{ padding: 16 }}>
                  <QueryErrorBanner error={vatErr} label="raportul TVA" onRetry={() => void refetchVat()} />
                </div>
              ) : vatGroups.length === 0 ? (
                <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
                  Nicio factură validată în perioada selectată.
                </div>
              ) : (
                <>
                  <table className="scr-table">
                    <thead>
                      <tr>
                        <th>Cotă</th>
                        <th>Categorie</th>
                        <th className="r">Bază</th>
                        <th className="r">TVA</th>
                        <th className="r">Total</th>
                      </tr>
                    </thead>
                    <tbody>
                      {vatGroups.map((g) => (
                        <tr key={`${g.rate}-${g.vatCategory}`}>
                          <td className="num" style={{ fontWeight: 700, color: "var(--text)" }}>{g.rate}%</td>
                          <td style={{ color: "var(--text-2)" }}>
                            {g.vatCategory} — {vatCategoryLabel(g.vatCategory)}
                          </td>
                          <td className="r num">{fmtRON(g.baseAmount)}</td>
                          <td className="r num" style={{ color: "var(--text-2)" }}>{fmtRON(g.vatAmount)}</td>
                          <td className="r num"><b>{fmtRON(parseDec(g.baseAmount) + parseDec(g.vatAmount))}</b></td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  <div className="tot-foot">
                    <span>Total: bază <b className="num">{fmtRON(vatTotals.base)}</b></span>
                    <span>TVA <b className="num">{fmtRON(vatTotals.vat)}</b></span>
                    <span>total <b className="num">{fmtRON(vatTotals.total)}</b></span>
                  </div>
                </>
              )}
            </div>

            {/* TVA bar chart (CSS-only) — real functionality kept, restyled */}
            {vatGroups.length > 0 && (
              <div className="scr-card">
                <div className="scr-toolbar">
                  <div className="tt">TVA pe cote</div>
                </div>
                <div style={{ display: "flex", flexDirection: "column", gap: 14, padding: "14px 16px 16px" }}>
                  {(() => {
                    const maxVat = Math.max(...vatGroups.map((g) => parseDec(g.vatAmount)));
                    return vatGroups.map((g) => {
                      const vatVal = parseDec(g.vatAmount);
                      const pct    = maxVat > 0 ? (vatVal / maxVat) * 100 : 0;
                      return (
                        <div key={`${g.rate}-${g.vatCategory}`}>
                          <div style={{ display: "flex", justifyContent: "space-between", fontSize: 12.5, marginBottom: 5 }}>
                            <span className="num" style={{ fontWeight: 600 }}>{g.rate}%</span>
                            <span className="num" style={{ color: "var(--text-2)" }}>{fmtRON(vatVal)}</span>
                          </div>
                          <div style={{ height: 9, background: "var(--fill)", borderRadius: 999 }}>
                            <div
                              style={{
                                width: `${pct}%`,
                                height: "100%",
                                background: "var(--black)",
                                borderRadius: 999,
                                minWidth: vatVal ? 4 : 0,
                                transition: "width .3s",
                              }}
                            />
                          </div>
                        </div>
                      );
                    });
                  })()}
                </div>
              </div>
            )}
          </div>

          {/* invoice list */}
          {periodInvoices.length > 0 && (
            <div className="scr-card" style={{ marginBottom: 16 }}>
              <div className="scr-toolbar">
                <div className="tt">Facturi emise — {MONTHS[selectedMonth - 1]} {selectedYear}</div>
              </div>
              {isLoading ? (
                <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
              ) : (
                <>
                  <table className="scr-table">
                    <thead>
                      <tr>
                        <th>Număr</th>
                        <th>Client</th>
                        <th>Data</th>
                        <th>Status</th>
                        <th className="r">Net (RON)</th>
                        <th className="r">TVA (RON)</th>
                        <th className="r">Total (RON)</th>
                      </tr>
                    </thead>
                    <tbody>
                      {periodInvoices.map((inv) => {
                        const chip = STATUS_CHIP[inv.status] ?? STATUS_CHIP.DRAFT;
                        return (
                          <tr key={inv.id}>
                            <td><span className="doc" style={{ fontWeight: 700, color: "var(--text)" }}>{inv.fullNumber}</span></td>
                            <td><div className="cli">{contactMap.get(inv.contactId) ?? inv.contactId}</div></td>
                            <td className="num">{fmtRoDate(inv.issueDate)}</td>
                            <td>
                              <span className={`chip ${chip.cls}`}><Ic name={chip.icon} cls="sic" />{chip.label}</span>
                            </td>
                            <td className="r num">{fmtRON(inv.subtotalAmount)}</td>
                            <td className="r num" style={{ color: "var(--text-2)" }}>{fmtRON(inv.vatAmount)}</td>
                            <td className="r num"><b>{fmtRON(inv.totalAmount)}</b></td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                  <div className="tot-foot">
                    <span>TOTAL perioadă: net <b className="num">{fmtRON(stats.totalNet)}</b></span>
                    <span>TVA <b className="num">{fmtRON(stats.totalVat)}</b></span>
                    <span>total <b className="num">{fmtRON(stats.totalGross)}</b></span>
                  </div>
                </>
              )}
            </div>
          )}

          {/* verificare integritate arhivă */}
          <div className="scr-card">
            <div className="scr-toolbar">
              <div className="tt">Verificare integritate arhivă</div>
              {integrity && !integrityOk && (
                <span className="chip late">
                  <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
                  {integrityMissing.length === 1 ? "1 fișier lipsă" : `${integrityMissing.length} fișiere lipsă`}
                </span>
              )}
              {integrity && integrityOk && (
                <span className="chip paid"><Ic name="checkC" cls="sic" />Arhivă integră</span>
              )}
              <div className="spacer" />
              <button
                className="pill-btn spin-btn"
                disabled={integrityFetching}
                onClick={() => void refetchIntegrity()}
              >
                <Ic name="sync" />{integrityFetching ? "Se verifică…" : "Rerulează verificarea"}
              </button>
            </div>
            {integrityLoading ? (
              <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se verifică arhiva…</div>
            ) : integrityError ? (
              <div style={{ padding: 16 }}>
                <QueryErrorBanner error={integrityErr} label="integritatea arhivei" onRetry={() => void refetchIntegrity()} />
              </div>
            ) : (
              <table className="scr-table">
                <thead>
                  <tr><th>Verificare</th><th>Detaliu</th><th className="r">Rezultat</th><th>Status</th></tr>
                </thead>
                <tbody>
                  <tr>
                    <td>Documente arhivate</td>
                    <td>XML e-Factura + PDF + recipise semnate ANAF</td>
                    <td className="r num">{integrityChecked - integrityMissing.length} / {integrityChecked}</td>
                    <td>
                      {integrityOk ? (
                        <span className="chip paid"><Ic name="checkC" cls="sic" />Complet</span>
                      ) : (
                        <span className="chip late">
                          <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
                          {integrityMissing.length} lipsă
                        </span>
                      )}
                    </td>
                  </tr>
                  {integrityMissing.length > 0 && (
                    <tr>
                      <td>Fișiere lipsă</td>
                      <td>
                        {integrityMissing.slice(0, 4).map((f, i) => (
                          <span key={f}>
                            {i > 0 && " · "}
                            <span className="doc">{f}</span>
                          </span>
                        ))}
                        {integrityMissing.length > 4 && ` … (+${integrityMissing.length - 4})`}
                        {" — pot fi redescărcate din SPV (dacă au sub 60 de zile) sau restaurate din backup"}
                      </td>
                      <td className="r num">{integrityMissing.length}</td>
                      <td>
                        {/* propunere — neimplementat: redescărcare țintită din SPV */}
                        <button
                          className="pill-btn"
                          style={{ height: 26, fontSize: 11.5, padding: "0 9px" }}
                          onClick={() => notify.info("În curând.")}
                        >
                          Redescarcă din SPV
                        </button>
                      </td>
                    </tr>
                  )}
                  <tr>
                    <td>Lipsă sub termenul de păstrare</td>
                    <td>Documente lipsă aflate încă în perioada legală de păstrare de <b>5 ani</b> — trebuie recuperate obligatoriu</td>
                    <td className={`r num${missingUnderRetention > 0 ? " neg" : ""}`}>{missingUnderRetention}</td>
                    <td>
                      {missingUnderRetention > 0 ? (
                        <span className="chip late">
                          <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
                          Acțiune necesară
                        </span>
                      ) : (
                        <span className="chip paid"><Ic name="checkC" cls="sic" />Conform</span>
                      )}
                    </td>
                  </tr>
                  <tr>
                    <td>Termen legal de păstrare</td>
                    <td>Documentele se păstrează <b>5 ani</b> de la sfârșitul exercițiului financiar (L82/1991)</td>
                    <td className="r num">—</td>
                    <td><span className="chip paid"><Ic name="checkC" cls="sic" />Conform</span></td>
                  </tr>
                </tbody>
              </table>
            )}
          </div>
        </>
      )}

      {/* ── e-TVA / declarații ─────────────────────────────────────────────── */}
      {view === "etva" && (
        <EtvaView dateFrom={dateFrom} dateTo={dateTo} />
      )}

      {view === "d390" && (
        <D390View dateFrom={dateFrom} dateTo={dateTo} />
      )}

      {view === "d394" && (
        <D394View dateFrom={dateFrom} dateTo={dateTo} />
      )}

      {view === "d101" && (
        <D101View dateFrom={dateFrom} dateTo={dateTo} />
      )}

      {view === "d100" && (
        <D100View dateFrom={dateFrom} dateTo={dateTo} />
      )}

      {view === "salariu" && <SalaryView />}

      {/* ── SAF-T ──────────────────────────────────────────────────────────── */}
      {view === "saft" && (
        <>
          {invoicesError && (
            <QueryErrorBanner
              error={invoicesErr}
              label="facturile anului"
              onRetry={() => void refetchInvoices()}
            />
          )}
          <SaftView
            selectedYear={selectedYear}
            selectedMonth={selectedMonth}
            allInvoicesForYear={yearValidatedInvoices}
          />
        </>
      )}

      {/* ── Jurnal vânzări ─────────────────────────────────────────────────── */}
      {view === "sales-journal" && (
        <>
          {invoicesError && (
            <QueryErrorBanner
              error={invoicesErr}
              label="facturile perioadei"
              onRetry={() => void refetchInvoices()}
            />
          )}
          <SalesJournalView
            periodInvoices={periodFiscalInvoices}
            contactMap={contactMap}
            dateFrom={dateFrom}
            dateTo={dateTo}
            isLoading={invoicesLoading}
          />
        </>
      )}

      {/* ── Jurnal cumpărări ───────────────────────────────────────────────── */}
      {view === "purchase-journal" && (
        <PurchaseJournalView dateFrom={dateFrom} dateTo={dateTo} />
      )}

      {/* ── Export contabil ────────────────────────────────────────────────── */}
      {view === "accounting-export" && (
        <>
          {invoicesError && (
            <QueryErrorBanner
              error={invoicesErr}
              label="facturile perioadei"
              onRetry={() => void refetchInvoices()}
            />
          )}
          <AccountingExportView
            periodInvoices={periodInvoices}
            dateFrom={dateFrom}
            dateTo={dateTo}
          />
        </>
      )}
    </div>
  );
}
