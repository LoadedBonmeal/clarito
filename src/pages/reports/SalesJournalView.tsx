/**
 * SalesJournalView — Jurnal de vânzări pentru perioadă (embedded in Rapoarte).
 * Claude-Design classes: .scr-card + .scr-toolbar .tt + .scr-table + .chip + .tot-foot.
 * ALL wiring preserved: api.journals.exportSales CSV export, period invoice list.
 */

import { useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Invoice, InvoiceStatus } from "@/types";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

// Status → design chip (.chip variants + icon + label) — same mapping as Invoices.tsx.
const STATUS_CHIP: Record<InvoiceStatus, { cls: string; icon: string; label: string }> = {
  DRAFT:     { cls: "sent", icon: "docText", label: "Schiță" },
  QUEUED:    { cls: "wait", icon: "clock",   label: "În coadă" },
  SUBMITTED: { cls: "sent", icon: "send",    label: "Trimisă" },
  VALIDATED: { cls: "paid", icon: "check",   label: "Validată" },
  REJECTED:  { cls: "late", icon: "xMark",   label: "Respinsă" },
  STORNED:   { cls: "wait", icon: "undo",    label: "Stornată" },
};

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
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">Jurnal de vânzări</div>
        <span className="muted" style={{ fontSize: 12, color: "var(--text-2)" }}>
          {dateFrom !== dateTo ? `${fmtRoDate(dateFrom)} — ${fmtRoDate(dateTo)}` : fmtRoDate(dateFrom)}
        </span>
        <div className="spacer" />
        <button
          className="pill-btn"
          disabled={exporting || !activeCompanyId}
          onClick={() => void handleExport()}
        >
          <Ic name="dl" />{exporting ? "Export…" : "Export CSV"}
        </button>
      </div>

      {isLoading ? (
        <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
      ) : periodInvoices.length === 0 ? (
        <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
          Nicio factură emisă în perioada selectată.
        </div>
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
            <span>TOTAL perioadă: net <b className="num">{fmtRON(totalNet)}</b></span>
            <span>TVA <b className="num">{fmtRON(totalVat)}</b></span>
            <span>total <b className="num">{fmtRON(totalGross)}</b></span>
          </div>
        </>
      )}
    </div>
  );
}
