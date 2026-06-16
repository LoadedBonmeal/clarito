/**
 * D100View — Declarația privind obligațiile de plată la bugetul de stat (rândul trimestrial).
 * Micro → poziția 5 (1% × venituri); profit → poziția 2 (16% × rezultat), din P&L-ul perioadei.
 * Trim. IV pe profit se regularizează prin D101. Depunerea rămâne manuală prin PDF inteligent + SPV.
 * Embedded in the Reports page — Claude-Design classes (.scr-card / .scr-table / .banner / .field).
 */

import { useMemo, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";
import type { D100Result } from "@/types";

interface Props {
  dateFrom: string;
  dateTo: string;
}

// Icons not in the Ic set — inlined verbatim from the prototype.
const IC_INFO =
  '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';
const IC_WARN =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

export function D100View({ dateFrom, dateTo }: Props) {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [prior, setPrior] = useState("");

  const { quarter, year } = useMemo(() => {
    const y = Number(dateFrom.slice(0, 4));
    const m = Number(dateFrom.slice(5, 7));
    return { quarter: Math.ceil(m / 3), year: y };
  }, [dateFrom]);

  const calc = useMutation({
    mutationFn: (): Promise<D100Result> => {
      if (!activeCompanyId) throw new Error(t("declarations.notify.selectCompany"));
      return api.declarations.computeD100(activeCompanyId, dateFrom, dateTo, quarter, year, prior.trim() || "0");
    },
    onError: (err) => notify.error(formatError(err, t("declarations.d100.computeFailed"))),
  });

  const r = calc.data;

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">{t("declarations.d100.title")}</div>
      </div>

      <div style={{ padding: "14px 16px 0" }}>
        <div className="banner">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_INFO }} />
          <span>
            {t("declarations.d100.banner", { q: quarter, year })}
          </span>
        </div>
      </div>

      <div style={{ display: "flex", gap: 12, flexWrap: "wrap", padding: "0 16px 16px", alignItems: "flex-end" }}>
        <div className="field" style={{ width: 200 }}>
          <label>{t("declarations.d100.priorLabel")}</label>
          <input
            className="input"
            inputMode="decimal"
            value={prior}
            onChange={(e) => setPrior(e.target.value)}
            placeholder="0"
          />
        </div>
        <button
          className="btn-dark"
          disabled={calc.isPending || !activeCompanyId}
          onClick={() => calc.mutate()}
        >
          {calc.isPending ? t("declarations.common.calcing") : t("declarations.d100.calc")}
        </button>
      </div>

      {r && r.note && (
        <div style={{ padding: "0 16px" }}>
          <div className={`banner${r.applicable ? "" : " warn"}`}>
            <svg
              className="ic"
              viewBox="0 0 24 24"
              dangerouslySetInnerHTML={{ __html: r.applicable ? IC_INFO : IC_WARN }}
            />
            <span>{r.note}</span>
          </div>
        </div>
      )}
      {r && r.applicable && (
        <table className="scr-table">
          <thead>
            <tr>
              <th>{t("declarations.d100.headers.code")}</th>
              <th>{t("declarations.d100.headers.name")}</th>
              <th className="r">{t("declarations.d100.headers.base")}</th>
              <th className="r">{t("declarations.d100.headers.rate")}</th>
              <th className="r">{t("declarations.d100.headers.owed")}</th>
              <th className="r">{t("declarations.d100.headers.toPay")}</th>
              <th>{t("declarations.d100.headers.deadline")}</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td className="doc" style={{ fontWeight: 700, color: "var(--text)" }}>{r.codOblig}</td>
              <td>{r.label}</td>
              <td className="r num">{fmtRON(r.base)}</td>
              <td className="r num">{r.ratePct}%</td>
              <td className="r num">{fmtRON(r.sumaDatorata)}</td>
              <td className="r num"><b>{fmtRON(r.sumaDePlata)}</b></td>
              <td className="num">{r.scadenta}</td>
            </tr>
          </tbody>
        </table>
      )}

      {/* Obligații de impozit pe dividende cu scadența în trimestru — INFORMATIV (D100 nu emite XML;
          se depune prin PDF inteligent + SPV). Afișat și când rândul micro/profit nu se aplică (T4 profit). */}
      {r && r.dividendObligations && r.dividendObligations.length > 0 && (
        <div style={{ padding: "12px 16px 0" }}>
          <div className="banner">
            <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_INFO }} />
            <span>{t("declarations.d100.dividends.note")}</span>
          </div>
          <table className="scr-table" style={{ marginTop: 10 }}>
            <thead>
              <tr>
                <th>{t("declarations.d100.headers.code")}</th>
                <th>{t("declarations.d100.dividends.title")}</th>
                <th className="r">{t("declarations.d100.dividends.amount")}</th>
                <th>{t("declarations.d100.headers.deadline")}</th>
              </tr>
            </thead>
            <tbody>
              {r.dividendObligations.map((o, i) => (
                <tr key={i}>
                  <td className="doc" style={{ fontWeight: 700, color: "var(--text)" }}>{o.codOblig}</td>
                  <td>
                    {o.label}
                    {o.count > 1 && (
                      <span style={{ color: "var(--text-2)" }}>
                        {" · "}{t("declarations.d100.dividends.count", { count: o.count })}
                      </span>
                    )}
                  </td>
                  <td className="r num"><b>{fmtRON(o.amount)}</b></td>
                  <td className="num">{o.deadline}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
