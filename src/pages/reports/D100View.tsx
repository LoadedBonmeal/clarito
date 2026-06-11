/**
 * D100View — Declarația privind obligațiile de plată la bugetul de stat (rândul trimestrial).
 * Micro → poziția 5 (1% × venituri); profit → poziția 2 (16% × rezultat), din P&L-ul perioadei.
 * Trim. IV pe profit se regularizează prin D101. Depunerea rămâne manuală prin PDF inteligent + SPV.
 * Embedded in the Reports page — Claude-Design classes (.scr-card / .scr-table / .banner / .field).
 */

import { useMemo, useState } from "react";
import { useMutation } from "@tanstack/react-query";

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
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">D100 — Obligații de plată (trimestrial)</div>
      </div>

      <div style={{ padding: "14px 16px 0" }}>
        <div className="banner">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_INFO }} />
          <span>
            Rândul trimestrial pentru perioada selectată (T{quarter} {year}): micro → poziția 5
            (1% × venituri), profit → poziția 2 (16% × rezultat). Scadența: 25 a lunii următoare
            trimestrului (profit T4 se regularizează prin D101). Depunerea se face manual prin
            PDF inteligent ANAF + SPV. Regimul micro (1%) presupune îndeplinirea continuă a
            condițiilor legale (plafon 100.000 EUR din 2026, salariat, structura veniturilor,
            asociați) — aplicația nu le verifică pe toate; confirmați eligibilitatea.
          </span>
        </div>
      </div>

      <div style={{ display: "flex", gap: 12, flexWrap: "wrap", padding: "0 16px 16px", alignItems: "flex-end" }}>
        <div className="field" style={{ width: 200 }}>
          <label>Plăți anticipate anterioare (lei)</label>
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
          {calc.isPending ? "Calculez…" : "Calculează D100"}
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
              <th>Cod obligație</th>
              <th>Denumire</th>
              <th className="r">Bază</th>
              <th className="r">Cotă</th>
              <th className="r">Datorat</th>
              <th className="r">De plată</th>
              <th>Scadență</th>
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
    </div>
  );
}
