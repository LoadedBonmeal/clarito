/**
 * AgingView — Balanță cu vechime sold (AR/AP aging report).
 * Embedded in Reports page — Claude-Design classes (.scr-card / .scr-toolbar / .scr-table / .chip).
 *
 * Features:
 *  - Toggle Clienți (AR / Receivable) / Furnizori (AP / Payable)
 *  - "La data" date picker (as-of date, default today)
 *  - Tabel parteneri × tranșe (Curent / 1–30 / 31–60 / 61–90 / >90 / Total)
 *  - Rând totaluri la baza tabelului
 *  - Export CSV
 *  - Loading / empty / error states
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { api } from "@/lib/tauri";
import type { AgingDirection } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";

// ─── Helpers ─────────────────────────────────────────────────────────────────

function todayIso(): string {
  return new Date().toISOString().slice(0, 10);
}

function fmtAmt(s: string): string {
  const n = parseDec(s);
  if (n === 0) return "—";
  return fmtRON(n);
}

// ─── Component ───────────────────────────────────────────────────────────────

export function AgingView() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const [direction, setDirection] = useState<AgingDirection>("RECEIVABLE");
  const [asOf, setAsOf] = useState(todayIso());
  const [exporting, setExporting] = useState(false);

  const {
    data: report,
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: ["aging", activeCompanyId ?? "", direction, asOf],
    queryFn: () => api.reports.aging(activeCompanyId!, direction, asOf),
    enabled: !!activeCompanyId,
    staleTime: 60_000,
  });

  const handleExportCsv = async () => {
    if (!activeCompanyId) {
      notify.warn(t("declarations.notify.selectCompany"));
      return;
    }
    if (!report || report.rows.length === 0) {
      notify.info(t("declarations.notify.noData"));
      return;
    }
    const defaultName =
      direction === "RECEIVABLE"
        ? `balanta-clienti-${asOf}.csv`
        : `balanta-furnizori-${asOf}.csv`;
    const savePath = await saveDialog({
      title: t("reports.aging.dialogTitle"),
      defaultPath: defaultName,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!savePath) return;
    setExporting(true);
    try {
      const saved = await api.reports.exportAgingCsv(
        activeCompanyId,
        direction,
        asOf,
        savePath,
      );
      notify.success(t("reports.aging.csvSaved", { path: saved }));
      try {
        await openPath(saved);
      } catch {
        /* reveal best-effort */
      }
    } catch (err) {
      notify.error(formatError(err, t("reports.aging.csvFailed")));
    } finally {
      setExporting(false);
    }
  };

  const rows = report?.rows ?? [];
  const totals = report?.totals;

  return (
    <div className="scr-card">
      {/* ── Toolbar ─────────────────────────────────────────────────────────── */}
      <div className="scr-toolbar">
        <span className="tt">{t("reports.aging.title")}</span>

        {/* Direction toggle */}
        <div className="pill-group" style={{ marginLeft: 12 }}>
          <button
            className={`pill-btn${direction === "RECEIVABLE" ? " active" : ""}`}
            onClick={() => setDirection("RECEIVABLE")}
          >
            {t("reports.aging.clients")}
          </button>
          <button
            className={`pill-btn${direction === "PAYABLE" ? " active" : ""}`}
            onClick={() => setDirection("PAYABLE")}
          >
            {t("reports.aging.suppliers")}
          </button>
        </div>

        {/* As-of date */}
        <label className="field-label" style={{ marginLeft: 16, display: "flex", alignItems: "center", gap: 6 }}>
          <span style={{ fontSize: 12, color: "var(--text-2)" }}>
            {t("reports.aging.asOf")}
          </span>
          <input
            type="date"
            value={asOf}
            max={todayIso()}
            onChange={(e) => setAsOf(e.target.value)}
            style={{ fontSize: 13, padding: "2px 6px", borderRadius: 6, border: "1px solid var(--border)" }}
          />
        </label>

        <div className="spacer" />

        {/* Export CSV */}
        <button
          className="pill-btn spin-btn"
          disabled={exporting || isLoading || rows.length === 0}
          onClick={() => void handleExportCsv()}
        >
          <Ic name="dl" />
          {exporting ? t("reports.aging.exporting") : t("reports.aging.exportCsv")}
        </button>
      </div>

      {/* ── Error banner ─────────────────────────────────────────────────────── */}
      {isError && (
        <div style={{ padding: 16 }}>
          <QueryErrorBanner
            error={error}
            label={t("reports.aging.errorLabel")}
            onRetry={() => void refetch()}
          />
        </div>
      )}

      {/* ── Loading ──────────────────────────────────────────────────────────── */}
      {isLoading && (
        <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>
          {t("reports.aging.loading")}
        </div>
      )}

      {/* ── Empty ────────────────────────────────────────────────────────────── */}
      {!isLoading && !isError && rows.length === 0 && (
        <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>
          {t("reports.aging.empty")}
        </div>
      )}

      {/* ── Table ────────────────────────────────────────────────────────────── */}
      {!isLoading && !isError && rows.length > 0 && (
        <table className="scr-table">
          <thead>
            <tr>
              <th>{t("reports.aging.colPartner")}</th>
              <th>{t("reports.aging.colCui")}</th>
              <th className="r">{t("reports.aging.colCurrent")}</th>
              <th className="r">{t("reports.aging.col130")}</th>
              <th className="r">{t("reports.aging.col3160")}</th>
              <th className="r">{t("reports.aging.col6190")}</th>
              <th className="r">{t("reports.aging.colOver90")}</th>
              <th className="r">{t("reports.aging.colTotal")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row, i) => (
              <tr key={`${row.partnerCui}-${i}`}>
                <td>{row.partnerName || "—"}</td>
                <td>
                  <span className="doc">{row.partnerCui || "—"}</span>
                </td>
                <td className="r num">{fmtAmt(row.current)}</td>
                <td className="r num">{fmtAmt(row.d130)}</td>
                <td className="r num">{fmtAmt(row.d3160)}</td>
                <td className="r num">{fmtAmt(row.d6190)}</td>
                <td className={`r num${parseDec(row.over90) > 0 ? " neg" : ""}`}>
                  {fmtAmt(row.over90)}
                </td>
                <td className="r num bold">{fmtRON(parseDec(row.totalOutstanding))}</td>
              </tr>
            ))}
          </tbody>
          {totals && (
            <tfoot className="tot-foot">
              <tr>
                <td colSpan={2}><b>{t("reports.aging.totals")}</b></td>
                <td className="r num"><b>{fmtRON(parseDec(totals.current))}</b></td>
                <td className="r num"><b>{fmtRON(parseDec(totals.d130))}</b></td>
                <td className="r num"><b>{fmtRON(parseDec(totals.d3160))}</b></td>
                <td className="r num"><b>{fmtRON(parseDec(totals.d6190))}</b></td>
                <td className={`r num${parseDec(totals.over90) > 0 ? " neg" : ""}`}>
                  <b>{fmtRON(parseDec(totals.over90))}</b>
                </td>
                <td className="r num"><b>{fmtRON(parseDec(totals.totalOutstanding))}</b></td>
              </tr>
            </tfoot>
          )}
        </table>
      )}
    </div>
  );
}
