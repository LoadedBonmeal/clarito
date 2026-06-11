/**
 * Jurnal contabil (GL) — verbatim port of the design "Contabilitate.html":
 *   .page-head (title + sub coduri registre + period pill + btn-dark spin-btn
 *   "Generează notele pe …") → .tabs (Registru-jurnal · Balanță · Închideri ·
 *   Reconciliere D300 · Bilanț XML + real-feature tabs Cartea mare · Profit și
 *   pierdere) → .panel per tab: scr-card + scr-table (registru cu pager,
 *   balanță cu .eq-row patru egalități), .cols-2-even .close-card (închidere
 *   TVA / rezultat / impozit), banner + tabel reconciliere, .crit încadrare
 *   entitate + export bilanț XML cu .modal-back/.modal.
 *
 * ALL wiring preserved: api.gl.generateEntries, api.gl.reconcile,
 * api.gl.closeVat, api.gl.trialBalance, api.gl.journalRegister,
 * api.gl.generalLedger, api.gl.profitAndLoss, api.gl.closePeriod,
 * api.gl.bilant, api.gl.postIncomeTax, api.gl.postAnnualClose,
 * api.gl.exportBilantXml, confirm dialogs, toasts, error handling.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { confirm, save as saveDialog } from "@tauri-apps/plugin-dialog";

import { Ic } from "@/components/shared/Ic";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec, MONTHS_RO } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type {
  GlPostResult, ReconcileReport, VatSettlementResult, TrialBalance,
  JournalRegister, LedgerAccount, ProfitLoss, BilantReport,
} from "@/types";

// ─── Helpers ──────────────────────────────────────────────────────────────────

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};
/** Datele din GL pot veni deja formatate — formatează doar ISO-urile. */
const fmtD = (s: string) => (/^\d{4}-\d{2}-\d{2}/.test(s) ? fmtRoDate(s.slice(0, 10)) : s || "—");

function periodDateRange(year: number, month: number): { dateFrom: string; dateTo: string } {
  const mm      = String(month).padStart(2, "0");
  const lastDay = new Date(year, month, 0).getDate();
  return {
    dateFrom: `${year}-${mm}-01`,
    dateTo:   `${year}-${mm}-${String(lastDay).padStart(2, "0")}`,
  };
}

/** Icoane din prototip absente din setul Ic (inline, verbatim). */
const CIRCLE_CHECK = '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>';
const EQ_CHECK     = '<path d="M4.5 12.75 10 18l9.5-11.5"/>';
const WARN_TRI     = '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';
const CHEV_L       = '<path d="M15.75 19.5 8.25 12l7.5-7.5"/>';

function InlineIc({ path, cls = "ic", style }: { path: string; cls?: string; style?: React.CSSProperties }) {
  return <svg className={cls} viewBox="0 0 24 24" aria-hidden="true" style={style} dangerouslySetInnerHTML={{ __html: path }} />;
}

const ChipPaid = ({ label }: { label: string }) => (
  <span className="chip paid"><InlineIc path={CIRCLE_CHECK} cls="sic" />{label}</span>
);
const ChipWait = ({ label }: { label: string }) => (
  <span className="chip wait"><Ic name="clock" cls="sic" />{label}</span>
);
const ChipLate = ({ label }: { label: string }) => (
  <span className="chip late"><InlineIc path={WARN_TRI} cls="sic" />{label}</span>
);

/** O sumă pe 2 zecimale e „zero” sub jumătate de ban. */
const isZero = (a: number) => Math.abs(a) < 0.005;

const JR_PAGE_SIZE = 100;

const TABS = [
  "Registru-jurnal",
  "Balanță",
  "Închideri",
  "Reconciliere D300",
  "Bilanț XML",
  "Cartea mare",
  "Profit și pierdere",
] as const;

// ─── Component ───────────────────────────────────────────────────────────────

export function GlLedgerPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const now = new Date();
  const [selectedYear,  setSelectedYear]  = useState(now.getFullYear());
  const [selectedMonth, setSelectedMonth] = useState(now.getMonth() + 1);
  const [tab, setTab] = useState(0);
  const [openPop, setOpenPop] = useState<"" | "period">("");

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

  const [jrQuery, setJrQuery] = useState("");
  const [jrPage,  setJrPage]  = useState(1);

  const [refreshTick, setRefreshTick] = useState(0);
  const attempted = useRef<Set<string>>(new Set());

  const { dateFrom, dateTo } = periodDateRange(selectedYear, selectedMonth);
  const monthName   = MONTHS_RO[selectedMonth - 1];
  const periodLabel = `${monthName} ${selectedYear}`;

  // Perioada selectabilă: ultimele 36 de luni.
  const periodOptions = useMemo(() => {
    const base = now.getFullYear() * 12 + now.getMonth();
    const opts: Array<{ y: number; m: number }> = [];
    for (let i = 0; i < 36; i++) {
      const t = base - i;
      opts.push({ y: Math.floor(t / 12), m: (t % 12) + 1 });
    }
    return opts;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Close pop on outside click.
  useEffect(() => {
    if (!openPop) return;
    const h = () => setOpenPop("");
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [openPop]);

  // ── Reset la schimbarea perioadei / companiei ─────────────────────────────
  const resetAll = () => {
    setPostResult(null);
    setReconcileReport(null);
    setVatClose(null);
    setTrialBal(null);
    setJournalReg(null);
    setLedger(null);
    setPnl(null);
    setBilant(null);
    setJrPage(1);
  };
  const prevCtx = useRef(`${activeCompanyId}|${dateFrom}`);
  useEffect(() => {
    const ctx = `${activeCompanyId}|${dateFrom}`;
    if (prevCtx.current !== ctx) {
      prevCtx.current = ctx;
      resetAll();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeCompanyId, dateFrom]);

  /** Datele postate invalidează rapoartele — panourile active se reîncarcă. */
  const invalidateReports = () => {
    setTrialBal(null);
    setJournalReg(null);
    setLedger(null);
    setPnl(null);
    setBilant(null);
    setReconcileReport(null);
    setRefreshTick((t) => t + 1);
  };

  // ── Loaders (silențioși — rezultatul se vede în panou; erorile dau toast) ──

  const loadJournal = async () => {
    if (!activeCompanyId || loadingJr) return;
    setLoadingJr(true);
    try {
      setJournalReg(await api.gl.journalRegister(activeCompanyId, dateFrom, dateTo));
      setJrPage(1);
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut genera registrul-jurnal."));
    } finally {
      setLoadingJr(false);
    }
  };

  const loadTrialBalance = async () => {
    if (!activeCompanyId || loadingTb) return;
    setLoadingTb(true);
    try {
      setTrialBal(await api.gl.trialBalance(activeCompanyId, dateFrom, dateTo));
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut genera balanța de verificare."));
    } finally {
      setLoadingTb(false);
    }
  };

  const loadLedger = async () => {
    if (!activeCompanyId || loadingCm) return;
    setLoadingCm(true);
    try {
      setLedger(await api.gl.generalLedger(activeCompanyId, dateFrom, dateTo));
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut genera cartea mare."));
    } finally {
      setLoadingCm(false);
    }
  };

  const loadPnl = async () => {
    if (!activeCompanyId || loadingPnl) return;
    setLoadingPnl(true);
    try {
      setPnl(await api.gl.profitAndLoss(activeCompanyId, dateFrom, dateTo));
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut genera contul de profit și pierdere."));
    } finally {
      setLoadingPnl(false);
    }
  };

  const loadBilant = async () => {
    if (!activeCompanyId || loadingBilant) return;
    setLoadingBilant(true);
    try {
      setBilant(await api.gl.bilant(activeCompanyId, dateFrom, dateTo));
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut genera bilanțul."));
    } finally {
      setLoadingBilant(false);
    }
  };

  const runReconcile = async (manual: boolean) => {
    if (!activeCompanyId) {
      if (manual) notify.warn("Selectați o companie activă.");
      return;
    }
    if (reconciling) return;
    setReconciling(true);
    if (manual) setReconcileReport(null);
    try {
      const report = await api.gl.reconcile(activeCompanyId, dateFrom, dateTo);
      setReconcileReport(report);
      if (manual) {
        if (report.balanced && report.discrepancies.length === 0) {
          notify.success("GL reconciliat cu succes — balansat și fără discrepanțe.");
        } else if (report.discrepancies.length > 0) {
          notify.warn(`Reconciliere completă cu ${report.discrepancies.length} discrepanțe.`);
        } else {
          notify.info("Reconciliere completă — verificați raportul de mai jos.");
        }
      }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut reconcilia GL."));
    } finally {
      setReconciling(false);
    }
  };

  // Auto-load per tab activ (o singură tentativă per perioadă/companie/tick).
  useEffect(() => {
    if (!activeCompanyId) return;
    const loader = tab === 0 ? "jr" : tab === 1 ? "tb" : tab === 2 || tab === 6 ? "pnl"
      : tab === 3 ? "rec" : tab === 4 ? "bil" : "cm";
    const key = `${loader}|${activeCompanyId}|${dateFrom}|${refreshTick}`;
    if (attempted.current.has(key)) return;
    attempted.current.add(key);
    if (loader === "jr") void loadJournal();
    else if (loader === "tb") void loadTrialBalance();
    else if (loader === "pnl") void loadPnl();
    else if (loader === "rec") void runReconcile(false);
    else if (loader === "bil") void loadBilant();
    else void loadLedger();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, activeCompanyId, dateFrom, refreshTick]);

  // ── Generează note contabile ──────────────────────────────────────────────

  const handleGenerate = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setGenerating(true);
    setPostResult(null);
    try {
      const result = await api.gl.generateEntries(activeCompanyId, dateFrom, dateTo);
      setPostResult(result);
      if (result.journalsInserted === 0) {
        notify.info(
          "Niciun document de înregistrat în perioada selectată — jurnalul a rămas neschimbat. " +
          "Notele contabile se generează pe perioadă: rulați după validări/stornări noi.",
        );
      } else {
        notify.success(
          `GL generat: ${result.journalsInserted} jurnale, ${result.entriesInserted} intrări` +
          (result.journalsReplaced > 0 ? ` (${result.journalsReplaced} re-generate)` : ""),
        );
      }
      if (result.skippedReceived > 0) {
        const refs = (result.skippedReceivedRefs ?? []).slice(0, 5).join(", ");
        notify.warn(
          `${result.skippedReceived} facturi primite NU au fost înregistrate (fără defalcare TVA): ` +
          refs + (result.skippedReceived > 5 ? " …" : "") +
          ". Completați defalcarea TVA, apoi regenerați.",
        );
      }
      invalidateReports();
    } catch (err) {
      notify.error(formatError(err, "Nu s-au putut genera notele contabile."));
    } finally {
      setGenerating(false);
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
      if (result.posted) invalidateReports();
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut posta închiderea TVA."));
    } finally {
      setClosing(false);
    }
  };

  // ── Închidere perioadă (6/7 → 121) + impozit + închidere anuală ───────────

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
        // Reîncarcă rapoartele (P&L-ul exclude închiderea, deci arată în continuare activitatea).
        invalidateReports();
      }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut posta închiderea perioadei."));
    } finally {
      setClosingPeriod(false);
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
      else {
        notify.success(`Impozit postat: ${r.amount} lei (${r.expenseAccount} → ${r.payableAccount})${r.estimated ? " — estimat" : ""}.`);
        invalidateReports();
      }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut posta impozitul."));
    }
  };

  const handleAnnualClose = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    const year = selectedYear;
    const ok = await confirm(
      `Postează închiderea anuală 121 → 117 pentru anul ${year}? Soldul contului 121 (rezultatul ` +
        `anului) se transferă în 117 «Rezultatul reportat», cu nota datată 01.01.${year + 1}. Idempotent.`,
      { title: "Închidere anuală 121 → 117", kind: "warning" },
    );
    if (!ok) return;
    try {
      const r = await api.gl.postAnnualClose(activeCompanyId, year);
      if (!r.posted) notify.info("Sold 121 zero — nimic de transferat.");
      else {
        notify.success(`Închidere anuală ${year}: ${r.kind === "profit" ? "profit" : "pierdere"} ${r.result121} lei → 117.`);
        invalidateReports();
      }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut posta închiderea anuală."));
    }
  };

  // ── Export bilanț XML ─────────────────────────────────────────────────────

  const runBilantExport = async (
    caen: string, avgEmployees: number | null, formOverride: string | null, priorYearForm: string | null,
  ) => {
    if (!activeCompanyId) return;
    const year = selectedYear;
    const dest = await saveDialog({
      title: "Salvează bilanț XML (ANAF S1005/S1003/S1002)",
      defaultPath: `bilant-${year}.xml`,
      filters: [{ name: "XML", extensions: ["xml"] }],
    });
    if (!dest) return;
    try {
      await api.gl.exportBilantXml(activeCompanyId, year, caen, avgEmployees, formOverride, priorYearForm, dest);
      notify.success(`Bilanț XML exportat (forma după criteriile de mărime) — F10 + F20. ` +
        `Importați-l în PDF-ul inteligent ANAF, verificați header-ul și completați F30.`);
      setShowBilantExport(false);
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta bilanțul XML."));
    }
  };

  // ── Date derivate pentru panouri ──────────────────────────────────────────

  // Registru-jurnal: căutare + paginare client.
  const jrRows = useMemo(() => {
    if (!journalReg) return [];
    const q = jrQuery.trim().toLowerCase();
    if (!q) return journalReg.rows;
    return journalReg.rows.filter((r) =>
      r.document.toLowerCase().includes(q) ||
      r.explanation.toLowerCase().includes(q) ||
      r.debitAccount.includes(q) ||
      r.creditAccount.includes(q),
    );
  }, [journalReg, jrQuery]);
  const jrPages    = Math.max(1, Math.ceil(jrRows.length / JR_PAGE_SIZE));
  const jrPageSafe = Math.min(jrPage, jrPages);
  const jrVisible  = jrRows.slice((jrPageSafe - 1) * JR_PAGE_SIZE, jrPageSafe * JR_PAGE_SIZE);
  const jrWindow   = useMemo(() => {
    let start = Math.max(1, jrPageSafe - 2);
    const end = Math.min(jrPages, start + 4);
    start = Math.max(1, end - 4);
    const pages: number[] = [];
    for (let p = start; p <= end; p++) pages.push(p);
    return pages;
  }, [jrPageSafe, jrPages]);

  // Balanță: cele patru egalități.
  const equalities = trialBal ? [
    { label: "Egalitatea 1 · Solduri inițiale D = C", ok: isZero(parseDec(trialBal.totalOpeningDebit) - parseDec(trialBal.totalOpeningCredit)) },
    { label: "Egalitatea 2 · Rulaje curente D = C",   ok: isZero(parseDec(trialBal.totalPeriodDebit)  - parseDec(trialBal.totalPeriodCredit)) },
    { label: "Egalitatea 3 · Total sume D = C",       ok: isZero(parseDec(trialBal.totalTotalDebit)   - parseDec(trialBal.totalTotalCredit)) },
    { label: "Egalitatea 4 · Solduri finale D = C",   ok: isZero(parseDec(trialBal.totalClosingDebit) - parseDec(trialBal.totalClosingCredit)) },
  ] : [];

  // Scadența TVA: 25 a lunii următoare perioadei.
  const vatDueNext = selectedMonth === 12
    ? { y: selectedYear + 1, m: 1 }
    : { y: selectedYear, m: selectedMonth + 1 };
  const vatDueLabel = fmtRoDate(`${vatDueNext.y}-${String(vatDueNext.m).padStart(2, "0")}-25`);

  // Reconciliere: rândurile tabelului.
  const recRows = reconcileReport ? [
    {
      label: "TVA colectată (4427)", d300Row: "—",
      gl: parseDec(reconcileReport.vatCollectedGl), d300: parseDec(reconcileReport.vatCollectedD300),
    },
    {
      label: "TVA deductibilă (4426)", d300Row: "—",
      gl: parseDec(reconcileReport.vatDeductibleGl), d300: parseDec(reconcileReport.vatDeductibleD300),
    },
  ] : [];

  const cellAmt = (v: string) => (isZero(parseDec(v)) ? <span className="muted">—</span> : fmtRON(v));

  // ── Empty state (fără companie) ───────────────────────────────────────────
  if (!activeCompanyId) {
    return (
      <div className="main-inner wide pg-gl">
        <div className="page-head"><div><h1>Jurnal contabil (GL)</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Selectați o companie activă pentru a lucra cu jurnalul contabil.
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide pg-gl">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Jurnal contabil (GL)</h1>
          <p className="sub">
            {periodLabel} · note generate automat din documente · registru-jurnal cod 14-1-1,
            balanță cod 14-6-30, cartea mare cod 14-1-3
          </p>
        </div>
        <div className="head-actions">
          <div className="nou-wrap" style={{ position: "relative" }}>
            <button
              className="pill-btn"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => setOpenPop(openPop === "period" ? "" : "period")}
            >
              <Ic name="calendar" />
              {periodLabel}
              <Ic name="chevD" cls="ic" />
            </button>
            {openPop === "period" && (
              <div className="pop show" style={{ right: 0, top: 40, width: 210, maxHeight: 300, overflowY: "auto" }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="col-title">Perioadă</div>
                {periodOptions.map(({ y, m }) => (
                  <button
                    key={`${y}-${m}`}
                    className="pop-item"
                    onClick={() => { setSelectedYear(y); setSelectedMonth(m); setOpenPop(""); }}
                  >
                    <span style={{ flex: 1 }}>{MONTHS_RO[m - 1]} {y}</span>
                    {selectedYear === y && selectedMonth === m && <Ic name="check" cls="co-check" />}
                  </button>
                ))}
              </div>
            )}
          </div>
          <button
            className={`btn-dark spin-btn${generating ? " spinning" : ""}`}
            disabled={generating}
            onClick={() => void handleGenerate()}
          >
            <Ic name="sync" />
            {generating ? "Generez…" : `Generează notele pe ${monthName.toLowerCase()}`}
          </button>
        </div>
      </div>

      {/* tabs */}
      <div className="tabs" style={{ display: "inline-flex", marginBottom: 16 }}>
        {TABS.map((t, i) => (
          <div key={t} className={`tab${tab === i ? " active" : ""}`} onClick={() => setTab(i)}>
            {t}
          </div>
        ))}
      </div>

      {/* ── 1. REGISTRU-JURNAL ─────────────────────────────────────────────── */}
      <div className={`panel${tab === 0 ? " show" : ""}`}>
        {postResult && (
          <div className={`banner ${postResult.skippedReceived > 0 ? "warn" : "ok"}`} style={{ marginBottom: 14 }}>
            <InlineIc path={postResult.skippedReceived > 0 ? WARN_TRI : CIRCLE_CHECK} />
            <span>
              <b>GL generat pentru {monthName.toLowerCase()}:</b>{" "}
              {postResult.journalsInserted} jurnale · {postResult.entriesInserted} intrări
              {postResult.journalsReplaced > 0 && <> · {postResult.journalsReplaced} re-generate</>}.
              {postResult.skippedReceived > 0 && (
                <>
                  {" "}<b className="neg">
                    {postResult.skippedReceived === 1
                      ? "1 factură primită sărită"
                      : `${postResult.skippedReceived} facturi primite sărite`}
                  </b>{" "}
                  (fără defalcare TVA):{" "}
                  {(postResult.skippedReceivedRefs ?? []).slice(0, 5).map((ref, i) => (
                    <span key={ref}>{i > 0 && ", "}<span className="doc">{ref}</span></span>
                  ))}
                  {postResult.skippedReceived > 5 && " …"} — completați defalcarea
                  («Recalculează TVA din XML» în Jurnal cumpărări), apoi regenerați.
                </>
              )}
            </span>
          </div>
        )}
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">Registru-jurnal (cod 14-1-1) — {periodLabel}</div>
            <div className="spacer" />
            <div className="scr-search" style={{ width: 190 }}>
              <Ic name="lens" />
              <input
                type="text"
                placeholder="Caută nota…"
                value={jrQuery}
                onChange={(e) => { setJrQuery(e.target.value); setJrPage(1); }}
              />
            </div>
            {/* propunere — neimplementat (nu există API de export registru-jurnal) */}
            <button className="pill-btn" onClick={() => notify.info("În curând.")}>
              <Ic name="dl" />Export
            </button>
          </div>
          {loadingJr ? (
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
          ) : !journalReg || journalReg.rows.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              Nicio notă contabilă în perioadă — apăsați «Generează notele pe {monthName.toLowerCase()}».
            </div>
          ) : jrRows.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              Nicio notă pentru căutarea aplicată.
            </div>
          ) : (
            <>
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>Nr.</th><th>Data</th><th>Document</th><th>Explicații</th>
                    <th>Cont D</th><th>Cont C</th>
                    <th className="r">Sume D</th><th className="r">Sume C</th>
                  </tr>
                </thead>
                <tbody>
                  {jrVisible.map((r) => (
                    <tr key={r.nrCrt}>
                      <td className="num">{r.nrCrt}</td>
                      <td className="num">{fmtD(r.date)}</td>
                      <td><span className="doc">{r.document || "—"}</span></td>
                      <td>{r.explanation}</td>
                      <td>{r.debitAccount ? <span className="doc">{r.debitAccount}</span> : <span className="muted">—</span>}</td>
                      <td>{r.creditAccount ? <span className="doc">{r.creditAccount}</span> : <span className="muted">—</span>}</td>
                      <td className="r num">{cellAmt(r.debit)}</td>
                      <td className="r num">{cellAmt(r.credit)}</td>
                    </tr>
                  ))}
                  {!jrQuery && (
                    <tr style={{ background: "#FCFCFD", fontWeight: 600 }}>
                      <td colSpan={6}>
                        Total rulaj — {journalReg.balanced
                          ? "echilibrat (D = C)"
                          : <span className="neg">DEZECHILIBRAT</span>}
                      </td>
                      <td className="r num">{fmtRON(journalReg.totalDebit)}</td>
                      <td className="r num">{fmtRON(journalReg.totalCredit)}</td>
                    </tr>
                  )}
                </tbody>
              </table>
              <div className="pager">
                <span>
                  Afișezi <b>{(jrPageSafe - 1) * JR_PAGE_SIZE + 1}–{Math.min(jrPageSafe * JR_PAGE_SIZE, jrRows.length)}</b>{" "}
                  din <b>{jrRows.length}</b> note · {monthName.toLowerCase()} {selectedYear}
                </span>
                <div className="pg-btns">
                  <button className="pg-btn" disabled={jrPageSafe <= 1} onClick={() => setJrPage(jrPageSafe - 1)}>
                    <InlineIc path={CHEV_L} />
                  </button>
                  {jrWindow.map((p) => (
                    <button key={p} className={`pg-btn${p === jrPageSafe ? " cur" : ""}`} onClick={() => setJrPage(p)}>
                      {p}
                    </button>
                  ))}
                  <button className="pg-btn" disabled={jrPageSafe >= jrPages} onClick={() => setJrPage(jrPageSafe + 1)}>
                    <Ic name="chevR" />
                  </button>
                </div>
              </div>
            </>
          )}
        </div>
      </div>

      {/* ── 2. BALANȚĂ ─────────────────────────────────────────────────────── */}
      <div className={`panel${tab === 1 ? " show" : ""}`}>
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">Balanța de verificare — {fmtRoDate(dateTo)}</div>
            <div className="spacer" />
            {/* propunere — neimplementat (nu există API de export balanță XLSX) */}
            <button className="pill-btn" onClick={() => notify.info("În curând.")}>
              <Ic name="dl" />Export XLSX
            </button>
          </div>
          {loadingTb ? (
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
          ) : !trialBal || trialBal.rows.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              Nicio mișcare contabilă în perioadă.
            </div>
          ) : (
            <>
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>Cont</th>
                    <th className="r">SI D</th><th className="r">SI C</th>
                    <th className="r">Rulaj D</th><th className="r">Rulaj C</th>
                    <th className="r">Total sume D</th><th className="r">Total sume C</th>
                    <th className="r">SF D</th><th className="r">SF C</th>
                  </tr>
                </thead>
                <tbody>
                  {trialBal.rows.map((r) => (
                    <tr key={r.accountCode}>
                      <td><span className="doc">{r.accountCode}</span> {r.accountName}</td>
                      {[r.openingDebit, r.openingCredit, r.periodDebit, r.periodCredit,
                        r.totalDebit, r.totalCredit, r.closingDebit, r.closingCredit].map((v, i) => (
                        <td key={i} className="r num">{cellAmt(v)}</td>
                      ))}
                    </tr>
                  ))}
                  <tr style={{ background: "#FCFCFD", fontWeight: 600 }}>
                    <td>Total</td>
                    {[trialBal.totalOpeningDebit, trialBal.totalOpeningCredit,
                      trialBal.totalPeriodDebit, trialBal.totalPeriodCredit,
                      trialBal.totalTotalDebit, trialBal.totalTotalCredit,
                      trialBal.totalClosingDebit, trialBal.totalClosingCredit].map((v, i) => (
                      <td key={i} className="r num">{fmtRON(v)}</td>
                    ))}
                  </tr>
                </tbody>
              </table>
              <div className="eq-row">
                {equalities.map((eq) => (
                  <span key={eq.label} className={`eq${eq.ok ? "" : " bad"}`}>
                    <InlineIc path={eq.ok ? EQ_CHECK : WARN_TRI} cls="sic" />
                    {eq.label}
                  </span>
                ))}
              </div>
            </>
          )}
        </div>
      </div>

      {/* ── 3. ÎNCHIDERI ───────────────────────────────────────────────────── */}
      <div className={`panel${tab === 2 ? " show" : ""}`}>
        <div className="cols-2-even">
          {/* Închidere TVA */}
          <div className="scr-card close-card">
            <div className="scr-toolbar">
              <div className="tt">Închidere TVA — {periodLabel}</div>
              <div className="spacer" />
              {vatClose
                ? vatClose.posted
                  ? <ChipPaid label="Rulată" />
                  : <span className="chip sent"><Ic name="dot" cls="sic" />Nimic de regularizat</span>
                : <ChipWait label="Nerulată" />}
            </div>
            <div className="crow">
              <div>
                <div className="c1">TVA colectată</div>
                <div className="c2"><span className="doc">4427</span>{vatClose?.posted ? " → închis" : " · sold perioadă"}</div>
              </div>
              <span className="amt num">{vatClose ? fmtRON(vatClose.collected) : "—"}</span>
            </div>
            <div className="crow">
              <div>
                <div className="c1">TVA deductibilă</div>
                <div className="c2"><span className="doc">4426</span>{vatClose?.posted ? " → închis" : " · sold perioadă"}</div>
              </div>
              <span className="amt num">{vatClose ? fmtRON(vatClose.deductible) : "—"}</span>
            </div>
            <div className="crow">
              <div>
                <div className="c1">
                  {vatClose && parseDec(vatClose.deRecuperat) > 0 ? "TVA de recuperat" : "TVA de plată"}
                </div>
                <div className="c2">
                  <span className="doc">{vatClose && parseDec(vatClose.deRecuperat) > 0 ? "4424" : "4423"}</span>
                  {" "}· scadență {vatDueLabel}
                </div>
              </div>
              <span className="amt num">
                <b>
                  {vatClose
                    ? parseDec(vatClose.deRecuperat) > 0 ? fmtRON(vatClose.deRecuperat) : fmtRON(vatClose.dePlata)
                    : "—"}
                </b>
              </span>
            </div>
            <div className="crow" style={{ background: "#FCFCFD" }}>
              <div className="c2" style={{ margin: 0 }}>
                Dacă <span className="doc">4426</span> &gt; <span className="doc">4427</span>, diferența
                merge în <span className="doc">4424</span> TVA de recuperat. <span className="doc">4428</span>
                {" "}«TVA neexigibilă» nu este afectat.
              </div>
              <button
                className="btn-dark"
                style={{ marginLeft: "auto", height: 30, fontSize: 12.5, flex: "none" }}
                disabled={closing}
                onClick={() => void handleCloseVat()}
              >
                {closing ? "Închid…" : vatClose?.posted ? "Rulează din nou" : "Rulează acum"}
              </button>
            </div>
          </div>

          {/* Închidere rezultat */}
          <div className="scr-card close-card">
            <div className="scr-toolbar">
              <div className="tt">Închidere rezultat — {periodLabel}</div>
              <div className="spacer" />
              {pnl
                ? parseDec(pnl.netResult) >= 0 ? <ChipPaid label="Profit" /> : <ChipLate label="Pierdere" />
                : <ChipWait label={loadingPnl ? "Se calculează…" : "Necalculat"} />}
            </div>
            <div className="crow">
              <div>
                <div className="c1">Venituri de închis</div>
                <div className="c2"><span className="doc">70x–76x</span> = <span className="doc">121</span></div>
              </div>
              <span className="amt num">{pnl ? fmtRON(pnl.totalRevenue) : "—"}</span>
            </div>
            <div className="crow">
              <div>
                <div className="c1">Cheltuieli de închis</div>
                <div className="c2"><span className="doc">121</span> = <span className="doc">60x–68x</span></div>
              </div>
              <span className="amt num">{pnl ? fmtRON(pnl.totalExpense) : "—"}</span>
            </div>
            <div className="crow">
              <div>
                <div className="c1">Rezultat net {pnl && parseDec(pnl.netResult) >= 0 ? "(profit)" : pnl ? "(pierdere)" : ""}</div>
                <div className="c2"><span className="doc">121</span> {pnl && parseDec(pnl.netResult) >= 0 ? "sold creditor" : "sold debitor"}</div>
              </div>
              <span className={`amt num${pnl ? (parseDec(pnl.netResult) >= 0 ? " pos" : " neg") : ""}`}>
                {pnl ? `${parseDec(pnl.netResult) >= 0 ? "+" : ""}${fmtRON(pnl.netResult)}` : "—"}
              </span>
            </div>
            <div className="crow" style={{ background: "#FCFCFD" }}>
              <div className="c2" style={{ margin: 0 }}>
                Postează închiderea <span className="doc">6/7</span> → <span className="doc">121</span> pe
                perioadă (idempotent)
              </div>
              <button
                className="btn-dark"
                style={{ marginLeft: "auto", height: 30, fontSize: 12.5, flex: "none" }}
                disabled={closingPeriod}
                onClick={() => void handleClosePeriod()}
              >
                {closingPeriod ? "Închid…" : "Închide perioada"}
              </button>
            </div>
            <div className="crow" style={{ background: "#FCFCFD" }}>
              <div className="c2" style={{ margin: 0 }}>
                Închiderea anuală <span className="doc">121</span> → <span className="doc">117</span>
                {" "}«Rezultatul reportat» se rulează după 31 dec
              </div>
              <button
                className="pill-btn"
                style={{ marginLeft: "auto", flex: "none" }}
                onClick={() => void handleAnnualClose()}
              >
                Rulează închiderea anuală
              </button>
            </div>
          </div>

          {/* Impozit */}
          <div className="scr-card close-card">
            <div className="scr-toolbar">
              <div className="tt">Impozit — {periodLabel}</div>
              <div className="spacer" />
              {pnl
                ? pnl.incomeTaxEstimated ? <ChipWait label="Estimat" /> : <ChipPaid label="Înregistrat" />
                : <ChipWait label={loadingPnl ? "Se calculează…" : "Necalculat"} />}
            </div>
            <div className="crow">
              <div>
                <div className="c1">Impozit micro 1% pe venit</div>
                <div className="c2">
                  <span className="doc">698</span> = <span className="doc">4418</span>
                  {pnl && pnl.taxRegime === "micro"
                    ? <> · baza: venituri {fmtRON(pnl.totalRevenue)}</>
                    : " · nu se aplică (impozit pe profit)"}
                </div>
              </div>
              <span className={`amt num${pnl && pnl.taxRegime !== "micro" ? " muted" : ""}`}>
                {pnl ? (pnl.taxRegime === "micro" ? fmtRON(pnl.incomeTax) : "—") : "—"}
              </span>
            </div>
            <div className="crow">
              <div>
                <div className="c1">Impozit pe profit 16%</div>
                <div className="c2">
                  <span className="doc">691</span> = <span className="doc">4411</span>
                  {pnl && pnl.taxRegime !== "micro"
                    ? <> · baza: rezultat brut {fmtRON(pnl.grossResult)}</>
                    : " · nu se aplică (regim micro)"}
                </div>
              </div>
              <span className={`amt num${pnl && pnl.taxRegime === "micro" ? " muted" : ""}`}>
                {pnl ? (pnl.taxRegime !== "micro" ? fmtRON(pnl.incomeTax) : "—") : "—"}
              </span>
            </div>
            <div className="crow" style={{ background: "#FCFCFD" }}>
              <div className="c2" style={{ margin: 0 }}>
                Se înregistrează D <span className="doc">698/691</span> = C{" "}
                <span className="doc">4418/4411</span> · idempotent per perioadă, înainte de
                «Închide perioada»
              </div>
              <button
                className="btn-dark"
                style={{ marginLeft: "auto", height: 30, fontSize: 12.5, flex: "none" }}
                onClick={() => void handleIncomeTax()}
              >
                Postează impozitul
              </button>
            </div>
          </div>
        </div>
      </div>

      {/* ── 4. RECONCILIERE D300 ───────────────────────────────────────────── */}
      <div className={`panel${tab === 3 ? " show" : ""}`}>
        {reconcileReport && reconcileReport.discrepancies.length > 0 && (
          <div className="banner danger">
            <InlineIc path={WARN_TRI} />
            <span>
              <b>
                {reconcileReport.discrepancies.length === 1
                  ? "1 discrepanță GL ↔ D300."
                  : `${reconcileReport.discrepancies.length} discrepanțe GL ↔ D300.`}
              </b>{" "}
              {reconcileReport.discrepancies.map((d, i) => (
                <span key={i}>{i > 0 && " · "}{d}</span>
              ))}
            </span>
          </div>
        )}
        {reconcileReport && reconcileReport.discrepancies.length === 0 && reconcileReport.balanced && (
          <div className="banner ok">
            <InlineIc path={CIRCLE_CHECK} />
            <span><b>Nicio discrepanță.</b> GL balansat și aliniat cu decontul D300 pentru {periodLabel.toLowerCase()}.</span>
          </div>
        )}
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">Reconciliere GL ↔ D300 — {periodLabel}</div>
            <div className="spacer" />
            <button
              className={`pill-btn spin-btn${reconciling ? " spinning" : ""}`}
              disabled={reconciling}
              onClick={() => void runReconcile(true)}
            >
              <Ic name="sync" />{reconciling ? "Verific…" : "Rerulează verificarea"}
            </button>
          </div>
          {reconciling && !reconcileReport ? (
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se verifică…</div>
          ) : !reconcileReport ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              Apăsați «Rerulează verificarea» pentru a compara balanța GL cu decontul D300.
            </div>
          ) : (
            <>
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>Indicator</th><th>Rând D300</th>
                    <th className="r">Balanța (GL)</th><th className="r">Decont (D300)</th>
                    <th className="r">Diferența</th><th>Status</th>
                  </tr>
                </thead>
                <tbody>
                  {recRows.map((r) => {
                    const diff = r.gl - r.d300;
                    return (
                      <tr key={r.label}>
                        <td>{r.label}</td>
                        <td><span className="doc">{r.d300Row}</span></td>
                        <td className="r num">{fmtRON(r.gl)}</td>
                        <td className="r num">{fmtRON(r.d300)}</td>
                        <td className={`r num${isZero(diff) ? "" : " neg"}`}>{fmtRON(Math.abs(diff))}</td>
                        <td>{isZero(diff) ? <ChipPaid label="OK" /> : <ChipLate label="Discrepanță" />}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              <div className="eq-row">
                <span className={`eq${reconcileReport.balanced ? "" : " bad"}`}>
                  <InlineIc path={reconcileReport.balanced ? EQ_CHECK : WARN_TRI} cls="sic" />
                  Rulaj total GL — D {fmtRON(reconcileReport.totalDebit)} {reconcileReport.balanced ? "=" : "≠"} C {fmtRON(reconcileReport.totalCredit)}
                </span>
                <span className={`eq${reconcileReport.discrepancies.length === 0 ? "" : " bad"}`}>
                  <InlineIc path={reconcileReport.discrepancies.length === 0 ? EQ_CHECK : WARN_TRI} cls="sic" />
                  {reconcileReport.discrepancies.length === 0
                    ? "Fără discrepanțe GL ↔ D300"
                    : `${reconcileReport.discrepancies.length} ${reconcileReport.discrepancies.length === 1 ? "discrepanță" : "discrepanțe"}`}
                </span>
              </div>
            </>
          )}
        </div>
      </div>

      {/* ── 5. BILANȚ XML ──────────────────────────────────────────────────── */}
      <div className={`panel${tab === 4 ? " show" : ""}`}>
        <div className="cols-2">
          <div className="scr-card">
            <div className="scr-toolbar">
              <div className="tt">Încadrare entitate — exercițiul {selectedYear}</div>
              <div className="spacer" />
              {bilant
                ? bilant.balanced ? <ChipPaid label="Echilibrat" /> : <ChipLate label="Neechilibrat" />
                : <ChipWait label={loadingBilant ? "Se calculează…" : "Necalculat"} />}
            </div>
            <div className="card-pad">
              {loadingBilant ? (
                <div style={{ fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
              ) : !bilant ? (
                <div style={{ fontSize: 13, color: "var(--text-2)" }}>
                  Nicio mișcare contabilă în perioadă — generați notele contabile mai întâi.
                </div>
              ) : (
                <>
                  <div className="crit">
                    <InlineIc path={CIRCLE_CHECK} cls="sic" />
                    <div>Total active <span className="muted">· criteriu de mărime OMFP</span></div>
                    <span className="cv num">{fmtRON(bilant.totalAssets)} lei</span>
                  </div>
                  <div className="crit">
                    <InlineIc path={CIRCLE_CHECK} cls="sic" />
                    <div>Capitaluri proprii <span className="muted">· incl. rezultat</span></div>
                    <span className="cv num">{fmtRON(bilant.equity)} lei</span>
                  </div>
                  <div className="crit">
                    <InlineIc path={CIRCLE_CHECK} cls="sic" />
                    <div>Rezultatul exercițiului <span className="muted">· sold 121</span></div>
                    <span className={`cv num ${parseDec(bilant.currentResult) >= 0 ? "pos" : "neg"}`}>
                      {parseDec(bilant.currentResult) >= 0 ? "+" : ""}{fmtRON(bilant.currentResult)} lei
                    </span>
                  </div>
                  <div className={`banner ${bilant.balanced ? "ok" : "warn"}`} style={{ margin: "14px 0 0" }}>
                    <InlineIc path={bilant.balanced ? CIRCLE_CHECK : WARN_TRI} />
                    <span>
                      {bilant.entitySizeNote && <><b>{bilant.entitySizeNote}</b>{" "}</>}
                      {bilant.balanced
                        ? <>Bilanțul se verifică: Active = Capitaluri + Datorii ({fmtRON(bilant.totalAssets)} lei).</>
                        : <><b>Bilanțul NU se verifică</b> (Active ≠ Capitaluri + Datorii) — verificați balanța.</>}
                    </span>
                  </div>
                </>
              )}
            </div>
          </div>

          <div className="scr-card">
            <div className="scr-toolbar"><div className="tt">Export bilanț XML</div></div>
            <div className="card-pad">
              <dl className="kv" style={{ gridTemplateColumns: "150px 1fr", fontSize: 12.5, marginBottom: 14 }}>
                <dt>S1005</dt><dd>Microentități — bilanț prescurtat</dd>
                <dt>S1003</dt><dd>Entități mici — bilanț prescurtat extins</dd>
                <dt>S1002</dt><dd>Entități mijlocii și mari — bilanț complet</dd>
              </dl>
              <button
                className="btn-dark"
                style={{ width: "100%", justifyContent: "center" }}
                onClick={() => setShowBilantExport(true)}
              >
                <Ic name="code" />Generează bilanț XML — {selectedYear}
              </button>
              <p className="muted" style={{ fontSize: 11.5, marginTop: 10, lineHeight: 1.5 }}>
                XML-ul (F10 + F20) se importă în PDF-ul inteligent ANAF și se validează cu soft-ul
                DUKIntegrator înainte de depunere. Termenul de depunere: 150 de zile de la închiderea
                exercițiului.
              </p>
            </div>
          </div>
        </div>

        {/* Bilanț contabil (sinteză) — real feature kept (the prototype lacks it) */}
        {bilant && (
          <div className="scr-card" style={{ marginTop: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Bilanț contabil (sinteză) — {fmtD(bilant.periodTo)}</div>
              <div className="spacer" />
              {bilant.balanced
                ? <ChipPaid label="Active = Capitaluri + Datorii" />
                : <ChipLate label="Nu se verifică" />}
            </div>
            <div className="cols-2-even" style={{ gap: 0 }}>
              <table className="scr-table">
                <thead>
                  <tr><th>Active</th><th className="r">Sold</th></tr>
                </thead>
                <tbody>
                  {([
                    ["Active imobilizate (net)", bilant.immobilizedAssets],
                    ["Stocuri", bilant.inventory],
                    ["Creanțe", bilant.receivables],
                    ["Investiții pe termen scurt", bilant.shortInvestments],
                    ["Casa și conturi la bănci", bilant.cashBank],
                    ["Cheltuieli în avans", bilant.prepaidExpenses],
                  ] as Array<[string, string]>).map(([label, v]) => (
                    <tr key={label}>
                      <td>{label}</td>
                      <td className="r num">{cellAmt(v)}</td>
                    </tr>
                  ))}
                  <tr style={{ background: "#FCFCFD", fontWeight: 600 }}>
                    <td>TOTAL ACTIVE</td>
                    <td className="r num">{fmtRON(bilant.totalAssets)}</td>
                  </tr>
                </tbody>
              </table>
              <table className="scr-table">
                <thead>
                  <tr><th>Capitaluri și datorii</th><th className="r">Sold</th></tr>
                </thead>
                <tbody>
                  {([
                    ["Capitaluri proprii (incl. rezultat)", bilant.equity],
                    ["— din care rezultatul exercițiului", bilant.currentResult],
                    ["Provizioane", bilant.provisions],
                    ["Datorii pe termen lung", bilant.longTermDebt],
                    ["Datorii curente", bilant.currentLiabilities],
                    ["Venituri în avans", bilant.deferredRevenue],
                  ] as Array<[string, string]>).map(([label, v]) => (
                    <tr key={label}>
                      <td>{label}</td>
                      <td className="r num">{cellAmt(v)}</td>
                    </tr>
                  ))}
                  <tr style={{ background: "#FCFCFD", fontWeight: 600 }}>
                    <td>TOTAL CAPITALURI + DATORII</td>
                    <td className="r num">{fmtRON(bilant.totalEquityLiabilities)}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        )}
      </div>

      {/* ── 6. CARTEA MARE (real feature, restyled with design cards) ──────── */}
      <div className={`panel${tab === 5 ? " show" : ""}`}>
        {loadingCm ? (
          <div className="scr-card">
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
          </div>
        ) : !ledger || ledger.length === 0 ? (
          <div className="scr-card">
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              Nicio mișcare contabilă în perioadă — cartea mare (cod 14-1-3) se completează după
              generarea notelor.
            </div>
          </div>
        ) : (
          ledger.map((a) => (
            <div key={a.accountCode} className="scr-card" style={{ marginBottom: 14 }}>
              <div className="scr-toolbar">
                <div className="tt"><span className="doc">{a.accountCode}</span> {a.accountName}</div>
                <div className="spacer" />
                <span className="muted" style={{ fontSize: 12 }}>
                  sold inițial{" "}
                  {parseDec(a.openingDebit) > 0
                    ? `${fmtRON(a.openingDebit)} D`
                    : parseDec(a.openingCredit) > 0
                      ? `${fmtRON(a.openingCredit)} C`
                      : "0"}
                </span>
              </div>
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>Data</th><th>Document</th><th>Explicații</th><th>Cont coresp.</th>
                    <th className="r">Debit</th><th className="r">Credit</th><th className="r">Sold</th>
                  </tr>
                </thead>
                <tbody>
                  {a.entries.map((e, i) => (
                    <tr key={i}>
                      <td className="num">{fmtD(e.date)}</td>
                      <td><span className="doc">{e.document || "—"}</span></td>
                      <td>{e.explanation}</td>
                      <td>{e.contra ? <span className="doc">{e.contra}</span> : <span className="muted">—</span>}</td>
                      <td className="r num">{cellAmt(e.debit)}</td>
                      <td className="r num">{cellAmt(e.credit)}</td>
                      <td className="r num">{fmtRON(e.balance)} {e.balanceSide}</td>
                    </tr>
                  ))}
                  <tr style={{ background: "#FCFCFD", fontWeight: 600 }}>
                    <td colSpan={4}>Rulaj / sold final</td>
                    <td className="r num">{fmtRON(a.totalDebit)}</td>
                    <td className="r num">{fmtRON(a.totalCredit)}</td>
                    <td className="r num">
                      {parseDec(a.closingDebit) > 0
                        ? `${fmtRON(a.closingDebit)} D`
                        : parseDec(a.closingCredit) > 0
                          ? `${fmtRON(a.closingCredit)} C`
                          : "0"}
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          ))
        )}
      </div>

      {/* ── 7. PROFIT ȘI PIERDERE (real feature, restyled) ─────────────────── */}
      <div className={`panel${tab === 6 ? " show" : ""}`}>
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">Cont de profit și pierdere — {periodLabel}</div>
            <div className="spacer" />
            <span className="muted" style={{ fontSize: 12 }}>
              {pnl
                ? pnl.taxRegime === "micro" ? "regim: microîntreprindere (1%)" : "regim: impozit pe profit (16%)"
                : ""}{pnl ? " · OMFP 1802/2014" : ""}
            </span>
            {pnl && (
              parseDec(pnl.netResult) >= 0
                ? <ChipPaid label={`Rezultat net +${fmtRON(pnl.netResult)}`} />
                : <ChipLate label={`Rezultat net ${fmtRON(pnl.netResult)}`} />
            )}
          </div>
          {loadingPnl ? (
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
          ) : !pnl ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              Nicio mișcare pe conturile de venituri/cheltuieli în perioadă.
            </div>
          ) : (
            <>
              <table className="scr-table">
                <tbody>
                  <tr style={{ background: "#FCFCFD", fontWeight: 600 }}>
                    <td>Venituri (clasa 7)</td>
                    <td className="r num">{fmtRON(pnl.totalRevenue)}</td>
                  </tr>
                  {pnl.revenueLines.map((l) => (
                    <tr key={l.code}>
                      <td style={{ paddingLeft: 32 }}><span className="doc">{l.code}</span> {l.name}</td>
                      <td className="r num">{fmtRON(l.amount)}</td>
                    </tr>
                  ))}
                  <tr style={{ background: "#FCFCFD", fontWeight: 600 }}>
                    <td>Cheltuieli (clasa 6, fără impozit pe venit/profit)</td>
                    <td className="r num">{fmtRON(pnl.totalExpense)}</td>
                  </tr>
                  {pnl.expenseLines.map((l) => (
                    <tr key={l.code}>
                      <td style={{ paddingLeft: 32 }}><span className="doc">{l.code}</span> {l.name}</td>
                      <td className="r num">{fmtRON(l.amount)}</td>
                    </tr>
                  ))}
                  <tr style={{ fontWeight: 600 }}>
                    <td>Rezultat brut (venituri − cheltuieli)</td>
                    <td className="r num">{fmtRON(pnl.grossResult)}</td>
                  </tr>
                  <tr>
                    <td>
                      Impozit pe {pnl.taxRegime === "micro" ? "venit" : "profit"}
                      {pnl.incomeTaxEstimated ? " (estimat)" : " (înregistrat)"}
                    </td>
                    <td className="r num">{fmtRON(pnl.incomeTax)}</td>
                  </tr>
                  <tr style={{ background: "#FCFCFD", fontWeight: 600 }}>
                    <td>Rezultat net</td>
                    <td className={`r num ${parseDec(pnl.netResult) >= 0 ? "pos" : "neg"}`}>
                      {fmtRON(pnl.netResult)}
                    </td>
                  </tr>
                </tbody>
              </table>
              {pnl.incomeTaxEstimated && (
                <div className="pager">
                  <span>
                    Impozitul este estimat ({pnl.taxRegime === "micro" ? "1% × venituri" : "16% × rezultat brut pozitiv"});
                    pentru profit, ajustările fiscale (cheltuieli nedeductibile, venituri neimpozabile) nu sunt
                    incluse. Notele de închidere (D 7xx / C 121, D 121 / C 6xx) sunt pregătite
                    ({pnl.closingEntries.length} rânduri).
                  </span>
                  <span></span>
                </div>
              )}
            </>
          )}
        </div>
      </div>

      {showBilantExport && (
        <BilantExportModal
          year={selectedYear}
          onClose={() => setShowBilantExport(false)}
          onExport={runBilantExport}
        />
      )}
    </div>
  );
}

// ─── BilantExportModal — design .modal-back/.modal with .fgrid fields ─────────
/** Bilanț XML export dialog — CAEN + nr. mediu salariați + alegerea formei (auto / UU / BS / BL),
 *  înlocuiește window.prompt (care nu funcționează în WebView-ul Tauri). */
function BilantExportModal({
  year,
  onClose,
  onExport,
}: {
  year: number;
  onClose: () => void;
  onExport: (
    caen: string, avgEmployees: number | null, formOverride: string | null, priorYearForm: string | null,
  ) => Promise<void>;
}) {
  const [caen, setCaen] = useState("");
  const [emp, setEmp] = useState("");
  const [form, setForm] = useState("auto");
  const [priorForm, setPriorForm] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (!/^\d{4}$/.test(caen.trim())) { notify.error("Cod CAEN invalid — 4 cifre (ex. 6201)."); return; }
    setBusy(true);
    try {
      await onExport(
        caen.trim(),
        emp.trim() === "" ? null : Number(emp),
        form === "auto" ? null : form,
        priorForm === "" ? null : priorForm,
      );
    } finally {
      setBusy(false);
    }
  };

  return createPortal(
    <div
      className="modal-back show"
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget && !busy) onClose(); }}
    >
      <div className="modal">
        <div className="modal-head">
          <div>
            <div className="mt">Generează bilanț XML — exercițiul {year}</div>
            <div className="ms">Confirmați datele de identificare pentru XML</div>
          </div>
          <button className="modal-x" onClick={onClose} aria-label="Închide">
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="banner" style={{ marginBottom: 14 }}>
            <InlineIc path={CIRCLE_CHECK} />
            <span>
              Forma (microîntreprindere <b>S1005</b> / mică <b>S1003</b> / mare <b>S1002</b>) se alege
              după criteriile OMFP (2 din 3: active, cifra de afaceri, nr. salariați). Conform OMFP
              1802/2014 pct. 13(2), categoria se schimbă doar dacă criteriile sunt depășite în{" "}
              <b>două exerciții consecutive</b> — o singură depășire păstrează forma anului precedent.
            </span>
          </div>
          <div className="fgrid">
            <div className="field">
              <label>Cod CAEN principal <span className="req">*</span></label>
              <input
                className="input num"
                type="text"
                placeholder="6201"
                value={caen}
                onChange={(e) => setCaen(e.target.value)}
                autoFocus
              />
              <span className="hint">4 cifre — activitatea principală</span>
            </div>
            <div className="field">
              <label>Număr mediu de salariați</label>
              <input
                className="input num"
                type="text"
                inputMode="numeric"
                placeholder="ex. 8"
                value={emp}
                onChange={(e) => setEmp(e.target.value)}
              />
              <span className="hint">criteriu de mărime OMFP</span>
            </div>
            <div className="field span2">
              <label>Formular (suprascriere automată)</label>
              <select className="select" value={form} onChange={(e) => setForm(e.target.value)}>
                <option value="auto">Auto — încadrare după criterii (2 din 3 + regula 2 ani)</option>
                <option value="UU">S1005 — Microentitate</option>
                <option value="BS">S1003 — Entitate mică</option>
                <option value="BL">S1002 — Entitate mijlocie / mare</option>
              </select>
            </div>
            <div className="field span2">
              <label>Forma depusă pentru anul precedent ({year - 1})</label>
              <select className="select" value={priorForm} onChange={(e) => setPriorForm(e.target.value)}>
                <option value="">Necunoscută</option>
                <option value="UU">S1005 — Microentitate</option>
                <option value="BS">S1003 — Entitate mică</option>
                <option value="BL">S1002 — Entitate mijlocie / mare</option>
              </select>
              <span className="hint">schimbarea categoriei se face doar după 2 ani consecutivi de depășire / încadrare</span>
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <span className="left">Se generează XML F10 + F20 (import în PDF inteligent ANAF)</span>
          <button className="pill-btn" onClick={onClose} disabled={busy}>Renunță</button>
          <button className="btn-dark" disabled={busy} onClick={() => void submit()}>
            <Ic name="dl" />{busy ? "Se exportă…" : "Generează XML"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
