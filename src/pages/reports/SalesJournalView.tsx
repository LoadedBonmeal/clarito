/**
 * SalesJournalView — Jurnal de vânzări pentru perioadă.
 * Wave 5 — rf look: SectionCard + rf-tbl + Btn
 */

import { useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { SectionCard, Btn } from "@/components/rf";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Invoice } from "@/types";

interface Props {
  periodInvoices: Invoice[];
  contactMap:     Map<string, string>;
  dateFrom:       string;
  dateTo:         string;
  isLoading:      boolean;
}

export function SalesJournalView({ periodInvoices, contactMap, dateFrom, dateTo, isLoading }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting, setExporting] = useState(false);

  const totalNet   = periodInvoices.reduce((s, i) => s + parseDec(i.subtotalAmount), 0);
  const totalVat   = periodInvoices.reduce((s, i) => s + parseDec(i.vatAmount), 0);
  const totalGross = periodInvoices.reduce((s, i) => s + parseDec(i.totalAmount), 0);

  const handleExport = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (periodInvoices.length === 0) {
      notify.info("Nu există date pentru perioada selectată.");
      return;
    }
    const savePath = await saveDialog({
      title:       "Salvează jurnal vânzări",
      defaultPath: `jurnal-vanzari-${dateFrom}-${dateTo}.csv`,
      filters:     [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!savePath) return;
    setExporting(true);
    try {
      const saved = await api.journals.exportSales(activeCompanyId, dateFrom, dateTo, savePath);
      notify.success(`Jurnal vânzări salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta jurnalul de vânzări."));
    } finally {
      setExporting(false);
    }
  };

  return (
    <SectionCard
      icon="fileOut"
      title="Jurnal de vânzări"
      subtitle={dateFrom !== dateTo ? `${dateFrom} — ${dateTo}` : dateFrom}
      actions={
        <Btn
          variant="secondary"
          size="sm"
          icon="download"
          disabled={exporting || !activeCompanyId}
          onClick={() => void handleExport()}
        >
          {exporting ? "Export…" : "Export CSV"}
        </Btn>
      }
    >
      {isLoading ? (
        <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>Se încarcă…</div>
      ) : periodInvoices.length === 0 ? (
        <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
          Nicio factură emisă în perioada selectată.
        </div>
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
                  <td style={{ fontSize: 12.5 }}>{contactMap.get(inv.contactId) ?? inv.contactId}</td>
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
                <td className="right rf-mono">{fmtRON(totalNet)}</td>
                <td className="right rf-mono">{fmtRON(totalVat)}</td>
                <td className="right rf-mono">{fmtRON(totalGross)}</td>
              </tr>
            </tfoot>
          </table>
        </div>
      )}
    </SectionCard>
  );
}
