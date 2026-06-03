/**
 * GlLedger — Jurnal contabil (GL auto-posting + reconciliere cu D300).
 *
 * P7 — rf kit: PageHeader + Segmented + SectionCard + Badge + Banner + Btn.
 * Comenzi backend: generate_gl_entries (→ GlPostResult) + reconcile_gl (→ ReconcileReport).
 */

import { useState } from "react";

import {
  PageHeader,
  Segmented,
  SectionCard,
  Badge,
  Banner,
  Btn,
} from "@/components/rf";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { GlPostResult, ReconcileReport } from "@/types";

// ─── Helpers ──────────────────────────────────────────────────────────────────

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

// ─── Component ───────────────────────────────────────────────────────────────

export function GlLedgerPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const now = new Date();
  const [selectedYear,  setSelectedYear]  = useState(now.getFullYear());
  const [selectedMonth, setSelectedMonth] = useState(now.getMonth() + 1);

  const [generating,      setGenerating]      = useState(false);
  const [reconciling,     setReconciling]     = useState(false);
  const [postResult,      setPostResult]      = useState<GlPostResult | null>(null);
  const [reconcileReport, setReconcileReport] = useState<ReconcileReport | null>(null);

  const yearOptions    = buildYearOptions();
  const { dateFrom, dateTo } = periodDateRange(selectedYear, selectedMonth);

  const monthSegOptions = MONTHS.map((label, idx) => ({
    value: String(idx + 1),
    label: label.slice(0, 3),
  }));
  const yearSegOptions = yearOptions.map((y) => ({ value: String(y), label: String(y) }));

  // ── Generează note contabile ──────────────────────────────────────────────

  const handleGenerate = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setGenerating(true);
    setPostResult(null);
    try {
      const result = await api.gl.generateEntries(activeCompanyId, dateFrom, dateTo);
      setPostResult(result);
      notify.success(
        `GL generat: ${result.journalsInserted} jurnale, ${result.entriesInserted} intrări` +
        (result.journalsReplaced > 0 ? ` (${result.journalsReplaced} re-generate)` : "") +
        (result.skippedReceived  > 0 ? ` · ${result.skippedReceived} facturi primite omise (fără defalcare TVA)` : ""),
      );
    } catch (err) {
      notify.error(formatError(err, "Nu s-au putut genera notele contabile."));
    } finally {
      setGenerating(false);
    }
  };

  // ── Reconciliază GL cu D300 ────────────────────────────────────────────────

  const handleReconcile = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setReconciling(true);
    setReconcileReport(null);
    try {
      const report = await api.gl.reconcile(activeCompanyId, dateFrom, dateTo);
      setReconcileReport(report);
      if (report.balanced && report.discrepancies.length === 0) {
        notify.success("GL reconciliat cu succes — balansat și fără discrepanțe.");
      } else if (report.discrepancies.length > 0) {
        notify.warn(`Reconciliere completă cu ${report.discrepancies.length} discrepanțe.`);
      } else {
        notify.info("Reconciliere completă — verificați raportul de mai jos.");
      }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut reconcilia GL."));
    } finally {
      setReconciling(false);
    }
  };

  // ── Reset on period change ────────────────────────────────────────────────

  const handlePeriodChange = () => {
    setPostResult(null);
    setReconcileReport(null);
  };

  return (
    <div className="rf-content">
      <PageHeader
        title="Jurnal contabil (GL)"
        actions={
          <>
            <Segmented
              options={monthSegOptions}
              value={String(selectedMonth)}
              onChange={(v) => { setSelectedMonth(Number(v)); handlePeriodChange(); }}
            />
            <Segmented
              options={yearSegOptions}
              value={String(selectedYear)}
              onChange={(v) => { setSelectedYear(Number(v)); handlePeriodChange(); }}
            />
            <Btn
              variant="secondary"
              icon="ledger"
              disabled={generating || !activeCompanyId}
              onClick={() => void handleGenerate()}
            >
              {generating ? "Generez…" : "Generează note contabile"}
            </Btn>
            <Btn
              variant="primary"
              icon="reports"
              disabled={reconciling || !activeCompanyId}
              onClick={() => void handleReconcile()}
            >
              {reconciling ? "Reconciliez…" : "Reconciliază cu D300"}
            </Btn>
          </>
        }
      />

      <div className="rf-page-body">
        {/* Info banner */}
        <Banner variant="info">
          Notele contabile GL sunt generate automat din facturile emise (VALIDATED/STORNED),
          facturile primite cu defalcare TVA și plățile înregistrate. Rularea este idempotentă —
          documentele existente sunt re-calculate fără duplicate.
        </Banner>

        {/* ── Rezultat generare ─────────────────────────────────────────────── */}
        {postResult && (
          <SectionCard icon="ledger" title="Rezultat generare note contabile">
            <div style={{ padding: "12px 16px", display: "flex", gap: 24, flexWrap: "wrap" }}>
              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                <span style={{ fontSize: 11.5, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>Jurnale inserate</span>
                <span className="rf-mono" style={{ fontSize: 22, fontWeight: 700 }}>{postResult.journalsInserted}</span>
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                <span style={{ fontSize: 11.5, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>Intrări GL</span>
                <span className="rf-mono" style={{ fontSize: 22, fontWeight: 700 }}>{postResult.entriesInserted}</span>
              </div>
              {postResult.journalsReplaced > 0 && (
                <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                  <span style={{ fontSize: 11.5, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>Re-generate</span>
                  <span className="rf-mono" style={{ fontSize: 22, fontWeight: 700, color: "var(--rf-warning)" }}>{postResult.journalsReplaced}</span>
                </div>
              )}
              {postResult.skippedReceived > 0 && (
                <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                  <span style={{ fontSize: 11.5, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>Omise (fără TVA)</span>
                  <span className="rf-mono" style={{ fontSize: 22, fontWeight: 700, color: "var(--rf-warning)" }}>{postResult.skippedReceived}</span>
                </div>
              )}
            </div>
            {postResult.skippedReceived > 0 && (
              <div style={{ padding: "0 16px 12px" }}>
                <Banner variant="warning">
                  {postResult.skippedReceived}{" "}
                  {postResult.skippedReceived === 1 ? "factură primită a fost omisă" : "facturi primite au fost omise"}{" "}
                  deoarece nu au defalcare TVA (net_amount IS NULL). Folosiți «Recalculează TVA din XML» în Jurnal cumpărări.
                </Banner>
              </div>
            )}
          </SectionCard>
        )}

        {/* ── Reconciliere GL ↔ D300 ───────────────────────────────────────── */}
        {reconcileReport && (
          <SectionCard icon="reports" title="Raport reconciliere GL ↔ D300">
            <div style={{ padding: "12px 16px 0" }}>
              {/* Balanced badge */}
              <div style={{ marginBottom: 16, display: "flex", alignItems: "center", gap: 12 }}>
                <Badge variant={reconcileReport.balanced ? "success" : "error"}>
                  {reconcileReport.balanced ? "Balansat ✓" : "Dezechilibrat ✗"}
                </Badge>
                {reconcileReport.discrepancies.length === 0 ? (
                  <Badge variant="success">Nicio discrepanță</Badge>
                ) : (
                  <Badge variant="warning">
                    {reconcileReport.discrepancies.length}{" "}
                    {reconcileReport.discrepancies.length === 1 ? "discrepanță" : "discrepanțe"}
                  </Badge>
                )}
              </div>

              {/* Debit / Credit totals */}
              <div className="rf-grid-2" style={{ gap: 16, marginBottom: 16 }}>
                <div style={{ background: "var(--rf-surface-2)", borderRadius: "var(--rf-radius)", padding: "12px 16px" }}>
                  <div style={{ fontSize: 11.5, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: 4 }}>Σ Debit total</div>
                  <div className="rf-mono" style={{ fontSize: 18, fontWeight: 700 }}>{fmtRON(parseDec(reconcileReport.totalDebit))} RON</div>
                </div>
                <div style={{ background: "var(--rf-surface-2)", borderRadius: "var(--rf-radius)", padding: "12px 16px" }}>
                  <div style={{ fontSize: 11.5, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: 4 }}>Σ Credit total</div>
                  <div className="rf-mono" style={{ fontSize: 18, fontWeight: 700 }}>{fmtRON(parseDec(reconcileReport.totalCredit))} RON</div>
                </div>
              </div>

              {/* TVA 4427 / 4426 GL vs D300 */}
              <div className="rf-grid-2" style={{ gap: 16, marginBottom: 16 }}>
                <div style={{ background: "var(--rf-surface-2)", borderRadius: "var(--rf-radius)", padding: "12px 16px" }}>
                  <div style={{ fontSize: 11.5, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: 8 }}>TVA colectată (4427)</div>
                  <div style={{ display: "flex", justifyContent: "space-between", fontSize: 12.5 }}>
                    <span style={{ color: "var(--rf-text-muted)" }}>GL (credit 4427):</span>
                    <span className="rf-mono" style={{ fontWeight: 600 }}>{fmtRON(parseDec(reconcileReport.vatCollectedGl))} RON</span>
                  </div>
                  <div style={{ display: "flex", justifyContent: "space-between", fontSize: 12.5, marginTop: 4 }}>
                    <span style={{ color: "var(--rf-text-muted)" }}>D300:</span>
                    <span className="rf-mono" style={{ fontWeight: 600 }}>{fmtRON(parseDec(reconcileReport.vatCollectedD300))} RON</span>
                  </div>
                </div>
                <div style={{ background: "var(--rf-surface-2)", borderRadius: "var(--rf-radius)", padding: "12px 16px" }}>
                  <div style={{ fontSize: 11.5, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: 8 }}>TVA deductibilă (4426)</div>
                  <div style={{ display: "flex", justifyContent: "space-between", fontSize: 12.5 }}>
                    <span style={{ color: "var(--rf-text-muted)" }}>GL (debit 4426):</span>
                    <span className="rf-mono" style={{ fontWeight: 600 }}>{fmtRON(parseDec(reconcileReport.vatDeductibleGl))} RON</span>
                  </div>
                  <div style={{ display: "flex", justifyContent: "space-between", fontSize: 12.5, marginTop: 4 }}>
                    <span style={{ color: "var(--rf-text-muted)" }}>D300:</span>
                    <span className="rf-mono" style={{ fontWeight: 600 }}>{fmtRON(parseDec(reconcileReport.vatDeductibleD300))} RON</span>
                  </div>
                </div>
              </div>
            </div>

            {/* Discrepancies */}
            {reconcileReport.discrepancies.length > 0 && (
              <div style={{ padding: "0 16px 16px" }}>
                <Banner variant="error">
                  <div style={{ fontWeight: 600, marginBottom: 8 }}>
                    Discrepanțe ({reconcileReport.discrepancies.length}):
                  </div>
                  <ul style={{ margin: 0, paddingLeft: 20, display: "flex", flexDirection: "column", gap: 4 }}>
                    {reconcileReport.discrepancies.map((d, i) => (
                      <li key={i} style={{ fontSize: 12.5 }}>{d}</li>
                    ))}
                  </ul>
                </Banner>
              </div>
            )}
          </SectionCard>
        )}

        {/* Empty state */}
        {!postResult && !reconcileReport && (
          <SectionCard icon="ledger" title="Jurnal contabil">
            <div style={{ padding: "24px 16px", textAlign: "center", color: "var(--rf-text-muted)", fontSize: 13 }}>
              Selectați perioada și apăsați «Generează note contabile» pentru a genera GL-ul,
              apoi «Reconciliază cu D300» pentru a verifica corectitudinea.
            </div>
          </SectionCard>
        )}
      </div>
    </div>
  );
}
