/**
 * PurchaseJournalView — Jurnal de cumpărări pentru perioadă.
 * Wave 5 — rf look: SectionCard + rf-tbl + Banner + Btn
 */

import { useState, useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { SectionCard, Btn, Banner } from "@/components/rf";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { queryKeys } from "@/lib/queries";

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
    <div className="rf-col">
      {hasUnparsed && (
        <Banner variant="warning">
          Pentru unele facturi primite, TVA nu a fost încă extrasă din XML. Apăsați
          «Recalculează TVA din XML» pentru raportare completă.
        </Banner>
      )}

      <SectionCard
        icon="fileIn"
        title="Jurnal de cumpărări"
        subtitle={dateFrom !== dateTo ? `${dateFrom} — ${dateTo}` : dateFrom}
        actions={
          <div style={{ display: "flex", gap: 8 }}>
            <Btn
              variant="ghost"
              size="sm"
              icon="refresh"
              disabled={reparsing || !activeCompanyId}
              onClick={() => void handleReparseVat()}
            >
              {reparsing ? "Se recalculează…" : "Recalculează TVA din XML"}
            </Btn>
            <Btn
              variant="secondary"
              size="sm"
              icon="download"
              disabled={exporting || !activeCompanyId}
              onClick={() => void handleExport()}
            >
              {exporting ? "Export…" : "Export CSV"}
            </Btn>
          </div>
        }
      >
        {isLoading ? (
          <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>Se încarcă…</div>
        ) : isError ? (
          <div style={{ padding: "0 16px 16px" }}>
            <QueryErrorBanner error={error} label="jurnalul de cumpărări" onRetry={() => void refetch()} />
          </div>
        ) : periodReceived.length === 0 ? (
          <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
            Nicio factură primită în perioada selectată.
          </div>
        ) : (
          <div className="rf-tbl-wrap">
            <table className="rf-tbl">
              <thead>
                <tr>
                  <th>Furnizor</th>
                  <th>CUI</th>
                  <th>Serie</th>
                  <th>Număr</th>
                  <th>Data</th>
                  <th className="right">Net (RON)</th>
                  <th className="right">TVA (RON)</th>
                  <th className="right">Total</th>
                </tr>
              </thead>
              <tbody>
                {periodReceived.map((inv) => (
                  <tr key={inv.id}>
                    <td style={{ fontWeight: 500 }}>{inv.issuerName}</td>
                    <td className="rf-mono">{inv.issuerCui || <span style={{ color: "var(--rf-text-dim)" }}>—</span>}</td>
                    <td style={{ color: "var(--rf-text-muted)" }}>{inv.series ?? "—"}</td>
                    <td className="rf-mono">{inv.number ?? "—"}</td>
                    <td style={{ color: "var(--rf-text-muted)" }}>{inv.issueDate}</td>
                    <td className="right rf-mono">
                      {inv.netAmount != null ? fmtRON(inv.netAmount) : <span style={{ color: "var(--rf-text-dim)" }}>—</span>}
                    </td>
                    <td className="right rf-mono">
                      {inv.vatAmount != null ? fmtRON(inv.vatAmount) : <span style={{ color: "var(--rf-text-dim)" }}>—</span>}
                    </td>
                    <td className="right rf-mono" style={{ fontWeight: 600 }}>
                      {fmtRON(inv.totalAmount)}
                      {inv.currency !== "RON" && (
                        <span style={{ marginLeft: 4, fontSize: 10, color: "var(--rf-text-muted)" }}>{inv.currency}</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr>
                  <td colSpan={5}>TOTAL perioadă</td>
                  <td className="right rf-mono">
                    {Number.isFinite(totalNet) ? fmtRON(totalNet) : <span style={{ color: "var(--rf-text-dim)" }}>—</span>}
                  </td>
                  <td className="right rf-mono">
                    {Number.isFinite(totalVat) ? fmtRON(totalVat) : <span style={{ color: "var(--rf-text-dim)" }}>—</span>}
                  </td>
                  <td className="right rf-mono">{fmtRON(totalAmount)}</td>
                </tr>
              </tfoot>
            </table>
          </div>
        )}
      </SectionCard>
    </div>
  );
}
