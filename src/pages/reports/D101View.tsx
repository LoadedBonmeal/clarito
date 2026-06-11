/**
 * D101View — fișă de calcul impozit pe profit (Formular 101, OPANAF 206/2025).
 * Baza (rezultat brut + cifră de afaceri) vine din contul de profit și pierdere al perioadei;
 * utilizatorul introduce ajustările fiscale (art. 19 Cod fiscal). Depunerea rămâne manuală prin
 * PDF inteligent ANAF + SPV (ca D300/D394).
 * Embedded in the Reports page — Claude-Design classes (.scr-card / .scr-table / .fgrid / .chip).
 */

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";

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

const FIELDS: { key: keyof Adjustments; label: string }[] = [
  { key: "nonDeductibleExpenses", label: "Cheltuieli nedeductibile (protocol >2%, amenzi, 50% auto…)" },
  { key: "nonTaxableRevenue", label: "Venituri neimpozabile (dividende primite, reluări provizioane…)" },
  { key: "fiscalDeductions", label: "Deduceri fiscale (amortizare fiscală, rezervă legală…)" },
  { key: "priorLoss", label: "Pierdere fiscală de recuperat din anii precedenți" },
  { key: "sponsorship", label: "Sponsorizare efectuată (credit plafonat)" },
  { key: "anticipatedPayments", label: "Plăți anticipate / impozit declarat D100" },
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
      if (!activeCompanyId) throw new Error("Selectați o companie activă.");
      return api.declarations.computeD101(activeCompanyId, dateFrom, dateTo, {
        nonDeductibleExpenses: adj.nonDeductibleExpenses.trim() || "0",
        nonTaxableRevenue: adj.nonTaxableRevenue.trim() || "0",
        fiscalDeductions: adj.fiscalDeductions.trim() || "0",
        priorLoss: adj.priorLoss.trim() || "0",
        sponsorship: adj.sponsorship.trim() || "0",
        anticipatedPayments: adj.anticipatedPayments.trim() || "0",
      });
    },
    onError: (err) => notify.error(formatError(err, "Nu s-a putut calcula D101.")),
  });

  const r = calc.data;

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">D101 — Impozit pe profit (fișă de calcul)</div>
      </div>

      <div style={{ padding: "14px 16px 0" }}>
        <div className="banner">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_INFO }} />
          <span>
            Doar pentru companiile plătitoare de <b>impozit pe profit</b> (nu microîntreprinderi).
            Rezultatul brut și cifra de afaceri se preiau din contul de profit și pierdere al
            perioadei; introduceți ajustările fiscale. Recuperarea pierderii din anii precedenți e
            plafonată la 70% din profitul fiscal (OUG 115/2023). Depunerea se face manual prin PDF
            inteligent ANAF + SPV (termen 25 iunie anul următor pentru exercițiile 2021-2025,
            ulterior 25 martie). Estimarea nu include toate ajustările posibile.
          </span>
        </div>
      </div>

      <div style={{ padding: "0 16px 16px" }}>
        <div className="fgrid" style={{ maxWidth: 720 }}>
          {FIELDS.map((f) => (
            <div className="field" key={f.key}>
              <label>{f.label}</label>
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
          {calc.isPending ? "Calculez…" : "Calculează D101"}
        </button>
      </div>

      {r && (
        <>
          <table className="scr-table">
            <tbody>
              {[
                ["Rezultat brut contabil (din P&L)", r.accountingResult],
                ["− Venituri neimpozabile", r.nonTaxableRevenue],
                ["− Deduceri fiscale", r.fiscalDeductions],
                ["+ Cheltuieli nedeductibile", r.nonDeductibleExpenses],
                ["= Rezultat fiscal", r.fiscalResult],
                ["Pierdere reportată disponibilă", r.priorLoss],
                ["− Pierdere recuperată (max 70%)", r.lossUsed],
                ["= Profit impozabil", r.taxableProfit],
                ["Impozit 16%", r.tax16],
                ["Plafon sponsorizare (0,75% CA / 20% impozit)", r.sponsorshipCap],
                ["− Credit sponsorizare", r.sponsorshipCredit],
                ["= Impozit după credite", r.taxAfterCredits],
                ["− Plăți anticipate", r.anticipatedPayments],
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
                Impozit de plată: {fmtRON(r.balanceDue)} lei
              </span>
            ) : Number(r.balanceRecoverable) > 0 ? (
              <span className="chip paid">
                <Ic name="checkC" cls="sic" />
                De recuperat: {fmtRON(r.balanceRecoverable)} lei
              </span>
            ) : (
              <span className="chip paid">
                <Ic name="checkC" cls="sic" />
                Sold zero
              </span>
            )}
            {Number(r.lossRemaining) > 0 && (
              <span>
                Pierdere rămasă de reportat: <b className="num">{fmtRON(r.lossRemaining)}</b> lei
              </span>
            )}
          </div>
        </>
      )}
    </div>
  );
}
