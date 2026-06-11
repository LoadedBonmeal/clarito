/**
 * PurchaseJournalView — Jurnal de cumpărări pentru perioadă (embedded in Rapoarte).
 * Claude-Design classes: .scr-card + .scr-toolbar .tt + .scr-table + .banner warn + .tot-foot.
 * ALL wiring preserved: api.received.list query, api.journals.exportPurchases CSV,
 * api.received.reparseVat + cache invalidation, QueryErrorBanner.
 */

import { useState, useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { queryKeys } from "@/lib/queries";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

// Warning icon absent from the Ic set — inlined verbatim (design banner pattern).
const IC_WARN = '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

interface Props {
  dateFrom: string;
  dateTo:   string;
}

export function PurchaseJournalView({ dateFrom, dateTo }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting, setExporting] = useState(false);
  const [reparsing, setReparsing] = useState(false);
  const queryClientHook = useQueryClient();

  const {
    data:    paged,
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: queryKeys.received.list({ companyId: activeCompanyId ?? undefined }),
    queryFn:  () => api.received.list({ companyId: activeCompanyId ?? undefined }),
    enabled:  !!activeCompanyId,
    staleTime: 60_000,
  });

  const allReceived = paged?.items ?? [];

  const periodReceived = useMemo(
    () =>
      allReceived.filter(
        (inv) => inv.issueDate >= dateFrom && inv.issueDate <= dateTo,
      ),
    [allReceived, dateFrom, dateTo],
  );

  const hasUnparsed = periodReceived.some((inv) => inv.netAmount == null);

  const totalNet    = periodReceived.reduce((s, i) => s + (i.netAmount    != null ? parseDec(i.netAmount)    : 0), 0);
  const totalVat    = periodReceived.reduce((s, i) => s + (i.vatAmount    != null ? parseDec(i.vatAmount)    : 0), 0);
  const totalAmount = periodReceived.reduce((s, i) => s + parseDec(i.totalAmount), 0);

  const handleExport = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (periodReceived.length === 0) {
      notify.info("Nu există date pentru perioada selectată.");
      return;
    }
    const savePath = await saveDialog({
      title:       "Salvează jurnal cumpărări",
      defaultPath: `jurnal-cumparari-${dateFrom}-${dateTo}.csv`,
      filters:     [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!savePath) return;
    setExporting(true);
    try {
      const saved = await api.journals.exportPurchases(activeCompanyId, dateFrom, dateTo, savePath);
      notify.success(`Jurnal cumpărări salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta jurnalul de cumpărări."));
    } finally {
      setExporting(false);
    }
  };

  const handleReparseVat = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setReparsing(true);
    try {
      const n = await api.received.reparseVat(activeCompanyId);
      notify.success(`${n} facturi actualizate`);
      await queryClientHook.invalidateQueries({
        queryKey: queryKeys.received.list({ companyId: activeCompanyId }),
      });
      void refetch();
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut recalcula TVA din XML."));
    } finally {
      setReparsing(false);
    }
  };

  return (
    <div>
      {hasUnparsed && (
        <div className="banner warn">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
          <span>
            Pentru unele facturi primite, TVA nu a fost încă extrasă din XML. Apăsați
            «Recalculează TVA din XML» pentru raportare completă.
          </span>
        </div>
      )}

      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">Jurnal de cumpărări</div>
          <span className="muted" style={{ fontSize: 12, color: "var(--text-2)" }}>
            {dateFrom !== dateTo ? `${fmtRoDate(dateFrom)} — ${fmtRoDate(dateTo)}` : fmtRoDate(dateFrom)}
          </span>
          <div className="spacer" />
          <button
            className="pill-btn spin-btn"
            disabled={reparsing || !activeCompanyId}
            onClick={() => void handleReparseVat()}
          >
            <Ic name="sync" />{reparsing ? "Se recalculează…" : "Recalculează TVA din XML"}
          </button>
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
        ) : isError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={error} label="jurnalul de cumpărări" onRetry={() => void refetch()} />
          </div>
        ) : periodReceived.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            Nicio factură primită în perioada selectată.
          </div>
        ) : (
          <>
            <table className="scr-table">
              <thead>
                <tr>
                  <th>Furnizor</th>
                  <th>CUI</th>
                  <th>Serie</th>
                  <th>Număr</th>
                  <th>Data</th>
                  <th className="r">Net (RON)</th>
                  <th className="r">TVA (RON)</th>
                  <th className="r">Total</th>
                </tr>
              </thead>
              <tbody>
                {periodReceived.map((inv) => (
                  <tr key={inv.id}>
                    <td><div className="cli">{inv.issuerName}</div></td>
                    <td className="num">{inv.issuerCui || <span className="muted">—</span>}</td>
                    <td style={{ color: "var(--text-2)" }}>{inv.series ?? "—"}</td>
                    <td className="num">{inv.number ?? "—"}</td>
                    <td className="num">{fmtRoDate(inv.issueDate)}</td>
                    <td className="r num">
                      {inv.netAmount != null ? fmtRON(inv.netAmount) : <span className="muted">—</span>}
                    </td>
                    <td className="r num" style={{ color: "var(--text-2)" }}>
                      {inv.vatAmount != null ? fmtRON(inv.vatAmount) : <span className="muted">—</span>}
                    </td>
                    <td className="r num">
                      <b>{fmtRON(inv.totalAmount)}</b>
                      {inv.currency !== "RON" && (
                        <span style={{ marginLeft: 4, fontSize: 10, color: "var(--text-2)" }}>{inv.currency}</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="tot-foot">
              <span>TOTAL perioadă: net <b className="num">{Number.isFinite(totalNet) ? fmtRON(totalNet) : "—"}</b></span>
              <span>TVA <b className="num">{Number.isFinite(totalVat) ? fmtRON(totalVat) : "—"}</b></span>
              <span>total <b className="num">{fmtRON(totalAmount)}</b></span>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
