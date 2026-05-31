/**
 * AccountingExportView — Export contabil (SAGA / WinMentor).
 */

import { useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Icon } from "@/components/shared/Icon";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";

interface Props {
  periodInvoices: { id: string }[];
  dateFrom: string;
  dateTo: string;
}

export function AccountingExportView({ periodInvoices, dateFrom, dateTo }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exportingSaga, setExportingSaga] = useState(false);
  const [exportingWinmentor, setExportingWinmentor] = useState(false);

  const handleExportSaga = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (periodInvoices.length === 0) {
      notify.info("Nu există date pentru perioada selectată.");
      return;
    }
    const savePath = await saveDialog({
      title: "Salvează export SAGA",
      defaultPath: `facturi-saga-${dateFrom}-${dateTo}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
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
      title: "Salvează export WinMentor",
      defaultPath: `facturi-winmentor-${dateFrom}-${dateTo}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
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
    <div>
      <h2 style={{ fontSize: 12, fontWeight: 600, color: "var(--text)", letterSpacing: "0.04em", textTransform: "uppercase", margin: "0 0 14px" }}>
        Export contabil
      </h2>

      <div style={{ display: "flex", gap: 14, flexWrap: "wrap" }}>
        {/* SAGA card */}
        <div
          style={{
            border: "1px solid var(--border)",
            background: "var(--bg)",
            padding: "16px 18px",
            minWidth: 240,
            maxWidth: 320,
            display: "flex",
            flexDirection: "column",
            gap: 10,
          }}
        >
          <div style={{ fontSize: 13, fontWeight: 700, color: "var(--text)" }}>
            <Icon name="download" size={14} /> Export SAGA (CSV)
          </div>
          <p style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.6, margin: 0 }}>
            Exportă facturile emise din perioadă în formatul CSV compatibil cu
            softul contabil <strong>SAGA</strong>. Fișierul include toate
            liniile de factură cu TVA defalcat.
          </p>
          <button
            type="button"
            className="btn primary"
            disabled={exportingSaga || !activeCompanyId}
            onClick={handleExportSaga}
            style={{ alignSelf: "flex-start" }}
          >
            <Icon name="download" size={12} /> {exportingSaga ? "Export…" : "Export SAGA (CSV)"}
          </button>
        </div>

        {/* WinMentor card */}
        <div
          style={{
            border: "1px solid var(--border)",
            background: "var(--bg)",
            padding: "16px 18px",
            minWidth: 240,
            maxWidth: 320,
            display: "flex",
            flexDirection: "column",
            gap: 10,
          }}
        >
          <div style={{ fontSize: 13, fontWeight: 700, color: "var(--text)" }}>
            <Icon name="download" size={14} /> Export WinMentor (CSV)
          </div>
          <p style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.6, margin: 0 }}>
            Exportă facturile emise din perioadă în formatul CSV compatibil cu
            softul contabil <strong>WinMentor</strong>. Coloanele respectă
            structura de import WinMentor standard.
          </p>
          <button
            type="button"
            className="btn primary"
            disabled={exportingWinmentor || !activeCompanyId}
            onClick={handleExportWinmentor}
            style={{ alignSelf: "flex-start" }}
          >
            <Icon name="download" size={12} /> {exportingWinmentor ? "Export…" : "Export WinMentor (CSV)"}
          </button>
        </div>
      </div>
    </div>
  );
}
