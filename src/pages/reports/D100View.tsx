/**
 * D100View — Declarația privind obligațiile de plată la bugetul de stat (rândul trimestrial).
 * Micro → cod 121 (1% × venituri); profit → cod 103 (16% × rezultat), din P&L-ul perioadei.
 * Depunerea rămâne manuală prin PDF inteligent + SPV.
 */

import { useMemo, useState } from "react";
import { useMutation } from "@tanstack/react-query";

import { SectionCard, Btn, Banner } from "@/components/rf";
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

export function D100View({ dateFrom, dateTo }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [prior, setPrior] = useState("");

  const { quarter, year } = useMemo(() => {
    const y = Number(dateFrom.slice(0, 4));
    const m = Number(dateFrom.slice(5, 7));
    return { quarter: Math.ceil(m / 3), year: y };
  }, [dateFrom]);

  const calc = useMutation({
    mutationFn: (): Promise<D100Result> => {
      if (!activeCompanyId) throw new Error("Selectați o companie activă.");
      return api.declarations.computeD100(activeCompanyId, dateFrom, dateTo, quarter, year, prior.trim() || "0");
    },
    onError: (err) => notify.error(formatError(err, "Nu s-a putut calcula D100.")),
  });

  const r = calc.data;

  return (
    <div className="rf-col">
      <SectionCard icon="declaration" title="D100 — Obligații de plată (trimestrial)">
        <div style={{ padding: "0 16px 12px" }}>
          <Banner variant="info">
            Rândul trimestrial pentru perioada selectată (T{quarter} {year}): micro → cod 121
            (1% × venituri), profit → cod 103 (16% × rezultat). Scadența: 25 a lunii următoare
            trimestrului. Depunerea se face manual prin PDF inteligent ANAF + SPV.
          </Banner>
        </div>
        <div style={{ display: "flex", gap: 12, flexWrap: "wrap", padding: "0 16px 12px", alignItems: "flex-end" }}>
          <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
            <span style={{ color: "var(--rf-text-muted)" }}>Plăți anticipate anterioare (lei)</span>
            <input className="rf-input" inputMode="decimal" value={prior} onChange={(e) => setPrior(e.target.value)} placeholder="0" style={{ maxWidth: 200 }} />
          </label>
          <Btn variant="primary" size="sm" disabled={calc.isPending || !activeCompanyId} onClick={() => calc.mutate()}>
            {calc.isPending ? "Calculez…" : "Calculează D100"}
          </Btn>
        </div>

        {r && (
          <div className="rf-tbl-wrap" style={{ padding: "0 16px 16px" }}>
            <table className="rf-tbl">
              <thead>
                <tr><th>Cod obligație</th><th>Denumire</th><th className="right">Bază</th><th className="right">Cotă</th><th className="right">Datorat</th><th className="right">De plată</th><th>Scadență</th></tr>
              </thead>
              <tbody>
                <tr>
                  <td className="rf-mono" style={{ fontWeight: 600 }}>{r.codOblig}</td>
                  <td>{r.label}</td>
                  <td className="right rf-mono">{fmtRON(r.base)}</td>
                  <td className="right rf-mono">{r.ratePct}%</td>
                  <td className="right rf-mono">{fmtRON(r.sumaDatorata)}</td>
                  <td className="right rf-mono" style={{ fontWeight: 700 }}>{fmtRON(r.sumaDePlata)}</td>
                  <td className="rf-mono">{r.scadenta}</td>
                </tr>
              </tbody>
            </table>
          </div>
        )}
      </SectionCard>
    </div>
  );
}
