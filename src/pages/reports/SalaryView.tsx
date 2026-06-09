/**
 * SalaryView — calculator salariu (nucleul D112): brut → net + contribuții + cost angajator,
 * ratele 2026 (CAS 25%, CASS 10%, impozit 10%, CAM 2,25%). D112 complet (evidența nominală a
 * salariaților, stările lunare, export XML cu cele două versiuni de schemă 2026 și notele GL)
 * este o extensie ulterioară — aici este doar calculul de salariu.
 */

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";

import { SectionCard, Btn, Banner } from "@/components/rf";
import { api } from "@/lib/tauri";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";
import type { PayrollResult } from "@/types";

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
    <div className="rf-col">
      <SectionCard icon="declaration" title="Calculator salariu (nucleul D112)">
        <div style={{ padding: "0 16px 12px" }}>
          <Banner variant="info">
            Ratele 2026: <b>CAS 25%</b>, <b>CASS 10%</b>, <b>impozit 10%</b>, <b>CAM 2,25%</b>
            (angajator). Scutirile IT/construcții/agricultură au fost eliminate (OUG 156/2024).
            Deducerea personală se preia din tabelul ANAF. D112 complet (salariați + export XML)
            este în lucru.
          </Banner>
        </div>

        <div style={{ display: "flex", gap: 12, flexWrap: "wrap", padding: "0 16px 12px", alignItems: "flex-end" }}>
          <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
            <span style={{ color: "var(--rf-text-muted)" }}>Salariu brut (lei)</span>
            <input className="rf-input" inputMode="decimal" value={gross} onChange={(e) => setGross(e.target.value)} placeholder="5000" />
          </label>
          <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
            <span style={{ color: "var(--rf-text-muted)" }}>Deducere personală (lei)</span>
            <input className="rf-input" inputMode="decimal" value={deduction} onChange={(e) => setDeduction(e.target.value)} placeholder="0" />
          </label>
          <Btn variant="primary" size="sm" disabled={calc.isPending} onClick={() => calc.mutate()}>
            {calc.isPending ? "Calculez…" : "Calculează"}
          </Btn>
        </div>

        {r && (
          <div className="rf-tbl-wrap" style={{ padding: "0 16px 16px" }}>
            <table className="rf-tbl">
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
                    <td className="right rf-mono">{fmtRON(v)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </SectionCard>
    </div>
  );
}
