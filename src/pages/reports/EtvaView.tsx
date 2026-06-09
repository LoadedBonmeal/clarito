/**
 * EtvaView — RO e-TVA reconciliation (pre-filing self-check): app-computed D300 vs the ANAF
 * "decont precompletat" (P300ETVA). 2026: the conformance notification is abolished
 * (OUG 89/2025 + OUG 13/2026) — this is an internal self-check, not a notification response.
 */

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";

import { SectionCard, Btn, Badge, Banner } from "@/components/rf";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { EtvaReconciliation } from "@/types";

interface Props {
  dateFrom: string;
  dateTo: string;
}

export function EtvaView({ dateFrom, dateTo }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [collectedVat, setCollectedVat] = useState("");
  const [deductibleVat, setDeductibleVat] = useState("");

  const recon = useMutation({
    mutationFn: (): Promise<EtvaReconciliation> => {
      if (!activeCompanyId) throw new Error("Selectați o companie activă.");
      return api.declarations.reconcileEtva(activeCompanyId, dateFrom, dateTo, {
        collectedVat: collectedVat.trim() || "0",
        deductibleVat: deductibleVat.trim() || "0",
      });
    },
    onError: (err) => notify.error(formatError(err, "Nu s-a putut reconcilia e-TVA.")),
  });

  // Fetch the precompletat zip from ANAF (an/luna from the period start) → its raw JSON files.
  const fetchP300 = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) throw new Error("Selectați o companie activă.");
      const an = Number(dateFrom.slice(0, 4));
      const luna = Number(dateFrom.slice(5, 7));
      return api.declarations.fetchEtvaPrecompletat(activeCompanyId, an, luna);
    },
    onSuccess: (files) => notify.success(`${files.length} fișier(e) e-TVA descărcate din SPV.`),
    onError: (err) => notify.error(formatError(err, "Nu s-a putut descărca precompletatul din SPV.")),
  });

  const result = recon.data;

  return (
    <div className="rf-col">
      <SectionCard icon="declaration" title="RO e-TVA — reconciliere decont precompletat (verificare internă)">
        <div style={{ padding: "0 16px 12px" }}>
          <Banner variant="info">
            Notificarea de conformare RO e-TVA a fost <b>abrogată</b> (OUG 89/2025 și OUG 13/2026,
            în vigoare 9 mar. 2026). Decontul precompletat (P300ETVA) rămâne <b>informativ</b> și
            este disponibil în SPV până pe data de 5 a lunii următoare termenului D300. Această
            verificare compară D300-ul calculat de aplicație cu valorile pe care le importați din
            precompletat, <b>înainte</b> de depunerea propriului D300 (singurul cu valoare juridică).
            Descărcarea automată din SPV nu este disponibilă local.
          </Banner>
        </div>

        <div style={{ display: "flex", gap: 12, flexWrap: "wrap", padding: "0 16px 12px", alignItems: "flex-end" }}>
          <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
            <span style={{ color: "var(--rf-text-muted)" }}>Precompletat — TVA colectată (lei)</span>
            <input
              className="rf-input"
              inputMode="decimal"
              value={collectedVat}
              onChange={(e) => setCollectedVat(e.target.value)}
              placeholder="0.00"
            />
          </label>
          <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12.5 }}>
            <span style={{ color: "var(--rf-text-muted)" }}>Precompletat — TVA deductibilă (lei)</span>
            <input
              className="rf-input"
              inputMode="decimal"
              value={deductibleVat}
              onChange={(e) => setDeductibleVat(e.target.value)}
              placeholder="0.00"
            />
          </label>
          <Btn
            variant="secondary"
            size="sm"
            disabled={fetchP300.isPending || !activeCompanyId}
            onClick={() => fetchP300.mutate()}
            title="Descarcă decontul precompletat (P300ETVA) din SPV"
          >
            {fetchP300.isPending ? "Se descarcă…" : "Solicită din SPV"}
          </Btn>
          <Btn
            variant="primary"
            size="sm"
            disabled={recon.isPending || !activeCompanyId}
            onClick={() => recon.mutate()}
          >
            {recon.isPending ? "Se reconciliază…" : "Reconciliază"}
          </Btn>
        </div>

        {fetchP300.data && fetchP300.data.length > 0 && (
          <div style={{ padding: "0 16px 12px" }}>
            <div style={{ fontSize: 12, color: "var(--rf-text-muted)", marginBottom: 6 }}>
              Precompletat din SPV (copiați valorile TVA colectată / deductibilă în câmpurile de mai sus):
            </div>
            {fetchP300.data.map((f) => (
              <details key={f.name} style={{ marginBottom: 6 }}>
                <summary style={{ cursor: "pointer", fontSize: 12.5, fontWeight: 500 }}>{f.name}</summary>
                <pre style={{ maxHeight: 240, overflow: "auto", fontSize: 11, background: "var(--rf-bg-subtle)", padding: 8, borderRadius: 6 }}>
                  {f.json}
                </pre>
              </details>
            ))}
          </div>
        )}

        {result && (
          <>
            {result.cashVat && (
              <div style={{ padding: "0 16px 12px" }}>
                <Banner variant="warning">
                  Compania aplică <b>TVA la încasare</b> — divergențele față de precompletat (construit
                  pe datele e-Factura, nu pe încasare) sunt <b>așteptate</b>.
                </Banner>
              </div>
            )}
            <div className="rf-tbl-wrap">
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th>Linie</th>
                    <th className="right">D300 (calculat)</th>
                    <th className="right">Precompletat</th>
                    <th className="right">Diferență</th>
                    <th className="right">%</th>
                    <th>Stare</th>
                  </tr>
                </thead>
                <tbody>
                  {result.lines.map((l, i) => (
                    <tr key={i}>
                      <td style={{ fontWeight: 500 }}>{l.label}</td>
                      <td className="right rf-mono">{l.d300}</td>
                      <td className="right rf-mono">{l.precompletat}</td>
                      <td className="right rf-mono">{l.diff}</td>
                      <td className="right rf-mono">{l.diffPct}%</td>
                      <td>
                        {l.significant ? (
                          <Badge variant="error">Semnificativ</Badge>
                        ) : (
                          <Badge variant="success">OK</Badge>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
              {result.anySignificant
                ? "Există diferențe semnificative (≥20% și ≥5.000 lei). Investigați (factură lipsă, decalaj de perioadă, taxare inversă) înainte de depunere."
                : "Nicio diferență semnificativă față de pragul de 20% și 5.000 lei."}
            </div>
          </>
        )}
      </SectionCard>
    </div>
  );
}
