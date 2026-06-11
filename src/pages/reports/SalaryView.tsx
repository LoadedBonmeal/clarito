/**
 * SalaryView — calculator salariu (nucleul D112): brut → net + contribuții + cost angajator,
 * ratele 2026 (CAS 25%, CASS 10%, impozit 10%, CAM 2,25%). D112 complet (evidența nominală a
 * salariaților, stările lunare, export XML cu cele două versiuni de schemă 2026 și notele GL)
 * este o extensie ulterioară — aici este doar calculul de salariu.
 * Claude-Design classes: .scr-card + .scr-toolbar .tt + .banner + .fgrid/.field + .btn-dark + .scr-table.
 * ALL wiring preserved: api.declarations.computePayroll mutation.
 */

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { api } from "@/lib/tauri";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";
import type { PayrollResult } from "@/types";

// Info icon absent from the Ic set — inlined verbatim (design banner pattern).
const SVG_INFO_CIRCLE = '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';

export function SalaryView() {
  const { t } = useTranslation();
  const [gross, setGross] = useState("");
  const [deduction, setDeduction] = useState("");

  const calc = useMutation({
    mutationFn: (): Promise<PayrollResult> =>
      api.declarations.computePayroll({
        gross: gross.trim() || "0",
        personalDeduction: deduction.trim() || "0",
      }),
    onError: (err) => notify.error(formatError(err, t("reports.salary.computeFailed"))),
  });

  const r = calc.data;

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">{t("reports.salary.title")}</div>
      </div>

      <div className="card-pad" style={{ paddingBottom: r ? 0 : 16 }}>
        <div className="banner">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_INFO_CIRCLE }} />
          <span>
            {t("reports.salary.banner1")} <b>{t("reports.salary.cas")}</b>, <b>{t("reports.salary.cass")}</b>, <b>{t("reports.salary.tax")}</b>, <b>{t("reports.salary.cam")}</b>
            {" "}{t("reports.salary.banner2")}
          </span>
        </div>

        <div style={{ display: "flex", gap: 14, flexWrap: "wrap", alignItems: "flex-end" }}>
          <div className="field" style={{ width: 200 }}>
            <label>{t("reports.salary.grossLabel")}</label>
            <input
              className="input num"
              inputMode="decimal"
              value={gross}
              onChange={(e) => setGross(e.target.value)}
              placeholder="5000"
            />
          </div>
          <div className="field" style={{ width: 200 }}>
            <label>{t("reports.salary.deductionLabel")}</label>
            <input
              className="input num"
              inputMode="decimal"
              value={deduction}
              onChange={(e) => setDeduction(e.target.value)}
              placeholder="0"
            />
          </div>
          <button
            className="btn-dark"
            style={{ opacity: calc.isPending ? 0.6 : 1 }}
            disabled={calc.isPending}
            onClick={() => calc.mutate()}
          >
            {calc.isPending ? t("declarations.common.calcing") : t("declarations.common.calc")}
          </button>
        </div>
      </div>

      {r && (
        <table className="scr-table" style={{ marginTop: 16 }}>
          <tbody>
            {[
              [t("reports.salary.rows.gross"), r.gross],
              [t("reports.salary.rows.cas"), r.cas],
              [t("reports.salary.rows.cass"), r.cass],
              [t("reports.salary.rows.deduction"), r.personalDeduction],
              [t("reports.salary.rows.taxableBase"), r.taxableBase],
              [t("reports.salary.rows.incomeTax"), r.incomeTax],
              [t("reports.salary.rows.net"), r.net],
              [t("reports.salary.rows.camEmployer"), r.cam],
              [t("reports.salary.rows.employerCost"), r.totalEmployerCost],
            ].map(([label, v], i) => (
              <tr key={i} style={i === 6 || i === 8 ? { fontWeight: 700 } : undefined}>
                <td>{label}</td>
                <td className="r num">{i === 6 || i === 8 ? <b>{fmtRON(v)}</b> : fmtRON(v)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
