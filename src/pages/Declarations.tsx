/**
 * Declarații ANAF — D300 Decont TVA.
 *
 * Această pagină calculează și exportă decontul de TVA (D300) — **vânzări**
 * (TVA colectată) + **achiziții** (TVA deductibilă din received_invoice_vat_lines).
 *
 * TVA deductibilă este auto-completată din datele parsate (Wave B). Câmpul
 * rămâne editabil manual ca fallback pentru facturile neparsate.
 */

import { useState, useEffect } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Icon } from "@/components/shared/Icon";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { D300Report } from "@/types";

// ─── Helpers ─────────────────────────────────────────────────────────────────

const MONTHS = [
  "Ianuarie", "Februarie", "Martie", "Aprilie", "Mai", "Iunie",
  "Iulie", "August", "Septembrie", "Octombrie", "Noiembrie", "Decembrie",
];

function buildYearOptions(): number[] {
  const current = new Date().getFullYear();
  const years: number[] = [];
  for (let y = current; y >= current - 5; y--) years.push(y);
  return years;
}

function periodDateRange(year: number, month: number): { dateFrom: string; dateTo: string } {
  const mm = String(month).padStart(2, "0");
  const lastDay = new Date(year, month, 0).getDate();
  return {
    dateFrom: `${year}-${mm}-01`,
    dateTo: `${year}-${mm}-${String(lastDay).padStart(2, "0")}`,
  };
}

// ─── Component ───────────────────────────────────────────────────────────────

export function DeclarationsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const now = new Date();
  const [selectedYear, setSelectedYear] = useState(now.getFullYear());
  const [selectedMonth, setSelectedMonth] = useState(now.getMonth() + 1);

  const [report, setReport] = useState<D300Report | null>(null);
  const [computing, setComputing] = useState(false);
  const [exporting, setExporting] = useState(false);

  // TVA deductibilă — pre-completată din date parsate (Wave B), editabilă manual.
  // Când raportul se încarcă, valoarea este setată din report.totalDeductibleVat.
  // Utilizatorul poate suprascrie manual (fallback pentru facturi neparsate).
  const [manualDeductible, setManualDeductible] = useState<string>("0.00");

  // Sincronizăm câmpul manual cu valoarea calculată din raport când raportul se încarcă.
  useEffect(() => {
    if (report) {
      setManualDeductible(report.totalDeductibleVat);
    }
  }, [report]);

  const yearOptions = buildYearOptions();
  const { dateFrom, dateTo } = periodDateRange(selectedYear, selectedMonth);

  // ── Calculează D300 ──────────────────────────────────────────────────────
  const handleCompute = async () => {
    if (!activeCompanyId) {
      notify.warn("Selectați o companie activă.");
      return;
    }
    setComputing(true);
    setReport(null);
    try {
      const result = await api.declarations.compute(activeCompanyId, dateFrom, dateTo);
      if (result.invoiceCount === 0) {
        notify.info("Nu există date pentru perioada selectată.");
      }
      setReport(result);
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut calcula D300."));
    } finally {
      setComputing(false);
    }
  };

  // ── Exportă D300 XML ─────────────────────────────────────────────────────
  const handleExport = async () => {
    if (!activeCompanyId) {
      notify.warn("Selectați o companie activă.");
      return;
    }
    if (!report || (report.invoiceCount === 0 && report.purchaseInvoiceCount === 0)) {
      notify.info("Nu există date de exportat. Calculați mai întâi D300.");
      return;
    }
    const savePath = await saveDialog({
      title: "Salvează D300 XML",
      defaultPath: `d300-${dateFrom}-${dateTo}.xml`,
      filters: [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;

    setExporting(true);
    try {
      const saved = await api.declarations.export(activeCompanyId, dateFrom, dateTo, savePath);
      notify.success(`D300 salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta D300."));
    } finally {
      setExporting(false);
    }
  };

  const totalBase = report ? parseDec(report.totalBase) : 0;
  const totalVat = report ? parseDec(report.totalVat) : 0;
  const deductibleVat = parseDec(manualDeductible) || 0;
  const netTvaDePlata = totalVat - deductibleVat;

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">Raportare</span>
          Declarații ANAF
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
          <button
            type="button"
            className="btn btn-primary"
            disabled={computing || !activeCompanyId}
            onClick={() => void handleCompute()}
          >
            <Icon name="reports" size={12} />
            {computing ? "Calculez…" : "Calculează D300"}
          </button>
          <button
            type="button"
            className="btn"
            disabled={exporting || !report || (report.invoiceCount === 0 && report.purchaseInvoiceCount === 0)}
            onClick={() => void handleExport()}
            title="Exportă decontul D300 ca fișier XML"
          >
            <Icon name="download" size={12} />
            {exporting ? "Export…" : "Exportă D300 (XML)"}
          </button>
        </span>
      </div>

      {/* Notă informativă */}
      <div
        style={{
          margin: "10px 14px 0",
          padding: "8px 12px",
          background: "var(--bg-hover)",
          border: "1px solid var(--border)",
          borderRadius: 4,
          fontSize: 11,
          color: "var(--text-muted)",
          lineHeight: 1.6,
        }}
      >
        <b style={{ color: "var(--text)" }}>Decont TVA — vânzări (TVA colectată) + achiziții (TVA deductibilă).</b>{" "}
        Vânzările sunt calculate din facturile VALIDATED ale perioadei. Achizițiile sunt
        calculate automat din defalcarea TVA a facturilor primite (dacă XMLurile au fost parsate).
        Puteți suprascrie manual valoarea TVA deductibilă în câmpul de mai jos.
      </div>

      {/* Period selector */}
      <div style={{ padding: "10px 14px 0", display: "flex", gap: 8, alignItems: "center" }}>
        <span style={{ fontSize: 11, color: "var(--text-muted)", fontWeight: 500 }}>Perioadă:</span>
        <div className="field" style={{ display: "inline-flex", gap: 6 }}>
          <select
            value={selectedMonth}
            onChange={(e) => { setSelectedMonth(Number(e.target.value)); setReport(null); }}
            style={{ fontSize: 12, padding: "3px 6px" }}
          >
            {MONTHS.map((m, idx) => (
              <option key={idx + 1} value={idx + 1}>{m}</option>
            ))}
          </select>
          <select
            value={selectedYear}
            onChange={(e) => { setSelectedYear(Number(e.target.value)); setReport(null); }}
            style={{ fontSize: 12, padding: "3px 6px" }}
          >
            {yearOptions.map((y) => (
              <option key={y} value={y}>{y}</option>
            ))}
          </select>
        </div>
        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
          {dateFrom} — {dateTo}
        </span>
      </div>

      <div style={{ padding: "14px 14px 0" }}>

        {/* ── D300 — Vânzări ────────────────────────────────────────────────── */}
        <section style={{ marginBottom: 24 }}>
          <h2
            style={{
              fontSize: 12,
              fontWeight: 600,
              marginBottom: 8,
              color: "var(--text)",
              letterSpacing: "0.04em",
              textTransform: "uppercase",
            }}
          >
            D300 — TVA Colectat (Vânzări) — {MONTHS[selectedMonth - 1]} {selectedYear}
          </h2>

          {computing ? (
            <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>
              Se calculează…
            </div>
          ) : !report ? (
            <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>
              Apăsați „Calculează D300" pentru a genera decontul pentru perioada selectată.
            </div>
          ) : report.invoiceCount === 0 ? (
            <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "12px 0" }}>
              Nu există facturi VALIDATED în perioada selectată.
            </div>
          ) : (
            <>
              {/* Metadata */}
              <div
                style={{
                  display: "flex",
                  gap: 16,
                  marginBottom: 12,
                  fontSize: 11,
                  color: "var(--text-muted)",
                }}
              >
                <span>CUI: <b style={{ color: "var(--text)" }}>{report.companyCui}</b></span>
                <span>Facturi incluse: <b style={{ color: "var(--text)" }}>{report.invoiceCount}</b></span>
              </div>

              {/* Tabel grupuri TVA */}
              <table className="dt">
                <thead>
                  <tr>
                    <th style={{ width: 100 }}>Cotă TVA</th>
                    <th style={{ width: 110 }}>Categorie</th>
                    <th className="num" style={{ width: 180 }}>Bază impozabilă (RON)</th>
                    <th className="num" style={{ width: 160 }}>TVA Colectat (RON)</th>
                    <th className="num" style={{ width: 160 }}>Total (RON)</th>
                  </tr>
                </thead>
                <tbody>
                  {report.groups.map((g, i) => {
                    const rowTotal = parseDec(g.base) + parseDec(g.vat);
                    return (
                      <tr key={i}>
                        <td><span className="mono">{g.vatRate}%</span></td>
                        <td>
                          <span
                            className="mono"
                            title={vatCategoryLabel(g.vatCategory)}
                            style={{ cursor: "help" }}
                          >
                            {g.vatCategory}
                          </span>
                          <span
                            style={{
                              marginLeft: 6,
                              fontSize: 10,
                              color: "var(--text-muted)",
                            }}
                          >
                            {vatCategoryLabel(g.vatCategory)}
                          </span>
                        </td>
                        <td className="num tnum">{fmtRON(g.base)}</td>
                        <td className="num tnum muted">{fmtRON(g.vat)}</td>
                        <td className="num tnum"><b>{fmtRON(rowTotal)}</b></td>
                      </tr>
                    );
                  })}
                </tbody>
                <tfoot>
                  <tr style={{ background: "var(--bg-hover)", fontWeight: 600 }}>
                    <td colSpan={2}>TOTAL VÂNZĂRI</td>
                    <td className="num tnum">{fmtRON(totalBase)}</td>
                    <td className="num tnum">{fmtRON(totalVat)}</td>
                    <td className="num tnum"><b>{fmtRON(totalBase + totalVat)}</b></td>
                  </tr>
                </tfoot>
              </table>
            </>
          )}
        </section>

        {/* ── Achiziții — TVA deductibilă ───────────────────────────────────── */}
        <section style={{ marginBottom: 24 }}>
          <h2
            style={{
              fontSize: 12,
              fontWeight: 600,
              marginBottom: 8,
              color: "var(--text)",
              letterSpacing: "0.04em",
              textTransform: "uppercase",
            }}
          >
            D300 — TVA Deductibil (Achiziții) — {MONTHS[selectedMonth - 1]} {selectedYear}
          </h2>

          {/* Tabel grupuri TVA deductibil — afișat doar când raportul există */}
          {report && report.purchaseGroups.length > 0 && (
            <table className="dt" style={{ marginBottom: 12 }}>
              <thead>
                <tr>
                  <th style={{ width: 100 }}>Cotă TVA</th>
                  <th style={{ width: 110 }}>Categorie</th>
                  <th className="num" style={{ width: 180 }}>Bază impozabilă (RON)</th>
                  <th className="num" style={{ width: 160 }}>TVA Deductibilă (RON)</th>
                </tr>
              </thead>
              <tbody>
                {report.purchaseGroups.map((g, i) => (
                  <tr key={i}>
                    <td><span className="mono">{g.vatRate}%</span></td>
                    <td>
                      <span
                        className="mono"
                        title={vatCategoryLabel(g.vatCategory)}
                        style={{ cursor: "help" }}
                      >
                        {g.vatCategory}
                      </span>
                      <span style={{ marginLeft: 6, fontSize: 10, color: "var(--text-muted)" }}>
                        {vatCategoryLabel(g.vatCategory)}
                      </span>
                    </td>
                    <td className="num tnum">{fmtRON(g.base)}</td>
                    <td className="num tnum muted">{fmtRON(g.vat)}</td>
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr style={{ background: "var(--bg-hover)", fontWeight: 600 }}>
                  <td colSpan={2}>TOTAL ACHIZIȚII (parsate)</td>
                  <td className="num tnum">{fmtRON(report.totalDeductibleBase)}</td>
                  <td className="num tnum">{fmtRON(report.totalDeductibleVat)}</td>
                </tr>
              </tfoot>
            </table>
          )}

          {/* Notă pentru facturi neparsate — afișată onest doar când există */}
          {report && report.purchaseUnparsedCount > 0 && (
            <div
              style={{
                padding: "8px 12px",
                background: "rgba(234,179,8,0.08)",
                border: "1px solid rgba(234,179,8,0.35)",
                borderRadius: 4,
                fontSize: 11,
                color: "var(--text-muted)",
                lineHeight: 1.6,
                marginBottom: 12,
              }}
            >
              <b style={{ color: "var(--text)" }}>
                {report.purchaseUnparsedCount} {report.purchaseUnparsedCount === 1 ? "factură primită nu are" : "facturi primite nu au"} încă defalcare TVA
              </b>{" "}
              — suma calculată automat poate fi parțială. Folosiți{" "}
              <b>«Recalculează TVA din XML»</b> în Jurnal cumpărări sau introduceți
              manual valoarea corectă în câmpul de mai jos.
            </div>
          )}

          {/* Câmp manual — pre-completat din totalDeductibleVat, editabil ca override */}
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 16 }}>
            <label
              htmlFor="manual-deductible"
              style={{ fontSize: 12, fontWeight: 500, color: "var(--text)", whiteSpace: "nowrap" }}
            >
              Total TVA deductibilă (RON):
            </label>
            <input
              id="manual-deductible"
              type="number"
              min="0"
              step="0.01"
              value={manualDeductible}
              onChange={(e) => setManualDeductible(e.target.value)}
              style={{
                fontSize: 12,
                padding: "4px 8px",
                width: 140,
                fontFamily: "var(--font-mono, monospace)",
                textAlign: "right",
              }}
            />
            {report && parseDec(manualDeductible) !== parseDec(report.totalDeductibleVat) && (
              <button
                type="button"
                className="btn"
                style={{ fontSize: 11, padding: "3px 8px" }}
                onClick={() => setManualDeductible(report.totalDeductibleVat)}
                title="Resetează la valoarea calculată automat"
              >
                Resetează
              </button>
            )}
          </div>

          {/* Net TVA de plată / recuperat — folosim report.netVat ca referință, dar afișăm valoarea din câmpul manual */}
          <div
            style={{
              display: "flex",
              gap: 24,
              padding: "10px 14px",
              background: netTvaDePlata >= 0 ? "var(--bg-hover)" : "rgba(22,163,74,0.06)",
              border: `1px solid ${netTvaDePlata >= 0 ? "var(--border)" : "#16A34A44"}`,
              borderRadius: 4,
              fontSize: 12,
            }}
          >
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              <span style={{ fontSize: 10, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>TVA Colectată</span>
              <span className="tnum" style={{ fontWeight: 600 }}>{fmtRON(totalVat)} RON</span>
            </div>
            <div style={{ display: "flex", alignItems: "center", color: "var(--text-muted)", fontSize: 14 }}>−</div>
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              <span style={{ fontSize: 10, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>TVA Deductibilă</span>
              <span className="tnum" style={{ fontWeight: 600 }}>{fmtRON(deductibleVat)} RON</span>
            </div>
            <div style={{ display: "flex", alignItems: "center", color: "var(--text-muted)", fontSize: 14 }}>=</div>
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              <span style={{ fontSize: 10, textTransform: "uppercase", letterSpacing: "0.05em", color: netTvaDePlata >= 0 ? "var(--text-muted)" : "#16A34A" }}>
                {netTvaDePlata >= 0 ? "TVA de plată" : "TVA de recuperat"}
              </span>
              <span
                className="tnum"
                style={{
                  fontWeight: 700,
                  fontSize: 14,
                  color: netTvaDePlata >= 0 ? "var(--text)" : "#16A34A",
                }}
              >
                {fmtRON(Math.abs(netTvaDePlata))} RON
              </span>
            </div>
          </div>
        </section>

      </div>
    </div>
  );
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function vatCategoryLabel(cat: string): string {
  switch (cat) {
    case "S":  return "Standard";
    case "Z":  return "Zero-rated";
    case "E":  return "Scutit";
    case "AE": return "Autolichidare";
    case "K":  return "Intracomunitar";
    case "G":  return "Guvernamental";
    case "O":  return "În afara TVA";
    default:   return cat;
  }
}
