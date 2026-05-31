/**
 * SalesJournalView — Jurnal de vânzări pentru perioadă.
 */

import { useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Invoice } from "@/types";

interface Props {
  periodInvoices: Invoice[];
  contactMap: Map<string, string>;
  dateFrom: string;
  dateTo: string;
  isLoading: boolean;
}

export function SalesJournalView({ periodInvoices, contactMap, dateFrom, dateTo, isLoading }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting, setExporting] = useState(false);

  const totalNet = periodInvoices.reduce((s, i) => s + parseDec(i.subtotalAmount), 0);
  const totalVat = periodInvoices.reduce((s, i) => s + parseDec(i.vatAmount), 0);
  const totalGross = periodInvoices.reduce((s, i) => s + parseDec(i.totalAmount), 0);

  const handleExport = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (periodInvoices.length === 0) {
      notify.info("Nu există date pentru perioada selectată.");
      return;
    }
    const savePath = await saveDialog({
      title: "Salvează jurnal vânzări",
      defaultPath: `jurnal-vanzari-${dateFrom}-${dateTo}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
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
    <div>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
        <h2 style={{ fontSize: 12, fontWeight: 600, color: "var(--text)", letterSpacing: "0.04em", textTransform: "uppercase", margin: 0 }}>
          Jurnal de vânzări
        </h2>
        <button
          type="button"
          className="btn"
          disabled={exporting || !activeCompanyId}
          onClick={handleExport}
        >
          <Icon name="download" size={12} /> {exporting ? "Export…" : "Exportă jurnal vânzări (CSV)"}
        </button>
      </div>

      {isLoading ? (
        <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>Se încarcă…</div>
      ) : periodInvoices.length === 0 ? (
        <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>
          Nicio factură emisă în perioada selectată.
        </div>
      ) : (
        <table className="dt">
          <thead>
            <tr>
              <th style={{ width: 130 }}>Număr</th>
              <th>Client</th>
              <th style={{ width: 96 }}>Data</th>
              <th style={{ width: 120 }}>Status</th>
              <th className="num" style={{ width: 130 }}>Net (RON)</th>
              <th className="num" style={{ width: 110 }}>TVA (RON)</th>
              <th className="num" style={{ width: 130 }}>Total (RON)</th>
            </tr>
          </thead>
          <tbody>
            {periodInvoices.map((inv) => (
              <tr key={inv.id}>
                <td className="mono"><b>{inv.fullNumber}</b></td>
                <td style={{ fontSize: 11 }}>{contactMap.get(inv.contactId) ?? inv.contactId}</td>
                <td className="muted">{inv.issueDate}</td>
                <td><StatusBadge status={inv.status} /></td>
                <td className="num tnum muted">{fmtRON(inv.subtotalAmount)}</td>
                <td className="num tnum dim">{fmtRON(inv.vatAmount)}</td>
                <td className="num tnum"><b>{fmtRON(inv.totalAmount)}</b></td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr style={{ background: "var(--bg-hover)", fontWeight: 600 }}>
              <td colSpan={4}>TOTAL perioadă</td>
              <td className="num tnum">{fmtRON(totalNet)}</td>
              <td className="num tnum">{fmtRON(totalVat)}</td>
              <td className="num tnum"><b>{fmtRON(totalGross)}</b></td>
            </tr>
          </tfoot>
        </table>
      )}
    </div>
  );
}
