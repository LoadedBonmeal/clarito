/**
 * EtvaView — RO e-TVA reconciliation (pre-filing self-check): app-computed D300 vs the ANAF
 * "decont precompletat" (P300ETVA). 2026: the conformance notification is abolished
 * (OUG 89/2025 + OUG 13/2026) — this is an internal self-check, not a notification response.
 * Embedded in the Reports page — Claude-Design classes (.scr-card / .scr-table / .chip / .banner).
 */

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { EtvaReconciliation } from "@/types";

interface Props {
  dateFrom: string;
  dateTo: string;
}

// Icons not in the Ic set — inlined verbatim from the prototype.
const IC_INFO =
  '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';
const IC_WARN =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

export function EtvaView({ dateFrom, dateTo }: Props) {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [collectedVat, setCollectedVat] = useState("");
  const [deductibleVat, setDeductibleVat] = useState("");

  const recon = useMutation({
    mutationFn: (): Promise<EtvaReconciliation> => {
      if (!activeCompanyId) throw new Error(t("declarations.notify.selectCompany"));
      return api.declarations.reconcileEtva(activeCompanyId, dateFrom, dateTo, {
        collectedVat: collectedVat.trim() || "0",
        deductibleVat: deductibleVat.trim() || "0",
      });
    },
    onError: (err) => notify.error(formatError(err, t("declarations.etva.notify.reconcileFailed"))),
  });

  // Fetch the precompletat zip from ANAF (an/luna from the period start) → its raw JSON files.
  const fetchP300 = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) throw new Error(t("declarations.notify.selectCompany"));
      const an = Number(dateFrom.slice(0, 4));
      const luna = Number(dateFrom.slice(5, 7));
      return api.declarations.fetchEtvaPrecompletat(activeCompanyId, an, luna);
    },
    onSuccess: (files) => notify.success(t("declarations.etva.notify.filesDownloaded", { count: files.length })),
    onError: (err) => notify.error(formatError(err, t("declarations.etva.notify.fetchFailed"))),
  });

  const result = recon.data;

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">{t("declarations.etva.title")}</div>
      </div>

      <div style={{ padding: "14px 16px 0" }}>
        <div className="banner">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_INFO }} />
          <span>
            {t("declarations.etva.banner1")} <b>{t("declarations.etva.bannerBold1")}</b>{" "}
            {t("declarations.etva.banner2")} <b>{t("declarations.etva.bannerBold2")}</b>{" "}
            {t("declarations.etva.banner3")} <b>{t("declarations.etva.bannerBold3")}</b>{" "}
            {t("declarations.etva.banner4")}
          </span>
        </div>
      </div>

      <div style={{ display: "flex", gap: 12, flexWrap: "wrap", padding: "0 16px 16px", alignItems: "flex-end" }}>
        <div className="field" style={{ width: 220 }}>
          <label>{t("declarations.etva.collectedLabel")}</label>
          <input
            className="input"
            inputMode="decimal"
            value={collectedVat}
            onChange={(e) => setCollectedVat(e.target.value)}
            placeholder="0.00"
          />
        </div>
        <div className="field" style={{ width: 220 }}>
          <label>{t("declarations.etva.deductibleLabel")}</label>
          <input
            className="input"
            inputMode="decimal"
            value={deductibleVat}
            onChange={(e) => setDeductibleVat(e.target.value)}
            placeholder="0.00"
          />
        </div>
        <button
          className="pill-btn"
          disabled={fetchP300.isPending || !activeCompanyId}
          onClick={() => fetchP300.mutate()}
          title={t("declarations.etva.fetchTitle")}
        >
          <Ic name="dl" />
          {fetchP300.isPending ? t("declarations.etva.fetching") : t("declarations.etva.fetch")}
        </button>
        <button
          className="btn-dark"
          disabled={recon.isPending || !activeCompanyId}
          onClick={() => recon.mutate()}
        >
          <Ic name="sync" />
          {recon.isPending ? t("declarations.etva.reconciling") : t("declarations.etva.reconcile")}
        </button>
      </div>

      {fetchP300.data && fetchP300.data.length > 0 && (
        <div style={{ padding: "0 16px 16px" }}>
          <div style={{ fontSize: 12, color: "var(--text-2)", marginBottom: 6 }}>
            {t("declarations.etva.filesHint")}
          </div>
          {fetchP300.data.map((f) => (
            <details key={f.name} style={{ marginBottom: 6 }}>
              <summary style={{ cursor: "pointer", fontSize: 12.5, fontWeight: 500 }}>{f.name}</summary>
              <pre style={{ maxHeight: 240, overflow: "auto", fontSize: 11, background: "var(--fill)", padding: 8, borderRadius: 6 }}>
                {f.json}
              </pre>
            </details>
          ))}
        </div>
      )}

      {result && (
        <>
          {result.cashVat && (
            <div style={{ padding: "0 16px" }}>
              <div className="banner warn">
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
                <span>
                  {t("declarations.etva.cash1")} <b>{t("declarations.etva.cashBold1")}</b>{" "}
                  {t("declarations.etva.cash2")} <b>{t("declarations.etva.cashBold2")}</b>.
                </span>
              </div>
            </div>
          )}
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("declarations.etva.headers.line")}</th>
                <th className="r">{t("declarations.etva.headers.computed")}</th>
                <th className="r">{t("declarations.etva.headers.precompletat")}</th>
                <th className="r">{t("declarations.etva.headers.diff")}</th>
                <th className="r">%</th>
                <th>{t("declarations.etva.headers.state")}</th>
              </tr>
            </thead>
            <tbody>
              {result.lines.map((l, i) => (
                <tr key={i}>
                  <td style={{ fontWeight: 500 }}>{l.label}</td>
                  <td className="r num">{l.d300}</td>
                  <td className="r num">{l.precompletat}</td>
                  <td className="r num">{l.diff}</td>
                  <td className="r num">{l.diffPct}%</td>
                  <td>
                    {l.significant ? (
                      <span className="chip late">
                        <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
                        {t("declarations.etva.chipSignificant")}
                      </span>
                    ) : (
                      <span className="chip paid">
                        <Ic name="checkC" cls="sic" />
                        {t("declarations.etva.chipOk")}
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="tot-foot">
            <span>
              {result.anySignificant
                ? t("declarations.etva.footSignificant")
                : t("declarations.etva.footOk")}
            </span>
          </div>
        </>
      )}
    </div>
  );
}
