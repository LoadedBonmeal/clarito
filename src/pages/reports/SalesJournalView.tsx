/**
 * SalesJournalView — Jurnal de vânzări pentru perioadă (embedded in Rapoarte).
 * Claude-Design classes: .scr-card + .scr-toolbar .tt + .scr-table + .chip + .tot-foot.
 * ALL wiring preserved: api.journals.exportSales CSV export, period invoice list.
 */

import { useState } from "react";
import { useTranslation } from "react-i18next";
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

// Status → design chip (.chip variants + icon + label key) — same mapping as Invoices.tsx.
const STATUS_CHIP: Record<InvoiceStatus, { cls: string; icon: string; labelKey: string }> = {
  DRAFT:     { cls: "sent", icon: "docText", labelKey: "reports.statuses.draft" },
  QUEUED:    { cls: "wait", icon: "clock",   labelKey: "reports.statuses.queued" },
  SUBMITTED: { cls: "sent", icon: "send",    labelKey: "reports.statuses.submitted" },
  VALIDATED: { cls: "paid", icon: "check",   labelKey: "reports.statuses.validated" },
  REJECTED:  { cls: "late", icon: "xMark",   labelKey: "reports.statuses.rejected" },
  STORNED:   { cls: "wait", icon: "undo",    labelKey: "reports.statuses.storned" },
};

interface Props {
  periodInvoices: Invoice[];
  contactMap:     Map<string, string>;
  dateFrom:       string;
  dateTo:         string;
  isLoading:      boolean;
}

export function SalesJournalView({ periodInvoices, contactMap, dateFrom, dateTo, isLoading }: Props) {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting, setExporting] = useState(false);

  const totalNet   = periodInvoices.reduce((s, i) => s + parseDec(i.subtotalAmount), 0);
  const totalVat   = periodInvoices.reduce((s, i) => s + parseDec(i.vatAmount), 0);
  const totalGross = periodInvoices.reduce((s, i) => s + parseDec(i.totalAmount), 0);

  const handleExport = async () => {
    if (!activeCompanyId) { notify.warn(t("declarations.notify.selectCompany")); return; }
    if (periodInvoices.length === 0) {
      notify.info(t("declarations.notify.noData"));
      return;
    }
    const savePath = await saveDialog({
      title:       t("reports.dialogs.saveSalesJournal"),
      defaultPath: `jurnal-vanzari-${dateFrom}-${dateTo}.csv`,
      filters:     [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!savePath) return;
    setExporting(true);
    try {
      const saved = await api.journals.exportSales(activeCompanyId, dateFrom, dateTo, savePath);
      notify.success(t("reports.notify.salesJournalSaved", { path: saved }));
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, t("reports.notify.salesJournalFailed")));
    } finally {
      setExporting(false);
    }
  };

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">{t("reports.salesJournal.title")}</div>
        <span className="muted" style={{ fontSize: 12, color: "var(--text-2)" }}>
          {dateFrom !== dateTo ? `${fmtRoDate(dateFrom)} — ${fmtRoDate(dateTo)}` : fmtRoDate(dateFrom)}
        </span>
        <div className="spacer" />
        <button
          className="pill-btn"
          disabled={exporting || !activeCompanyId}
          onClick={() => void handleExport()}
        >
          <Ic name="dl" />{exporting ? t("declarations.common.exporting") : t("reports.actions.exportCsv")}
        </button>
      </div>

      {isLoading ? (
        <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("declarations.common.loading")}</div>
      ) : periodInvoices.length === 0 ? (
        <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
          {t("reports.salesJournal.empty")}
        </div>
      ) : (
        <>
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("reports.table.number")}</th>
                <th>{t("reports.table.client")}</th>
                <th>{t("reports.table.date")}</th>
                <th>{t("reports.table.status")}</th>
                <th className="r">{t("reports.table.netRon")}</th>
                <th className="r">{t("reports.table.vatRon")}</th>
                <th className="r">{t("reports.table.totalRon")}</th>
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
                      <span className={`chip ${chip.cls}`}><Ic name={chip.icon} cls="sic" />{t(chip.labelKey)}</span>
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
            <span>{t("reports.foot.periodNet")} <b className="num">{fmtRON(totalNet)}</b></span>
            <span>{t("reports.foot.vat")} <b className="num">{fmtRON(totalVat)}</b></span>
            <span>{t("reports.foot.total")} <b className="num">{fmtRON(totalGross)}</b></span>
          </div>
        </>
      )}
    </div>
  );
}
