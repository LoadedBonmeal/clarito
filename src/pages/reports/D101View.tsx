/**
 * D101View — fișă de calcul impozit pe profit (Formular 101, OPANAF 206/2025).
 * Baza (rezultat brut + cifră de afaceri) vine din contul de profit și pierdere al perioadei;
 * utilizatorul introduce ajustările fiscale (art. 19 Cod fiscal). Depunerea rămâne manuală prin
 * PDF inteligent ANAF + SPV (ca D300/D394).
 */

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";

import { SectionCard, Btn, Badge, Banner } from "@/components/rf";
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
    <div className="rf-col">
      <SectionCard icon="declaration" title="D101 — Impozit pe profit (fișă de calcul)">
        <div style={{ padding: "0 16px 12px" }}>
          <Banner variant="info">
            Doar pentru companiile plătitoare de <b>impozit pe profit</b> (nu microîntreprinderi).
            Rezultatul brut și cifra de afaceri se preiau din contul de profit și pierdere al
            perioadei; introduceți ajustările fiscale. Recuperarea pierderii din anii precedenți e
            plafonată la 70% din profitul fiscal (OUG 115/2023). Depunerea se face manual prin PDF
            inteligent ANAF + SPV (termen 25 iunie anul următor pentru exercițiile 2021-2025,
            ulterior 25 martie). Estimarea nu include toate ajustările posibile.
          </Banner>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 10, padding: "0 16px 12px" }}>
          {FIELDS.map((f) => (
            <label key={f.key} style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
              <span style={{ color: "var(--rf-text-muted)" }}>{f.label}</span>
              <input
                className="rf-input"
                inputMode="decimal"
                value={adj[f.key]}
                onChange={(e) => setAdj((a) => ({ ...a, [f.key]: e.target.value }))}
                placeholder="0.00"
                style={{ maxWidth: 260 }}
              />
            </label>
          ))}
          <Btn
            variant="primary"
            size="sm"
            disabled={calc.isPending || !activeCompanyId}
            onClick={() => calc.mutate()}
            style={{ alignSelf: "flex-start" }}
          >
            {calc.isPending ? "Calculez…" : "Calculează D101"}
          </Btn>
        </div>

        {r && (
          <div className="rf-tbl-wrap" style={{ padding: "0 16px 16px" }}>
            <table className="rf-tbl">
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
                    <td className="right rf-mono">{fmtRON(v)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div style={{ marginTop: 10 }}>
              {Number(r.balanceDue) > 0 ? (
                <Badge variant="error">Impozit de plată: {fmtRON(r.balanceDue)} lei</Badge>
              ) : Number(r.balanceRecoverable) > 0 ? (
                <Badge variant="success">De recuperat: {fmtRON(r.balanceRecoverable)} lei</Badge>
              ) : (
                <Badge variant="success">Sold zero</Badge>
              )}
              {Number(r.lossRemaining) > 0 && (
                <span style={{ marginLeft: 8, fontSize: 12, color: "var(--rf-text-muted)" }}>
                  Pierdere rămasă de reportat: {fmtRON(r.lossRemaining)} lei
                </span>
              )}
            </div>
          </div>
        )}
      </SectionCard>
    </div>
  );
}
