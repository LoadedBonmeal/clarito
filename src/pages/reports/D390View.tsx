/**
 * D390View — declarația recapitulativă (VIES) intra-UE: operațiuni grupate pe
 * partener + tip (L/T/A/P/S/R). Aggregated from sales/received vat_category='K' lines.
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { SectionCard, Btn, Badge, Banner } from "@/components/rf";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";

interface Props {
  dateFrom: string;
  dateTo: string;
}

const TIP_LABEL: Record<string, string> = {
  L: "Livrări bunuri (L)",
  T: "Triunghiulare (T)",
  A: "Achiziții bunuri (A)",
  P: "Prestări servicii (P)",
  S: "Achiziții servicii (S)",
  R: "Regim agricultori (R)",
};

const fmtLei = (n: number) => n.toLocaleString("ro-RO");

export function D390View({ dateFrom, dateTo }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting, setExporting] = useState(false);

  const {
    data: doc,
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: ["d390", activeCompanyId ?? "", dateFrom, dateTo],
    queryFn: () => api.d390.compute(activeCompanyId!, dateFrom, dateTo),
    enabled: !!activeCompanyId && !!dateFrom && !!dateTo,
    staleTime: 60_000,
  });

  const ops = doc?.operations ?? [];
  const totalBaza = ops.reduce((s, o) => s + o.baza, 0);

  const handleExport = async () => {
    if (!activeCompanyId) {
      notify.warn("Selectați o companie activă.");
      return;
    }
    if (ops.length === 0) {
      notify.info("Nu există operațiuni intra-UE în perioada selectată.");
      return;
    }
    const savePath = await saveDialog({
      title: "Salvează D390 XML",
      defaultPath: `d390-${dateFrom}-${dateTo}.xml`,
      filters: [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExporting(true);
    try {
      const saved = await api.d390.export(activeCompanyId, dateFrom, dateTo, savePath);
      notify.success(`D390 salvat: ${saved}`);
      try {
        await openPath(saved);
      } catch {
        /* reveal best-effort */
      }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta D390."));
    } finally {
      setExporting(false);
    }
  };

  return (
    <div className="rf-col">
      <SectionCard
        icon="declaration"
        title="D390 — Declarație recapitulativă (VIES) intra-UE"
        actions={
          <Btn
            variant="primary"
            size="sm"
            icon="xml"
            disabled={exporting || !activeCompanyId || ops.length === 0}
            onClick={() => void handleExport()}
            title="Export XML D390 (declaratie390 v3)"
          >
            {exporting ? "Export…" : "Export XML"}
          </Btn>
        }
      >
        {isLoading ? (
          <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
            Se încarcă…
          </div>
        ) : isError ? (
          <div style={{ padding: "0 16px 16px" }}>
            <QueryErrorBanner error={error} label="raportul D390" onRetry={() => void refetch()} />
          </div>
        ) : ops.length === 0 ? (
          <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
            Nicio operațiune intra-UE (vat_category «K») în perioada selectată.
          </div>
        ) : (
          <>
          {(doc?.dropped ?? 0) > 0 && (
            <div style={{ padding: "0 16px 12px" }}>
              <Banner variant="warning">
                <b>{doc!.dropped}</b>{" "}
                {doc!.dropped === 1 ? "operațiune intra-UE a fost ignorată" : "operațiuni intra-UE au fost ignorate"}{" "}
                — partenerul nu are un cod TVA UE valid (cod lipsă sau prefix non-UE). Completați
                CUI-ul partenerului pentru a evita sub-raportarea în VIES.
              </Banner>
            </div>
          )}
          <div className="rf-tbl-wrap">
            <table className="rf-tbl">
              <thead>
                <tr>
                  <th>Tip</th>
                  <th>Țară</th>
                  <th>Cod operator (fără prefix)</th>
                  <th>Denumire</th>
                  <th className="right">Bază (lei)</th>
                </tr>
              </thead>
              <tbody>
                {ops.map((o, i) => (
                  <tr key={i}>
                    <td>
                      <Badge variant="info">{TIP_LABEL[o.tip] ?? o.tip}</Badge>
                    </td>
                    <td className="rf-mono">{o.tara}</td>
                    <td className="rf-mono">{o.codO}</td>
                    <td style={{ fontWeight: 500 }}>{o.denO}</td>
                    <td className="right rf-mono">{fmtLei(o.baza)}</td>
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr>
                  <td colSpan={4}>TOTAL ({ops.length} operatori)</td>
                  <td className="right rf-mono">{fmtLei(totalBaza)}</td>
                </tr>
              </tfoot>
            </table>
          </div>
          </>
        )}
      </SectionCard>
    </div>
  );
}
