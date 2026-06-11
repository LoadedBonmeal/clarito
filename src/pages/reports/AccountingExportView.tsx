/**
 * AccountingExportView — Export contabil SAGA / WinMentor (embedded in Rapoarte).
 * Claude-Design classes: .scr-card grid + .scr-toolbar .tt + .pill-btn.
 * ALL wiring preserved: api.integrations.exportSagaCsv / exportWinmentorCsv.
 */

import { useState } from "react";
import { useTranslation } from "react-i18next";
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
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exportingSaga,      setExportingSaga]      = useState(false);
  const [exportingWinmentor, setExportingWinmentor] = useState(false);

  const handleExportSaga = async () => {
    if (!activeCompanyId) { notify.warn(t("declarations.notify.selectCompany")); return; }
    if (periodInvoices.length === 0) {
      notify.info(t("declarations.notify.noData"));
      return;
    }
    const savePath = await saveDialog({
      title:       t("reports.dialogs.saveSaga"),
      defaultPath: `facturi-saga-${dateFrom}-${dateTo}.csv`,
      filters:     [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!savePath) return;
    setExportingSaga(true);
    try {
      const saved = await api.integrations.exportSagaCsv(activeCompanyId, dateFrom, dateTo, savePath);
      notify.success(t("reports.notify.sagaSaved", { path: saved }));
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, t("reports.notify.sagaFailed")));
    } finally {
      setExportingSaga(false);
    }
  };

  const handleExportWinmentor = async () => {
    if (!activeCompanyId) { notify.warn(t("declarations.notify.selectCompany")); return; }
    if (periodInvoices.length === 0) {
      notify.info(t("declarations.notify.noData"));
      return;
    }
    const savePath = await saveDialog({
      title:       t("reports.dialogs.saveWinmentor"),
      defaultPath: `facturi-winmentor-${dateFrom}-${dateTo}.csv`,
      filters:     [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!savePath) return;
    setExportingWinmentor(true);
    try {
      const saved = await api.integrations.exportWinmentorCsv(activeCompanyId, dateFrom, dateTo, savePath);
      notify.success(t("reports.notify.winmentorSaved", { path: saved }));
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, t("reports.notify.winmentorFailed")));
    } finally {
      setExportingWinmentor(false);
    }
  };

  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16, alignItems: "start" }}>
      {/* SAGA card */}
      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">{t("reports.accounting.saga.title")}</div>
        </div>
        <div className="card-pad" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <p style={{ fontSize: 13, color: "var(--text-2)", lineHeight: 1.6, margin: 0 }}>
            {t("reports.accounting.saga.desc1")} <b style={{ color: "var(--text)" }}>SAGA C/PS</b>.{" "}
            {t("reports.accounting.saga.desc2")}
          </p>
          <button
            className="pill-btn"
            style={{ width: "fit-content" }}
            disabled={exportingSaga || !activeCompanyId}
            onClick={() => void handleExportSaga()}
          >
            <Ic name="dl" />{exportingSaga ? t("declarations.common.exporting") : t("reports.accounting.saga.btn")}
          </button>
        </div>
      </div>

      {/* WinMentor card */}
      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">{t("reports.accounting.winmentor.title")}</div>
        </div>
        <div className="card-pad" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <p style={{ fontSize: 13, color: "var(--text-2)", lineHeight: 1.6, margin: 0 }}>
            {t("reports.accounting.winmentor.desc1")} <b style={{ color: "var(--text)" }}>WinMentor Enterprise</b>{" "}
            {t("reports.accounting.winmentor.desc2")}
          </p>
          <button
            className="pill-btn"
            style={{ width: "fit-content" }}
            disabled={exportingWinmentor || !activeCompanyId}
            onClick={() => void handleExportWinmentor()}
          >
            <Ic name="dl" />{exportingWinmentor ? t("declarations.common.exporting") : t("reports.accounting.winmentor.btn")}
          </button>
        </div>
      </div>
    </div>
  );
}
