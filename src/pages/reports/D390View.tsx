/**
 * D390View — declarația recapitulativă (VIES) intra-UE: operațiuni grupate pe
 * partener + tip (L/T/A/P/S/R). Aggregated from sales/received vat_category='K' lines.
 * Embedded in the Reports page — Claude-Design classes (.scr-card / .scr-table / .chip / .banner).
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
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

// Warn triangle — not in the Ic set, inlined verbatim from the prototype.
const IC_WARN =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

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
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">D390 — Declarație recapitulativă (VIES) intra-UE</div>
        <div className="spacer" />
        <button
          className="btn-dark"
          disabled={exporting || !activeCompanyId || ops.length === 0}
          onClick={() => void handleExport()}
          title="Export XML D390 (declaratie390 v3)"
        >
          <Ic name="dl" />
          {exporting ? "Export…" : "Export XML"}
        </button>
      </div>

      {isLoading ? (
        <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
      ) : isError ? (
        <div style={{ padding: 16 }}>
          <QueryErrorBanner error={error} label="raportul D390" onRetry={() => void refetch()} />
        </div>
      ) : ops.length === 0 ? (
        <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
          Nicio operațiune intra-UE (vat_category «K») în perioada selectată.
        </div>
      ) : (
        <>
          {(doc?.dropped ?? 0) > 0 && (
            <div style={{ padding: "14px 16px 0" }}>
              <div className="banner warn">
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
                <span>
                  <b>{doc!.dropped}</b>{" "}
                  {doc!.dropped === 1 ? "operațiune intra-UE a fost ignorată" : "operațiuni intra-UE au fost ignorate"}{" "}
                  — partener fără cod TVA UE valid (cod lipsă / prefix non-UE) sau bază netă negativă
                  (stornare peste altă perioadă — regularizarea «R» se declară manual; tipurile
                  T/triunghiular și R nu sunt încă generate automat). Completați CUI-ul partenerului
                  sau declarați regularizarea manual pentru a evita sub-raportarea în VIES.
                </span>
              </div>
            </div>
          )}
          <table className="scr-table">
            <thead>
              <tr>
                <th>Tip</th>
                <th>Țară</th>
                <th>Cod operator (fără prefix)</th>
                <th>Denumire</th>
                <th className="r">Bază (lei)</th>
              </tr>
            </thead>
            <tbody>
              {ops.map((o, i) => (
                <tr key={i}>
                  <td>
                    <span className="chip sent">{TIP_LABEL[o.tip] ?? o.tip}</span>
                  </td>
                  <td className="doc">{o.tara}</td>
                  <td className="doc">{o.codO}</td>
                  <td style={{ fontWeight: 500 }}>{o.denO}</td>
                  <td className="r num">{fmtLei(o.baza)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="tot-foot">
            <span>
              TOTAL ({ops.length} operatori): bază <b className="num">{fmtLei(totalBaza)}</b> lei
            </span>
          </div>
        </>
      )}
    </div>
  );
}
