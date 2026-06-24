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

  const { data: entries = [] } = useQuery({
    queryKey: ["registru-inventar", activeCompanyId, fiscalYear],
    queryFn: () =>
      activeCompanyId
        ? api.inventory.listRegistru(activeCompanyId, fiscalYear)
        : Promise.resolve([]),
    enabled: !!activeCompanyId,
  });

  if (!activeCompanyId) {
    return (
      <div className="main-inner" style={{ padding: 32, color: "var(--text-2)" }}>
        Selectați o companie activă.
      </div>
    );
  }

  const totalContabila = entries.reduce((s, e) => s + parseDec(e.valueContabila), 0);
  const totalInventar = entries.reduce((s, e) => s + parseDec(e.valueInventar), 0);
  const totalDiff = entries.reduce((s, e) => s + parseDec(e.diffValue), 0);

  return (
    <div className="main-inner">
      {/* Page header */}
      <div className="page-head">
        <div>
          <h1 className="page-title">{t("inventory.registerTitle")}</h1>
          <div className="page-sub">{t("inventory.registerSubtitle")}</div>
        </div>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          {/* Year picker */}
          <select
            className="fsel"
            value={fiscalYear}
            onChange={(e) => setFiscalYear(Number(e.target.value))}
            style={{ minWidth: 90 }}
          >
            {Array.from({ length: 6 }, (_, i) => currentYear() - i).map((y) => (
              <option key={y} value={y}>{y}</option>
            ))}
          </select>
          <button
            className="btn-dark"
            onClick={() => window.print()}
            title="Imprimă registrul"
          >
            Imprimă / PDF
          </button>
        </div>
      </div>

      {/* Register table */}
      {entries.length === 0 ? (
        <div className="scr-card">
          <div className="state-row muted">{t("inventory.registru.noEntries")}</div>
        </div>
      ) : (
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

          <div style={{ overflowX: "auto" }}>
            <table className="scr-table" style={{ fontSize: 13 }}>
              <thead>
                <tr>
                  <th style={{ width: 48 }}>{t("inventory.registru.seqNo")}</th>
                  <th>{t("inventory.registru.recapText")}</th>
                  <th className="num">{t("inventory.registru.valueContabila")}</th>
                  <th className="num">{t("inventory.registru.valueInventar")}</th>
                  <th className="num">{t("inventory.registru.diffValue")}</th>
                  <th>{t("inventory.registru.diffCause")}</th>
                </tr>
              </thead>
              <tbody>
                {entries.map((entry) => (
                  <tr key={entry.id}>
                    <td className="num">{entry.seqNo}</td>
                    <td>{entry.recapText}</td>
                    <td className="num">{fmtRON(entry.valueContabila)}</td>
                    <td className="num">{fmtRON(entry.valueInventar)}</td>
                    <td className="num" style={{ color: diffColor(entry.diffValue), fontWeight: 600 }}>
                      {parseDec(entry.diffValue) !== 0
                        ? (parseDec(entry.diffValue) > 0 ? "+" : "") + fmtRON(entry.diffValue)
                        : "—"}
                    </td>
                    <td style={{ fontSize: 12, color: "var(--text-2)" }}>
                      {entry.diffCause || "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr style={{ fontWeight: 700, borderTop: "2px solid var(--line)" }}>
                  <td colSpan={2} style={{ textAlign: "right" }}>
                    {t("inventory.registru.totals")}
                  </td>
                  <td className="num">{fmtRON(totalContabila)}</td>
                  <td className="num">{fmtRON(totalInventar)}</td>
                  <td className="num" style={{ color: diffColor(String(totalDiff)) }}>
                    {totalDiff !== 0
                      ? (totalDiff > 0 ? "+" : "") + fmtRON(totalDiff)
                      : "—"}
                  </td>
                  <td />
                </tr>
              </tfoot>
            </table>
          </div>

          <div style={{ marginTop: 12, fontSize: 11, color: "var(--text-2)", borderTop: "1px solid var(--line)", paddingTop: 8 }}>
            {entries.length} înregistrări · An fiscal {fiscalYear}
          </div>
        </div>
      )}

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
