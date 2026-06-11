/**
 * EtvaView — RO e-TVA reconciliation (pre-filing self-check): app-computed D300 vs the ANAF
 * "decont precompletat" (P300ETVA). 2026: the conformance notification is abolished
 * (OUG 89/2025 + OUG 13/2026) — this is an internal self-check, not a notification response.
 * Embedded in the Reports page — Claude-Design classes (.scr-card / .scr-table / .chip / .banner).
 */

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";

import { Ic } from "@/components/shared/Ic";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { EtvaReconciliation } from "@/types";

interface Props {
  dateFrom: string;
  dateTo: string;
}

// Icons not in the Ic set — inlined verbatim from the prototype.
const IC_INFO =
  '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';
const IC_WARN =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

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
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">RO e-TVA — reconciliere decont precompletat (verificare internă)</div>
      </div>

      <div style={{ padding: "14px 16px 0" }}>
        <div className="banner">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_INFO }} />
          <span>
            Notificarea de conformare RO e-TVA a fost <b>abrogată</b> (OUG 89/2025 și OUG 13/2026,
            în vigoare 9 mar. 2026). Decontul precompletat (P300ETVA) rămâne <b>informativ</b> și
            este disponibil în SPV până pe data de 5 a lunii următoare termenului D300. Această
            verificare compară D300-ul calculat de aplicație cu valorile pe care le importați din
            precompletat, <b>înainte</b> de depunerea propriului D300 (singurul cu valoare juridică).
            Descărcarea automată din SPV nu este disponibilă local.
          </span>
        </div>
      </div>

      <div style={{ display: "flex", gap: 12, flexWrap: "wrap", padding: "0 16px 16px", alignItems: "flex-end" }}>
        <div className="field" style={{ width: 220 }}>
          <label>Precompletat — TVA colectată (lei)</label>
          <input
            className="input"
            inputMode="decimal"
            value={collectedVat}
            onChange={(e) => setCollectedVat(e.target.value)}
            placeholder="0.00"
          />
        </div>
        <div className="field" style={{ width: 220 }}>
          <label>Precompletat — TVA deductibilă (lei)</label>
          <input
            className="input"
            inputMode="decimal"
            value={deductibleVat}
            onChange={(e) => setDeductibleVat(e.target.value)}
            placeholder="0.00"
          />
        </div>
        <button
          className="pill-btn"
          disabled={fetchP300.isPending || !activeCompanyId}
          onClick={() => fetchP300.mutate()}
          title="Descarcă decontul precompletat (P300ETVA) din SPV"
        >
          <Ic name="dl" />
          {fetchP300.isPending ? "Se descarcă…" : "Solicită din SPV"}
        </button>
        <button
          className="btn-dark"
          disabled={recon.isPending || !activeCompanyId}
          onClick={() => recon.mutate()}
        >
          <Ic name="sync" />
          {recon.isPending ? "Se reconciliază…" : "Reconciliază"}
        </button>
      </div>

      {fetchP300.data && fetchP300.data.length > 0 && (
        <div style={{ padding: "0 16px 16px" }}>
          <div style={{ fontSize: 12, color: "var(--text-2)", marginBottom: 6 }}>
            Precompletat din SPV (copiați valorile TVA colectată / deductibilă în câmpurile de mai sus):
          </div>
          {fetchP300.data.map((f) => (
            <details key={f.name} style={{ marginBottom: 6 }}>
              <summary style={{ cursor: "pointer", fontSize: 12.5, fontWeight: 500 }}>{f.name}</summary>
              <pre style={{ maxHeight: 240, overflow: "auto", fontSize: 11, background: "var(--fill)", padding: 8, borderRadius: 6 }}>
                {f.json}
              </pre>
            </details>
          ))}
        </div>
      )}

      {result && (
        <>
          {result.cashVat && (
            <div style={{ padding: "0 16px" }}>
              <div className="banner warn">
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
                <span>
                  Compania aplică <b>TVA la încasare</b> — divergențele față de precompletat (construit
                  pe datele e-Factura, nu pe încasare) sunt <b>așteptate</b>.
                </span>
              </div>
            </div>
          )}
          <table className="scr-table">
            <thead>
              <tr>
                <th>Linie</th>
                <th className="r">D300 (calculat)</th>
                <th className="r">Precompletat</th>
                <th className="r">Diferență</th>
                <th className="r">%</th>
                <th>Stare</th>
              </tr>
            </thead>
            <tbody>
              {result.lines.map((l, i) => (
                <tr key={i}>
                  <td style={{ fontWeight: 500 }}>{l.label}</td>
                  <td className="r num">{l.d300}</td>
                  <td className="r num">{l.precompletat}</td>
                  <td className="r num">{l.diff}</td>
                  <td className="r num">{l.diffPct}%</td>
                  <td>
                    {l.significant ? (
                      <span className="chip late">
                        <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
                        Semnificativ
                      </span>
                    ) : (
                      <span className="chip paid">
                        <Ic name="checkC" cls="sic" />
                        OK
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="tot-foot">
            <span>
              {result.anySignificant
                ? "Există diferențe semnificative (≥20% și ≥5.000 lei). Investigați (factură lipsă, decalaj de perioadă, taxare inversă) înainte de depunere."
                : "Nicio diferență semnificativă față de pragul de 20% și 5.000 lei."}
            </span>
          </div>
        </>
      )}
    </div>
  );
}
