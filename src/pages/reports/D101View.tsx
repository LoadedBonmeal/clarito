/**
 * D101View — fișă de calcul impozit pe profit (Formular 101, OPANAF 206/2025).
 * Baza (rezultat brut + cifră de afaceri) vine din contul de profit și pierdere al perioadei;
 * utilizatorul introduce ajustările fiscale (art. 19 Cod fiscal). Depunerea rămâne manuală prin
 * PDF inteligent ANAF + SPV (ca D300/D394).
 * Embedded in the Reports page — Claude-Design classes (.scr-card / .scr-table / .fgrid / .chip).
 */

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";
import type { D101Result } from "@/types";

interface Props {
  dateFrom: string;
  dateTo: string;
}

// Info circle — not in the Ic set, inlined verbatim from the prototype.
const IC_INFO =
  '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';

const FIELDS: { key: keyof Adjustments; labelKey: string }[] = [
  { key: "nonDeductibleExpenses", labelKey: "declarations.d101.fields.nonDeductibleExpenses" },
  { key: "nonTaxableRevenue", labelKey: "declarations.d101.fields.nonTaxableRevenue" },
  { key: "fiscalDeductions", labelKey: "declarations.d101.fields.fiscalDeductions" },
  { key: "priorLoss", labelKey: "declarations.d101.fields.priorLoss" },
  { key: "sponsorship", labelKey: "declarations.d101.fields.sponsorship" },
  { key: "anticipatedPayments", labelKey: "declarations.d101.fields.anticipatedPayments" },
];

type Adjustments = {
  nonDeductibleExpenses: string;
  nonTaxableRevenue: string;
  fiscalDeductions: string;
  priorLoss: string;
  sponsorship: string;
  anticipatedPayments: string;
};

export function D101View({ dateFrom, dateTo }: Props) {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [adj, setAdj] = useState<Adjustments>({
    nonDeductibleExpenses: "",
    nonTaxableRevenue: "",
    fiscalDeductions: "",
    priorLoss: "",
    sponsorship: "",
    anticipatedPayments: "",
  });

  const calc = useMutation({
    mutationFn: (): Promise<D101Result> => {
      if (!activeCompanyId) throw new Error(t("declarations.notify.selectCompany"));
      return api.declarations.computeD101(activeCompanyId, dateFrom, dateTo, {
        nonDeductibleExpenses: adj.nonDeductibleExpenses.trim() || "0",
        nonTaxableRevenue: adj.nonTaxableRevenue.trim() || "0",
        fiscalDeductions: adj.fiscalDeductions.trim() || "0",
        priorLoss: adj.priorLoss.trim() || "0",
        sponsorship: adj.sponsorship.trim() || "0",
        anticipatedPayments: adj.anticipatedPayments.trim() || "0",
      });
    },
    onError: (err) => notify.error(formatError(err, t("declarations.d101.computeFailed"))),
  });

  const r = calc.data;

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">{t("declarations.d101.title")}</div>
      </div>

      <div style={{ padding: "14px 16px 0" }}>
        <div className="banner">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_INFO }} />
          <span>
            {t("declarations.d101.banner1")} <b>{t("declarations.d101.bannerBold")}</b>{" "}
            {t("declarations.d101.banner2")}
          </span>
        </div>
      </div>

      <div style={{ padding: "0 16px 16px" }}>
        <div className="fgrid" style={{ maxWidth: 720 }}>
          {FIELDS.map((f) => (
            <div className="field" key={f.key}>
              <label>{t(f.labelKey)}</label>
              <input
                className="input"
                inputMode="decimal"
                value={adj[f.key]}
                onChange={(e) => setAdj((a) => ({ ...a, [f.key]: e.target.value }))}
                placeholder="0.00"
              />
            </div>
          ))}
        </div>
        <button
          className="btn-dark"
          disabled={calc.isPending || !activeCompanyId}
          onClick={() => calc.mutate()}
          style={{ marginTop: 14 }}
        >
          {calc.isPending ? t("declarations.common.calcing") : t("declarations.d101.calc")}
        </button>
      </div>

      {r && (
        <>
          <table className="scr-table">
            <tbody>
              {[
                [t("declarations.d101.rows.accountingResult"), r.accountingResult],
                [t("declarations.d101.rows.nonTaxableRevenue"), r.nonTaxableRevenue],
                [t("declarations.d101.rows.fiscalDeductions"), r.fiscalDeductions],
                [t("declarations.d101.rows.nonDeductibleExpenses"), r.nonDeductibleExpenses],
                [t("declarations.d101.rows.fiscalResult"), r.fiscalResult],
                [t("declarations.d101.rows.priorLoss"), r.priorLoss],
                [t("declarations.d101.rows.lossUsed"), r.lossUsed],
                [t("declarations.d101.rows.taxableProfit"), r.taxableProfit],
                [t("declarations.d101.rows.tax16"), r.tax16],
                [t("declarations.d101.rows.sponsorshipCap"), r.sponsorshipCap],
                [t("declarations.d101.rows.sponsorshipCredit"), r.sponsorshipCredit],
                [t("declarations.d101.rows.taxAfterCredits"), r.taxAfterCredits],
                [t("declarations.d101.rows.anticipatedPayments"), r.anticipatedPayments],
              ].map(([label, v], i) => (
                <tr key={i}>
                  <td>{label}</td>
                  <td className="r num">{fmtRON(v)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="tot-foot">
            {Number(r.balanceDue) > 0 ? (
              <span className="chip late">
                <Ic name="xMark" cls="sic" />
                {t("declarations.d101.taxDue", { amount: fmtRON(r.balanceDue) })}
              </span>
            ) : Number(r.balanceRecoverable) > 0 ? (
              <span className="chip paid">
                <Ic name="checkC" cls="sic" />
                {t("declarations.d101.recoverable", { amount: fmtRON(r.balanceRecoverable) })}
              </span>
            ) : (
              <span className="chip paid">
                <Ic name="checkC" cls="sic" />
                {t("declarations.d101.zeroBalance")}
              </span>
            )}
            {Number(r.lossRemaining) > 0 && (
              <span>
                {t("declarations.d101.lossRemaining")} <b className="num">{fmtRON(r.lossRemaining)}</b> lei
              </span>
            )}
          </div>
        </>
      )}
    </div>
  );
}
