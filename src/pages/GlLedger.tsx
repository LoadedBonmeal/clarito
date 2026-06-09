/**
 * GlLedger — Jurnal contabil (GL auto-posting + reconciliere cu D300).
 *
 * P7 — rf kit: PageHeader + Segmented + SectionCard + Badge + Banner + Btn.
 * Comenzi backend: generate_gl_entries (→ GlPostResult) + reconcile_gl (→ ReconcileReport).
 */

import { useState } from "react";
import { confirm, save as saveDialog } from "@tauri-apps/plugin-dialog";

import {
  PageHeader,
  Segmented,
  SectionCard,
  Badge,
  Banner,
  Btn,
  Modal,
  Field,
  Input,
} from "@/components/rf";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type {
  GlPostResult, ReconcileReport, VatSettlementResult, TrialBalance,
  JournalRegister, LedgerAccount, ProfitLoss, BilantReport,
} from "@/types";

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
  const [closing,         setClosing]         = useState(false);
  const [loadingTb,       setLoadingTb]       = useState(false);
  const [loadingJr,       setLoadingJr]       = useState(false);
  const [loadingCm,       setLoadingCm]       = useState(false);
  const [postResult,      setPostResult]      = useState<GlPostResult | null>(null);
  const [reconcileReport, setReconcileReport] = useState<ReconcileReport | null>(null);
  const [vatClose,        setVatClose]        = useState<VatSettlementResult | null>(null);
  const [showBilantExport, setShowBilantExport] = useState(false);
  const [trialBal,        setTrialBal]        = useState<TrialBalance | null>(null);
  const [journalReg,      setJournalReg]      = useState<JournalRegister | null>(null);
  const [ledger,          setLedger]          = useState<LedgerAccount[] | null>(null);
  const [pnl,             setPnl]             = useState<ProfitLoss | null>(null);
  const [loadingPnl,      setLoadingPnl]      = useState(false);
  const [closingPeriod,   setClosingPeriod]   = useState(false);
  const [bilant,          setBilant]          = useState<BilantReport | null>(null);
  const [loadingBilant,   setLoadingBilant]   = useState(false);

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

  // ── Închiderea TVA (regularizare 4426/4427 → 4423/4424) ───────────────────

  const handleCloseVat = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setClosing(true);
    setVatClose(null);
    try {
      const result = await api.gl.closeVat(activeCompanyId, dateFrom, dateTo);
      setVatClose(result);
      if (!result.posted) {
        notify.info("Nimic de regularizat — conturile 4426/4427 sunt deja zero pentru perioadă.");
      } else if (parseDec(result.dePlata) > 0) {
        notify.success(`Închidere TVA: de plată ${result.dePlata} lei (4423).`);
      } else if (parseDec(result.deRecuperat) > 0) {
        notify.success(`Închidere TVA: de recuperat ${result.deRecuperat} lei (4424).`);
      } else {
        notify.success("Închidere TVA: TVA colectată = deductibilă (sold zero).");
      }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut posta închiderea TVA."));
    } finally {
      setClosing(false);
    }
  };

  // ── Balanța de verificare ─────────────────────────────────────────────────

  const handleTrialBalance = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setLoadingTb(true);
    setTrialBal(null);
    try {
      const tb = await api.gl.trialBalance(activeCompanyId, dateFrom, dateTo);
      setTrialBal(tb);
      if (!tb.balanced) {
        notify.warn("Balanța NU este echilibrată — verificați notele contabile (rulați «Generează»).");
      } else if (tb.rows.length === 0) {
        notify.info("Nicio mișcare contabilă în perioadă.");
      } else {
        notify.success("Balanță de verificare generată — echilibrată (patru egalități).");
      }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut genera balanța de verificare."));
    } finally {
      setLoadingTb(false);
    }
  };

  // ── Cont de profit și pierdere ────────────────────────────────────────────

  const handleProfitLoss = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setLoadingPnl(true);
    setPnl(null);
    try {
      const r = await api.gl.profitAndLoss(activeCompanyId, dateFrom, dateTo);
      setPnl(r);
      notify.success(`Cont de profit și pierdere — rezultat net: ${r.netResult} lei.`);
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut genera contul de profit și pierdere."));
    } finally {
      setLoadingPnl(false);
    }
  };

  const handleClosePeriod = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    const ok = await confirm(
      "Postează închiderea conturilor de venituri și cheltuieli (clasele 6 și 7) în contul 121 " +
        `pentru perioada ${dateFrom} … ${dateTo}? Operațiunea este idempotentă (re-postarea ` +
        "înlocuiește închiderea anterioară a aceleiași perioade).",
      { title: "Închidere perioadă (6/7 → 121)", kind: "warning" },
    );
    if (!ok) return;
    setClosingPeriod(true);
    try {
      const r = await api.gl.closePeriod(activeCompanyId, dateFrom, dateTo);
      if (!r.posted) {
        notify.info("Nicio mișcare pe conturile de venituri/cheltuieli în perioadă.");
      } else {
        notify.success(`Închidere postată: rezultat ${r.result} lei (${r.entriesCount} note).`);
        // Refresh the P&L (it excludes the close, so it still shows the activity).
        await handleProfitLoss();
      }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut posta închiderea perioadei."));
    } finally {
      setClosingPeriod(false);
    }
  };

  const handleBilant = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setLoadingBilant(true);
    setBilant(null);
    try {
      const b = await api.gl.bilant(activeCompanyId, dateFrom, dateTo);
      setBilant(b);
      if (!b.balanced) {
        notify.warn("Bilanțul NU se verifică (Active ≠ Capitaluri + Datorii) — verificați balanța.");
      } else {
        notify.success(`Bilanț generat — total active ${b.totalAssets} lei (echilibrat).`);
      }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut genera bilanțul."));
    } finally {
      setLoadingBilant(false);
    }
  };

  const handleIncomeTax = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    const ok = await confirm(
      `Postează impozitul pe venit/profit (estimat) pentru ${dateFrom} … ${dateTo}? Se înregistrează ` +
        "D 698/691 = C 4418/4411. Idempotent per perioadă; rulați înainte de «Închide perioada».",
      { title: "Postare impozit", kind: "warning" },
    );
    if (!ok) return;
    try {
      const r = await api.gl.postIncomeTax(activeCompanyId, dateFrom, dateTo);
      if (!r.posted) notify.info("Impozit zero — nimic de postat.");
      else notify.success(`Impozit postat: ${r.amount} lei (${r.expenseAccount} → ${r.payableAccount})${r.estimated ? " — estimat" : ""}.`);
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut posta impozitul."));
    }
  };

  const handleAnnualClose = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    const year = Number(dateFrom.slice(0, 4));
    const ok = await confirm(
      `Postează închiderea anuală 121 → 117 pentru anul ${year}? Soldul contului 121 (rezultatul ` +
        `anului) se transferă în 117 «Rezultatul reportat», cu nota datată 01.01.${year + 1}. Idempotent.`,
      { title: "Închidere anuală 121 → 117", kind: "warning" },
    );
    if (!ok) return;
    try {
      const r = await api.gl.postAnnualClose(activeCompanyId, year);
      if (!r.posted) notify.info("Sold 121 zero — nimic de transferat.");
      else notify.success(`Închidere anuală ${year}: ${r.kind === "profit" ? "profit" : "pierdere"} ${r.result121} lei → 117.`);
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut posta închiderea anuală."));
    }
  };

  const runBilantExport = async (caen: string, avgEmployees: number | null, formOverride: string | null) => {
    if (!activeCompanyId) return;
    const year = Number(dateFrom.slice(0, 4));
    const dest = await saveDialog({
      title: "Salvează bilanț XML (ANAF S1005/S1003/S1002)",
      defaultPath: `bilant-${year}.xml`,
      filters: [{ name: "XML", extensions: ["xml"] }],
    });
    if (!dest) return;
    try {
      await api.gl.exportBilantXml(activeCompanyId, year, caen, avgEmployees, formOverride, dest);
      notify.success(`Bilanț XML exportat (forma după criteriile de mărime) — F10 + F20. ` +
        `Importați-l în PDF-ul inteligent ANAF, verificați header-ul și completați F30.`);
      setShowBilantExport(false);
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta bilanțul XML."));
    }
  };

  // ── Registru-jurnal + Cartea mare ─────────────────────────────────────────

  const handleJournalRegister = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setLoadingJr(true);
    setJournalReg(null);
    try {
      const jr = await api.gl.journalRegister(activeCompanyId, dateFrom, dateTo);
      setJournalReg(jr);
      if (jr.rows.length === 0) notify.info("Nicio notă contabilă în perioadă.");
      else notify.success(`Registru-jurnal: ${jr.rows.length} rânduri${jr.balanced ? " (echilibrat)" : " — DEZECHILIBRAT"}.`);
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut genera registrul-jurnal."));
    } finally {
      setLoadingJr(false);
    }
  };

  const handleGeneralLedger = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setLoadingCm(true);
    setLedger(null);
    try {
      const cm = await api.gl.generalLedger(activeCompanyId, dateFrom, dateTo);
      setLedger(cm);
      notify.success(`Cartea mare: ${cm.length} conturi.`);
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut genera cartea mare."));
    } finally {
      setLoadingCm(false);
    }
  };

  // ── Reset on period change ────────────────────────────────────────────────

  const handlePeriodChange = () => {
    setPostResult(null);
    setReconcileReport(null);
    setVatClose(null);
    setTrialBal(null);
    setJournalReg(null);
    setLedger(null);
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
            <Btn
              variant="secondary"
              icon="ledger"
              disabled={closing || !activeCompanyId}
              onClick={() => void handleCloseVat()}
            >
              {closing ? "Închid TVA…" : "Închidere TVA"}
            </Btn>
            <Btn
              variant="secondary"
              icon="reports"
              disabled={loadingTb || !activeCompanyId}
              onClick={() => void handleTrialBalance()}
            >
              {loadingTb ? "Calculez…" : "Balanță de verificare"}
            </Btn>
            <Btn
              variant="secondary"
              icon="ledger"
              disabled={loadingJr || !activeCompanyId}
              onClick={() => void handleJournalRegister()}
            >
              {loadingJr ? "…" : "Registru-jurnal"}
            </Btn>
            <Btn
              variant="secondary"
              icon="ledger"
              disabled={loadingCm || !activeCompanyId}
              onClick={() => void handleGeneralLedger()}
            >
              {loadingCm ? "…" : "Cartea mare"}
            </Btn>
            <Btn
              variant="secondary"
              icon="reports"
              disabled={loadingPnl || !activeCompanyId}
              onClick={() => void handleProfitLoss()}
            >
              {loadingPnl ? "…" : "Profit și pierdere"}
            </Btn>
            <Btn
              variant="secondary"
              icon="reports"
              disabled={loadingBilant || !activeCompanyId}
              onClick={() => void handleBilant()}
            >
              {loadingBilant ? "…" : "Bilanț"}
            </Btn>
            <Btn
              variant="secondary"
              icon="declaration"
              disabled={!activeCompanyId}
              onClick={() => setShowBilantExport(true)}
              title="Exportă bilanțul XML oficial ANAF (S1005/S1003/S1002) pentru import în PDF inteligent"
            >
              Bilanț XML
            </Btn>
            <Btn
              variant="secondary"
              icon="ledger"
              disabled={!activeCompanyId}
              onClick={() => void handleIncomeTax()}
              title="Postează impozitul pe venit/profit (698/691 → 4418/4411)"
            >
              Impozit
            </Btn>
            <Btn
              variant="secondary"
              icon="ledger"
              disabled={closingPeriod || !activeCompanyId}
              onClick={() => void handleClosePeriod()}
              title="Postează închiderea conturilor 6/7 → 121 pentru perioadă"
            >
              {closingPeriod ? "Închid…" : "Închide perioada"}
            </Btn>
            <Btn
              variant="secondary"
              icon="ledger"
              disabled={!activeCompanyId}
              onClick={() => void handleAnnualClose()}
              title="Închiderea anuală 121 → 117 (rezultat reportat)"
            >
              Închidere anuală
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

        {/* ── Rezultat închidere TVA ───────────────────────────────────────── */}
        {vatClose && (
          <SectionCard icon="ledger" title="Închidere TVA (regularizare)">
            <div style={{ padding: "12px 16px", display: "flex", gap: 24, flexWrap: "wrap", alignItems: "flex-end" }}>
              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                <span style={{ fontSize: 11.5, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>TVA colectată (4427)</span>
                <span className="rf-mono" style={{ fontSize: 18, fontWeight: 600 }}>{fmtRON(vatClose.collected)}</span>
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                <span style={{ fontSize: 11.5, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>TVA deductibilă (4426)</span>
                <span className="rf-mono" style={{ fontSize: 18, fontWeight: 600 }}>{fmtRON(vatClose.deductible)}</span>
              </div>
              {parseDec(vatClose.dePlata) > 0 && (
                <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                  <span style={{ fontSize: 11.5, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>De plată (4423)</span>
                  <span className="rf-mono" style={{ fontSize: 22, fontWeight: 700, color: "var(--rf-warning)" }}>{fmtRON(vatClose.dePlata)}</span>
                </div>
              )}
              {parseDec(vatClose.deRecuperat) > 0 && (
                <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                  <span style={{ fontSize: 11.5, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>De recuperat (4424)</span>
                  <span className="rf-mono" style={{ fontSize: 22, fontWeight: 700, color: "var(--rf-success)" }}>{fmtRON(vatClose.deRecuperat)}</span>
                </div>
              )}
            </div>
            <div style={{ padding: "0 16px 12px" }}>
              {vatClose.posted ? (
                <Banner variant="info">
                  Nota de regularizare a fost postată la <b>{vatClose.entryDate}</b>: conturile
                  4426/4427 au fost aduse la sold zero, diferența în {parseDec(vatClose.dePlata) > 0 ? "4423 «TVA de plată»" : parseDec(vatClose.deRecuperat) > 0 ? "4424 «TVA de recuperat»" : "—"}.
                  Contul 4428 «TVA neexigibilă» nu este afectat. TVA de plată se achită până la
                  data de 25 a lunii următoare.
                </Banner>
              ) : (
                <Banner variant="info">
                  Nimic de regularizat — conturile 4426/4427 sunt deja zero pentru perioadă
                  (eventuala TVA neexigibilă rămâne în 4428 până la încasare/plată).
                </Banner>
              )}
            </div>
          </SectionCard>
        )}

        {/* ── Balanță de verificare ────────────────────────────────────────── */}
        {trialBal && (
          <SectionCard icon="reports" title="Balanță de verificare (cod 14-6-30)">
            <div style={{ padding: "12px 16px 4px", display: "flex", alignItems: "center", gap: 12 }}>
              <Badge variant={trialBal.balanced ? "success" : "error"}>
                {trialBal.balanced ? "Echilibrată — patru egalități" : "NEechilibrată"}
              </Badge>
              <span style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>
                {trialBal.rows.length} conturi · obligatorie lunar (Legea 82/1991)
              </span>
            </div>
            <div style={{ overflowX: "auto", padding: "0 16px 16px" }}>
              <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
                <thead>
                  <tr style={{ borderBottom: "2px solid var(--rf-border)", textAlign: "right" }}>
                    <th style={{ textAlign: "left", padding: "4px 8px" }}>Cont</th>
                    <th style={{ textAlign: "left", padding: "4px 8px" }}>Denumire</th>
                    <th colSpan={2} style={{ padding: "4px 8px" }}>Solduri inițiale</th>
                    <th colSpan={2} style={{ padding: "4px 8px" }}>Rulaje perioadă</th>
                    <th colSpan={2} style={{ padding: "4px 8px" }}>Total sume</th>
                    <th colSpan={2} style={{ padding: "4px 8px" }}>Solduri finale</th>
                  </tr>
                  <tr style={{ borderBottom: "1px solid var(--rf-border)", textAlign: "right", color: "var(--rf-text-muted)" }}>
                    <th colSpan={2}></th>
                    <th style={{ padding: "2px 8px" }}>D</th><th style={{ padding: "2px 8px" }}>C</th>
                    <th style={{ padding: "2px 8px" }}>D</th><th style={{ padding: "2px 8px" }}>C</th>
                    <th style={{ padding: "2px 8px" }}>D</th><th style={{ padding: "2px 8px" }}>C</th>
                    <th style={{ padding: "2px 8px" }}>D</th><th style={{ padding: "2px 8px" }}>C</th>
                  </tr>
                </thead>
                <tbody>
                  {trialBal.rows.map((r) => (
                    <tr key={r.accountCode} style={{ borderBottom: "1px solid var(--rf-border-subtle, var(--rf-border))" }}>
                      <td className="rf-mono" style={{ padding: "2px 8px" }}>{r.accountCode}</td>
                      <td style={{ padding: "2px 8px" }}>{r.accountName}</td>
                      {[r.openingDebit, r.openingCredit, r.periodDebit, r.periodCredit, r.totalDebit, r.totalCredit, r.closingDebit, r.closingCredit].map((v, i) => (
                        <td key={i} className="rf-mono" style={{ padding: "2px 8px", textAlign: "right", color: parseDec(v) === 0 ? "var(--rf-text-muted)" : undefined }}>
                          {parseDec(v) === 0 ? "—" : fmtRON(v)}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
                <tfoot>
                  <tr style={{ borderTop: "2px solid var(--rf-border)", fontWeight: 700 }}>
                    <td colSpan={2} style={{ padding: "4px 8px" }}>TOTAL</td>
                    {[trialBal.totalOpeningDebit, trialBal.totalOpeningCredit, trialBal.totalPeriodDebit, trialBal.totalPeriodCredit, trialBal.totalTotalDebit, trialBal.totalTotalCredit, trialBal.totalClosingDebit, trialBal.totalClosingCredit].map((v, i) => (
                      <td key={i} className="rf-mono" style={{ padding: "4px 8px", textAlign: "right" }}>{fmtRON(v)}</td>
                    ))}
                  </tr>
                </tfoot>
              </table>
            </div>
          </SectionCard>
        )}

        {/* ── Cont de profit și pierdere ───────────────────────────────────── */}
        {pnl && (
          <SectionCard icon="reports" title="Cont de profit și pierdere (închidere 6/7 → 121)">
            <div style={{ padding: "12px 16px 4px", display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
              <Badge variant={parseDec(pnl.netResult) >= 0 ? "success" : "error"}>
                Rezultat net: {fmtRON(pnl.netResult)} lei
              </Badge>
              <span style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>
                Regim: {pnl.taxRegime === "micro" ? "microîntreprindere (1%)" : "impozit pe profit (16%)"}
                {" · "}OMFP 1802/2014
              </span>
            </div>
            <div style={{ overflowX: "auto", padding: "0 16px 16px" }}>
              <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
                <tbody>
                  <tr style={{ fontWeight: 700, borderBottom: "1px solid var(--rf-border)" }}>
                    <td style={{ padding: "6px 8px" }}>Venituri (clasa 7)</td>
                    <td className="rf-mono" style={{ padding: "6px 8px", textAlign: "right" }}>{fmtRON(pnl.totalRevenue)}</td>
                  </tr>
                  {pnl.revenueLines.map((l) => (
                    <tr key={l.code}>
                      <td style={{ padding: "2px 8px 2px 24px", color: "var(--rf-text-muted)" }}>
                        <span className="rf-mono">{l.code}</span> {l.name}
                      </td>
                      <td className="rf-mono" style={{ padding: "2px 8px", textAlign: "right" }}>{fmtRON(l.amount)}</td>
                    </tr>
                  ))}
                  <tr style={{ fontWeight: 700, borderBottom: "1px solid var(--rf-border)", borderTop: "1px solid var(--rf-border)" }}>
                    <td style={{ padding: "6px 8px" }}>Cheltuieli (clasa 6, fără impozit pe venit/profit)</td>
                    <td className="rf-mono" style={{ padding: "6px 8px", textAlign: "right" }}>{fmtRON(pnl.totalExpense)}</td>
                  </tr>
                  {pnl.expenseLines.map((l) => (
                    <tr key={l.code}>
                      <td style={{ padding: "2px 8px 2px 24px", color: "var(--rf-text-muted)" }}>
                        <span className="rf-mono">{l.code}</span> {l.name}
                      </td>
                      <td className="rf-mono" style={{ padding: "2px 8px", textAlign: "right" }}>{fmtRON(l.amount)}</td>
                    </tr>
                  ))}
                </tbody>
                <tfoot>
                  <tr style={{ borderTop: "2px solid var(--rf-border)", fontWeight: 600 }}>
                    <td style={{ padding: "4px 8px" }}>Rezultat brut (venituri − cheltuieli)</td>
                    <td className="rf-mono" style={{ padding: "4px 8px", textAlign: "right" }}>{fmtRON(pnl.grossResult)}</td>
                  </tr>
                  <tr>
                    <td style={{ padding: "4px 8px" }}>
                      Impozit pe {pnl.taxRegime === "micro" ? "venit" : "profit"}
                      {pnl.incomeTaxEstimated ? " (estimat)" : " (înregistrat)"}
                    </td>
                    <td className="rf-mono" style={{ padding: "4px 8px", textAlign: "right" }}>{fmtRON(pnl.incomeTax)}</td>
                  </tr>
                  <tr style={{ fontWeight: 700, borderTop: "1px solid var(--rf-border)" }}>
                    <td style={{ padding: "4px 8px" }}>Rezultat net</td>
                    <td className="rf-mono" style={{ padding: "4px 8px", textAlign: "right" }}>{fmtRON(pnl.netResult)}</td>
                  </tr>
                </tfoot>
              </table>
              {pnl.incomeTaxEstimated && (
                <div style={{ fontSize: 11.5, color: "var(--rf-text-muted)", marginTop: 8 }}>
                  Impozitul este estimat ({pnl.taxRegime === "micro" ? "1% × venituri" : "16% × rezultat brut pozitiv"});
                  pentru profit, ajustările fiscale (cheltuieli nedeductibile, venituri neimpozabile) nu sunt incluse.
                  Notele de închidere (D 7xx / C 121, D 121 / C 6xx) sunt pregătite ({pnl.closingEntries.length} rânduri).
                </div>
              )}
            </div>
          </SectionCard>
        )}

        {/* ── Bilanț contabil ──────────────────────────────────────────────── */}
        {bilant && (
          <SectionCard icon="reports" title="Bilanț contabil (sinteză)">
            <div style={{ padding: "12px 16px 4px", display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
              <Badge variant={bilant.balanced ? "success" : "error"}>
                {bilant.balanced ? "Verificat — Active = Capitaluri + Datorii" : "NEverificat"}
              </Badge>
              <span style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>{bilant.entitySizeNote}</span>
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16, padding: "8px 16px 16px" }}>
              <table style={{ borderCollapse: "collapse", fontSize: 12, width: "100%" }}>
                <tbody>
                  <tr style={{ fontWeight: 700, borderBottom: "1px solid var(--rf-border)" }}>
                    <td style={{ padding: "4px 8px" }}>ACTIVE</td><td />
                  </tr>
                  {[
                    ["Active imobilizate (net)", bilant.immobilizedAssets],
                    ["Stocuri", bilant.inventory],
                    ["Creanțe", bilant.receivables],
                    ["Investiții pe termen scurt", bilant.shortInvestments],
                    ["Casa și conturi la bănci", bilant.cashBank],
                    ["Cheltuieli în avans", bilant.prepaidExpenses],
                  ].map(([label, v]) => (
                    <tr key={label}>
                      <td style={{ padding: "2px 8px", color: "var(--rf-text-muted)" }}>{label}</td>
                      <td className="rf-mono" style={{ padding: "2px 8px", textAlign: "right" }}>{fmtRON(v)}</td>
                    </tr>
                  ))}
                  <tr style={{ fontWeight: 700, borderTop: "2px solid var(--rf-border)" }}>
                    <td style={{ padding: "4px 8px" }}>TOTAL ACTIVE</td>
                    <td className="rf-mono" style={{ padding: "4px 8px", textAlign: "right" }}>{fmtRON(bilant.totalAssets)}</td>
                  </tr>
                </tbody>
              </table>
              <table style={{ borderCollapse: "collapse", fontSize: 12, width: "100%" }}>
                <tbody>
                  <tr style={{ fontWeight: 700, borderBottom: "1px solid var(--rf-border)" }}>
                    <td style={{ padding: "4px 8px" }}>CAPITALURI ȘI DATORII</td><td />
                  </tr>
                  {[
                    ["Capitaluri proprii (incl. rezultat)", bilant.equity],
                    ["— din care rezultatul exercițiului", bilant.currentResult],
                    ["Provizioane", bilant.provisions],
                    ["Datorii pe termen lung", bilant.longTermDebt],
                    ["Datorii curente", bilant.currentLiabilities],
                    ["Venituri în avans", bilant.deferredRevenue],
                  ].map(([label, v]) => (
                    <tr key={label}>
                      <td style={{ padding: "2px 8px", color: "var(--rf-text-muted)" }}>{label}</td>
                      <td className="rf-mono" style={{ padding: "2px 8px", textAlign: "right" }}>{fmtRON(v)}</td>
                    </tr>
                  ))}
                  <tr style={{ fontWeight: 700, borderTop: "2px solid var(--rf-border)" }}>
                    <td style={{ padding: "4px 8px" }}>TOTAL CAPITALURI + DATORII</td>
                    <td className="rf-mono" style={{ padding: "4px 8px", textAlign: "right" }}>{fmtRON(bilant.totalEquityLiabilities)}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </SectionCard>
        )}

        {/* ── Registru-jurnal ──────────────────────────────────────────────── */}
        {journalReg && (
          <SectionCard icon="ledger" title="Registru-jurnal (cod 14-1-1)">
            <div style={{ padding: "12px 16px 4px", display: "flex", alignItems: "center", gap: 12 }}>
              <Badge variant={journalReg.balanced ? "success" : "error"}>
                {journalReg.balanced ? "Echilibrat" : "DEZECHILIBRAT"}
              </Badge>
              <span style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>{journalReg.rows.length} înregistrări</span>
            </div>
            <div style={{ overflowX: "auto", padding: "0 16px 16px" }}>
              <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
                <thead>
                  <tr style={{ borderBottom: "2px solid var(--rf-border)", textAlign: "left" }}>
                    <th style={{ padding: "4px 8px" }}>Nr.</th>
                    <th style={{ padding: "4px 8px" }}>Data</th>
                    <th style={{ padding: "4px 8px" }}>Document</th>
                    <th style={{ padding: "4px 8px" }}>Explicații</th>
                    <th style={{ padding: "4px 8px" }}>Cont D</th>
                    <th style={{ padding: "4px 8px" }}>Cont C</th>
                    <th style={{ padding: "4px 8px", textAlign: "right" }}>Sume D</th>
                    <th style={{ padding: "4px 8px", textAlign: "right" }}>Sume C</th>
                  </tr>
                </thead>
                <tbody>
                  {journalReg.rows.map((r) => (
                    <tr key={r.nrCrt} style={{ borderBottom: "1px solid var(--rf-border)" }}>
                      <td style={{ padding: "2px 8px" }}>{r.nrCrt}</td>
                      <td style={{ padding: "2px 8px" }}>{r.date}</td>
                      <td style={{ padding: "2px 8px" }}>{r.document}</td>
                      <td style={{ padding: "2px 8px" }}>{r.explanation}</td>
                      <td className="rf-mono" style={{ padding: "2px 8px" }}>{r.debitAccount || "—"}</td>
                      <td className="rf-mono" style={{ padding: "2px 8px" }}>{r.creditAccount || "—"}</td>
                      <td className="rf-mono" style={{ padding: "2px 8px", textAlign: "right" }}>{parseDec(r.debit) === 0 ? "—" : fmtRON(r.debit)}</td>
                      <td className="rf-mono" style={{ padding: "2px 8px", textAlign: "right" }}>{parseDec(r.credit) === 0 ? "—" : fmtRON(r.credit)}</td>
                    </tr>
                  ))}
                </tbody>
                <tfoot>
                  <tr style={{ borderTop: "2px solid var(--rf-border)", fontWeight: 700 }}>
                    <td colSpan={6} style={{ padding: "4px 8px" }}>TOTAL</td>
                    <td className="rf-mono" style={{ padding: "4px 8px", textAlign: "right" }}>{fmtRON(journalReg.totalDebit)}</td>
                    <td className="rf-mono" style={{ padding: "4px 8px", textAlign: "right" }}>{fmtRON(journalReg.totalCredit)}</td>
                  </tr>
                </tfoot>
              </table>
            </div>
          </SectionCard>
        )}

        {/* ── Cartea mare ──────────────────────────────────────────────────── */}
        {ledger && (
          <SectionCard icon="ledger" title="Cartea mare (cod 14-1-3)">
            <div style={{ padding: "8px 16px 16px", display: "flex", flexDirection: "column", gap: 16 }}>
              {ledger.length === 0 && (
                <span style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>Nicio mișcare contabilă în perioadă.</span>
              )}
              {ledger.map((a) => (
                <div key={a.accountCode} style={{ border: "1px solid var(--rf-border)", borderRadius: 6 }}>
                  <div style={{ padding: "6px 10px", background: "var(--rf-surface-2, transparent)", display: "flex", justifyContent: "space-between", fontWeight: 600, fontSize: 13 }}>
                    <span><span className="rf-mono">{a.accountCode}</span> · {a.accountName}</span>
                    <span style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>
                      sold inițial {parseDec(a.openingDebit) > 0 ? `${fmtRON(a.openingDebit)} D` : parseDec(a.openingCredit) > 0 ? `${fmtRON(a.openingCredit)} C` : "0"}
                    </span>
                  </div>
                  <div style={{ overflowX: "auto" }}>
                    <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
                      <thead>
                        <tr style={{ borderBottom: "1px solid var(--rf-border)", textAlign: "left", color: "var(--rf-text-muted)" }}>
                          <th style={{ padding: "2px 8px" }}>Data</th>
                          <th style={{ padding: "2px 8px" }}>Document</th>
                          <th style={{ padding: "2px 8px" }}>Cont coresp.</th>
                          <th style={{ padding: "2px 8px", textAlign: "right" }}>Debit</th>
                          <th style={{ padding: "2px 8px", textAlign: "right" }}>Credit</th>
                          <th style={{ padding: "2px 8px", textAlign: "right" }}>Sold</th>
                        </tr>
                      </thead>
                      <tbody>
                        {a.entries.map((e, i) => (
                          <tr key={i} style={{ borderBottom: "1px solid var(--rf-border-subtle, var(--rf-border))" }}>
                            <td style={{ padding: "2px 8px" }}>{e.date}</td>
                            <td style={{ padding: "2px 8px" }}>{e.document}</td>
                            <td className="rf-mono" style={{ padding: "2px 8px" }}>{e.contra || "—"}</td>
                            <td className="rf-mono" style={{ padding: "2px 8px", textAlign: "right" }}>{parseDec(e.debit) === 0 ? "—" : fmtRON(e.debit)}</td>
                            <td className="rf-mono" style={{ padding: "2px 8px", textAlign: "right" }}>{parseDec(e.credit) === 0 ? "—" : fmtRON(e.credit)}</td>
                            <td className="rf-mono" style={{ padding: "2px 8px", textAlign: "right" }}>{fmtRON(e.balance)} {e.balanceSide}</td>
                          </tr>
                        ))}
                      </tbody>
                      <tfoot>
                        <tr style={{ borderTop: "1px solid var(--rf-border)", fontWeight: 600 }}>
                          <td colSpan={3} style={{ padding: "3px 8px" }}>Rulaj / sold final</td>
                          <td className="rf-mono" style={{ padding: "3px 8px", textAlign: "right" }}>{fmtRON(a.totalDebit)}</td>
                          <td className="rf-mono" style={{ padding: "3px 8px", textAlign: "right" }}>{fmtRON(a.totalCredit)}</td>
                          <td className="rf-mono" style={{ padding: "3px 8px", textAlign: "right" }}>
                            {parseDec(a.closingDebit) > 0 ? `${fmtRON(a.closingDebit)} D` : parseDec(a.closingCredit) > 0 ? `${fmtRON(a.closingCredit)} C` : "0"}
                          </td>
                        </tr>
                      </tfoot>
                    </table>
                  </div>
                </div>
              ))}
            </div>
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

      {showBilantExport && (
        <BilantExportModal
          onClose={() => setShowBilantExport(false)}
          onExport={runBilantExport}
        />
      )}
    </div>
  );
}

/** Bilanț XML export dialog — CAEN + nr. mediu salariați + alegerea formei (auto / UU / BS / BL),
 *  înlocuiește window.prompt (care nu funcționează în WebView-ul Tauri). */
function BilantExportModal({
  onClose,
  onExport,
}: {
  onClose: () => void;
  onExport: (caen: string, avgEmployees: number | null, formOverride: string | null) => Promise<void>;
}) {
  const [caen, setCaen] = useState("");
  const [emp, setEmp] = useState("");
  const [form, setForm] = useState("auto");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (!/^\d{4}$/.test(caen.trim())) { notify.error("Cod CAEN invalid — 4 cifre (ex. 6201)."); return; }
    setBusy(true);
    try {
      await onExport(
        caen.trim(),
        emp.trim() === "" ? null : Number(emp),
        form === "auto" ? null : form,
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal open onOpenChange={(o) => { if (!o) onClose(); }} title="Export bilanț XML (ANAF)" width={460}
      footer={
        <>
          <Btn variant="secondary" onClick={onClose} disabled={busy}>Anulează</Btn>
          <Btn variant="primary" icon="declaration" disabled={busy} onClick={() => void submit()}>
            {busy ? "Se exportă…" : "Exportă"}
          </Btn>
        </>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        <Banner variant="info">
          Forma (microîntreprindere S1005 / mică S1003 / mare S1002) se alege după criteriile OMFP
          (2 din 3: active, cifra de afaceri, nr. salariați). Puteți forța forma dacă o cunoașteți.
        </Banner>
        <Field label="Cod CAEN (4 cifre)" required>
          <Input className="mono" placeholder="6201" value={caen} onChange={(e) => setCaen(e.target.value)} autoFocus />
        </Field>
        <Field label="Nr. mediu de salariați (criteriu de mărime)">
          <Input inputMode="numeric" placeholder="ex. 8" value={emp} onChange={(e) => setEmp(e.target.value)} />
        </Field>
        <Field label="Forma">
          <select className="rf-input" value={form} onChange={(e) => setForm(e.target.value)}>
            <option value="auto">Automat (după criterii)</option>
            <option value="UU">Microîntreprindere (S1005)</option>
            <option value="BS">Entitate mică (S1003)</option>
            <option value="BL">Entitate mare/mijlocie (S1002)</option>
          </select>
        </Field>
      </div>
    </Modal>
  );
}
