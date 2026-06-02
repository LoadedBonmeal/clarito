/**
 * Rapoarte — shell cu tab-uri per tip de raport, selectat prin ?view= param.
 * Wave 5 — rf look: PageHeader + Segmented + Tabs + SectionCard + rf-tbl
 *
 * Views disponibile:
 *  tva              — sumar TVA (default)
 *  d394             — D394 livrări per partener
 *  saft             — D406 SAF-T export
 *  sales-journal    — jurnal de vânzări
 *  purchase-journal — jurnal de cumpărări
 *  accounting-export— export SAGA / WinMentor
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useSearch, useNavigate } from "@tanstack/react-router";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import {
  PageHeader,
  Segmented,
  Tabs,
  SectionCard,
  Card,
  Btn,
  Empty,
} from "@/components/rf";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Contact } from "@/types";
import type { ReportView } from "@/router";

import { D394View }            from "./reports/D394View";
import { SaftView }            from "./reports/SaftView";
import { SalesJournalView }    from "./reports/SalesJournalView";
import { PurchaseJournalView } from "./reports/PurchaseJournalView";
import { AccountingExportView } from "./reports/AccountingExportView";

// ─── helpers ─────────────────────────────────────────────────────────────────

const MONTHS = [
  "Ianuarie", "Februarie", "Martie", "Aprilie", "Mai", "Iunie",
  "Iulie", "August", "Septembrie", "Octombrie", "Noiembrie", "Decembrie",
];

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

// ─── Tab definitions ─────────────────────────────────────────────────────────

const TABS: { value: ReportView; label: string }[] = [
  { value: "tva",               label: "Sumar TVA"          },
  { value: "d394",              label: "D394"                },
  { value: "saft",              label: "D406 SAF-T"          },
  { value: "sales-journal",     label: "Jurnal vânzări"      },
  { value: "purchase-journal",  label: "Jurnal cumpărări"    },
  { value: "accounting-export", label: "Export contabil"     },
];

// ─── Period options (month + year) ───────────────────────────────────────────

function buildMonthOptions(): { value: string; label: string }[] {
  return MONTHS.map((label, idx) => ({ value: String(idx + 1), label }));
}

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

  const yearOptions  = buildYearOptions();
  const monthOptions = buildMonthOptions();
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
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 500 } }),
    queryFn:  () =>
      api.invoices.list({
        companyId: activeCompanyId ?? undefined,
        page: { offset: 0, limit: 500 },
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

  const periodValidatedInvoices = useMemo(
    () => validatedInvoices.filter((inv) => inv.issueDate.startsWith(prefix)),
    [validatedInvoices, prefix],
  );

  const yearValidatedInvoices = useMemo(
    () => validatedInvoices.filter((inv) => inv.issueDate.startsWith(String(selectedYear))),
    [validatedInvoices, selectedYear],
  );

  const stats = useMemo(() => {
    const totalCount = periodValidatedInvoices.length;
    const totalNet   = periodValidatedInvoices.reduce((s, i) => s + parseDec(i.subtotalAmount), 0);
    const totalVat   = periodValidatedInvoices.reduce((s, i) => s + parseDec(i.vatAmount), 0);
    const totalGross = periodValidatedInvoices.reduce((s, i) => s + parseDec(i.totalAmount), 0);
    return { totalCount, totalNet, totalVat, totalGross };
  }, [periodValidatedInvoices]);

  const vatGroups = vatReport?.vatGroups ?? [];
  const vatTotals = vatReport
    ? { base: parseDec(vatReport.totalBase), vat: parseDec(vatReport.totalVat), total: parseDec(vatReport.totalAmount) }
    : { base: 0, vat: 0, total: 0 };

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

  // ── Tab navigation ────────────────────────────────────────────────────────

  function goToView(v: ReportView) {
    void navigate({ to: "/reports", search: { view: v } });
  }

  // ── Period segments for header ────────────────────────────────────────────

  const monthSegOptions = monthOptions.map((m) => ({ value: m.value, label: m.label.slice(0, 3) }));
  const yearSegOptions  = yearOptions.map((y) => ({ value: String(y), label: String(y) }));

  if (!activeCompanyId) {
    return (
      <div className="rf-content">
        <PageHeader title="Rapoarte" />
        <div className="rf-page-body">
          <Card pad>
            <Empty icon="chart" title="Selectați o companie activă pentru a vedea rapoartele." />
          </Card>
        </div>
      </div>
    );
  }

  return (
    <div className="rf-content">
      <PageHeader
        title="Rapoarte"
        actions={
          <>
            {view !== "saft" && (
              <Segmented
                options={monthSegOptions}
                value={String(selectedMonth)}
                onChange={(v) => setSelectedMonth(Number(v))}
              />
            )}
            <Segmented
              options={yearSegOptions}
              value={String(selectedYear)}
              onChange={(v) => setSelectedYear(Number(v))}
            />
          </>
        }
      />

      {/* ── Tab bar ─────────────────────────────────────────────────────── */}
      <div style={{ padding: "0 32px", background: "var(--rf-app-bg)" }}>
        <Tabs tabs={TABS} value={view} onChange={goToView} />
      </div>

      {/* ── Period info line ─────────────────────────────────────────────── */}
      {view !== "saft" && (
        <div style={{ padding: "10px 32px 0", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
          {MONTHS[selectedMonth - 1]} {selectedYear} · {periodInvoices.length} facturi emise în perioadă
        </div>
      )}

      {/* ── View content ─────────────────────────────────────────────────── */}
      <div className="rf-page-body" style={{ paddingTop: 20 }}>

        {/* ── TVA (default) ──────────────────────────────────────────────── */}
        {view === "tva" && (
          <>
            {/* Stats strip */}
            <div className="rf-grid-3" style={{ gridTemplateColumns: "repeat(4, 1fr)" }}>
              <StatMiniCard label="Total facturi emise" value={String(stats.totalCount)} />
              <StatMiniCard label="Total net (RON)"     value={fmtRON(stats.totalNet)} />
              <StatMiniCard label="Total TVA (RON)"     value={fmtRON(stats.totalVat)} />
              <StatMiniCard label="Total cu TVA (RON)"  value={fmtRON(stats.totalGross)} highlight />
            </div>

            <div style={{ display: "grid", gridTemplateColumns: "1fr 320px", gap: 20, alignItems: "start" }}>
              {/* TVA table */}
              <SectionCard
                icon="chart"
                title={`TVA colectată pe cote — ${MONTHS[selectedMonth - 1]} ${selectedYear}`}
                actions={
                  <Btn
                    variant="secondary"
                    size="sm"
                    icon="download"
                    disabled={exportingVat}
                    onClick={() => void handleExportVatCsv()}
                  >
                    {exportingVat ? "Export…" : "Export TVA CSV"}
                  </Btn>
                }
              >
                {isLoading ? (
                  <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>Se încarcă…</div>
                ) : vatError ? (
                  <div style={{ padding: "0 16px 16px" }}>
                    <QueryErrorBanner error={vatErr} label="raportul TVA" onRetry={() => void refetchVat()} />
                  </div>
                ) : vatGroups.length === 0 ? (
                  <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
                    Nicio factură validată în perioada selectată.
                  </div>
                ) : (
                  <div className="rf-tbl-wrap">
                    <table className="rf-tbl">
                      <thead>
                        <tr>
                          <th>Cotă</th>
                          <th>Categorie</th>
                          <th className="right">Bază</th>
                          <th className="right">TVA</th>
                          <th className="right">Total</th>
                        </tr>
                      </thead>
                      <tbody>
                        {vatGroups.map((g) => (
                          <tr key={`${g.rate}-${g.vatCategory}`}>
                            <td className="rf-mono" style={{ fontWeight: 600 }}>{g.rate}%</td>
                            <td style={{ color: "var(--rf-text-muted)" }}>
                              {g.vatCategory} — {vatCategoryLabel(g.vatCategory)}
                            </td>
                            <td className="right rf-mono">{fmtRON(g.baseAmount)}</td>
                            <td className="right rf-mono" style={{ color: "var(--rf-text-muted)" }}>{fmtRON(g.vatAmount)}</td>
                            <td className="right rf-mono" style={{ fontWeight: 600 }}>
                              {fmtRON(parseDec(g.baseAmount) + parseDec(g.vatAmount))}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                      <tfoot>
                        <tr>
                          <td colSpan={2}>Total</td>
                          <td className="right rf-mono">{fmtRON(vatTotals.base)}</td>
                          <td className="right rf-mono">{fmtRON(vatTotals.vat)}</td>
                          <td className="right rf-mono">{fmtRON(vatTotals.total)}</td>
                        </tr>
                      </tfoot>
                    </table>
                  </div>
                )}
              </SectionCard>

              {/* TVA bar chart (CSS-only) */}
              {vatGroups.length > 0 && (
                <SectionCard icon="chart" title="TVA pe cote">
                  <div style={{ display: "flex", flexDirection: "column", gap: 14, paddingTop: 4 }}>
                    {(() => {
                      const maxVat = Math.max(...vatGroups.map((g) => parseDec(g.vatAmount)));
                      return vatGroups.map((g) => {
                        const vatVal = parseDec(g.vatAmount);
                        const pct    = maxVat > 0 ? (vatVal / maxVat) * 100 : 0;
                        return (
                          <div key={`${g.rate}-${g.vatCategory}`}>
                            <div style={{ display: "flex", justifyContent: "space-between", fontSize: 12.5, marginBottom: 5 }}>
                              <span style={{ fontWeight: 600 }} className="rf-mono">{g.rate}%</span>
                              <span className="rf-mono" style={{ color: "var(--rf-text-muted)" }}>{fmtRON(vatVal)}</span>
                            </div>
                            <div style={{ height: 9, background: "var(--rf-neutral-bg)", borderRadius: 999 }}>
                              <div
                                style={{
                                  width: `${pct}%`,
                                  height: "100%",
                                  background: "var(--rf-accent)",
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
                </SectionCard>
              )}
            </div>

            {/* Invoice list */}
            {periodInvoices.length > 0 && (
              <SectionCard
                icon="fileOut"
                title={`Facturi emise — ${MONTHS[selectedMonth - 1]} ${selectedYear}`}
              >
                {isLoading ? (
                  <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>Se încarcă…</div>
                ) : (
                  <div className="rf-tbl-wrap">
                    <table className="rf-tbl">
                      <thead>
                        <tr>
                          <th>Număr</th>
                          <th>Client</th>
                          <th>Data</th>
                          <th>Status</th>
                          <th className="right">Net (RON)</th>
                          <th className="right">TVA (RON)</th>
                          <th className="right">Total (RON)</th>
                        </tr>
                      </thead>
                      <tbody>
                        {periodInvoices.map((inv) => (
                          <tr key={inv.id}>
                            <td className="rf-mono" style={{ fontWeight: 600 }}>{inv.fullNumber}</td>
                            <td style={{ fontSize: 12.5 }}>
                              {contactMap.get(inv.contactId) ?? inv.contactId}
                            </td>
                            <td style={{ color: "var(--rf-text-muted)" }}>{inv.issueDate}</td>
                            <td><StatusBadge status={inv.status} /></td>
                            <td className="right rf-mono" style={{ color: "var(--rf-text-muted)" }}>{fmtRON(inv.subtotalAmount)}</td>
                            <td className="right rf-mono" style={{ color: "var(--rf-text-dim)" }}>{fmtRON(inv.vatAmount)}</td>
                            <td className="right rf-mono" style={{ fontWeight: 600 }}>{fmtRON(inv.totalAmount)}</td>
                          </tr>
                        ))}
                      </tbody>
                      <tfoot>
                        <tr>
                          <td colSpan={4}>TOTAL perioadă</td>
                          <td className="right rf-mono">{fmtRON(stats.totalNet)}</td>
                          <td className="right rf-mono">{fmtRON(stats.totalVat)}</td>
                          <td className="right rf-mono">{fmtRON(stats.totalGross)}</td>
                        </tr>
                      </tfoot>
                    </table>
                  </div>
                )}
              </SectionCard>
            )}
          </>
        )}

        {/* ── D394 ───────────────────────────────────────────────────────── */}
        {view === "d394" && (
          <D394View dateFrom={dateFrom} dateTo={dateTo} />
        )}

        {/* ── SAF-T ──────────────────────────────────────────────────────── */}
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
              allInvoicesForYear={yearValidatedInvoices}
            />
          </>
        )}

        {/* ── Jurnal vânzări ─────────────────────────────────────────────── */}
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

        {/* ── Jurnal cumpărări ───────────────────────────────────────────── */}
        {view === "purchase-journal" && (
          <PurchaseJournalView dateFrom={dateFrom} dateTo={dateTo} />
        )}

        {/* ── Export contabil ────────────────────────────────────────────── */}
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
    </div>
  );
}

// ─── StatMiniCard ─────────────────────────────────────────────────────────────

function StatMiniCard({
  label,
  value,
  highlight = false,
}: {
  label: string;
  value: string;
  highlight?: boolean;
}) {
  return (
    <Card>
      <div style={{ padding: "14px 16px" }}>
        <div style={{ fontSize: 11.5, color: "var(--rf-text-muted)", fontWeight: 500, marginBottom: 6 }}>
          {label}
        </div>
        <div
          style={{
            fontSize: 18,
            fontWeight: 700,
            color: highlight ? "var(--rf-accent)" : "var(--rf-text)",
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {value}
        </div>
      </div>
    </Card>
  );
}
