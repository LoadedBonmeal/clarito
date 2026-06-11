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

import { api } from "@/lib/tauri";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";
import type { PayrollResult } from "@/types";

// Info icon absent from the Ic set — inlined verbatim (design banner pattern).
const SVG_INFO_CIRCLE = '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';

export function SalaryView() {
  const [gross, setGross] = useState("");
  const [deduction, setDeduction] = useState("");

  const calc = useMutation({
    mutationFn: (): Promise<PayrollResult> =>
      api.declarations.computePayroll({
        gross: gross.trim() || "0",
        personalDeduction: deduction.trim() || "0",
      }),
    onError: (err) => notify.error(formatError(err, "Nu s-a putut calcula salariul.")),
  });

  const r = calc.data;

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">Calculator salariu (nucleul D112)</div>
      </div>

      <div className="card-pad" style={{ paddingBottom: r ? 0 : 16 }}>
        <div className="banner">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_INFO_CIRCLE }} />
          <span>
            Ratele 2026: <b>CAS 25%</b>, <b>CASS 10%</b>, <b>impozit 10%</b>, <b>CAM 2,25%</b>
            {" "}(angajator). Scutirile IT/construcții/agricultură au fost eliminate (OUG 156/2024).
            Deducerea personală se preia din tabelul ANAF. D112 complet (salariați + export XML)
            este în lucru.
          </span>
        </div>

        <div style={{ display: "flex", gap: 14, flexWrap: "wrap", alignItems: "flex-end" }}>
          <div className="field" style={{ width: 200 }}>
            <label>Salariu brut (lei)</label>
            <input
              className="input num"
              inputMode="decimal"
              value={gross}
              onChange={(e) => setGross(e.target.value)}
              placeholder="5000"
            />
          </div>
          <div className="field" style={{ width: 200 }}>
            <label>Deducere personală (lei)</label>
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
            {calc.isPending ? "Calculez…" : "Calculează"}
          </button>
        </div>
      </div>

      {r && (
        <table className="scr-table" style={{ marginTop: 16 }}>
          <tbody>
            {[
              ["Salariu brut", r.gross],
              ["CAS (pensie 25%)", r.cas],
              ["CASS (sănătate 10%)", r.cass],
              ["Deducere personală", r.personalDeduction],
              ["Bază impozabilă", r.taxableBase],
              ["Impozit pe venit (10%)", r.incomeTax],
              ["Salariu net", r.net],
              ["CAM angajator (2,25%)", r.cam],
              ["Cost total angajator", r.totalEmployerCost],
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
