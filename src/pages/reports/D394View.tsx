/**
 * D394View — D394 livrări grupate pe partener.
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Icon } from "@/components/shared/Icon";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";

interface Props {
  dateFrom: string;
  dateTo: string;
}

export function D394View({ dateFrom, dateTo }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting, setExporting] = useState(false);

  // periodFrom / periodTo are the first-of-month and last-of-month YYYY-MM-DD strings
  const periodFrom = dateFrom;
  const periodTo = dateTo;

  const {
    data: report,
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: ["d394", activeCompanyId ?? "", periodFrom, periodTo],
    queryFn: () =>
      api.d394.compute(activeCompanyId!, periodFrom, periodTo),
    enabled: !!activeCompanyId && !!periodFrom && !!periodTo,
    staleTime: 60_000,
  });

  const handleExport = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (!report || report.partners.length === 0) {
      notify.info("Nu există date pentru perioada selectată.");
      return;
    }
    const savePath = await saveDialog({
      title: "Salvează D394 XML",
      defaultPath: `d394-${periodFrom}-${periodTo}.xml`,
      filters: [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExporting(true);
    try {
      const saved = await api.d394.export(activeCompanyId, periodFrom, periodTo, savePath);
      notify.success(`D394 XML salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta D394."));
    } finally {
      setExporting(false);
    }
  };

  const totalBase = parseDec(report?.totalBase ?? "0");
  const totalVat = parseDec(report?.totalVat ?? "0");

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
        <h2 style={{ fontSize: 12, fontWeight: 600, color: "var(--text)", letterSpacing: "0.04em", textTransform: "uppercase", margin: 0 }}>
          D394 — Livrări per partener
        </h2>
        <button
          type="button"
          className="btn"
          disabled={exporting || !activeCompanyId}
          onClick={handleExport}
        >
          <Icon name="download" size={12} /> {exporting ? "Export…" : "Exportă D394 (XML)"}
        </button>
      </div>

      {isLoading ? (
        <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>Se încarcă…</div>
      ) : isError ? (
        <QueryErrorBanner error={error} label="raportul D394" onRetry={() => void refetch()} />
      ) : !report || report.partners.length === 0 ? (
        <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>
          Nicio livrare validată în perioada selectată.
        </div>
      ) : (
        <table className="dt">
          <thead>
            <tr>
              <th>Partener</th>
              <th style={{ width: 130 }}>CUI</th>
              <th className="num" style={{ width: 100 }}>Nr. facturi</th>
              <th className="num" style={{ width: 150 }}>Bază (RON)</th>
              <th className="num" style={{ width: 130 }}>TVA (RON)</th>
            </tr>
          </thead>
          <tbody>
            {report.partners.map((p, i) => (
              <tr key={i}>
                <td style={{ fontSize: 11 }}>{p.partnerName}</td>
                <td className="mono">{p.partnerCui || <span className="muted">—</span>}</td>
                <td className="num tnum">{p.invoiceCount}</td>
                <td className="num tnum">{fmtRON(p.base)}</td>
                <td className="num tnum muted">{fmtRON(p.vat)}</td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr style={{ background: "var(--bg-hover)", fontWeight: 600 }}>
              <td colSpan={2}>TOTAL</td>
              <td className="num tnum">{report.invoiceCount}</td>
              <td className="num tnum">{fmtRON(totalBase)}</td>
              <td className="num tnum">{fmtRON(totalVat)}</td>
            </tr>
          </tfoot>
        </table>
      )}
    </div>
  );
}
