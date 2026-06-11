/**
 * AccountingExportView — Export contabil SAGA / WinMentor (embedded in Rapoarte).
 * Claude-Design classes: .scr-card grid + .scr-toolbar .tt + .pill-btn.
 * ALL wiring preserved: api.integrations.exportSagaCsv / exportWinmentorCsv.
 */

import { useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";

interface Props {
  periodInvoices: { id: string }[];
  dateFrom: string;
  dateTo:   string;
}

export function AccountingExportView({ periodInvoices, dateFrom, dateTo }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exportingSaga,      setExportingSaga]      = useState(false);
  const [exportingWinmentor, setExportingWinmentor] = useState(false);

  const handleExportSaga = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (periodInvoices.length === 0) {
      notify.info("Nu există date pentru perioada selectată.");
      return;
    }
    const savePath = await saveDialog({
      title:       "Salvează export SAGA",
      defaultPath: `facturi-saga-${dateFrom}-${dateTo}.csv`,
      filters:     [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!savePath) return;
    setExportingSaga(true);
    try {
      const saved = await api.integrations.exportSagaCsv(activeCompanyId, dateFrom, dateTo, savePath);
      notify.success(`Export SAGA salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta în SAGA."));
    } finally {
      setExportingSaga(false);
    }
  };

  const handleExportWinmentor = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (periodInvoices.length === 0) {
      notify.info("Nu există date pentru perioada selectată.");
      return;
    }
    const savePath = await saveDialog({
      title:       "Salvează export WinMentor",
      defaultPath: `facturi-winmentor-${dateFrom}-${dateTo}.csv`,
      filters:     [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!savePath) return;
    setExportingWinmentor(true);
    try {
      const saved = await api.integrations.exportWinmentorCsv(activeCompanyId, dateFrom, dateTo, savePath);
      notify.success(`Export WinMentor salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta în WinMentor."));
    } finally {
      setExportingWinmentor(false);
    }
  };

  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16, alignItems: "start" }}>
      {/* SAGA card */}
      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">Export SAGA</div>
        </div>
        <div className="card-pad" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <p style={{ fontSize: 13, color: "var(--text-2)", lineHeight: 1.6, margin: 0 }}>
            Export note contabile compatibil cu <b style={{ color: "var(--text)" }}>SAGA C/PS</b>.
            Generează un fișier CSV cu înregistrările contabile ale perioadei. Fișierul include
            toate liniile de factură cu TVA defalcat.
          </p>
          <button
            className="pill-btn"
            style={{ width: "fit-content" }}
            disabled={exportingSaga || !activeCompanyId}
            onClick={() => void handleExportSaga()}
          >
            <Ic name="dl" />{exportingSaga ? "Export…" : "Export SAGA (CSV)"}
          </button>
        </div>
      </div>

      {/* WinMentor card */}
      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">Export WinMentor</div>
        </div>
        <div className="card-pad" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <p style={{ fontSize: 13, color: "var(--text-2)", lineHeight: 1.6, margin: 0 }}>
            Export în format CSV compatibil <b style={{ color: "var(--text)" }}>WinMentor Enterprise</b> pentru
            import în contabilitatea principală. Coloanele respectă structura de import WinMentor standard.
          </p>
          <button
            className="pill-btn"
            style={{ width: "fit-content" }}
            disabled={exportingWinmentor || !activeCompanyId}
            onClick={() => void handleExportWinmentor()}
          >
            <Ic name="dl" />{exportingWinmentor ? "Export…" : "Export WinMentor (CSV)"}
          </button>
        </div>
      </div>
    </div>
  );
}
