/**
 * PurchaseJournalView — Jurnal de cumpărări pentru perioadă.
 */

import { useState, useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Icon } from "@/components/shared/Icon";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { queryKeys } from "@/lib/queries";

interface Props {
  dateFrom: string;
  dateTo: string;
}

export function PurchaseJournalView({ dateFrom, dateTo }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting, setExporting] = useState(false);
  const [reparsing, setReparsing] = useState(false);
  const queryClient = useQueryClient();

  const {
    data: paged,
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: queryKeys.received.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () =>
      api.received.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
    staleTime: 60_000,
  });

  const allReceived = paged?.items ?? [];

  const periodReceived = useMemo(
    () =>
      allReceived.filter(
        (inv) =>
          inv.issueDate >= dateFrom && inv.issueDate <= dateTo,
      ),
    [allReceived, dateFrom, dateTo],
  );

  const hasUnparsed = periodReceived.some((inv) => inv.netAmount == null);

  const totalNet = periodReceived.reduce(
    (s, i) => s + (i.netAmount != null ? parseDec(i.netAmount) : 0),
    0,
  );
  const totalVat = periodReceived.reduce(
    (s, i) => s + (i.vatAmount != null ? parseDec(i.vatAmount) : 0),
    0,
  );
  const totalAmount = periodReceived.reduce(
    (s, i) => s + parseDec(i.totalAmount),
    0,
  );

  const handleExport = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (periodReceived.length === 0) {
      notify.info("Nu există date pentru perioada selectată.");
      return;
    }
    const savePath = await saveDialog({
      title: "Salvează jurnal cumpărări",
      defaultPath: `jurnal-cumparari-${dateFrom}-${dateTo}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
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
      await queryClient.invalidateQueries({
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
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
        <h2 style={{ fontSize: 12, fontWeight: 600, color: "var(--text)", letterSpacing: "0.04em", textTransform: "uppercase", margin: 0 }}>
          Jurnal de cumpărări
        </h2>
        <div style={{ display: "flex", gap: 8 }}>
          <button
            type="button"
            className="btn"
            disabled={reparsing || !activeCompanyId}
            onClick={() => void handleReparseVat()}
          >
            <Icon name="refresh-cw" size={12} /> {reparsing ? "Se recalculează…" : "Recalculează TVA din XML"}
          </button>
          <button
            type="button"
            className="btn"
            disabled={exporting || !activeCompanyId}
            onClick={() => void handleExport()}
          >
            <Icon name="download" size={12} /> {exporting ? "Export…" : "Exportă jurnal cumpărări (CSV)"}
          </button>
        </div>
      </div>

      {hasUnparsed && (
        <div style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 10, fontStyle: "italic" }}>
          Unele facturi nu au încă defalcare TVA — apăsați «Recalculează TVA din XML» pentru a extrage Net/TVA din fișierele XML primite.
        </div>
      )}

      {isLoading ? (
        <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>Se încarcă…</div>
      ) : isError ? (
        <QueryErrorBanner error={error} label="jurnalul de cumpărări" onRetry={() => void refetch()} />
      ) : periodReceived.length === 0 ? (
        <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>
          Nicio factură primită în perioada selectată.
        </div>
      ) : (
        <table className="dt">
          <thead>
            <tr>
              <th>Furnizor</th>
              <th style={{ width: 130 }}>CUI</th>
              <th style={{ width: 80 }}>Serie</th>
              <th style={{ width: 100 }}>Număr</th>
              <th style={{ width: 96 }}>Data</th>
              <th className="num" style={{ width: 120 }}>Net (RON)</th>
              <th className="num" style={{ width: 120 }}>TVA (RON)</th>
              <th className="num" style={{ width: 130 }}>Total</th>
            </tr>
          </thead>
          <tbody>
            {periodReceived.map((inv) => (
              <tr key={inv.id}>
                <td style={{ fontSize: 11 }}>{inv.issuerName}</td>
                <td className="mono">{inv.issuerCui || <span className="muted">—</span>}</td>
                <td className="muted">{inv.series ?? "—"}</td>
                <td className="mono">{inv.number ?? "—"}</td>
                <td className="muted">{inv.issueDate}</td>
                <td className="num tnum">
                  {inv.netAmount != null ? fmtRON(inv.netAmount) : <span className="muted">—</span>}
                </td>
                <td className="num tnum">
                  {inv.vatAmount != null ? fmtRON(inv.vatAmount) : <span className="muted">—</span>}
                </td>
                <td className="num tnum">
                  <b>{fmtRON(inv.totalAmount)}</b>
                  {inv.currency !== "RON" && (
                    <span className="muted" style={{ marginLeft: 4, fontSize: 10 }}>{inv.currency}</span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr style={{ background: "var(--bg-hover)", fontWeight: 600 }}>
              <td colSpan={5}>TOTAL perioadă</td>
              <td className="num tnum">{totalNet > 0 ? <b>{fmtRON(totalNet)}</b> : <span className="muted">—</span>}</td>
              <td className="num tnum">{totalVat > 0 ? <b>{fmtRON(totalVat)}</b> : <span className="muted">—</span>}</td>
              <td className="num tnum"><b>{fmtRON(totalAmount)}</b></td>
            </tr>
          </tfoot>
        </table>
      )}
    </div>
  );
}
