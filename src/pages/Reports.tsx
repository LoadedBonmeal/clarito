/**
 * Rapoarte — TVA summary, statistici facturi, tabel per perioadă.
 * TVA breakdown din backend (generate_vat_report); lista facturi din invoices.list.
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON } from "@/lib/utils";
import type { Invoice, Contact } from "@/types";

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
  const mm = String(month).padStart(2, "0");
  const lastDay = new Date(year, month, 0).getDate();
  return {
    dateFrom: `${year}-${mm}-01`,
    dateTo: `${year}-${mm}-${String(lastDay).padStart(2, "0")}`,
  };
}

// ─── component ───────────────────────────────────────────────────────────────

export function ReportsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const now = new Date();
  const [selectedYear, setSelectedYear] = useState(now.getFullYear());
  const [selectedMonth, setSelectedMonth] = useState(now.getMonth() + 1); // 1-indexed
  const [exportingSaga, setExportingSaga] = useState(false);
  const [exportingWinmentor, setExportingWinmentor] = useState(false);
  const [exportingVat, setExportingVat] = useState(false);

  const yearOptions = buildYearOptions();
  const { dateFrom, dateTo } = periodDateRange(selectedYear, selectedMonth);

  // Backend VAT report — accurate per-rate breakdown from invoice_line_items
  const { data: vatReport, isLoading: vatLoading } = useQuery({
    queryKey: ["vatReport", selectedYear, selectedMonth, activeCompanyId],
    queryFn: () =>
      api.reports.generateVatReport(dateFrom, dateTo, activeCompanyId ?? undefined),
    enabled: true,
    staleTime: 60_000,
  });

  // Fetch invoices for the period list table
  const { data: paged, isLoading: invoicesLoading } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () =>
      api.invoices.list({
        companyId: activeCompanyId ?? undefined,
        page: { offset: 0, limit: 500 },
      }),
    enabled: true,
  });

  const allInvoices = paged?.items ?? [];

  const { data: contactList = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const contactMap = useMemo(
    () => new Map(contactList.map((c: Contact) => [c.id, c.legalName])),
    [contactList],
  );

  const handleExportSaga = async () => {
    if (!activeCompanyId) { alert("Selectați o companie activă."); return; }
    setExportingSaga(true);
    try {
      const path = await api.integrations.exportSagaCsv(activeCompanyId, dateFrom, dateTo);
      alert(`Export Saga salvat:\n${path}`);
    } catch (err) {
      alert("Eroare export Saga: " + String(err));
    } finally {
      setExportingSaga(false);
    }
  };

  const handleExportWinmentor = async () => {
    if (!activeCompanyId) { alert("Selectați o companie activă."); return; }
    setExportingWinmentor(true);
    try {
      const path = await api.integrations.exportWinmentorCsv(activeCompanyId, dateFrom, dateTo);
      alert(`Export WinMentor salvat:\n${path}`);
    } catch (err) {
      alert("Eroare export WinMentor: " + String(err));
    } finally {
      setExportingWinmentor(false);
    }
  };

  const handleExportVatCsv = async () => {
    setExportingVat(true);
    try {
      const outputPath = await saveDialog({
        title: "Salvează raport TVA",
        defaultPath: `raport-tva-${selectedYear}-${String(selectedMonth).padStart(2, "0")}.csv`,
        filters: [{ name: "CSV", extensions: ["csv"] }],
      });
      if (!outputPath) return;
      const saved = await api.reports.exportReport(
        "vat",
        { dateFrom, dateTo, companyId: activeCompanyId ?? undefined },
        "csv",
        outputPath
      );
      alert(`Raport TVA salvat:\n${saved}`);
    } catch (err) {
      alert("Eroare export raport TVA: " + String(err));
    } finally {
      setExportingVat(false);
    }
  };

  // Filter by selected period for the invoice list
  const prefix = periodPrefix(selectedYear, selectedMonth);
  const periodInvoices = useMemo(
    () => allInvoices.filter((inv) => inv.issueDate.startsWith(prefix)),
    [allInvoices, prefix],
  );

  // Invoice statistics from period list
  const stats = useMemo(() => {
    const totalCount = periodInvoices.length;
    const totalNet = periodInvoices.reduce((s, i) => s + i.subtotalAmount, 0);
    const totalVat = periodInvoices.reduce((s, i) => s + i.vatAmount, 0);
    const totalGross = periodInvoices.reduce((s, i) => s + i.totalAmount, 0);
    return { totalCount, totalNet, totalVat, totalGross };
  }, [periodInvoices]);

  // Use backend VAT groups if available, fallback to zeros
  const vatGroups = vatReport?.vatGroups ?? [];
  const vatTotals = vatReport
    ? { base: vatReport.totalBase, vat: vatReport.totalVat, total: vatReport.totalAmount }
    : { base: 0, vat: 0, total: 0 };

  const isLoading = invoicesLoading;

  void vatLoading; // suppress unused warning — used via vatReport

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">e-Factura</span>
          Rapoarte
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
          <button
            type="button"
            className="btn"
            disabled={exportingVat}
            onClick={handleExportVatCsv}
          >
            <Icon name="download" size={12} /> {exportingVat ? "Export…" : "Export TVA CSV"}
          </button>
          <button
            type="button"
            className="btn"
            disabled={exportingSaga || !activeCompanyId}
            onClick={handleExportSaga}
          >
            <Icon name="download" size={12} /> {exportingSaga ? "Export…" : "Export Saga"}
          </button>
          <button
            type="button"
            className="btn"
            disabled={exportingWinmentor || !activeCompanyId}
            onClick={handleExportWinmentor}
          >
            <Icon name="download" size={12} /> {exportingWinmentor ? "Export…" : "Export WinMentor"}
          </button>
        </span>
      </div>

      {/* Period selector */}
      <div style={{ padding: "10px 14px 0", display: "flex", gap: 8, alignItems: "center" }}>
        <span style={{ fontSize: 11, color: "var(--text-muted)", fontWeight: 500 }}>Perioadă:</span>
        <div className="field" style={{ display: "inline-flex", gap: 6 }}>
          <select
            value={selectedMonth}
            onChange={(e) => setSelectedMonth(Number(e.target.value))}
            style={{ fontSize: 12, padding: "3px 6px" }}
          >
            {MONTHS.map((m, idx) => (
              <option key={idx + 1} value={idx + 1}>{m}</option>
            ))}
          </select>
          <select
            value={selectedYear}
            onChange={(e) => setSelectedYear(Number(e.target.value))}
            style={{ fontSize: 12, padding: "3px 6px" }}
          >
            {yearOptions.map((y) => (
              <option key={y} value={y}>{y}</option>
            ))}
          </select>
        </div>
        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
          {periodInvoices.length} facturi în perioadă
        </span>
      </div>

      <div style={{ padding: "14px 14px 0" }}>

        {/* ── Statistics cards ─────────────────────────────────────────────── */}
        <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 10, marginBottom: 18 }}>
          <StatCard
            label="Total facturi emise"
            value={String(stats.totalCount)}
            icon="invoice"
          />
          <StatCard
            label="Total net (RON)"
            value={fmtRON(stats.totalNet)}
            icon="bank"
          />
          <StatCard
            label="Total TVA (RON)"
            value={fmtRON(stats.totalVat)}
            icon="receipt"
          />
          <StatCard
            label="Total cu TVA (RON)"
            value={fmtRON(stats.totalGross)}
            icon="reports"
            highlight
          />
        </div>

        {/* ── TVA Summary table ─────────────────────────────────────────────── */}
        <section style={{ marginBottom: 24 }}>
          <h2 style={{ fontSize: 12, fontWeight: 600, marginBottom: 8, color: "var(--text)", letterSpacing: "0.04em", textTransform: "uppercase" }}>
            Sumar TVA — {MONTHS[selectedMonth - 1]} {selectedYear}
          </h2>
          {isLoading ? (
            <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>Se încarcă…</div>
          ) : vatGroups.length === 0 ? (
            <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>Nicio factură în perioada selectată.</div>
          ) : (
            <>
              <table className="dt">
                <thead>
                  <tr>
                    <th style={{ width: 120 }}>Rată TVA</th>
                    <th className="num" style={{ width: 160 }}>Bază impozabilă (RON)</th>
                    <th className="num" style={{ width: 130 }}>TVA (RON)</th>
                    <th className="num" style={{ width: 160 }}>Total (RON)</th>
                  </tr>
                </thead>
                <tbody>
                  {vatGroups.map((g) => (
                    <tr key={g.rate}>
                      <td><span className="mono">{g.rate}%</span></td>
                      <td className="num tnum">{fmtRON(g.baseAmount)}</td>
                      <td className="num tnum muted">{fmtRON(g.vatAmount)}</td>
                      <td className="num tnum"><b>{fmtRON(g.baseAmount + g.vatAmount)}</b></td>
                    </tr>
                  ))}
                </tbody>
                <tfoot>
                  <tr style={{ background: "var(--bg-hover)", fontWeight: 600 }}>
                    <td>TOTAL</td>
                    <td className="num tnum">{fmtRON(vatTotals.base)}</td>
                    <td className="num tnum">{fmtRON(vatTotals.vat)}</td>
                    <td className="num tnum"><b>{fmtRON(vatTotals.total)}</b></td>
                  </tr>
                </tfoot>
              </table>
              {/* Simple CSS bar chart for VAT groups */}
              <div style={{ display: "flex", gap: 8, alignItems: "flex-end", height: 80, marginTop: 12 }}>
                {vatGroups.map(g => {
                  const gTotal = g.baseAmount + g.vatAmount;
                  const pct = vatTotals.total > 0 ? (gTotal / vatTotals.total) * 100 : 0;
                  return (
                    <div key={g.rate} style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 4 }}>
                      <div style={{ fontSize: 9, color: "var(--text-muted)" }}>{fmtRON(gTotal)}</div>
                      <div style={{ width: 32, height: `${Math.max(pct * 0.7, 4)}px`, background: "var(--accent)", borderRadius: 2 }} />
                      <div style={{ fontSize: 9, color: "var(--text-muted)" }}>{g.rate}%</div>
                    </div>
                  );
                })}
              </div>
            </>
          )}
        </section>

        {/* ── Invoice list ──────────────────────────────────────────────────── */}
        <section style={{ marginBottom: 24 }}>
          <h2 style={{ fontSize: 12, fontWeight: 600, marginBottom: 8, color: "var(--text)", letterSpacing: "0.04em", textTransform: "uppercase" }}>
            Facturi emise — {MONTHS[selectedMonth - 1]} {selectedYear}
          </h2>
          {isLoading ? (
            <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>Se încarcă…</div>
          ) : periodInvoices.length === 0 ? (
            <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>Nicio factură în perioada selectată.</div>
          ) : (
            <table className="dt">
              <thead>
                <tr>
                  <th style={{ width: 130 }}>Număr</th>
                  <th>Client / ID contact</th>
                  <th style={{ width: 96 }}>Data</th>
                  <th style={{ width: 120 }}>Status</th>
                  <th className="num" style={{ width: 130 }}>Valoare net (RON)</th>
                  <th className="num" style={{ width: 110 }}>TVA (RON)</th>
                  <th className="num" style={{ width: 130 }}>Total (RON)</th>
                </tr>
              </thead>
              <tbody>
                {periodInvoices.map((inv) => (
                  <InvoiceRow key={inv.id} invoice={inv} contactMap={contactMap} />
                ))}
              </tbody>
              <tfoot>
                <tr style={{ background: "var(--bg-hover)", fontWeight: 600 }}>
                  <td colSpan={4}>TOTAL perioadă</td>
                  <td className="num tnum">{fmtRON(stats.totalNet)}</td>
                  <td className="num tnum">{fmtRON(stats.totalVat)}</td>
                  <td className="num tnum"><b>{fmtRON(stats.totalGross)}</b></td>
                </tr>
              </tfoot>
            </table>
          )}
        </section>

      </div>
    </div>
  );
}

// ─── sub-components ───────────────────────────────────────────────────────────

function StatCard({
  label,
  value,
  icon,
  highlight = false,
}: {
  label: string;
  value: string;
  icon: string;
  highlight?: boolean;
}) {
  return (
    <div
      style={{
        padding: "12px 14px",
        border: "1px solid var(--border)",
        background: highlight ? "var(--accent-subtle, var(--bg-hover))" : "var(--bg)",
        display: "flex",
        flexDirection: "column",
        gap: 4,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 10.5, color: "var(--text-muted)", fontWeight: 500 }}>
        <Icon name={icon} size={12} />
        {label}
      </div>
      <div style={{ fontSize: 17, fontWeight: 700, color: highlight ? "var(--accent)" : "var(--text)", fontVariantNumeric: "tabular-nums" }}>
        {value}
      </div>
    </div>
  );
}

function InvoiceRow({ invoice, contactMap }: { invoice: Invoice; contactMap: Map<string, string> }) {
  return (
    <tr>
      <td className="mono"><b>{invoice.fullNumber}</b></td>
      <td style={{ fontSize: 11 }}>{contactMap.get(invoice.contactId) ?? invoice.contactId}</td>
      <td className="muted">{invoice.issueDate}</td>
      <td><StatusBadge status={invoice.status} /></td>
      <td className="num tnum muted">{fmtRON(invoice.subtotalAmount)}</td>
      <td className="num tnum dim">{fmtRON(invoice.vatAmount)}</td>
      <td className="num tnum"><b>{fmtRON(invoice.totalAmount)}</b></td>
    </tr>
  );
}
