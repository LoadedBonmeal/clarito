/**
 * AccountingExportView — Export contabil (SAGA / WinMentor).
 * Wave 5 — rf look: SectionCard cards grid + Btn
 */

import { useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { SectionCard, Btn } from "@/components/rf";
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
    <div className="rf-grid-2">
      {/* SAGA card */}
      <SectionCard icon="ledger" title="Export SAGA">
        <div style={{ padding: "4px 16px 16px", display: "flex", flexDirection: "column", gap: 14 }}>
          <p style={{ fontSize: 13, color: "var(--rf-text-muted)", lineHeight: 1.6, margin: 0 }}>
            Export note contabile compatibil cu <b>SAGA C/PS</b>. Generează un fișier CSV cu
            înregistrările contabile ale perioadei. Fișierul include toate liniile de factură cu
            TVA defalcat.
          </p>
          <Btn
            variant="secondary"
            icon="download"
            disabled={exportingSaga || !activeCompanyId}
            onClick={() => void handleExportSaga()}
          >
            {exportingSaga ? "Export…" : "Export SAGA (CSV)"}
          </Btn>
        </div>
      </SectionCard>

      {/* WinMentor card */}
      <SectionCard icon="ledger" title="Export WinMentor">
        <div style={{ padding: "4px 16px 16px", display: "flex", flexDirection: "column", gap: 14 }}>
          <p style={{ fontSize: 13, color: "var(--rf-text-muted)", lineHeight: 1.6, margin: 0 }}>
            Export în format CSV compatibil <b>WinMentor Enterprise</b> pentru import în
            contabilitatea principală. Coloanele respectă structura de import WinMentor standard.
          </p>
          <Btn
            variant="secondary"
            icon="download"
            disabled={exportingWinmentor || !activeCompanyId}
            onClick={() => void handleExportWinmentor()}
          >
            {exportingWinmentor ? "Export…" : "Export WinMentor (CSV)"}
          </Btn>
        </div>
      </SectionCard>
    </div>
  );
}
