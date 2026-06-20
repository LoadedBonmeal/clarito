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
import { Trans, useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { MonthPicker } from "@/components/shared/MonthPicker";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { useOpenXml } from "@/hooks/use-open-xml";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type {
  Account,
  Contact,
  GlPostResult, ReconcileReport, VatSettlementResult, TrialBalance,
  JournalRegister, LedgerAccount, ProfitLoss, BilantReport,
  ManualJournalView, ManualLineInput,
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

// ─── Component ───────────────────────────────────────────────────────────────

export function GlLedgerPage() {
  const { t, i18n } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const openXml = useOpenXml();

  const MONTHS = [
    t("gl.months.jan"), t("gl.months.feb"), t("gl.months.mar"),
    t("gl.months.apr"), t("gl.months.may"), t("gl.months.jun"),
    t("gl.months.jul"), t("gl.months.aug"), t("gl.months.sep"),
    t("gl.months.oct"), t("gl.months.nov"), t("gl.months.dec"),
  ];

  const TABS = [
    t("gl.tabs.journal"),
    t("gl.tabs.balance"),
    t("gl.tabs.closings"),
    t("gl.tabs.reconcile"),
    t("gl.tabs.bilant"),
    t("gl.tabs.ledger"),
    t("gl.tabs.pnl"),
    t("gl.tabs.partner"),
    t("gl.nc.tabLabel"),
  ];

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

  // ── Fișă partener ────────────────────────────────────────────────────────
  const [partnerContacts,   setPartnerContacts]   = useState<Contact[] | null>(null);
  const [partnerCui,        setPartnerCui]        = useState("");
  const [partnerLedger,     setPartnerLedger]     = useState<LedgerAccount[] | null>(null);
  const [loadingPartner,    setLoadingPartner]    = useState(false);

  // ── Note contabile manuale ────────────────────────────────────────────────
  const [ncList,            setNcList]            = useState<ManualJournalView[] | null>(null);
  const [loadingNc,         setLoadingNc]         = useState(false);
  const [showNcModal,       setShowNcModal]       = useState(false);
  const [ncAccounts,        setNcAccounts]        = useState<Account[] | null>(null);

  // ── Reevaluare valutară (P1 Wave 7) ─────────────────────────────────────
  const [fxRevalRunning,    setFxRevalRunning]    = useState(false);
  const [fxRevalResult,     setFxRevalResult]     = useState<import("@/types").FxRevaluationResult | null>(null);
  const [fxRevalRows,       setFxRevalRows]       = useState<import("@/types").FxRevaluationRow[] | null>(null);

  const [refreshTick, setRefreshTick] = useState(0);
  const attempted = useRef<Set<string>>(new Set());

  const { dateFrom, dateTo } = periodDateRange(selectedYear, selectedMonth);
  const monthName   = MONTHS[selectedMonth - 1];
  /** Numele lunii în interiorul frazelor — minuscul doar în RO. */
  const monthInline = i18n.language.startsWith("ro") ? monthName.toLowerCase() : monthName;
  const periodLabel = `${monthName} ${selectedYear}`;

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
    setPartnerLedger(null);
    setNcList(null);
    setFxRevalResult(null);
    setFxRevalRows(null);
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
    setNcList(null);
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
      notify.error(formatError(err, t("gl.notify.journalError")));
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
      notify.error(formatError(err, t("gl.notify.balanceError")));
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
      notify.error(formatError(err, t("gl.notify.ledgerError")));
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
      notify.error(formatError(err, t("gl.notify.pnlError")));
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
      notify.error(formatError(err, t("gl.notify.bilantError")));
    } finally {
      setLoadingBilant(false);
    }
  };

  const loadPartnerContacts = async () => {
    if (!activeCompanyId) return;
    try {
      const all = await api.contacts.list({ companyId: activeCompanyId });
      setPartnerContacts(all.filter((c) => c.cui));
    } catch {
      // non-fatal — partner selector stays empty
    }
  };

  const loadNcList = async () => {
    if (!activeCompanyId || loadingNc) return;
    setLoadingNc(true);
    try {
      setNcList(await api.gl.listManualJournals(activeCompanyId, dateFrom, dateTo));
    } catch (err) {
      notify.error(formatError(err, t("gl.nc.loading")));
    } finally {
      setLoadingNc(false);
    }
  };

  const loadNcAccounts = async () => {
    if (!activeCompanyId || ncAccounts) return;
    try {
      setNcAccounts(await api.accounts.list(activeCompanyId));
    } catch {
      // non-fatal — account picker stays empty
    }
  };

  const handleDeleteNc = async (nc: ManualJournalView) => {
    if (!activeCompanyId) return;
    const ok = await confirm(t("gl.nc.deleteConfirm", { date: fmtD(nc.date) }));
    if (!ok) return;
    try {
      await api.gl.deleteManualJournal(activeCompanyId, nc.sourceId);
      notify.success(t("gl.nc.deleteOk"));
      invalidateReports();
      void loadNcList();
    } catch (err) {
      notify.error(formatError(err, t("gl.nc.deleteError")));
    }
  };

  const loadPartnerLedgerForCui = async (cui: string) => {
    if (!activeCompanyId || !cui || loadingPartner) return;
    setLoadingPartner(true);
    setPartnerLedger(null);
    try {
      setPartnerLedger(await api.gl.partnerLedger(activeCompanyId, cui, dateFrom, dateTo));
    } catch (err) {
      notify.error(formatError(err, t("gl.partner.error")));
    } finally {
      setLoadingPartner(false);
    }
  };

  const runReconcile = async (manual: boolean) => {
    if (!activeCompanyId) {
      if (manual) notify.warn(t("gl.notify.selectCompany"));
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
          notify.success(t("gl.notify.reconcileOk"));
        } else if (report.discrepancies.length > 0) {
          notify.warn(t("gl.notify.reconcileDiscrepancies", { count: report.discrepancies.length }));
        } else {
          notify.info(t("gl.notify.reconcileInfo"));
        }
      }
    } catch (err) {
      notify.error(formatError(err, t("gl.notify.reconcileError")));
    } finally {
      setReconciling(false);
    }
  };

  // Auto-load per tab activ (o singură tentativă per perioadă/companie/tick).
  useEffect(() => {
    if (!activeCompanyId) return;
    const loader = tab === 0 ? "jr" : tab === 1 ? "tb" : tab === 2 || tab === 6 ? "pnl"
      : tab === 3 ? "rec" : tab === 4 ? "bil" : tab === 7 ? "partner" : tab === 8 ? "nc" : "cm";
    const key = `${loader}|${activeCompanyId}|${dateFrom}|${refreshTick}`;
    if (attempted.current.has(key)) return;
    attempted.current.add(key);
    if (loader === "jr") void loadJournal();
    else if (loader === "tb") void loadTrialBalance();
    else if (loader === "pnl") void loadPnl();
    else if (loader === "rec") void runReconcile(false);
    else if (loader === "bil") void loadBilant();
    else if (loader === "partner") void loadPartnerContacts();
    else if (loader === "nc") { void loadNcList(); void loadNcAccounts(); }
    else void loadLedger();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, activeCompanyId, dateFrom, refreshTick]);

  // ── Generează note contabile ──────────────────────────────────────────────

  const handleGenerate = async () => {
    if (!activeCompanyId) { notify.warn(t("gl.notify.selectCompany")); return; }
    setGenerating(true);
    setPostResult(null);
    try {
      const result = await api.gl.generateEntries(activeCompanyId, dateFrom, dateTo);
      setPostResult(result);
      if (result.journalsInserted === 0) {
        notify.info(t("gl.notify.generateNone"));
      } else {
        notify.success(
          t("gl.notify.generateOk", { journals: result.journalsInserted, entries: result.entriesInserted }) +
          (result.journalsReplaced > 0 ? t("gl.notify.generateReplaced", { n: result.journalsReplaced }) : ""),
        );
      }
      if (result.skippedReceived > 0) {
        const refs = (result.skippedReceivedRefs ?? []).slice(0, 5).join(", ") +
          (result.skippedReceived > 5 ? " …" : "");
        notify.warn(t("gl.notify.skippedReceived", { count: result.skippedReceived, refs }));
      }
      invalidateReports();
    } catch (err) {
      notify.error(formatError(err, t("gl.notify.generateError")));
    } finally {
      setGenerating(false);
    }
  };

  // ── Închiderea TVA (regularizare 4426/4427 → 4423/4424) ───────────────────

  const handleCloseVat = async () => {
    if (!activeCompanyId) { notify.warn(t("gl.notify.selectCompany")); return; }
    setClosing(true);
    setVatClose(null);
    try {
      const result = await api.gl.closeVat(activeCompanyId, dateFrom, dateTo);
      setVatClose(result);
      if (!result.posted) {
        notify.info(t("gl.notify.vatNothing"));
      } else if (parseDec(result.dePlata) > 0) {
        notify.success(t("gl.notify.vatPay", { amount: result.dePlata }));
      } else if (parseDec(result.deRecuperat) > 0) {
        notify.success(t("gl.notify.vatRecover", { amount: result.deRecuperat }));
      } else {
        notify.success(t("gl.notify.vatZero"));
      }
      if (result.posted) invalidateReports();
    } catch (err) {
      notify.error(formatError(err, t("gl.notify.vatError")));
    } finally {
      setClosing(false);
    }
  };

  // ── Închidere perioadă (6/7 → 121) + impozit + închidere anuală ───────────

  const handleClosePeriod = async () => {
    if (!activeCompanyId) { notify.warn(t("gl.notify.selectCompany")); return; }
    const ok = await confirm(
      t("gl.confirm.closePeriodText", { from: dateFrom, to: dateTo }),
      { title: t("gl.confirm.closePeriodTitle"), kind: "warning" },
    );
    if (!ok) return;
    setClosingPeriod(true);
    try {
      const r = await api.gl.closePeriod(activeCompanyId, dateFrom, dateTo);
      if (!r.posted) {
        notify.info(t("gl.notify.closePeriodNone"));
      } else {
        notify.success(t("gl.notify.closePeriodOk", { result: r.result, n: r.entriesCount }));
        // Reîncarcă rapoartele (P&L-ul exclude închiderea, deci arată în continuare activitatea).
        invalidateReports();
      }
    } catch (err) {
      notify.error(formatError(err, t("gl.notify.closePeriodError")));
    } finally {
      setClosingPeriod(false);
    }
  };

  const handleIncomeTax = async () => {
    if (!activeCompanyId) { notify.warn(t("gl.notify.selectCompany")); return; }
    const ok = await confirm(
      t("gl.confirm.taxText", { from: dateFrom, to: dateTo }),
      { title: t("gl.confirm.taxTitle"), kind: "warning" },
    );
    if (!ok) return;
    try {
      const r = await api.gl.postIncomeTax(activeCompanyId, dateFrom, dateTo);
      if (!r.posted) notify.info(t("gl.notify.taxZero"));
      else {
        notify.success(t(r.estimated ? "gl.notify.taxOkEstimated" : "gl.notify.taxOk", {
          amount: r.amount, expense: r.expenseAccount, payable: r.payableAccount,
        }));
        invalidateReports();
      }
    } catch (err) {
      notify.error(formatError(err, t("gl.notify.taxError")));
    }
  };

  const handleAnnualClose = async () => {
    if (!activeCompanyId) { notify.warn(t("gl.notify.selectCompany")); return; }
    const year = selectedYear;
    const ok = await confirm(
      t("gl.confirm.annualText", { year, nextYear: year + 1 }),
      { title: t("gl.confirm.annualTitle"), kind: "warning" },
    );
    if (!ok) return;
    try {
      const r = await api.gl.postAnnualClose(activeCompanyId, year);
      if (!r.posted) notify.info(t("gl.notify.annualZero"));
      else {
        notify.success(t("gl.notify.annualOk", {
          year,
          kind: t(r.kind === "profit" ? "gl.notify.kindProfit" : "gl.notify.kindLoss"),
          amount: r.result121,
        }));
        invalidateReports();
      }
    } catch (err) {
      notify.error(formatError(err, t("gl.notify.annualError")));
    }
  };

  // ── Reevaluare valutară ──────────────────────────────────────────────────
  const period = `${selectedYear}-${String(selectedMonth).padStart(2, "0")}`;

  const handleFxReval = async () => {
    if (!activeCompanyId) { notify.warn(t("gl.notify.selectCompany")); return; }
    const ok = await confirm(
      t("gl.fxReval.confirmText", { period }),
      { title: t("gl.fxReval.confirmTitle"), kind: "warning" },
    );
    if (!ok) return;
    setFxRevalRunning(true);
    try {
      const r = await api.gl.computeFxRevaluation(activeCompanyId, period);
      setFxRevalResult(r);
      const rows = await api.gl.listFxRevaluations(activeCompanyId, period);
      setFxRevalRows(rows);
      if (r.rowsPosted === 0) {
        notify.info(t("gl.fxReval.notifyNone", { period }));
      } else {
        notify.success(t("gl.fxReval.notifyOk", {
          period,
          n: r.rowsPosted,
          fav: r.totalFavorable,
          unfav: r.totalUnfavorable,
        }));
        invalidateReports();
      }
    } catch (err) {
      notify.error(formatError(err, t("gl.fxReval.notifyError")));
    } finally {
      setFxRevalRunning(false);
    }
  };

  // ── Export bilanț XML ─────────────────────────────────────────────────────

  const runBilantExport = async (
    caen: string, avgEmployees: number | null, formOverride: string | null, priorYearForm: string | null,
  ) => {
    if (!activeCompanyId) return;
    const year = selectedYear;
    const dest = await saveDialog({
      title: t("gl.export.saveTitle"),
      defaultPath: `bilant-${year}.xml`,
      filters: [{ name: "XML", extensions: ["xml"] }],
    });
    if (!dest) return;
    try {
      await api.gl.exportBilantXml(activeCompanyId, year, caen, avgEmployees, formOverride, priorYearForm, dest);
      notify.success(t("gl.notify.exportOk"));
      setShowBilantExport(false);
    } catch (err) {
      notify.error(formatError(err, t("gl.notify.exportError")));
    }
  };

  // Previzualizare/editare bilanț XML în vizualizatorul din aplicație (fără scriere, fără DUK).
  const runBilantPreview = async (
    caen: string, avgEmployees: number | null, formOverride: string | null, priorYearForm: string | null,
  ) => {
    if (!activeCompanyId) return;
    const year = selectedYear;
    try {
      const xml = await api.gl.previewBilantXml(activeCompanyId, year, caen, avgEmployees, formOverride, priorYearForm);
      openXml({ xml, name: `bilant-${year}.xml` });
    } catch (err) {
      notify.error(formatError(err, t("gl.bilant.previewFailed")));
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
    { label: t("gl.balance.eq1"), ok: isZero(parseDec(trialBal.totalOpeningDebit) - parseDec(trialBal.totalOpeningCredit)) },
    { label: t("gl.balance.eq2"), ok: isZero(parseDec(trialBal.totalPeriodDebit)  - parseDec(trialBal.totalPeriodCredit)) },
    { label: t("gl.balance.eq3"), ok: isZero(parseDec(trialBal.totalTotalDebit)   - parseDec(trialBal.totalTotalCredit)) },
    { label: t("gl.balance.eq4"), ok: isZero(parseDec(trialBal.totalClosingDebit) - parseDec(trialBal.totalClosingCredit)) },
  ] : [];

  // Scadența TVA: 25 a lunii următoare perioadei.
  const vatDueNext = selectedMonth === 12
    ? { y: selectedYear + 1, m: 1 }
    : { y: selectedYear, m: selectedMonth + 1 };
  const vatDueLabel = fmtRoDate(`${vatDueNext.y}-${String(vatDueNext.m).padStart(2, "0")}-25`);

  // Reconciliere: rândurile tabelului.
  const recRows = reconcileReport ? [
    {
      label: t("gl.reconcile.vatCollected"), d300Row: "—",
      gl: parseDec(reconcileReport.vatCollectedGl), d300: parseDec(reconcileReport.vatCollectedD300),
    },
    {
      label: t("gl.reconcile.vatDeductible"), d300Row: "—",
      gl: parseDec(reconcileReport.vatDeductibleGl), d300: parseDec(reconcileReport.vatDeductibleD300),
    },
  ] : [];

  const cellAmt = (v: string) => (isZero(parseDec(v)) ? <span className="muted">—</span> : fmtRON(v));

  // ── Empty state (fără companie) ───────────────────────────────────────────
  if (!activeCompanyId) {
    return (
      <div className="main-inner wide pg-gl">
        <div className="page-head"><div><h1>{t("gl.title")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("gl.noCompany")}
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide pg-gl">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("gl.title")}</h1>
          <p className="sub">
            {periodLabel} · {t("gl.sub")}
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
              <MonthPicker
                year={selectedYear}
                month={selectedMonth}
                monthsFull={MONTHS}
                prevYearLabel={t("declarations.periodPop.prevYear")}
                nextYearLabel={t("declarations.periodPop.nextYear")}
                onPrevYear={() => setSelectedYear(selectedYear - 1)}
                onNextYear={() => setSelectedYear(selectedYear + 1)}
                onPick={(m) => { setSelectedMonth(m); setOpenPop(""); }}
              />
            )}
          </div>
          <button
            className={`btn-dark spin-btn${generating ? " spinning" : ""}`}
            disabled={generating}
            onClick={() => void handleGenerate()}
          >
            <Ic name="sync" />
            {generating ? t("gl.head.generating") : t("gl.head.generate", { month: monthInline })}
          </button>
        </div>
      </div>

      {/* tabs */}
      <div className="tabs" style={{ display: "inline-flex", marginBottom: 16 }}>
        {TABS.map((label, i) => (
          <div key={label} className={`tab${tab === i ? " active" : ""}`} onClick={() => setTab(i)}>
            {label}
          </div>
        ))}
      </div>

      {/* ── 1. REGISTRU-JURNAL ─────────────────────────────────────────────── */}
      <div className={`panel${tab === 0 ? " show" : ""}`}>
        {postResult && (
          <div className={`banner ${postResult.skippedReceived > 0 ? "warn" : "ok"}`} style={{ marginBottom: 14 }}>
            <InlineIc path={postResult.skippedReceived > 0 ? WARN_TRI : CIRCLE_CHECK} />
            <span>
              <b>{t("gl.banner.generatedFor", { month: monthInline })}</b>{" "}
              {t("gl.banner.stats", { journals: postResult.journalsInserted, entries: postResult.entriesInserted })}
              {postResult.journalsReplaced > 0 && <> · {t("gl.banner.regenerated", { n: postResult.journalsReplaced })}</>}.
              {postResult.skippedReceived > 0 && (
                <>
                  {" "}<b className="neg">
                    {t("gl.banner.skipped", { count: postResult.skippedReceived })}
                  </b>{" "}
                  {t("gl.banner.noBreakdown")}{" "}
                  {(postResult.skippedReceivedRefs ?? []).slice(0, 5).map((ref, i) => (
                    <span key={ref}>{i > 0 && ", "}<span className="doc">{ref}</span></span>
                  ))}
                  {postResult.skippedReceived > 5 && " …"} {t("gl.banner.skippedAction")}
                </>
              )}
            </span>
          </div>
        )}
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">{t("gl.journal.title", { period: periodLabel })}</div>
            <div className="spacer" />
            <div className="scr-search" style={{ width: 190 }}>
              <Ic name="lens" />
              <input
                type="text"
                placeholder={t("gl.journal.search")}
                value={jrQuery}
                onChange={(e) => { setJrQuery(e.target.value); setJrPage(1); }}
              />
            </div>
            {/* propunere — neimplementat (nu există API de export registru-jurnal) */}
            <button className="pill-btn" onClick={() => notify.info(t("gl.common.soon"))}>
              <Ic name="dl" />{t("gl.common.export")}
            </button>
          </div>
          {loadingJr ? (
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("gl.common.loading")}</div>
          ) : !journalReg || journalReg.rows.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {t("gl.journal.empty", { month: monthInline })}
            </div>
          ) : jrRows.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {t("gl.journal.emptySearch")}
            </div>
          ) : (
            <>
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>{t("gl.journal.th.nr")}</th><th>{t("gl.journal.th.date")}</th><th>{t("gl.journal.th.document")}</th><th>{t("gl.journal.th.explanation")}</th>
                    <th>{t("gl.journal.th.debitAcc")}</th><th>{t("gl.journal.th.creditAcc")}</th>
                    <th className="r">{t("gl.journal.th.debitAmt")}</th><th className="r">{t("gl.journal.th.creditAmt")}</th>
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
                    <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                      <td colSpan={6}>
                        {t("gl.journal.total")} {journalReg.balanced
                          ? t("gl.journal.balanced")
                          : <span className="neg">{t("gl.journal.unbalanced")}</span>}
                      </td>
                      <td className="r num">{fmtRON(journalReg.totalDebit)}</td>
                      <td className="r num">{fmtRON(journalReg.totalCredit)}</td>
                    </tr>
                  )}
                </tbody>
              </table>
              <div className="pager">
                <span>
                  {t("gl.journal.pagerShowing")} <b>{(jrPageSafe - 1) * JR_PAGE_SIZE + 1}–{Math.min(jrPageSafe * JR_PAGE_SIZE, jrRows.length)}</b>{" "}
                  {t("gl.journal.pagerOf")} <b>{jrRows.length}</b> {t("gl.journal.pagerNotes", { month: monthInline, year: selectedYear })}
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
            <div className="tt">{t("gl.balance.title", { date: fmtRoDate(dateTo) })}</div>
            <div className="spacer" />
            {/* propunere — neimplementat (nu există API de export balanță XLSX) */}
            <button className="pill-btn" onClick={() => notify.info(t("gl.common.soon"))}>
              <Ic name="dl" />{t("gl.common.exportXlsx")}
            </button>
          </div>
          {loadingTb ? (
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("gl.common.loading")}</div>
          ) : !trialBal || trialBal.rows.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {t("gl.balance.empty")}
            </div>
          ) : (
            <>
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>{t("gl.balance.th.account")}</th>
                    <th className="r">{t("gl.balance.th.siD")}</th><th className="r">{t("gl.balance.th.siC")}</th>
                    <th className="r">{t("gl.balance.th.rulajD")}</th><th className="r">{t("gl.balance.th.rulajC")}</th>
                    <th className="r">{t("gl.balance.th.totalD")}</th><th className="r">{t("gl.balance.th.totalC")}</th>
                    <th className="r">{t("gl.balance.th.sfD")}</th><th className="r">{t("gl.balance.th.sfC")}</th>
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
                  <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                    <td>{t("gl.balance.total")}</td>
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
        {/* WF-10 / WF-08: guided close checklist — the recommended ordered sequence + the one hard
            ordering rule. Steps are idempotent, so this is guidance, not a gate. */}
        <div className="scr-card" style={{ marginBottom: 12, padding: 14 }}>
          <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>
            {t("gl.closings.checklistTitle", { period: periodLabel })}
          </div>
          <div className="hint" style={{ marginBottom: 10 }}>{t("gl.closings.checklistHint")}</div>
          <ol style={{ margin: 0, paddingLeft: 18, fontSize: 12.5, lineHeight: 1.7 }}>
            <li>{t("gl.closings.checklistS1")}</li>
            <li>{t("gl.closings.checklistS2")}</li>
            <li>{t("gl.closings.checklistS3")}</li>
            <li>{t("gl.closings.checklistS4")}</li>
            <li>{t("gl.closings.checklistS5")}</li>
            <li>{t("gl.closings.checklistS6")}</li>
            <li>{t("gl.closings.checklistS7")}</li>
            <li>{t("gl.closings.checklistS8")}</li>
          </ol>
          <div
            className="hint"
            style={{ marginTop: 10, display: "flex", gap: 6, alignItems: "flex-start", color: "var(--red)" }}
          >
            <Ic name="triangle" />
            <span>{t("gl.closings.checklistOrderWarn")}</span>
          </div>
        </div>
        <div className="cols-2-even">
          {/* Închidere TVA */}
          <div className="scr-card close-card">
            <div className="scr-toolbar">
              <div className="tt">{t("gl.closings.vatTitle", { period: periodLabel })}</div>
              <div className="spacer" />
              {vatClose
                ? vatClose.posted
                  ? <ChipPaid label={t("gl.chip.run")} />
                  : <span className="chip sent"><Ic name="dot" cls="sic" />{t("gl.chip.nothingToSettle")}</span>
                : <ChipWait label={t("gl.chip.notRun")} />}
            </div>
            <div className="crow">
              <div>
                <div className="c1">{t("gl.closings.vatCollected")}</div>
                <div className="c2"><span className="doc">4427</span> {vatClose?.posted ? t("gl.closings.closed") : t("gl.closings.periodBalance")}</div>
              </div>
              <span className="amt num">{vatClose ? fmtRON(vatClose.collected) : "—"}</span>
            </div>
            <div className="crow">
              <div>
                <div className="c1">{t("gl.closings.vatDeductible")}</div>
                <div className="c2"><span className="doc">4426</span> {vatClose?.posted ? t("gl.closings.closed") : t("gl.closings.periodBalance")}</div>
              </div>
              <span className="amt num">{vatClose ? fmtRON(vatClose.deductible) : "—"}</span>
            </div>
            <div className="crow">
              <div>
                <div className="c1">
                  {vatClose && parseDec(vatClose.deRecuperat) > 0 ? t("gl.closings.vatRecover") : t("gl.closings.vatPay")}
                </div>
                <div className="c2">
                  <span className="doc">{vatClose && parseDec(vatClose.deRecuperat) > 0 ? "4424" : "4423"}</span>
                  {" "}{t("gl.closings.dueDate", { date: vatDueLabel })}
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
            <div className="crow" style={{ background: "var(--bg-table-header)" }}>
              <div className="c2" style={{ margin: 0 }}>
                <Trans i18nKey="gl.closings.vatNote" components={{ doc: <span className="doc" /> }} />
              </div>
              <button
                className="btn-dark"
                style={{ marginLeft: "auto", height: 30, fontSize: 12.5, flex: "none" }}
                disabled={closing}
                onClick={() => void handleCloseVat()}
              >
                {closing ? t("gl.closings.closingBtn") : vatClose?.posted ? t("gl.closings.runAgain") : t("gl.closings.runNow")}
              </button>
            </div>
          </div>

          {/* Închidere rezultat */}
          <div className="scr-card close-card">
            <div className="scr-toolbar">
              <div className="tt">{t("gl.closings.resultTitle", { period: periodLabel })}</div>
              <div className="spacer" />
              {pnl
                ? parseDec(pnl.netResult) >= 0 ? <ChipPaid label={t("gl.chip.profit")} /> : <ChipLate label={t("gl.chip.loss")} />
                : <ChipWait label={loadingPnl ? t("gl.chip.calculating") : t("gl.chip.notCalculated")} />}
            </div>
            <div className="crow">
              <div>
                <div className="c1">{t("gl.closings.revenuesToClose")}</div>
                <div className="c2"><span className="doc">70x–76x</span> = <span className="doc">121</span></div>
              </div>
              <span className="amt num">{pnl ? fmtRON(pnl.totalRevenue) : "—"}</span>
            </div>
            <div className="crow">
              <div>
                <div className="c1">{t("gl.closings.expensesToClose")}</div>
                <div className="c2"><span className="doc">121</span> = <span className="doc">60x–68x</span></div>
              </div>
              <span className="amt num">{pnl ? fmtRON(pnl.totalExpense) : "—"}</span>
            </div>
            <div className="crow">
              <div>
                <div className="c1">{t("gl.closings.netResult")} {pnl && parseDec(pnl.netResult) >= 0 ? t("gl.closings.profitParen") : pnl ? t("gl.closings.lossParen") : ""}</div>
                <div className="c2"><span className="doc">121</span> {pnl && parseDec(pnl.netResult) >= 0 ? t("gl.closings.creditBalance") : t("gl.closings.debitBalance")}</div>
              </div>
              <span className={`amt num${pnl ? (parseDec(pnl.netResult) >= 0 ? " pos" : " neg") : ""}`}>
                {pnl ? `${parseDec(pnl.netResult) >= 0 ? "+" : ""}${fmtRON(pnl.netResult)}` : "—"}
              </span>
            </div>
            <div className="crow" style={{ background: "var(--bg-table-header)" }}>
              <div className="c2" style={{ margin: 0 }}>
                <Trans i18nKey="gl.closings.postNote" components={{ doc: <span className="doc" /> }} />
              </div>
              <button
                className="btn-dark"
                style={{ marginLeft: "auto", height: 30, fontSize: 12.5, flex: "none" }}
                disabled={closingPeriod}
                onClick={() => void handleClosePeriod()}
              >
                {closingPeriod ? t("gl.closings.closingBtn") : t("gl.closings.closeBtn")}
              </button>
            </div>
            <div className="crow" style={{ background: "var(--bg-table-header)" }}>
              <div className="c2" style={{ margin: 0 }}>
                <Trans i18nKey="gl.closings.annualNote" components={{ doc: <span className="doc" /> }} />
              </div>
              <button
                className="pill-btn"
                style={{ marginLeft: "auto", flex: "none" }}
                onClick={() => void handleAnnualClose()}
              >
                {t("gl.closings.runAnnual")}
              </button>
            </div>
          </div>

          {/* Impozit */}
          <div className="scr-card close-card">
            <div className="scr-toolbar">
              <div className="tt">{t("gl.closings.taxTitle", { period: periodLabel })}</div>
              <div className="spacer" />
              {pnl
                ? pnl.incomeTaxEstimated ? <ChipWait label={t("gl.chip.estimated")} /> : <ChipPaid label={t("gl.chip.recorded")} />
                : <ChipWait label={loadingPnl ? t("gl.chip.calculating") : t("gl.chip.notCalculated")} />}
            </div>
            <div className="crow">
              <div>
                <div className="c1">{t("gl.closings.taxMicroLabel")}</div>
                <div className="c2">
                  <span className="doc">698</span> = <span className="doc">4418</span>
                  {pnl && pnl.taxRegime === "micro"
                    ? <> {t("gl.closings.taxMicroBase", { amount: fmtRON(pnl.totalRevenue) })}</>
                    : <> {t("gl.closings.taxNotApplicableProfit")}</>}
                </div>
              </div>
              <span className={`amt num${pnl && pnl.taxRegime !== "micro" ? " muted" : ""}`}>
                {pnl ? (pnl.taxRegime === "micro" ? fmtRON(pnl.incomeTax) : "—") : "—"}
              </span>
            </div>
            <div className="crow">
              <div>
                <div className="c1">{t("gl.closings.taxProfitLabel")}</div>
                <div className="c2">
                  <span className="doc">691</span> = <span className="doc">4411</span>
                  {pnl && pnl.taxRegime !== "micro"
                    ? <> {t("gl.closings.taxProfitBase", { amount: fmtRON(pnl.grossResult) })}</>
                    : <> {t("gl.closings.taxNotApplicableMicro")}</>}
                </div>
              </div>
              <span className={`amt num${pnl && pnl.taxRegime === "micro" ? " muted" : ""}`}>
                {pnl ? (pnl.taxRegime !== "micro" ? fmtRON(pnl.incomeTax) : "—") : "—"}
              </span>
            </div>
            <div className="crow" style={{ background: "var(--bg-table-header)" }}>
              <div className="c2" style={{ margin: 0 }}>
                <Trans i18nKey="gl.closings.taxNote" components={{ doc: <span className="doc" /> }} />
              </div>
              <button
                className="btn-dark"
                style={{ marginLeft: "auto", height: 30, fontSize: 12.5, flex: "none" }}
                onClick={() => void handleIncomeTax()}
              >
                {t("gl.closings.postTaxBtn")}
              </button>
            </div>
          </div>

          {/* Reevaluare valutară (P1 Wave 7) */}
          <div className="scr-card close-card">
            <div className="scr-toolbar">
              <div className="tt">{t("gl.fxReval.title", { period: periodLabel })}</div>
              <div className="spacer" />
              {fxRevalResult
                ? fxRevalResult.rowsPosted > 0
                  ? <ChipPaid label={t("gl.fxReval.chipPosted", { n: fxRevalResult.rowsPosted })} />
                  : <ChipWait label={t("gl.fxReval.chipNone")} />
                : <ChipWait label={t("gl.fxReval.chipNotRun")} />}
            </div>
            <div className="crow">
              <div>
                <div className="c1">{t("gl.fxReval.favorable")}</div>
                <div className="c2"><span className="doc">4111</span> / <span className="doc">401</span> → <span className="doc">765</span></div>
              </div>
              <span className="amt num pos">
                {fxRevalResult && fxRevalResult.rowsPosted > 0 ? `+${fmtRON(fxRevalResult.totalFavorable)}` : "—"}
              </span>
            </div>
            <div className="crow">
              <div>
                <div className="c1">{t("gl.fxReval.unfavorable")}</div>
                <div className="c2"><span className="doc">665</span> → <span className="doc">4111</span> / <span className="doc">401</span></div>
              </div>
              <span className="amt num neg">
                {fxRevalResult && fxRevalResult.rowsPosted > 0 ? `-${fmtRON(fxRevalResult.totalUnfavorable)}` : "—"}
              </span>
            </div>
            <div className="crow">
              <div>
                <div className="c1">{t("gl.fxReval.net")}</div>
                <div className="c2">{t("gl.fxReval.netNote")}</div>
              </div>
              <span className={`amt num${fxRevalResult && fxRevalResult.rowsPosted > 0
                ? (parseFloat(fxRevalResult.netDiff) >= 0 ? " pos" : " neg")
                : ""}`}>
                {fxRevalResult && fxRevalResult.rowsPosted > 0
                  ? `${parseFloat(fxRevalResult.netDiff) >= 0 ? "+" : ""}${fmtRON(fxRevalResult.netDiff)}`
                  : "—"}
              </span>
            </div>
            <div className="crow" style={{ background: "var(--bg-table-header)" }}>
              <div className="c2" style={{ margin: 0 }}>
                {t("gl.fxReval.note")}
              </div>
              <button
                className="btn-dark"
                style={{ marginLeft: "auto", height: 30, fontSize: 12.5, flex: "none" }}
                disabled={fxRevalRunning}
                onClick={() => void handleFxReval()}
              >
                {fxRevalRunning ? t("gl.fxReval.running") : (fxRevalResult?.rowsPosted ?? 0) > 0 ? t("gl.fxReval.runAgain") : t("gl.fxReval.run")}
              </button>
            </div>

            {/* Tabel detaliu per factură */}
            {fxRevalRows && fxRevalRows.length > 0 && (
              <div style={{ marginTop: 8, overflowX: "auto" }}>
                <table className="scr-table" style={{ fontSize: 12 }}>
                  <thead>
                    <tr>
                      <th>{t("gl.fxReval.th.kind")}</th>
                      <th>{t("gl.fxReval.th.currency")}</th>
                      <th className="num">{t("gl.fxReval.th.outstanding")}</th>
                      <th className="num">{t("gl.fxReval.th.priorRate")}</th>
                      <th className="num">{t("gl.fxReval.th.monthEndRate")}</th>
                      <th className="num">{t("gl.fxReval.th.priorLei")}</th>
                      <th className="num">{t("gl.fxReval.th.revaluedLei")}</th>
                      <th className="num">{t("gl.fxReval.th.diffLei")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {fxRevalRows.map((row) => (
                      <tr key={row.id}>
                        <td>
                          <span className="doc">
                            {row.invoiceKind === "ISSUED" ? "4111" : "401"}
                          </span>
                        </td>
                        <td>{row.currency}</td>
                        <td className="num">{Number(row.foreignOutstanding).toFixed(2)}</td>
                        <td className="num">{Number(row.priorRate).toFixed(4)}</td>
                        <td className="num">{Number(row.monthEndRate).toFixed(4)}</td>
                        <td className="num">{fmtRON(row.priorLei)}</td>
                        <td className="num">{fmtRON(row.revaluedLei)}</td>
                        <td className={`num${parseFloat(row.diffLei) >= 0 ? " pos" : " neg"}`}>
                          {parseFloat(row.diffLei) >= 0 ? "+" : ""}{fmtRON(row.diffLei)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
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
                {t("gl.reconcile.bannerCount", { count: reconcileReport.discrepancies.length })}
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
            <span><Trans i18nKey="gl.reconcile.bannerOk" components={{ b: <b /> }} values={{ period: `${monthInline} ${selectedYear}` }} /></span>
          </div>
        )}
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">{t("gl.reconcile.title", { period: periodLabel })}</div>
            <div className="spacer" />
            <button
              className={`pill-btn spin-btn${reconciling ? " spinning" : ""}`}
              disabled={reconciling}
              onClick={() => void runReconcile(true)}
            >
              <Ic name="sync" />{reconciling ? t("gl.reconcile.checking") : t("gl.reconcile.rerun")}
            </button>
          </div>
          {reconciling && !reconcileReport ? (
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("gl.reconcile.verifying")}</div>
          ) : !reconcileReport ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {t("gl.reconcile.emptyHint")}
            </div>
          ) : (
            <>
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>{t("gl.reconcile.th.indicator")}</th><th>{t("gl.reconcile.th.d300Row")}</th>
                    <th className="r">{t("gl.reconcile.th.gl")}</th><th className="r">{t("gl.reconcile.th.d300")}</th>
                    <th className="r">{t("gl.reconcile.th.diff")}</th><th>{t("gl.reconcile.th.status")}</th>
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
                        <td>{isZero(diff) ? <ChipPaid label={t("gl.chip.ok")} /> : <ChipLate label={t("gl.chip.discrepancy")} />}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              <div className="eq-row">
                <span className={`eq${reconcileReport.balanced ? "" : " bad"}`}>
                  <InlineIc path={reconcileReport.balanced ? EQ_CHECK : WARN_TRI} cls="sic" />
                  {t("gl.reconcile.totalEq", {
                    debit: fmtRON(reconcileReport.totalDebit),
                    eq: reconcileReport.balanced ? "=" : "≠",
                    credit: fmtRON(reconcileReport.totalCredit),
                  })}
                </span>
                <span className={`eq${reconcileReport.discrepancies.length === 0 ? "" : " bad"}`}>
                  <InlineIc path={reconcileReport.discrepancies.length === 0 ? EQ_CHECK : WARN_TRI} cls="sic" />
                  {reconcileReport.discrepancies.length === 0
                    ? t("gl.reconcile.noDiscrepancies")
                    : t("gl.reconcile.discrepancyCount", { count: reconcileReport.discrepancies.length })}
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
              <div className="tt">{t("gl.bilant.entityTitle", { year: selectedYear })}</div>
              <div className="spacer" />
              {bilant
                ? bilant.balanced ? <ChipPaid label={t("gl.chip.balanced")} /> : <ChipLate label={t("gl.chip.unbalanced")} />
                : <ChipWait label={loadingBilant ? t("gl.chip.calculating") : t("gl.chip.notCalculated")} />}
            </div>
            <div className="card-pad">
              {loadingBilant ? (
                <div style={{ fontSize: 13, color: "var(--text-2)" }}>{t("gl.common.loading")}</div>
              ) : !bilant ? (
                <div style={{ fontSize: 13, color: "var(--text-2)" }}>
                  {t("gl.bilant.empty")}
                </div>
              ) : (
                <>
                  <div className="crit">
                    <InlineIc path={CIRCLE_CHECK} cls="sic" />
                    <div>{t("gl.bilant.totalAssets")} <span className="muted">{t("gl.bilant.sizeCriterion")}</span></div>
                    <span className="cv num">{fmtRON(bilant.totalAssets)} {t("gl.bilant.lei")}</span>
                  </div>
                  <div className="crit">
                    <InlineIc path={CIRCLE_CHECK} cls="sic" />
                    <div>{t("gl.bilant.equity")} <span className="muted">{t("gl.bilant.inclResult")}</span></div>
                    <span className="cv num">{fmtRON(bilant.equity)} {t("gl.bilant.lei")}</span>
                  </div>
                  <div className="crit">
                    <InlineIc path={CIRCLE_CHECK} cls="sic" />
                    <div>{t("gl.bilant.yearResult")} <span className="muted">{t("gl.bilant.balance121")}</span></div>
                    <span className={`cv num ${parseDec(bilant.currentResult) >= 0 ? "pos" : "neg"}`}>
                      {parseDec(bilant.currentResult) >= 0 ? "+" : ""}{fmtRON(bilant.currentResult)} {t("gl.bilant.lei")}
                    </span>
                  </div>
                  <div className={`banner ${bilant.balanced ? "ok" : "warn"}`} style={{ margin: "14px 0 0" }}>
                    <InlineIc path={bilant.balanced ? CIRCLE_CHECK : WARN_TRI} />
                    <span>
                      {bilant.entitySizeNote && <><b>{bilant.entitySizeNote}</b>{" "}</>}
                      {bilant.balanced
                        ? <>{t("gl.bilant.balancedNote", { amount: fmtRON(bilant.totalAssets) })}</>
                        : <Trans i18nKey="gl.bilant.unbalancedNote" components={{ b: <b /> }} />}
                    </span>
                  </div>
                </>
              )}
            </div>
          </div>

          <div className="scr-card">
            <div className="scr-toolbar"><div className="tt">{t("gl.bilant.exportTitle")}</div></div>
            <div className="card-pad">
              <dl className="kv" style={{ gridTemplateColumns: "150px 1fr", fontSize: 12.5, marginBottom: 14 }}>
                <dt>S1005</dt><dd>{t("gl.bilant.s1005")}</dd>
                <dt>S1003</dt><dd>{t("gl.bilant.s1003")}</dd>
                <dt>S1002</dt><dd>{t("gl.bilant.s1002")}</dd>
              </dl>
              <button
                className="btn-dark"
                style={{ width: "100%", justifyContent: "center" }}
                onClick={() => setShowBilantExport(true)}
              >
                <Ic name="code" />{t("gl.bilant.generateBtn", { year: selectedYear })}
              </button>
              <p className="muted" style={{ fontSize: 11.5, marginTop: 10, lineHeight: 1.5 }}>
                {t("gl.bilant.exportHint")}
              </p>
            </div>
          </div>
        </div>

        {/* Bilanț contabil (sinteză) — real feature kept (the prototype lacks it) */}
        {bilant && (
          <div className="scr-card" style={{ marginTop: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("gl.bilant.synthTitle", { date: fmtD(bilant.periodTo) })}</div>
              <div className="spacer" />
              {bilant.balanced
                ? <ChipPaid label={t("gl.chip.assetsEq")} />
                : <ChipLate label={t("gl.chip.notVerified")} />}
            </div>
            <div className="cols-2-even" style={{ gap: 0 }}>
              <table className="scr-table">
                <thead>
                  <tr><th>{t("gl.bilant.th.assets")}</th><th className="r">{t("gl.bilant.th.balance")}</th></tr>
                </thead>
                <tbody>
                  {([
                    [t("gl.bilant.rows.immobilized"), bilant.immobilizedAssets],
                    [t("gl.bilant.rows.inventory"), bilant.inventory],
                    [t("gl.bilant.rows.receivables"), bilant.receivables],
                    [t("gl.bilant.rows.shortInvestments"), bilant.shortInvestments],
                    [t("gl.bilant.rows.cashBank"), bilant.cashBank],
                    [t("gl.bilant.rows.prepaidExpenses"), bilant.prepaidExpenses],
                  ] as Array<[string, string]>).map(([label, v]) => (
                    <tr key={label}>
                      <td>{label}</td>
                      <td className="r num">{cellAmt(v)}</td>
                    </tr>
                  ))}
                  <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                    <td>{t("gl.bilant.rows.totalAssets")}</td>
                    <td className="r num">{fmtRON(bilant.totalAssets)}</td>
                  </tr>
                </tbody>
              </table>
              <table className="scr-table">
                <thead>
                  <tr><th>{t("gl.bilant.th.equityLiab")}</th><th className="r">{t("gl.bilant.th.balance")}</th></tr>
                </thead>
                <tbody>
                  {([
                    [t("gl.bilant.rows.equityIncl"), bilant.equity],
                    [t("gl.bilant.rows.ofWhichResult"), bilant.currentResult],
                    [t("gl.bilant.rows.provisions"), bilant.provisions],
                    [t("gl.bilant.rows.longTermDebt"), bilant.longTermDebt],
                    [t("gl.bilant.rows.currentLiabilities"), bilant.currentLiabilities],
                    [t("gl.bilant.rows.deferredRevenue"), bilant.deferredRevenue],
                  ] as Array<[string, string]>).map(([label, v]) => (
                    <tr key={label}>
                      <td>{label}</td>
                      <td className="r num">{cellAmt(v)}</td>
                    </tr>
                  ))}
                  <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                    <td>{t("gl.bilant.rows.totalEquityLiab")}</td>
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
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("gl.common.loading")}</div>
          </div>
        ) : !ledger || ledger.length === 0 ? (
          <div className="scr-card">
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {t("gl.ledger.empty")}
            </div>
          </div>
        ) : (
          ledger.map((a) => (
            <div key={a.accountCode} className="scr-card" style={{ marginBottom: 14 }}>
              <div className="scr-toolbar">
                <div className="tt"><span className="doc">{a.accountCode}</span> {a.accountName}</div>
                <div className="spacer" />
                <span className="muted" style={{ fontSize: 12 }}>
                  {t("gl.ledger.openingBalance")}{" "}
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
                    <th>{t("gl.ledger.th.date")}</th><th>{t("gl.ledger.th.document")}</th><th>{t("gl.ledger.th.explanation")}</th><th>{t("gl.ledger.th.contra")}</th>
                    <th className="r">{t("gl.ledger.th.debit")}</th><th className="r">{t("gl.ledger.th.credit")}</th><th className="r">{t("gl.ledger.th.balance")}</th>
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
                  <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                    <td colSpan={4}>{t("gl.ledger.totalRow")}</td>
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
            <div className="tt">{t("gl.pnl.title", { period: periodLabel })}</div>
            <div className="spacer" />
            <span className="muted" style={{ fontSize: 12 }}>
              {pnl
                ? pnl.taxRegime === "micro" ? t("gl.pnl.regimeMicro") : t("gl.pnl.regimeProfit")
                : ""}{pnl ? " · OMFP 1802/2014" : ""}
            </span>
            {pnl && (
              parseDec(pnl.netResult) >= 0
                ? <ChipPaid label={t("gl.pnl.netChip", { amount: `+${fmtRON(pnl.netResult)}` })} />
                : <ChipLate label={t("gl.pnl.netChip", { amount: fmtRON(pnl.netResult) })} />
            )}
          </div>
          {loadingPnl ? (
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("gl.common.loading")}</div>
          ) : !pnl ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {t("gl.pnl.empty")}
            </div>
          ) : (
            <>
              <table className="scr-table">
                <tbody>
                  <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                    <td>{t("gl.pnl.revenues")}</td>
                    <td className="r num">{fmtRON(pnl.totalRevenue)}</td>
                  </tr>
                  {pnl.revenueLines.map((l) => (
                    <tr key={l.code}>
                      <td style={{ paddingLeft: 32 }}><span className="doc">{l.code}</span> {l.name}</td>
                      <td className="r num">{fmtRON(l.amount)}</td>
                    </tr>
                  ))}
                  <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                    <td>{t("gl.pnl.expenses")}</td>
                    <td className="r num">{fmtRON(pnl.totalExpense)}</td>
                  </tr>
                  {pnl.expenseLines.map((l) => (
                    <tr key={l.code}>
                      <td style={{ paddingLeft: 32 }}><span className="doc">{l.code}</span> {l.name}</td>
                      <td className="r num">{fmtRON(l.amount)}</td>
                    </tr>
                  ))}
                  <tr style={{ fontWeight: 600 }}>
                    <td>{t("gl.pnl.grossResult")}</td>
                    <td className="r num">{fmtRON(pnl.grossResult)}</td>
                  </tr>
                  <tr>
                    <td>
                      {pnl.taxRegime === "micro" ? t("gl.pnl.taxIncome") : t("gl.pnl.taxProfit")}
                      {" "}{pnl.incomeTaxEstimated ? t("gl.pnl.estimatedParen") : t("gl.pnl.recordedParen")}
                    </td>
                    <td className="r num">{fmtRON(pnl.incomeTax)}</td>
                  </tr>
                  <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                    <td>{t("gl.pnl.netResult")}</td>
                    <td className={`r num ${parseDec(pnl.netResult) >= 0 ? "pos" : "neg"}`}>
                      {fmtRON(pnl.netResult)}
                    </td>
                  </tr>
                </tbody>
              </table>
              {pnl.incomeTaxEstimated && (
                <div className="pager">
                  <span>
                    {t("gl.pnl.estimateNote", {
                      basis: pnl.taxRegime === "micro" ? t("gl.pnl.basisMicro") : t("gl.pnl.basisProfit"),
                      rows: pnl.closingEntries.length,
                    })}
                  </span>
                  <span></span>
                </div>
              )}
            </>
          )}
        </div>
      </div>

      {/* ── 8. FIȘĂ PARTENER ──────────────────────────────────────────────── */}
      <div className={`panel${tab === 7 ? " show" : ""}`}>
        <div className="scr-card" style={{ marginBottom: 14, padding: 14 }}>
          <select
            className="select"
            style={{ width: "100%", maxWidth: 480 }}
            value={partnerCui}
            onChange={(e) => {
              const cui = e.target.value;
              setPartnerCui(cui);
              if (cui) void loadPartnerLedgerForCui(cui);
              else setPartnerLedger(null);
            }}
          >
            <option value="">{t("gl.partner.selectPlaceholder")}</option>
            {(partnerContacts ?? []).map((c) => (
              <option key={c.id} value={c.cui!}>
                {c.legalName}{c.cui ? ` (${c.cui})` : ""}
              </option>
            ))}
          </select>
        </div>

        {!partnerCui ? (
          <div className="scr-card">
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {t("gl.partner.empty")}
            </div>
          </div>
        ) : loadingPartner ? (
          <div className="scr-card">
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("gl.partner.loading")}</div>
          </div>
        ) : !partnerLedger || partnerLedger.length === 0 ? (
          <div className="scr-card">
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {t("gl.partner.noMovements")}
            </div>
          </div>
        ) : (
          partnerLedger.map((a) => (
            <div key={a.accountCode} className="scr-card" style={{ marginBottom: 14 }}>
              <div className="scr-toolbar">
                <div className="tt"><span className="doc">{a.accountCode}</span> {a.accountName}</div>
                <div className="spacer" />
                <span className="muted" style={{ fontSize: 12 }}>
                  {t("gl.ledger.openingBalance")}{" "}
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
                    <th>{t("gl.ledger.th.date")}</th><th>{t("gl.ledger.th.document")}</th><th>{t("gl.ledger.th.explanation")}</th><th>{t("gl.ledger.th.contra")}</th>
                    <th className="r">{t("gl.ledger.th.debit")}</th><th className="r">{t("gl.ledger.th.credit")}</th><th className="r">{t("gl.ledger.th.balance")}</th>
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
                  <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                    <td colSpan={4}>{t("gl.ledger.totalRow")}</td>
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

      {/* ── 9. NOTE CONTABILE MANUALE (cod 14-6-2A) ──────────────────────── */}
      <div className={`panel${tab === 8 ? " show" : ""}`}>
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">{t("gl.nc.tabLabel")} — {periodLabel}</div>
            <div className="spacer" />
            <button
              className="btn-dark"
              style={{ height: 30, fontSize: 12.5, flex: "none" }}
              onClick={() => { void loadNcAccounts(); setShowNcModal(true); }}
            >
              <Ic name="plus" />{t("gl.nc.newBtn")}
            </button>
          </div>
          {loadingNc ? (
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("gl.nc.loading")}</div>
          ) : !ncList || ncList.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {t("gl.nc.empty")}
            </div>
          ) : (
            <table className="scr-table">
              <thead>
                <tr>
                  <th>{t("gl.nc.th.date")}</th>
                  <th>{t("gl.nc.th.description")}</th>
                  <th>{t("gl.nc.th.accounts")}</th>
                  <th className="r">{t("gl.nc.th.debit")}</th>
                  <th className="r">{t("gl.nc.th.credit")}</th>
                  <th style={{ width: 36 }}>{t("gl.nc.th.actions")}</th>
                </tr>
              </thead>
              <tbody>
                {ncList.map((nc) => (
                  <tr key={nc.sourceId}>
                    <td className="num">{fmtD(nc.date)}</td>
                    <td>{nc.description || <span className="muted">—</span>}</td>
                    <td>
                      {nc.lines.map((l, i) => (
                        <span key={i}>
                          {i > 0 && " · "}
                          <span className="doc">{l.accountCode}</span>
                          {l.accountName ? <span className="muted" style={{ fontSize: 11.5 }}> {l.accountName}</span> : null}
                        </span>
                      ))}
                    </td>
                    <td className="r num">{fmtRON(nc.totalDebit)}</td>
                    <td className="r num">{fmtRON(nc.totalCredit)}</td>
                    <td style={{ textAlign: "center" }}>
                      <button
                        className="pill-btn"
                        style={{ padding: "2px 8px", fontSize: 12, color: "var(--danger, #e53e3e)" }}
                        title={t("gl.nc.deleteConfirm", { date: fmtD(nc.date) })}
                        onClick={() => void handleDeleteNc(nc)}
                      >
                        <Ic name="minus" />
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>

      {showBilantExport && (
        <BilantExportModal
          year={selectedYear}
          onClose={() => setShowBilantExport(false)}
          onExport={runBilantExport}
          onPreview={runBilantPreview}
        />
      )}

      {showNcModal && (
        <ManualJournalModal
          accounts={ncAccounts ?? []}
          onClose={() => setShowNcModal(false)}
          onSave={async (date, description, lines) => {
            if (!activeCompanyId) return;
            const id = await api.gl.createManualJournal(activeCompanyId, date, description, lines);
            notify.success(t("gl.nc.createOk", { id: id.slice(0, 8) }));
            setShowNcModal(false);
            invalidateReports();
            void loadNcList();
          }}
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
  onPreview,
}: {
  year: number;
  onClose: () => void;
  onExport: (
    caen: string, avgEmployees: number | null, formOverride: string | null, priorYearForm: string | null,
  ) => Promise<void>;
  onPreview: (
    caen: string, avgEmployees: number | null, formOverride: string | null, priorYearForm: string | null,
  ) => Promise<void>;
}) {
  const { t } = useTranslation();
  const { closing, close } = useAnimatedClose(onClose);
  const [caen, setCaen] = useState("");
  const [emp, setEmp] = useState("");
  const [form, setForm] = useState("auto");
  const [priorForm, setPriorForm] = useState("");
  const [busy, setBusy] = useState(false);
  const [previewing, setPreviewing] = useState(false);

  const submit = async () => {
    if (!/^\d{4}$/.test(caen.trim())) { notify.error(t("gl.modal.caenInvalid")); return; }
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

  const preview = async () => {
    if (!/^\d{4}$/.test(caen.trim())) { notify.error(t("gl.modal.caenInvalid")); return; }
    setPreviewing(true);
    try {
      await onPreview(
        caen.trim(),
        emp.trim() === "" ? null : Number(emp),
        form === "auto" ? null : form,
        priorForm === "" ? null : priorForm,
      );
    } finally {
      setPreviewing(false);
    }
  };

  return createPortal(
    <div
      className={`modal-back ${closing ? "closing" : "show"}`}
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget && !busy) close(); }}
    >
      <div className="modal">
        <div className="modal-head">
          <div>
            <div className="mt">{t("gl.modal.title", { year })}</div>
            <div className="ms">{t("gl.modal.subtitle")}</div>
          </div>
          <button className="modal-x" onClick={close} aria-label={t("gl.modal.close")}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="banner" style={{ marginBottom: 14 }}>
            <InlineIc path={CIRCLE_CHECK} />
            <span>
              <Trans i18nKey="gl.modal.banner" components={{ b: <b /> }} />
            </span>
          </div>
          <div className="fgrid">
            <div className="field">
              <label>{t("gl.modal.caenLabel")} <span className="req">*</span></label>
              <input
                className="input num"
                type="text"
                placeholder="6201"
                value={caen}
                onChange={(e) => setCaen(e.target.value)}
                autoFocus
              />
              <span className="hint">{t("gl.modal.caenHint")}</span>
            </div>
            <div className="field">
              <label>{t("gl.modal.empLabel")}</label>
              <input
                className="input num"
                type="text"
                inputMode="numeric"
                placeholder={t("gl.modal.empPlaceholder")}
                value={emp}
                onChange={(e) => setEmp(e.target.value)}
              />
              <span className="hint">{t("gl.modal.empHint")}</span>
            </div>
            <div className="field span2">
              <label>{t("gl.modal.formLabel")}</label>
              <select className="select" value={form} onChange={(e) => setForm(e.target.value)}>
                <option value="auto">{t("gl.modal.formAuto")}</option>
                <option value="UU">{t("gl.modal.formMicro")}</option>
                <option value="BS">{t("gl.modal.formSmall")}</option>
                <option value="BL">{t("gl.modal.formLarge")}</option>
              </select>
            </div>
            <div className="field span2">
              <label>{t("gl.modal.priorLabel", { year: year - 1 })}</label>
              <select className="select" value={priorForm} onChange={(e) => setPriorForm(e.target.value)}>
                <option value="">{t("gl.modal.priorUnknown")}</option>
                <option value="UU">{t("gl.modal.formMicro")}</option>
                <option value="BS">{t("gl.modal.formSmall")}</option>
                <option value="BL">{t("gl.modal.formLarge")}</option>
              </select>
              <span className="hint">{t("gl.modal.priorHint")}</span>
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <span className="left">{t("gl.modal.footNote")}</span>
          <button className="pill-btn" onClick={close} disabled={busy || previewing}>{t("gl.modal.cancel")}</button>
          <button className="pill-btn" disabled={busy || previewing} onClick={() => void preview()}>
            <Ic name="eye" />{previewing ? t("gl.bilant.previewing") : t("gl.bilant.previewXml")}
          </button>
          <button className="btn-dark" disabled={busy || previewing} onClick={() => void submit()}>
            <Ic name="dl" />{busy ? t("gl.modal.exporting") : t("gl.modal.generate")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ─── ManualJournalModal — editor de notă contabilă manuală (cod 14-6-2A) ─────
/**
 * Modal pentru crearea unei note contabile manuale:
 * - câmpuri: dată + descriere + linii dinamice debit/credit (minim 2)
 * - footer live: Σdebit, Σcredit, diferență; buton Save activ doar când nota e echilibrată
 */
function ManualJournalModal({
  accounts,
  onClose,
  onSave,
}: {
  accounts: Account[];
  onClose: () => void;
  onSave: (date: string, description: string, lines: ManualLineInput[]) => Promise<void>;
}) {
  const { t } = useTranslation();
  const { closing, close } = useAnimatedClose(onClose);

  const today = new Date().toISOString().slice(0, 10);
  const [date,        setDate]        = useState(today);
  const [description, setDescription] = useState("");
  const [lines, setLines] = useState<ManualLineInput[]>([
    { accountCode: "", debit: "", credit: "" },
    { accountCode: "", debit: "", credit: "" },
  ]);
  const [saving, setSaving] = useState(false);
  const [clientError, setClientError] = useState<string | null>(null);

  // ── Linie editor helpers ───────────────────────────────────────────────────
  const setLine = (i: number, patch: Partial<ManualLineInput>) =>
    setLines((ls) => ls.map((l, j) => (j === i ? { ...l, ...patch } : l)));

  const addLine = () =>
    setLines((ls) => [...ls, { accountCode: "", debit: "", credit: "" }]);

  const removeLine = (i: number) =>
    setLines((ls) => ls.filter((_, j) => j !== i));

  // ── Live balance ───────────────────────────────────────────────────────────
  const parseAmt = (s: string) => {
    const n = parseFloat(s.replace(",", "."));
    return isNaN(n) || n < 0 ? 0 : n;
  };
  const totalD = lines.reduce((s, l) => s + parseAmt(l.debit), 0);
  const totalC = lines.reduce((s, l) => s + parseAmt(l.credit), 0);
  const diff   = Math.abs(totalD - totalC);
  const balanced = diff < 0.005 && totalD > 0;

  // ── Client-side validation (mirrors backend) ───────────────────────────────
  const validate = (): string | null => {
    if (!date) return t("gl.nc.modal.errorMinLines");
    if (lines.length < 2) return t("gl.nc.modal.errorMinLines");
    for (let i = 0; i < lines.length; i++) {
      const l = lines[i];
      if (!l.accountCode) return t("gl.nc.modal.errorNoAccount");
      const d = parseAmt(l.debit);
      const c = parseAmt(l.credit);
      if (d > 0 && c > 0) return t("gl.nc.modal.errorBothSides");
      if (d === 0 && c === 0) return t("gl.nc.modal.errorDebitOrCredit");
    }
    if (!balanced) return t("gl.nc.modal.errorUnbalanced");
    return null;
  };

  const canSave = balanced && lines.every((l) => l.accountCode) && !saving;

  const handleSave = async () => {
    const err = validate();
    if (err) { setClientError(err); return; }
    setClientError(null);
    setSaving(true);
    try {
      await onSave(date, description, lines);
    } catch (e) {
      setClientError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return createPortal(
    <div
      className={`modal-back ${closing ? "closing" : "show"}`}
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget && !saving) close(); }}
    >
      <div className="modal" style={{ maxWidth: 740, width: "96vw" }}>
        <div className="modal-head">
          <div>
            <div className="mt">{t("gl.nc.modal.title")}</div>
          </div>
          <button className="modal-x" onClick={close} disabled={saving} aria-label={t("gl.nc.modal.cancel")}>
            <Ic name="xMark" />
          </button>
        </div>

        <div className="modal-body">
          {/* Date + description */}
          <div className="fgrid" style={{ marginBottom: 16 }}>
            <div className="field">
              <label>{t("gl.nc.modal.dateLabel")} <span className="req">*</span></label>
              <input
                className="input num"
                type="date"
                value={date}
                onChange={(e) => setDate(e.target.value)}
              />
            </div>
            <div className="field span2">
              <label>{t("gl.nc.modal.descLabel")}</label>
              <input
                className="input"
                type="text"
                placeholder={t("gl.nc.modal.descPlaceholder")}
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
            </div>
          </div>

          {/* Lines table */}
          <div style={{ marginBottom: 8, fontWeight: 600, fontSize: 12.5 }}>
            {t("gl.nc.modal.linesTitle")}
          </div>
          <table className="scr-table" style={{ marginBottom: 4 }}>
            <thead>
              <tr>
                <th style={{ width: "45%" }}>{t("gl.nc.modal.th.account")}</th>
                <th className="r" style={{ width: "22%" }}>{t("gl.nc.modal.th.debit")}</th>
                <th className="r" style={{ width: "22%" }}>{t("gl.nc.modal.th.credit")}</th>
                <th style={{ width: 36 }}>{t("gl.nc.modal.th.remove")}</th>
              </tr>
            </thead>
            <tbody>
              {lines.map((line, i) => (
                <tr key={i}>
                  <td>
                    <select
                      className="select"
                      style={{ width: "100%" }}
                      value={line.accountCode}
                      onChange={(e) => setLine(i, { accountCode: e.target.value })}
                    >
                      <option value="">{t("gl.nc.modal.accountPlaceholder")}</option>
                      {accounts.map((a) => (
                        <option key={a.id} value={a.accountCode}>
                          {a.accountCode} — {a.accountName}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td>
                    <input
                      className="input num"
                      type="text"
                      inputMode="decimal"
                      placeholder="0.00"
                      style={{ width: "100%", textAlign: "right" }}
                      value={line.debit}
                      onChange={(e) => setLine(i, { debit: e.target.value, credit: e.target.value ? line.credit : "" })}
                      disabled={!!line.credit && parseAmt(line.credit) > 0}
                    />
                  </td>
                  <td>
                    <input
                      className="input num"
                      type="text"
                      inputMode="decimal"
                      placeholder="0.00"
                      style={{ width: "100%", textAlign: "right" }}
                      value={line.credit}
                      onChange={(e) => setLine(i, { credit: e.target.value, debit: e.target.value ? line.debit : "" })}
                      disabled={!!line.debit && parseAmt(line.debit) > 0}
                    />
                  </td>
                  <td style={{ textAlign: "center" }}>
                    {lines.length > 2 && (
                      <button
                        className="pill-btn"
                        style={{ padding: "2px 7px", color: "var(--danger, #e53e3e)", fontSize: 12 }}
                        onClick={() => removeLine(i)}
                        tabIndex={-1}
                      >
                        <Ic name="xMark" />
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
            {/* Live balance footer */}
            <tfoot>
              <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                <td>
                  <button
                    className="pill-btn"
                    style={{ fontSize: 12 }}
                    onClick={addLine}
                  >
                    {t("gl.nc.modal.addLine")}
                  </button>
                </td>
                <td className="r num">
                  <span style={{ fontSize: 11, color: "var(--text-2)", marginRight: 4 }}>{t("gl.nc.modal.footerDebit")}</span>
                  {fmtRON(String(totalD.toFixed(2)))}
                </td>
                <td className="r num">
                  <span style={{ fontSize: 11, color: "var(--text-2)", marginRight: 4 }}>{t("gl.nc.modal.footerCredit")}</span>
                  {fmtRON(String(totalC.toFixed(2)))}
                </td>
                <td style={{ textAlign: "center" }}>
                  {balanced
                    ? <InlineIc path={CIRCLE_CHECK} style={{ color: "var(--ok, #38a169)" }} />
                    : <InlineIc path={WARN_TRI} style={{ color: "var(--warn, #d69e2e)" }} />}
                </td>
              </tr>
              {!balanced && totalD > 0 && (
                <tr style={{ background: "var(--bg-table-header)" }}>
                  <td colSpan={4} style={{ textAlign: "right", fontSize: 12, color: "var(--warn, #d69e2e)", padding: "2px 10px 6px" }}>
                    {t("gl.nc.modal.footerDiff")}: {fmtRON(String(diff.toFixed(2)))} — {t("gl.nc.modal.unbalanced")}
                  </td>
                </tr>
              )}
              {balanced && (
                <tr style={{ background: "var(--bg-table-header)" }}>
                  <td colSpan={4} style={{ textAlign: "right", fontSize: 12, color: "var(--ok, #38a169)", padding: "2px 10px 6px" }}>
                    {t("gl.nc.modal.balanced")} ✓
                  </td>
                </tr>
              )}
            </tfoot>
          </table>

          {clientError && (
            <div className="banner danger" style={{ marginTop: 10 }}>
              <InlineIc path={WARN_TRI} />
              <span>{clientError}</span>
            </div>
          )}
        </div>

        <div className="modal-foot">
          <span className="left" />
          <button className="pill-btn" onClick={close} disabled={saving}>
            {t("gl.nc.modal.cancel")}
          </button>
          <button
            className="btn-dark"
            disabled={!canSave}
            onClick={() => void handleSave()}
          >
            {saving ? t("gl.nc.modal.saving") : t("gl.nc.modal.save")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
