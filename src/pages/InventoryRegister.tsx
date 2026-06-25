/**
 * Registru-inventar — cod 14-1-2 (OMFP 2634/2015).
 *
 * Registrul anual imutabil cu 6 coloane:
 *   1. Nr. crt.  2. Recapitulație  3. Valoare contabilă
 *   4. Valoare inventar  5. Diferențe  6. Cauze
 *
 * Picker de an fiscal → table → rând TOTAL → print-friendly.
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { queryKeys } from "@/lib/queries";
import { Ic } from "@/components/shared/Ic";

// ─── Helpers ─────────────────────────────────────────────────────────────────

const currentYear = () => new Date().getFullYear();

function diffColor(diff: string) {
  const v = parseDec(diff);
  if (v < 0) return "var(--red)";
  if (v > 0) return "var(--green)";
  return "var(--text-2)";
}

// ─── Main ─────────────────────────────────────────────────────────────────────

export function InventoryRegisterPage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [fiscalYear, setFiscalYear] = useState(currentYear());

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  const { data: entries = [] } = useQuery({
    queryKey: ["registru-inventar", activeCompanyId, fiscalYear],
    queryFn: () =>
      activeCompanyId
        ? api.inventory.listRegistru(activeCompanyId, fiscalYear)
        : Promise.resolve([]),
    enabled: !!activeCompanyId,
  });

  const activeCompany = companies.find((c) => c.id === activeCompanyId);
  const companyName = activeCompany?.legalName ?? "";

  const totalContabila = entries.reduce((s, e) => s + parseDec(e.valueContabila), 0);
  // totalInventar and totalDiff retained for print/export parity
  void entries.reduce((s, e) => s + parseDec(e.valueInventar), 0);
  void entries.reduce((s, e) => s + parseDec(e.diffValue), 0);

  if (!activeCompanyId) {
    return (
      <div className="main-inner" style={{ padding: 32, color: "var(--text-2)" }}>
        Selectați o companie activă.
      </div>
    );
  }

  return (
    <div className="main-inner wide">

      {/* ── Page header ─────────────────────────────────────────────────── */}
      <div className="page-head">
        <div>
          <h1>{t("inventory.registerTitle")}</h1>
          <p className="sub">
            cod 14-1-2 (OMFP 2634/2015) · exercitiul {fiscalYear}
            {companyName ? ` · ${companyName}` : ""}
          </p>
        </div>
        <div className="head-actions">
          {/* Year selector styled as pill-btn */}
          <select
            className="pill-btn"
            value={fiscalYear}
            onChange={(e) => setFiscalYear(Number(e.target.value))}
            style={{ cursor: "pointer" }}
            aria-label="An fiscal"
          >
            {Array.from({ length: 6 }, (_, i) => currentYear() - i).map((y) => (
              <option key={y} value={y}>{y}</option>
            ))}
          </select>

          <button
            className="pill-btn"
            onClick={() => window.print()}
            title="Imprimă registrul"
          >
            <Ic name="printer" />
            Imprima / PDF
          </button>

          <button
            className="btn-dark"
            onClick={() => {/* Element nou — handler reserved for future modal */}}
            title={t("inventory.addEntry") as string}
          >
            <Ic name="plus" />
            {t("inventory.addEntry") ?? "Element nou"}
          </button>
        </div>
      </div>

      {/* ── Register table ──────────────────────────────────────────────── */}
      <div className="scr-card" id="registru-print">

        {/* Print title (visible only when printing) */}
        <div className="print-only" style={{ marginBottom: 16, display: "none" }}>
          <h2 style={{ margin: 0 }}>
            REGISTRU-INVENTAR — An fiscal {fiscalYear}
          </h2>
          <p style={{ margin: "4px 0", fontSize: 12 }}>
            Cod 14-1-2 (OMFP 2634/2015)
          </p>
        </div>

        <table className="scr-table">
          <thead>
            <tr>
              <th style={{ width: 70 }}>Nr</th>
              <th style={{ width: 130 }}>Data</th>
              <th>Element patrimonial</th>
              <th className="r" style={{ width: 150 }}>Valoare</th>
              <th style={{ width: 220 }}>Observatii</th>
            </tr>
          </thead>

          {entries.length === 0 ? (
            <tbody>
              <tr>
                <td colSpan={5} style={{ padding: 0 }}>
                  <div className="empty">
                    <div className="ei"><Ic name="book" /></div>
                    <b>Registru-inventar gol.</b>
                    Elementele patrimoniale apar aici la inchiderea exercitiului.
                  </div>
                </td>
              </tr>
            </tbody>
          ) : (
            <>
              <tbody>
                {entries.map((entry) => (
                  <tr key={entry.id}>
                    <td>{entry.seqNo}</td>
                    <td style={{ fontSize: 13, color: "var(--text-2)" }}>
                      {/* date field — falls back to em-dash when absent on the row type */}
                      {(entry as { date?: string }).date ?? "—"}
                    </td>
                    <td>{entry.recapText}</td>
                    <td className="r" style={{ color: diffColor(entry.valueContabila), fontWeight: 500 }}>
                      {fmtRON(entry.valueContabila)}
                    </td>
                    <td style={{ fontSize: 12, color: "var(--text-2)" }}>
                      {entry.diffCause || "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr style={{ fontWeight: 700, borderTop: "2px solid var(--line)" }}>
                  <td colSpan={3} style={{ textAlign: "right" }}>
                    {t("inventory.registru.totals")}
                  </td>
                  <td className="r">{fmtRON(totalContabila)}</td>
                  <td />
                </tr>
              </tfoot>
            </>
          )}
        </table>

        {entries.length > 0 && (
          <div style={{ marginTop: 12, fontSize: 11, color: "var(--text-2)", borderTop: "1px solid var(--line)", paddingTop: 8 }}>
            {entries.length} înregistrări · An fiscal {fiscalYear}
            {companyName ? ` · ${companyName}` : ""}
          </div>
        )}
      </div>

      {/* Print styles */}
      <style>{`
        @media print {
          .sidebar, .topbar, .page-head button, .page-head select { display: none !important; }
          .print-only { display: block !important; }
          .scr-card { border: none; box-shadow: none; }
          body { color: #000; background: #fff; }
        }
      `}</style>
    </div>
  );
}
