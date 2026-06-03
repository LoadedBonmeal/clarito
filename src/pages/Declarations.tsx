/**
 * Declarații ANAF — D300 Decont TVA.
 * Wave 5 — rf look: PageHeader + Segmented + Banner + SectionCard + rf-tbl
 *
 * Preserve: api.declarations.compute, api.declarations.export, manualDeductible override,
 * TVA colectată/deductibilă tables, TVA de plată/recuperat summary card, year/month selectors,
 * company guard (all real wiring unchanged).
 */

import { useState, useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import {
  PageHeader,
  Segmented,
  SectionCard,
  Card,
  Btn,
  Banner,
  Field,
  Input,
} from "@/components/rf";
import { Icon } from "@/components/shared/Icon";
import { D300SubmissionModal } from "@/components/modals/D300SubmissionModal";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { queryKeys } from "@/lib/queries";
import type { D300Report, D300Submission } from "@/types";

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
  const mm      = String(month).padStart(2, "0");
  const lastDay = new Date(year, month, 0).getDate();
  return {
    dateFrom: `${year}-${mm}-01`,
    dateTo:   `${year}-${mm}-${String(lastDay).padStart(2, "0")}`,
  };
}

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

// ─── Component ───────────────────────────────────────────────────────────────

export function DeclarationsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const now = new Date();
  const [selectedYear, setSelectedYear]   = useState(now.getFullYear());
  const [selectedMonth, setSelectedMonth] = useState(now.getMonth() + 1);

  const [report,    setReport]    = useState<D300Report | null>(null);
  const [computing, setComputing] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [exportingOfficial, setExportingOfficial] = useState(false);
  const [showD300Modal,     setShowD300Modal]     = useState(false);

  // TVA deductibilă — pre-completată din totalDeductibleVat; editabilă manual ca override.
  const [manualDeductible, setManualDeductible] = useState<string>("0.00");

  // Fetch active company for pre-filling submission modal (bank/IBAN).
  const { data: activeCompany } = useQuery({
    queryKey: queryKeys.companies.detail(activeCompanyId ?? ""),
    queryFn: () => api.companies.get(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  useEffect(() => {
    if (report) {
      setManualDeductible(report.totalDeductibleVat);
    }
  }, [report]);

  const yearOptions = buildYearOptions();
  const { dateFrom, dateTo } = periodDateRange(selectedYear, selectedMonth);

  const monthSegOptions = MONTHS.map((label, idx) => ({
    value: String(idx + 1),
    label: label.slice(0, 3),
  }));
  const yearSegOptions = yearOptions.map((y) => ({ value: String(y), label: String(y) }));

  // ── Calculează D300 ────────────────────────────────────────────────────────
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

  // ── Exportă D300 XML ───────────────────────────────────────────────────────
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
      title:       "Salvează D300 XML",
      defaultPath: `d300-${dateFrom}-${dateTo}.xml`,
      filters:     [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExporting(true);
    try {
      const saved = await api.declarations.export(
        activeCompanyId,
        dateFrom,
        dateTo,
        savePath,
        manualDeductible,
      );
      notify.success(`D300 salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta D300."));
    } finally {
      setExporting(false);
    }
  };

  // ── Exportă D300 oficial ANAF (schema v12) ────────────────────────────────
  const handleExportOfficial = async (submission: D300Submission) => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    const savePath = await saveDialog({
      title:       "Salvează D300 oficial ANAF (XML)",
      defaultPath: `d300-${selectedYear}-${String(selectedMonth).padStart(2, "0")}.xml`,
      filters:     [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExportingOfficial(true);
    try {
      const saved = await api.declarations.exportD300Official(
        activeCompanyId,
        dateFrom,
        dateTo,
        savePath,
        submission,
      );
      notify.success(`D300 oficial salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta D300 oficial."));
    } finally {
      setExportingOfficial(false);
    }
  };

  const totalBase        = report ? parseDec(report.totalBase) : 0;
  const totalVat         = report ? parseDec(report.totalVat) : 0;
  const deductibleVat    = parseDec(manualDeductible) || 0;
  const netTvaDePlata    = totalVat - deductibleVat;

  return (
    <div className="rf-content">
      <PageHeader
        title="D300 — Decont TVA"
        actions={
          <>
            <Segmented
              options={monthSegOptions}
              value={String(selectedMonth)}
              onChange={(v) => { setSelectedMonth(Number(v)); setReport(null); }}
            />
            <Segmented
              options={yearSegOptions}
              value={String(selectedYear)}
              onChange={(v) => { setSelectedYear(Number(v)); setReport(null); }}
            />
            <Btn
              variant="primary"
              icon="reports"
              disabled={computing || !activeCompanyId}
              onClick={() => void handleCompute()}
            >
              {computing ? "Calculez…" : "Calculează D300"}
            </Btn>
            <Btn
              variant="secondary"
              icon="xml"
              disabled={exporting || !report || (report.invoiceCount === 0 && report.purchaseInvoiceCount === 0)}
              onClick={() => void handleExport()}
              title="Exportă extras D300 ca fișier XML (document de lucru, nu schema ANAF)"
            >
              {exporting ? "Export…" : "Extract D300 (XML)"}
            </Btn>
            <Btn
              variant="primary"
              icon="anaf"
              disabled={exportingOfficial || !report || (report.invoiceCount === 0 && report.purchaseInvoiceCount === 0) || !activeCompany}
              onClick={() => setShowD300Modal(true)}
              title="Exportă D300 conform schemei oficiale ANAF v12"
            >
              {exportingOfficial ? "Export…" : "Export oficial ANAF"}
            </Btn>
          </>
        }
      />

      <div className="rf-page-body">
        {/* Nota informativa */}
        <Banner variant="warning">
          TVA deductibilă se calculează din facturile primite procesate. Verificați că toate
          facturile lunii au fost descărcate și parsate din SPV pentru un decont corect.
        </Banner>

        {/* ── TVA colectată + deductibilă ─────────────────────────────────── */}
        <div className="rf-grid-2">
          {/* TVA colectată (vânzări) */}
          <SectionCard icon="fileOut" title="TVA colectată (vânzări)">
            {computing ? (
              <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>Se calculează…</div>
            ) : !report ? (
              <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
                Apăsați «Calculează D300» pentru a genera decontul pentru{" "}
                <b>{MONTHS[selectedMonth - 1]} {selectedYear}</b>.
              </div>
            ) : report.invoiceCount === 0 ? (
              <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
                Nu există facturi VALIDATED în perioada selectată.
              </div>
            ) : (
              <>
                <div style={{ padding: "6px 16px 10px", fontSize: 12, color: "var(--rf-text-muted)", display: "flex", gap: 16 }}>
                  <span>CUI: <b style={{ color: "var(--rf-text)" }}>{report.companyCui}</b></span>
                  <span>Facturi: <b style={{ color: "var(--rf-text)" }}>{report.invoiceCount}</b></span>
                </div>
                <div className="rf-tbl-wrap">
                  <table className="rf-tbl">
                    <thead>
                      <tr>
                        <th>Cotă</th>
                        <th>Cat.</th>
                        <th className="right">Bază</th>
                        <th className="right">TVA</th>
                      </tr>
                    </thead>
                    <tbody>
                      {report.groups.map((g, i) => (
                        <tr key={i}>
                          <td className="rf-mono" style={{ fontWeight: 600 }}>{g.vatRate}%</td>
                          <td>
                            <span
                              className="rf-mono"
                              title={vatCategoryLabel(g.vatCategory)}
                              style={{ cursor: "help", color: "var(--rf-text-muted)" }}
                            >
                              {g.vatCategory}
                            </span>
                          </td>
                          <td className="right rf-mono">{fmtRON(g.base)}</td>
                          <td className="right rf-mono">{fmtRON(g.vat)}</td>
                        </tr>
                      ))}
                    </tbody>
                    <tfoot>
                      <tr>
                        <td colSpan={2} className="right">Total colectată</td>
                        <td className="right rf-mono">{fmtRON(totalBase)}</td>
                        <td className="right rf-mono">{fmtRON(totalVat)}</td>
                      </tr>
                    </tfoot>
                  </table>
                </div>
              </>
            )}
          </SectionCard>

          {/* TVA deductibilă (achiziții) */}
          <SectionCard icon="fileIn" title="TVA deductibilă (achiziții)">
            {report && report.purchaseGroups.length > 0 ? (
              <div className="rf-tbl-wrap">
                <table className="rf-tbl">
                  <thead>
                    <tr>
                      <th>Cotă</th>
                      <th>Cat.</th>
                      <th className="right">Bază</th>
                      <th className="right">TVA</th>
                    </tr>
                  </thead>
                  <tbody>
                    {report.purchaseGroups.map((g, i) => (
                      <tr key={i}>
                        <td className="rf-mono" style={{ fontWeight: 600 }}>{g.vatRate}%</td>
                        <td>
                          <span
                            className="rf-mono"
                            title={vatCategoryLabel(g.vatCategory)}
                            style={{ cursor: "help", color: "var(--rf-text-muted)" }}
                          >
                            {g.vatCategory}
                          </span>
                        </td>
                        <td className="right rf-mono">{fmtRON(g.base)}</td>
                        <td className="right rf-mono">{fmtRON(g.vat)}</td>
                      </tr>
                    ))}
                  </tbody>
                  <tfoot>
                    <tr>
                      <td colSpan={2} className="right">Total deductibilă</td>
                      <td className="right rf-mono">{fmtRON(report.totalDeductibleBase)}</td>
                      <td className="right rf-mono">{fmtRON(report.totalDeductibleVat)}</td>
                    </tr>
                  </tfoot>
                </table>
              </div>
            ) : (
              <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
                {!report ? "Calculați D300 pentru a vedea datele." : "Nicio factură primită parsată în perioadă."}
              </div>
            )}

            {/* Unparsed note */}
            {report && report.purchaseUnparsedCount > 0 && (
              <div style={{ padding: "0 16px 12px" }}>
                <Banner variant="warning">
                  <b>{report.purchaseUnparsedCount}{" "}
                  {report.purchaseUnparsedCount === 1 ? "factură primită nu are" : "facturi primite nu au"}{" "}
                  încă defalcare TVA</b>{" "}
                  — suma calculată automat poate fi parțială. Introduceți manual valoarea corectă
                  mai jos sau folosiți «Recalculează TVA din XML» în Jurnal cumpărări.
                </Banner>
              </div>
            )}

            {/* Manual override input */}
            <div style={{ padding: "8px 16px 16px" }}>
              <Field
                label="Ajustare manuală TVA deductibilă"
                help="Pentru achiziții fără factură SPV parsată"
              >
                <Input
                  type="number"
                  id="manual-deductible"
                  min="0"
                  step="0.01"
                  num
                  value={manualDeductible}
                  onChange={(e) => setManualDeductible(e.target.value)}
                />
              </Field>
              {report && parseDec(manualDeductible) !== parseDec(report.totalDeductibleVat) && (
                <button
                  type="button"
                  className="rf-btn rf-btn--ghost rf-btn--sm"
                  style={{ marginTop: 6 }}
                  onClick={() => setManualDeductible(report.totalDeductibleVat)}
                  title="Resetează la valoarea calculată automat"
                >
                  <Icon name="refresh" size={12} /> Resetează la valoarea calculată
                </button>
              )}
            </div>
          </SectionCard>
        </div>

        {/* ── TVA de plată / recuperat summary ────────────────────────────── */}
        <Card>
          <div
            style={{
              padding: "18px 22px",
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
              background:   netTvaDePlata > 0 ? "var(--rf-warning-bg)"  : "var(--rf-success-bg)",
              borderRadius: "var(--rf-radius)",
              border: `1.5px solid ${netTvaDePlata > 0 ? "var(--rf-warning-bd)" : "var(--rf-success-bd)"}`,
            }}
          >
            <div>
              <div
                style={{
                  fontSize: 13,
                  fontWeight: 700,
                  color: netTvaDePlata > 0 ? "var(--rf-warning)" : "var(--rf-success)",
                  textTransform: "uppercase",
                  letterSpacing: "0.04em",
                }}
              >
                {netTvaDePlata >= 0 ? "TVA de plată" : "TVA de recuperat"}
              </div>
              <div style={{ fontSize: 12.5, color: "var(--rf-text-muted)", marginTop: 4 }}>
                Colectată <b className="rf-mono">{fmtRON(totalVat)}</b> RON −
                Deductibilă <b className="rf-mono">{fmtRON(deductibleVat)}</b> RON
              </div>
            </div>
            <div
              className="rf-mono"
              style={{
                fontSize: 32,
                fontWeight: 700,
                color: netTvaDePlata > 0 ? "var(--rf-warning)" : "var(--rf-success)",
              }}
            >
              {fmtRON(Math.abs(netTvaDePlata))}{" "}
              <span style={{ fontSize: 16 }}>RON</span>
            </div>
          </div>
        </Card>
      </div>

      {/* D300 Submission Modal (export oficial) */}
      {activeCompany && (
        <D300SubmissionModal
          open={showD300Modal}
          onOpenChange={setShowD300Modal}
          company={activeCompany}
          onSubmit={(sub) => void handleExportOfficial(sub)}
        />
      )}
    </div>
  );
}
