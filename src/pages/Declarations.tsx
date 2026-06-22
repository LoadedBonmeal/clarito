/**
 * Declarații — verbatim port of the design "Declaratii.html":
 *   .page-head (title + perioadă sub + head-actions: period pop · Calendar termene ·
 *   Recalculează toate) → .dec-grid hub cards (D300 / D390 / D394 / D406 SAF-T /
 *   D100 / D101 / e-TVA / Intrastat) with .dh/.dt/.ds/.dkv/.dfoot, status chips,
 *   Calculează/Export buttons → D300 detail section (real compute results).
 *
 * ALL wiring preserved: api.declarations.compute/export/exportD300Official,
 * api.declarations.preflight, api.declarations.intrastatStatus, manualDeductible
 * override, regularizări R16/R30 (Wave 8), PreflightPanel, DUK block + "Exportă
 * oricum", D300SubmissionModal, year/month selectors, company guard.
 * D390/D394/D406/D100/D101/e-TVA cards link to their real views in /reports.
 */

import { useState, useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { MonthPicker } from "@/components/shared/MonthPicker";
import { PreflightPanel } from "@/components/shared/PreflightPanel";
import { D300SubmissionModal } from "@/components/modals/D300SubmissionModal";
import { useOpenXml } from "@/hooks/use-open-xml";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { queryKeys } from "@/lib/queries";
import type { D300Report, D300Submission } from "@/types";
import type { PreflightIssue } from "@/lib/tauri";
import type { ReportView } from "@/router";

// ─── Helpers ─────────────────────────────────────────────────────────────────

const MONTH_KEYS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"] as const;

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

/** Date → ISO yyyy-mm-dd (local). */
function toIso(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function periodDateRange(year: number, month: number): { dateFrom: string; dateTo: string } {
  const mm      = String(month).padStart(2, "0");
  const lastDay = new Date(year, month, 0).getDate();
  return {
    dateFrom: `${year}-${mm}-01`,
    dateTo:   `${year}-${mm}-${String(lastDay).padStart(2, "0")}`,
  };
}

const VAT_CAT_KEY: Record<string, string> = {
  S: "s", Z: "z", E: "e", AE: "ae", K: "k", G: "g", O: "o",
};

// Inline icons NOT in Ic (verbatim from prototype).
const WARN_PATH = '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

const numInputStyle: React.CSSProperties = {
  width: "100%", height: 32, fontSize: 12.5, padding: "0 10px",
  border: "1px solid var(--line)", borderRadius: 8, fontFamily: "var(--mono)",
};

// ─── Component ───────────────────────────────────────────────────────────────

export function DeclarationsPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const MONTHS = MONTH_KEYS.map((k) => t(`declarations.months.${k}`));
  const vatCategoryLabel = (cat: string): string =>
    VAT_CAT_KEY[cat] ? t(`declarations.vatCat.${VAT_CAT_KEY[cat]}`) : cat;

  const now = new Date();
  const [selectedYear, setSelectedYear]   = useState(now.getFullYear());
  const [selectedMonth, setSelectedMonth] = useState(now.getMonth() + 1);

  const [report,    setReport]    = useState<D300Report | null>(null);
  const [computing, setComputing] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [exportingOfficial, setExportingOfficial] = useState(false);
  const [showD300Modal,     setShowD300Modal]     = useState(false);
  const [dukBlock,          setDukBlock]          = useState<PreflightIssue[] | null>(null);
  const [lastSubmission,    setLastSubmission]    = useState<D300Submission | null>(null);
  const [previewingD300,    setPreviewingD300]    = useState(false);
  const [openPop, setOpenPop] = useState<"" | "period">("");

  const openXml = useOpenXml();

  // TVA deductibilă — pre-completată din totalDeductibleVat; editabilă manual ca override.
  const [manualDeductible, setManualDeductible] = useState<string>("0.00");

  // Wave 8: regularizări cote vechi (R16/R30) — pre-completate din report; editabile.
  const [regColectataBaza, setRegColectataBaza] = useState<string>("0");
  const [regColectataTva, setRegColectataTva]   = useState<string>("0");
  const [regDedusaBaza, setRegDedusaBaza]       = useState<string>("0");
  const [regDedusaTva, setRegDedusaTva]         = useState<string>("0");

  // Close toolbar pops on outside click
  useEffect(() => {
    if (!openPop) return;
    const h = () => setOpenPop("");
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [openPop]);

  // Fetch active company for pre-filling submission modal (bank/IBAN).
  const { data: activeCompany } = useQuery({
    queryKey: queryKeys.companies.detail(activeCompanyId ?? ""),
    queryFn: () => api.companies.get(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  useEffect(() => {
    if (report) {
      setManualDeductible(report.totalDeductibleVat);
      // Wave 8: pre-fill regularizări from auto-computed values (rounded to integer lei).
      setRegColectataBaza(String(Math.round(parseDec(report.regColectataBaza))));
      setRegColectataTva(String(Math.round(parseDec(report.regColectataTva))));
      setRegDedusaBaza(String(Math.round(parseDec(report.regDedusaBaza))));
      setRegDedusaTva(String(Math.round(parseDec(report.regDedusaTva))));
    }
  }, [report]);

  const { dateFrom, dateTo } = periodDateRange(selectedYear, selectedMonth);

  // ── Pre-export validation (preflight) ─────────────────────────────────────
  const { data: preflightIssues = [] } = useQuery({
    queryKey: ["preflight", "d300", activeCompanyId ?? "", dateFrom, dateTo],
    queryFn: () => api.declarations.preflight(activeCompanyId!, "D300", dateFrom, dateTo),
    enabled: !!activeCompanyId,
    staleTime: 30_000,
  });

  // ── Intrastat threshold monitor (real backend, same wiring as Dashboard) ──
  const currentYear = now.getFullYear();
  const { data: intrastat, refetch: refetchIntrastat, isFetching: intrastatFetching } = useQuery({
    queryKey: ["intrastatStatus", activeCompanyId, currentYear],
    enabled: !!activeCompanyId,
    staleTime: 5 * 60_000,
    queryFn: () =>
      api.declarations.intrastatStatus(activeCompanyId!, new Date().toISOString().slice(0, 10)),
  });

  // ── Istoricul depunerilor ──────────────────────────────────────────────────
  const queryClient = useQueryClient();
  const filingsQueryKey = ["declaration-filings", activeCompanyId ?? ""];
  const { data: filings = [] } = useQuery({
    queryKey: filingsQueryKey,
    queryFn: () => api.declarations.listFilings(activeCompanyId!),
    enabled: !!activeCompanyId,
    staleTime: 60_000,
  });

  const handleDeleteFiling = async (id: string) => {
    if (!activeCompanyId) return;
    if (!window.confirm(t("declarations.filings.confirmDelete"))) return;
    try {
      await api.declarations.deleteFiling(id, activeCompanyId);
      await queryClient.invalidateQueries({ queryKey: filingsQueryKey });
    } catch {
      notify.error(t("declarations.filings.deleteFailed"));
    }
  };

  // ── Calculează D300 ────────────────────────────────────────────────────────
  // Invalidate the computed report together with any DUK block + the cached
  // submission. A DUK block belongs to one computed period; if the report is
  // reset (recompute, month/year change) the stale block must go too, otherwise
  // its "Exportă oricum" button would export the previous period's submission.
  const clearReportState = () => {
    setReport(null);
    setDukBlock(null);
    setLastSubmission(null);
  };

  const handleCompute = async () => {
    if (!activeCompanyId) {
      notify.warn(t("declarations.notify.selectCompany"));
      return;
    }
    setComputing(true);
    clearReportState();
    try {
      const result = await api.declarations.compute(activeCompanyId, dateFrom, dateTo);
      if (result.invoiceCount === 0) {
        notify.info(t("declarations.notify.noData"));
      }
      setReport(result);
    } catch (err) {
      notify.error(formatError(err, t("declarations.notify.computeD300Failed")));
    } finally {
      setComputing(false);
    }
  };

  // ── Exportă D300 XML ───────────────────────────────────────────────────────
  const handleExport = async () => {
    if (!activeCompanyId) {
      notify.warn(t("declarations.notify.selectCompany"));
      return;
    }
    if (!report || (report.invoiceCount === 0 && report.purchaseInvoiceCount === 0)) {
      notify.info(t("declarations.notify.noExportData"));
      return;
    }
    const savePath = await saveDialog({
      title:       t("declarations.dialogs.saveD300"),
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
      notify.success(t("declarations.notify.d300Saved", { path: saved }));
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, t("declarations.notify.exportD300Failed")));
    } finally {
      setExporting(false);
    }
  };

  // ── Exportă D300 oficial ANAF (schema v12) ────────────────────────────────
  const handleExportOfficial = async (submission: D300Submission, override = false) => {
    if (!activeCompanyId) { notify.warn(t("declarations.notify.selectCompany")); return; }
    setLastSubmission(submission);
    const savePath = await saveDialog({
      title:       t("declarations.dialogs.saveD300Official"),
      defaultPath: `d300-${selectedYear}-${String(selectedMonth).padStart(2, "0")}.xml`,
      filters:     [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExportingOfficial(true);
    try {
      // Wave 8: merge regularizări overrides into submission.
      // Only pass non-zero values so the backend uses None (auto-computed) when 0.
      const regCB = Math.round(parseDec(regColectataBaza));
      const regCT = Math.round(parseDec(regColectataTva));
      const regDB = Math.round(parseDec(regDedusaBaza));
      const regDT = Math.round(parseDec(regDedusaTva));
      const submissionWithReg: D300Submission = {
        ...submission,
        regColectataBaza: regCB !== 0 ? regCB : null,
        regColectataTva:  regCT !== 0 ? regCT : null,
        regDedusaBaza:    regDB !== 0 ? regDB : null,
        regDedusaTva:     regDT !== 0 ? regDT : null,
      };
      const res = await api.declarations.exportD300Official(
        activeCompanyId,
        dateFrom,
        dateTo,
        savePath,
        submissionWithReg,
        override,
      );
      if (!res.written) {
        setDukBlock(res.issues);
        notify.error(t("declarations.notify.dukErrors"));
        return;
      }
      setDukBlock(null);
      notify.success(
        res.dukAvailable
          ? t("declarations.notify.d300OfficialSavedDuk", { path: res.path })
          : t("declarations.notify.d300OfficialSavedNoDuk", { path: res.path }),
      );
      try { await openPath(res.path); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, t("declarations.notify.exportD300OfficialFailed")));
    } finally {
      setExportingOfficial(false);
    }
  };

  // ── Vizualizează / Editează XML D300 ──────────────────────────────────────
  // Construiește EXACT același XML ca exportul oficial (aceiași pași de build, fără scriere/DUK gate)
  // și îl deschide în vizualizatorul/editorul XML din aplicație (cu re-validare DUK).
  const handlePreviewD300 = async (submission: D300Submission) => {
    if (!activeCompanyId) { notify.warn(t("declarations.notify.selectCompany")); return; }
    setLastSubmission(submission);
    setPreviewingD300(true);
    try {
      // Wave 8: aplică regularizările (R16/R30) ca la exportul oficial — doar valorile ne-nule.
      const regCB = Math.round(parseDec(regColectataBaza));
      const regCT = Math.round(parseDec(regColectataTva));
      const regDB = Math.round(parseDec(regDedusaBaza));
      const regDT = Math.round(parseDec(regDedusaTva));
      const submissionWithReg: D300Submission = {
        ...submission,
        regColectataBaza: regCB !== 0 ? regCB : null,
        regColectataTva:  regCT !== 0 ? regCT : null,
        regDedusaBaza:    regDB !== 0 ? regDB : null,
        regDedusaTva:     regDT !== 0 ? regDT : null,
      };
      const xml = await api.declarations.previewD300Xml(activeCompanyId, dateFrom, dateTo, submissionWithReg);
      openXml({
        xml,
        name: `d300-${selectedYear}-${String(selectedMonth).padStart(2, "0")}.xml`,
        declKind: "D300",
      });
    } catch (err) {
      notify.error(formatError(err, t("declarations.cards.d300.previewFailed")));
    } finally {
      setPreviewingD300(false);
    }
  };

  // ── Derived values ─────────────────────────────────────────────────────────

  const totalBase        = report ? parseDec(report.totalBase) : 0;
  const totalVat         = report ? parseDec(report.totalVat) : 0;
  const deductibleVat    = parseDec(manualDeductible) || 0;
  const netTvaDePlata    = totalVat - deductibleVat;

  const periodLabel = `${MONTHS[selectedMonth - 1]} ${selectedYear}`;

  // Termene scadente (real calendar math from the selected period).
  const termenLunar  = toIso(new Date(selectedYear, selectedMonth, 25));     // 25 a lunii următoare (D300/D390/D394)
  const termenD406   = toIso(new Date(selectedYear, selectedMonth + 1, 0));  // ultima zi a lunii următoare
  const quarter      = Math.ceil(selectedMonth / 3);
  const termenD100   = toIso(new Date(selectedYear, quarter * 3, 25));       // 25 a lunii după trimestru
  // Termen D101 (anul fiscal precedent = selectedYear-1): 25 IUNIE pentru exercițiile 2021-2025
  // (derogarea OUG 153/2020, ultimul an de aplicare = 2025); revine la 25 MARTIE pentru exercițiul
  // fiscal 2026 și ulterior (art. 42 Cod fiscal). Banner-ul descrie exact această regulă.
  const termenD101   = (selectedYear - 1) <= 2025
    ? toIso(new Date(selectedYear, 5, 25))   // 25 iunie (derogare OUG 153/2020)
    : toIso(new Date(selectedYear, 2, 25));  // 25 martie (general)

  const preflightErrors = preflightIssues.filter((i) => i.severity === "error").length;
  const noD300Data = !report || (report.invoiceCount === 0 && report.purchaseInvoiceCount === 0);

  // Intrastat chip from real threshold levels.
  const intrastatLevel = intrastat
    ? (intrastat.arrivals.level === "exceeded" || intrastat.dispatches.level === "exceeded"
        ? "exceeded"
        : intrastat.arrivals.level === "approaching" || intrastat.dispatches.level === "approaching"
          ? "approaching"
          : "ok")
    : null;

  const goReports = (v: ReportView) => {
    void navigate({ to: "/reports", search: { view: v } });
  };

  const exportXmlIcon = <Ic name="code" />;
  const calcIcon = <Ic name="sync" />;

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("declarations.title")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("declarations.noCompany")}
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("declarations.title")}</h1>
          <p className="sub">
            {t("declarations.subtitle", { period: periodLabel })}
          </p>
        </div>
        <div className="head-actions">
          {/* period pop (real selector — the prototype hardcodes the period) */}
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
                onPrevYear={() => { setSelectedYear(selectedYear - 1); clearReportState(); }}
                onNextYear={() => { setSelectedYear(selectedYear + 1); clearReportState(); }}
                onPick={(m) => { setSelectedMonth(m); clearReportState(); setOpenPop(""); }}
              />
            )}
          </div>
          {/* propunere — neimplementat (fără backend pentru calendar termene) */}
          <button className="pill-btn" onClick={() => notify.info(t("declarations.common.comingSoon"))}>
            <Ic name="calendar" />{t("declarations.head.deadlinesCalendar")}
          </button>
          <button
            className="btn-dark spin-btn"
            disabled={computing}
            onClick={() => { void handleCompute(); void refetchIntrastat(); }}
          >
            <Ic name="sync" />{computing ? t("declarations.head.recalcing") : t("declarations.head.recalcAll")}
          </button>
        </div>
      </div>

      <div className="dec-grid">

        {/* ── D300 ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D300</span>{t("declarations.cards.d300.title")}</div>
              <div className="ds">
                {t("declarations.cards.d300.desc1")} <b>{t("declarations.cards.d300.descBold")}</b>{" "}
                {t("declarations.cards.d300.desc2")}
              </div>
            </div>
            {dukBlock ? (
              <span className="chip late">
                <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_PATH }} />
                {t("declarations.chips.dukErrors")}
              </span>
            ) : report ? (
              <span className="chip wait"><Ic name="clock" cls="sic" />{t("declarations.chips.toFile")}</span>
            ) : (
              <span className="chip sent"><Ic name="dot" cls="sic" />{t("declarations.chips.notComputed")}</span>
            )}
          </div>
          <div className="dkv">
            <span>{t("declarations.common.period")} <b>{periodLabel}</b></span>
            <span>{t("declarations.common.due")} <b className="num">{fmtRoDate(termenLunar)}</b></span>
            <span>
              {report && netTvaDePlata < 0 ? t("declarations.kv.vatRecover") : t("declarations.kv.vatPay")}{" "}
              <b className="num">{report ? `${fmtRON(Math.abs(netTvaDePlata))} RON` : "—"}</b>
            </span>
            <span>
              {t("declarations.kv.preflight")}{" "}
              {preflightIssues.length === 0
                ? <b className="pos">{t("declarations.kv.noIssues")}</b>
                : <b className={preflightErrors > 0 ? "neg" : "pos"}>
                    {preflightErrors > 0
                      ? t("declarations.kv.errors", { count: preflightErrors })
                      : t("declarations.kv.warnings", { count: preflightIssues.length })}
                  </b>}
            </span>
          </div>
          <div className="dfoot">
            <button className="pill-btn spin-btn" disabled={computing} onClick={() => void handleCompute()}>
              {calcIcon}{computing ? t("declarations.common.calcing") : t("declarations.common.calc")}
            </button>
            <button
              className="pill-btn"
              disabled={exporting || noD300Data}
              style={exporting || noD300Data ? { opacity: 0.5, cursor: "default" } : undefined}
              title={t("declarations.cards.d300.extractTitle")}
              onClick={() => void handleExport()}
            >
              {exportXmlIcon}{exporting ? t("declarations.common.exporting") : t("declarations.common.extractXml")}
            </button>
            <span className="spacer" />
            <button
              className="pill-btn"
              disabled={previewingD300 || noD300Data || !activeCompany}
              style={previewingD300 || noD300Data || !activeCompany ? { opacity: 0.5, cursor: "default" } : undefined}
              onClick={() => setShowD300Modal(true)}
            >
              <Ic name="eye" />
              {previewingD300 ? t("declarations.cards.d300.previewing") : t("declarations.cards.d300.previewXml")}
            </button>
            <button
              className="btn-dark"
              disabled={exportingOfficial || noD300Data || !activeCompany}
              style={exportingOfficial || noD300Data || !activeCompany ? { opacity: 0.5, cursor: "default" } : undefined}
              title={t("declarations.cards.d300.officialTitle")}
              onClick={() => setShowD300Modal(true)}
            >
              {exportXmlIcon}{exportingOfficial ? t("declarations.common.exporting") : t("declarations.cards.d300.exportOfficial")}
            </button>
          </div>
        </div>

        {/* ── D390 ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D390</span>{t("declarations.cards.d390.title")}</div>
              <div className="ds">
                {t("declarations.cards.d390.descIntro")} <b>L</b> {t("declarations.cards.d390.deliveries")} ·{" "}
                <b>T</b> {t("declarations.cards.d390.triangular")} ·{" "}
                <b>A</b> {t("declarations.cards.d390.acquisitions")} · <b>P</b> {t("declarations.cards.d390.services")} ·{" "}
                <b>S</b> {t("declarations.cards.d390.servicesReceived")} ·{" "}
                <b>R</b> {t("declarations.cards.d390.adjustments")}. {t("declarations.cards.d390.descOutro")}
              </div>
            </div>
            <span className="chip wait"><Ic name="clock" cls="sic" />{t("declarations.chips.toFile")}</span>
          </div>
          <div className="dkv">
            <span>{t("declarations.common.period")} <b>{periodLabel}</b></span>
            <span>{t("declarations.common.due")} <b className="num">{fmtRoDate(termenLunar)}</b></span>
            <span>{t("declarations.kv.calcExport")} <b>{t("declarations.kv.inReports", { view: "D390" })}</b></span>
          </div>
          <div className="dfoot">
            <button className="pill-btn spin-btn" onClick={() => goReports("d390")}>
              {calcIcon}{t("declarations.common.calc")}
            </button>
            <span className="spacer" />
            {/* exportul XML real e în vizualizarea D390 din Rapoarte */}
            <button className="btn-dark" onClick={() => goReports("d390")}>
              {exportXmlIcon}{t("declarations.common.exportXml")}
            </button>
          </div>
        </div>

        {/* ── D394 ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D394</span>{t("declarations.cards.d394.title")}</div>
              <div className="ds">
                {t("declarations.cards.d394.desc")}
              </div>
            </div>
            <span className="chip wait"><Ic name="clock" cls="sic" />{t("declarations.chips.toFile")}</span>
          </div>
          <div className="dkv">
            <span>{t("declarations.common.period")} <b>{periodLabel}</b></span>
            <span>{t("declarations.common.due")} <b className="num">{fmtRoDate(termenLunar)}</b></span>
            <span>{t("declarations.kv.calcExport")} <b>{t("declarations.kv.inReports", { view: "D394" })}</b></span>
          </div>
          <div className="dfoot">
            <button className="pill-btn spin-btn" onClick={() => goReports("d394")}>
              {calcIcon}{t("declarations.common.calc")}
            </button>
            <span className="spacer" />
            {/* exportul XML real e în vizualizarea D394 din Rapoarte */}
            <button className="btn-dark" onClick={() => goReports("d394")}>
              {exportXmlIcon}{t("declarations.common.exportXml")}
            </button>
          </div>
        </div>

        {/* ── D406 SAF-T ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D406</span>{t("declarations.cards.saft.title")}</div>
              <div className="ds">
                {t("declarations.cards.saft.desc")}
              </div>
            </div>
            <span className="chip wait"><Ic name="clock" cls="sic" />{t("declarations.chips.toFile")}</span>
          </div>
          <div className="dkv">
            <span>{t("declarations.common.period")} <b>{periodLabel}</b></span>
            <span>{t("declarations.common.due")} <b className="num">{fmtRoDate(termenD406)}</b></span>
            <span>{t("declarations.cards.saft.genValidate")} <b>{t("declarations.kv.inReports", { view: "D406 SAF-T" })}</b></span>
          </div>
          <div className="dfoot">
            <button className="pill-btn spin-btn" onClick={() => goReports("saft")}>
              {calcIcon}{t("declarations.cards.saft.generate")}
            </button>
            <button className="pill-btn" onClick={() => goReports("saft")}>
              <Ic name="checkC" />{t("declarations.cards.saft.validate")}
            </button>
            <span className="spacer" />
            <button className="btn-dark" onClick={() => goReports("saft")}>
              {exportXmlIcon}{t("declarations.common.exportXml")}
            </button>
          </div>
        </div>

        {/* ── D100 ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D100</span>{t("declarations.cards.d100.title")}</div>
              <div className="ds">
                {t("declarations.cards.d100.desc1")} <b>{t("declarations.cards.d100.pos5")}</b>{" "}
                {t("declarations.cards.d100.desc2")}{" "}
                <b>{t("declarations.cards.d100.pos2")}</b>{t("declarations.cards.d100.desc3")}
              </div>
            </div>
            <span className="chip wait"><Ic name="clock" cls="sic" />{t("declarations.chips.estimated")}</span>
          </div>
          <div className="dkv">
            <span>{t("declarations.common.period")} <b>{t("declarations.kv.quarterPeriod", { q: quarter, year: selectedYear })}</b></span>
            <span>{t("declarations.common.due")} <b className="num">{fmtRoDate(termenD100)}</b></span>
            <span>{t("declarations.kv.calcExport")} <b>{t("declarations.kv.inReports", { view: "D100" })}</b></span>
          </div>
          <div className="dfoot">
            <button className="pill-btn spin-btn" onClick={() => goReports("d100")}>
              {calcIcon}{t("declarations.common.calc")}
            </button>
            <span className="spacer" />
            {/* exportul XML real e în vizualizarea D100 din Rapoarte */}
            <button className="btn-dark" onClick={() => goReports("d100")}>
              {exportXmlIcon}{t("declarations.common.exportXml")}
            </button>
          </div>
        </div>

        {/* ── D101 ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D101</span>{t("declarations.cards.d101.title")}</div>
              <div className="ds">
                {t("declarations.cards.d101.desc1")} <b>70%</b> {t("declarations.cards.d101.desc2")}
              </div>
            </div>
            <span className="chip sent"><Ic name="docText" cls="sic" />{t("declarations.chips.year", { year: selectedYear - 1 })}</span>
          </div>
          <div className="dkv">
            <span>{t("declarations.common.period")} <b>{t("declarations.chips.year", { year: selectedYear - 1 })}</b></span>
            <span>{t("declarations.common.due")} <b className="num">{fmtRoDate(termenD101)}</b></span>
            <span>{t("declarations.cards.d101.sheet")} <b>{t("declarations.kv.inReports", { view: "D101" })}</b></span>
          </div>
          <div className="dfoot">
            {/* propunere — neimplementat (recipisa ANAF nu are backend) */}
            <button className="pill-btn" onClick={() => notify.info(t("declarations.common.comingSoon"))}>
              <Ic name="eye" />{t("declarations.cards.d101.viewReceipt")}
            </button>
            <span className="spacer" />
            <button className="pill-btn" onClick={() => goReports("d101")}>
              {exportXmlIcon}{t("declarations.cards.d101.viewXml")}
            </button>
          </div>
        </div>

        {/* ── e-TVA ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">e-TVA</span>{t("declarations.cards.etva.title")}</div>
              <div className="ds">
                {t("declarations.cards.etva.desc1")}{" "}
                <b>{t("declarations.cards.etva.descBold")}</b> {t("declarations.cards.etva.desc2")}
              </div>
            </div>
            <span className="chip sent"><Ic name="dot" cls="sic" />{t("declarations.chips.toReconcile")}</span>
          </div>
          <div className="dkv">
            <span>{t("declarations.common.period")} <b>{periodLabel}</b></span>
            <span>{t("declarations.cards.etva.p300")} <b>{t("declarations.cards.etva.p300Download")}</b></span>
            <span>{t("declarations.cards.etva.recon")} <b>{t("declarations.kv.inReports", { view: "e-TVA" })}</b></span>
          </div>
          <div className="dfoot">
            <button className="pill-btn spin-btn" onClick={() => goReports("etva")}>
              {calcIcon}{t("declarations.cards.etva.viewRecon")}
            </button>
            <span className="spacer" />
            {/* propunere — neimplementat (trimiterea notei justificative nu are backend) */}
            <button className="btn-dark" onClick={() => notify.info(t("declarations.common.comingSoon"))}>
              <Ic name="send" />{t("declarations.cards.etva.sendJustification")}
            </button>
          </div>
        </div>

        {/* ── Intrastat ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">Intrastat</span>{t("declarations.cards.intrastat.title")}</div>
              <div className="ds">
                {t("declarations.cards.intrastat.desc")}
              </div>
            </div>
            {intrastatLevel === "exceeded" ? (
              <span className="chip late">
                <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_PATH }} />
                {t("declarations.chips.overThreshold")}
              </span>
            ) : intrastatLevel === "approaching" ? (
              <span className="chip wait"><Ic name="clock" cls="sic" />{t("declarations.chips.nearThreshold")}</span>
            ) : (
              <span className="chip sent"><Ic name="dot" cls="sic" />{t("declarations.chips.underThreshold")}</span>
            )}
          </div>
          <div className="dkv">
            <span>
              {t("declarations.cards.intrastat.arrivals", { year: currentYear })}{" "}
              <b className="num">{intrastat ? `${fmtRON(intrastat.arrivals.ytdRon)} lei` : "—"}</b>
            </span>
            <span>
              {t("declarations.cards.intrastat.dispatches", { year: currentYear })}{" "}
              <b className="num">{intrastat ? `${fmtRON(intrastat.dispatches.ytdRon)} lei` : "—"}</b>
            </span>
            <span>
              {t("declarations.cards.intrastat.threshold")} <b className="num">{intrastat ? `${fmtRON(intrastat.thresholdRon)} lei` : "1.000.000,00 lei"}</b>
            </span>
          </div>
          <div className="dfoot">
            <button
              className="pill-btn spin-btn"
              disabled={intrastatFetching}
              onClick={() => {
                void refetchIntrastat().then(() => notify.success(t("declarations.notify.intrastatUpdated")));
              }}
            >
              {calcIcon}{intrastatFetching ? t("declarations.cards.intrastat.checking") : t("declarations.cards.intrastat.check")}
            </button>
            <span className="spacer" />
            {/* propunere — neimplementat (export Intrastat fără backend; sub prag nu e cazul) */}
            <button className="pill-btn" disabled style={{ opacity: 0.5, cursor: "default" }}>
              {intrastatLevel === "exceeded" ? t("declarations.cards.intrastat.exportSoon") : t("declarations.cards.intrastat.exportNa")}
            </button>
          </div>
        </div>

        {/* ── D301 — Decont special de TVA ───────────────────────────────── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D301</span>{t("declarations.d301.title")}</div>
              <div className="ds">{t("declarations.d301.desc")}</div>
            </div>
          </div>
          {/* DUK validation warning banner */}
          <div style={{ margin: "0 14px 10px", padding: "8px 10px", fontSize: 11.5, color: "var(--amber)", background: "var(--rf-warning-bg)", border: "1px solid var(--rf-warning-bd)", borderRadius: 7, lineHeight: 1.5 }}>
            ⚠ {t("declarations.d301.dukWarning")}
          </div>
          <div className="dkv">
            <span>{t("declarations.d301.sect1")}</span>
            <span>{t("declarations.d301.sect4")}</span>
          </div>
          <div className="dfoot">
            <span className="spacer" />
            <button
              className="pill-btn"
              onClick={() => navigate({ to: "/reports", search: { view: "D301" as ReportView } })}
            >
              <Ic name="eye" />{t("declarations.d301.previewXml")}
            </button>
          </div>
        </div>

        {/* ── D700 — Declarație de mențiuni / vector fiscal ──────────────── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D700</span>{t("declarations.d700.title")}</div>
              <div className="ds">{t("declarations.d700.desc")}</div>
            </div>
          </div>
          {/* DUK validation warning banner */}
          <div style={{ margin: "0 14px 10px", padding: "8px 10px", fontSize: 11.5, color: "var(--amber)", background: "var(--rf-warning-bg)", border: "1px solid var(--rf-warning-bd)", borderRadius: 7, lineHeight: 1.5 }}>
            ⚠ {t("declarations.d700.dukWarning")}
          </div>
          <div className="dkv">
            <span>{t("declarations.d700.sectB")}: {t("declarations.d700.tvaMentiune")}, {t("declarations.d700.regimFiscal")}</span>
            <span>{t("declarations.d700.sectC")} · {t("declarations.d700.sectD")}</span>
          </div>
          <div className="dfoot">
            <span className="spacer" />
            <button
              className="pill-btn"
              onClick={() => navigate({ to: "/reports", search: { view: "D700" as ReportView } })}
            >
              <Ic name="eye" />{t("declarations.d700.previewXml")}
            </button>
          </div>
        </div>

        {/* ── D710 — Declarație rectificativă obligații D100 ─────────────── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D710</span>{t("declarations.d710.title")}</div>
              <div className="ds">{t("declarations.d710.desc")}</div>
            </div>
          </div>
          {/* DUK validation warning banner */}
          <div style={{ margin: "0 14px 10px", padding: "8px 10px", fontSize: 11.5, color: "var(--amber)", background: "var(--rf-warning-bg)", border: "1px solid var(--rf-warning-bd)", borderRadius: 7, lineHeight: 1.5 }}>
            ⚠ {t("declarations.d710.dukWarning")}
          </div>
          <div className="dkv">
            <span>{t("declarations.d710.obligations")}</span>
            <span>{t("declarations.d710.sumaCorecta")}</span>
          </div>
          <div className="dfoot">
            <span className="spacer" />
            <button
              className="pill-btn"
              onClick={() => navigate({ to: "/reports", search: { view: "D710" as ReportView } })}
            >
              <Ic name="eye" />{t("declarations.d710.previewXml")}
            </button>
          </div>
        </div>

      </div>

      {/* ── Preflight validation panel (advisory) ─────────────────────────── */}
      {preflightIssues.length > 0 && (
        <div style={{ marginTop: 16 }}>
          <PreflightPanel issues={preflightIssues} />
        </div>
      )}

      {/* ── DUK block panel ────────────────────────────────────────────────── */}
      {dukBlock && (
        <div style={{ marginTop: 12 }}>
          <PreflightPanel issues={dukBlock} />
          <button
            className="pill-btn"
            style={{ marginTop: 8, color: "var(--red)", borderColor: "rgba(220,38,38,.4)" }}
            onClick={() => lastSubmission && void handleExportOfficial(lastSubmission, true)}
          >
            {t("declarations.common.exportAnyway")}
          </button>
        </div>
      )}

      {/* ── D300 detail (real compute results — the prototype lacks this) ──── */}
      {(computing || report) && (
        <>
          <div className="col-title" style={{ margin: "20px 0 8px", padding: 0 }}>
            {t("declarations.detail.title", { period: periodLabel })}
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14 }}>

            {/* TVA colectată (vânzări) */}
            <div className="scr-card" style={{ alignSelf: "start" }}>
              <div style={{ padding: "13px 16px 11px", fontSize: 13, fontWeight: 600, borderBottom: "1px solid var(--line)" }}>
                {t("declarations.detail.collected.title")}
              </div>
              {computing ? (
                <div style={{ padding: "14px 16px", fontSize: 12.5, color: "var(--text-2)" }}>{t("declarations.common.computing")}</div>
              ) : !report || report.invoiceCount === 0 ? (
                <div style={{ padding: "14px 16px", fontSize: 12.5, color: "var(--text-2)" }}>
                  {t("declarations.detail.collected.empty")}
                </div>
              ) : (
                <>
                  <div style={{ padding: "10px 16px 6px", fontSize: 12, color: "var(--text-2)", display: "flex", gap: 16 }}>
                    <span>{t("declarations.detail.collected.cui")} <b className="num" style={{ color: "var(--text)" }}>{report.companyCui}</b></span>
                    <span>{t("declarations.detail.collected.invoices")} <b className="num" style={{ color: "var(--text)" }}>{report.invoiceCount}</b></span>
                  </div>
                  <table className="scr-table">
                    <thead>
                      <tr>
                        <th>{t("declarations.detail.table.rate")}</th>
                        <th>{t("declarations.detail.table.cat")}</th>
                        <th className="r">{t("declarations.detail.table.base")}</th>
                        <th className="r">{t("declarations.detail.table.vat")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {report.groups.map((g, i) => (
                        <tr key={i}>
                          <td className="num" style={{ fontWeight: 600 }}>{g.vatRate}%</td>
                          <td>
                            <span
                              className="num"
                              title={vatCategoryLabel(g.vatCategory)}
                              style={{ cursor: "help", color: "var(--text-2)" }}
                            >
                              {g.vatCategory}
                            </span>
                          </td>
                          <td className="r num">{fmtRON(g.base)}</td>
                          <td className="r num">{fmtRON(g.vat)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  <div className="tot-foot">
                    <span>{t("declarations.detail.collected.total")} <b className="num">{fmtRON(totalBase)}</b></span>
                    <span>{t("declarations.common.vat")} <b className="num">{fmtRON(totalVat)}</b></span>
                  </div>
                </>
              )}
            </div>

            {/* TVA deductibilă (achiziții) */}
            <div className="scr-card" style={{ alignSelf: "start" }}>
              <div style={{ padding: "13px 16px 11px", fontSize: 13, fontWeight: 600, borderBottom: "1px solid var(--line)" }}>
                {t("declarations.detail.deductible.title")}
              </div>
              <div style={{ padding: "10px 16px 0", fontSize: 12, color: "var(--text-2)", lineHeight: 1.5 }}>
                {t("declarations.detail.deductible.desc")}
              </div>
              {report && report.purchaseGroups.length > 0 ? (
                <>
                  <table className="scr-table" style={{ marginTop: 8 }}>
                    <thead>
                      <tr>
                        <th>{t("declarations.detail.table.rate")}</th>
                        <th>{t("declarations.detail.table.cat")}</th>
                        <th className="r">{t("declarations.detail.table.base")}</th>
                        <th className="r">{t("declarations.detail.table.vat")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {report.purchaseGroups.map((g, i) => (
                        <tr key={i}>
                          <td className="num" style={{ fontWeight: 600 }}>{g.vatRate}%</td>
                          <td>
                            <span
                              className="num"
                              title={vatCategoryLabel(g.vatCategory)}
                              style={{ cursor: "help", color: "var(--text-2)" }}
                            >
                              {g.vatCategory}
                            </span>
                          </td>
                          <td className="r num">{fmtRON(g.base)}</td>
                          <td className="r num">{fmtRON(g.vat)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  <div className="tot-foot">
                    <span>{t("declarations.detail.deductible.total")} <b className="num">{fmtRON(report.totalDeductibleBase)}</b></span>
                    <span>{t("declarations.common.vat")} <b className="num">{fmtRON(report.totalDeductibleVat)}</b></span>
                  </div>
                </>
              ) : (
                <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--text-2)" }}>
                  {computing
                    ? t("declarations.common.computing")
                    : !report
                      ? t("declarations.detail.deductible.computeFirst")
                      : t("declarations.detail.deductible.noneParsed")}
                </div>
              )}

              {/* Unparsed note */}
              {report && report.purchaseUnparsedCount > 0 && (
                <div style={{ margin: "0 16px 4px", padding: "8px 10px", fontSize: 12, color: "var(--amber)", background: "var(--rf-warning-bg)", border: "1px solid var(--rf-warning-bd)", borderRadius: 8, lineHeight: 1.5 }}>
                  <b>
                    {t("declarations.detail.unparsedVat", { count: report.purchaseUnparsedCount })}
                  </b>{" "}
                  {t("declarations.detail.unparsedRest")}
                </div>
              )}

              {/* Manual override input */}
              <div style={{ padding: "8px 16px 16px" }}>
                <label htmlFor="manual-deductible" style={{ display: "block", fontSize: 12, color: "var(--text-2)", marginBottom: 6 }}>
                  {t("declarations.detail.deductible.manualLabel")}{" "}
                  <span style={{ color: "var(--dim)" }}>{t("declarations.detail.deductible.manualHint")}</span>
                </label>
                <input
                  type="number"
                  id="manual-deductible"
                  min="0"
                  step="0.01"
                  className="num"
                  style={numInputStyle}
                  value={manualDeductible}
                  onChange={(e) => setManualDeductible(e.target.value)}
                />
                {report && parseDec(manualDeductible) !== parseDec(report.totalDeductibleVat) && (
                  <button
                    type="button"
                    className="pill-btn"
                    style={{ marginTop: 8, height: 28, fontSize: 12 }}
                    onClick={() => setManualDeductible(report.totalDeductibleVat)}
                    title={t("declarations.detail.deductible.resetTitle")}
                  >
                    <Ic name="sync" />{t("declarations.detail.deductible.reset")}
                  </button>
                )}
              </div>
            </div>
          </div>

          {/* ── Regularizări cote vechi (19%/9%/5%) — Wave 8 ───────────────── */}
          {report && (parseDec(report.regColectataTva) !== 0 || parseDec(report.regDedusaTva) !== 0) && (
            <div className="scr-card" style={{ marginTop: 14 }}>
              <div style={{ padding: "13px 16px 11px", fontSize: 13, fontWeight: 600, borderBottom: "1px solid var(--line)" }}>
                {t("declarations.detail.reg.title")}
              </div>
              <div style={{ padding: "10px 16px 4px", fontSize: 12.5, color: "var(--text-2)", lineHeight: 1.5 }}>
                {t("declarations.detail.reg.desc")}
              </div>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 0 }}>
                {/* R16 — regularizări colectată */}
                <div style={{ padding: "8px 16px 14px", borderRight: "1px solid var(--line)" }}>
                  <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 8 }}>
                    {t("declarations.detail.reg.r16")}
                  </div>
                  <label htmlFor="reg-colectata-baza" style={{ display: "block", fontSize: 12, color: "var(--text-2)", marginBottom: 4 }}>
                    {t("declarations.detail.reg.base")}
                  </label>
                  <input
                    type="number"
                    id="reg-colectata-baza"
                    step="1"
                    className="num"
                    style={numInputStyle}
                    value={regColectataBaza}
                    onChange={(e) => setRegColectataBaza(e.target.value)}
                  />
                  <label htmlFor="reg-colectata-tva" style={{ display: "block", fontSize: 12, color: "var(--text-2)", margin: "8px 0 4px" }}>
                    {t("declarations.detail.reg.collectedVat")}
                  </label>
                  <input
                    type="number"
                    id="reg-colectata-tva"
                    step="1"
                    className="num"
                    style={numInputStyle}
                    value={regColectataTva}
                    onChange={(e) => setRegColectataTva(e.target.value)}
                  />
                  {report && (
                    parseDec(regColectataBaza) !== Math.round(parseDec(report.regColectataBaza)) ||
                    parseDec(regColectataTva)  !== Math.round(parseDec(report.regColectataTva))
                  ) && (
                    <button
                      type="button"
                      className="pill-btn"
                      style={{ marginTop: 8, height: 28, fontSize: 12 }}
                      onClick={() => {
                        setRegColectataBaza(String(Math.round(parseDec(report.regColectataBaza))));
                        setRegColectataTva(String(Math.round(parseDec(report.regColectataTva))));
                      }}
                      title={t("declarations.detail.reg.resetTitle")}
                    >
                      <Ic name="sync" />{t("declarations.detail.reg.reset")}
                    </button>
                  )}
                </div>
                {/* R30 — regularizări dedusă */}
                <div style={{ padding: "8px 16px 14px" }}>
                  <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 8 }}>
                    {t("declarations.detail.reg.r30")}
                  </div>
                  <label htmlFor="reg-dedusa-baza" style={{ display: "block", fontSize: 12, color: "var(--text-2)", marginBottom: 4 }}>
                    {t("declarations.detail.reg.base")}
                  </label>
                  <input
                    type="number"
                    id="reg-dedusa-baza"
                    step="1"
                    className="num"
                    style={numInputStyle}
                    value={regDedusaBaza}
                    onChange={(e) => setRegDedusaBaza(e.target.value)}
                  />
                  <label htmlFor="reg-dedusa-tva" style={{ display: "block", fontSize: 12, color: "var(--text-2)", margin: "8px 0 4px" }}>
                    {t("declarations.detail.reg.deductedVat")}
                  </label>
                  <input
                    type="number"
                    id="reg-dedusa-tva"
                    step="1"
                    className="num"
                    style={numInputStyle}
                    value={regDedusaTva}
                    onChange={(e) => setRegDedusaTva(e.target.value)}
                  />
                  {report && (
                    parseDec(regDedusaBaza) !== Math.round(parseDec(report.regDedusaBaza)) ||
                    parseDec(regDedusaTva)  !== Math.round(parseDec(report.regDedusaTva))
                  ) && (
                    <button
                      type="button"
                      className="pill-btn"
                      style={{ marginTop: 8, height: 28, fontSize: 12 }}
                      onClick={() => {
                        setRegDedusaBaza(String(Math.round(parseDec(report.regDedusaBaza))));
                        setRegDedusaTva(String(Math.round(parseDec(report.regDedusaTva))));
                      }}
                      title={t("declarations.detail.reg.resetTitle")}
                    >
                      <Ic name="sync" />{t("declarations.detail.reg.reset")}
                    </button>
                  )}
                </div>
              </div>
            </div>
          )}

          {/* ── TVA de plată / recuperat summary ───────────────────────────── */}
          {report && (
            <div
              style={{
                marginTop: 14,
                padding: "18px 22px",
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                background: netTvaDePlata > 0 ? "var(--rf-warning-bg)" : "var(--rf-success-bg)",
                borderRadius: 12,
                border: `1.5px solid ${netTvaDePlata > 0 ? "var(--rf-warning-bd)" : "var(--rf-success-bd)"}`,
              }}
            >
              <div>
                <div
                  style={{
                    fontSize: 13,
                    fontWeight: 700,
                    color: netTvaDePlata > 0 ? "var(--amber)" : "var(--green)",
                    textTransform: "uppercase",
                    letterSpacing: "0.04em",
                  }}
                >
                  {netTvaDePlata >= 0 ? t("declarations.kv.vatPay") : t("declarations.kv.vatRecover")}
                </div>
                <div style={{ fontSize: 12.5, color: "var(--text-2)", marginTop: 4 }}>
                  {t("declarations.detail.summary.collected")} <b className="num">{fmtRON(totalVat)}</b> RON −{" "}
                  {t("declarations.detail.summary.deductible")} <b className="num">{fmtRON(deductibleVat)}</b> RON
                </div>
              </div>
              <div
                className="num"
                style={{
                  fontSize: 32,
                  fontWeight: 700,
                  color: netTvaDePlata > 0 ? "var(--amber)" : "var(--green)",
                }}
              >
                {fmtRON(Math.abs(netTvaDePlata))}{" "}
                <span style={{ fontSize: 16 }}>RON</span>
              </div>
            </div>
          )}
        </>
      )}

      {/* ── Declarații depuse (istoricul exporturilor) ─────────────────────── */}
      <div style={{ marginTop: 32 }}>
        <div
          style={{
            background: "var(--card)",
            borderRadius: 14,
            border: "1px solid var(--line)",
            overflow: "hidden",
          }}
        >
          <div
            style={{
              padding: "14px 20px",
              borderBottom: "1px solid var(--line)",
              fontWeight: 700,
              fontSize: 13.5,
            }}
          >
            {t("declarations.filings.title")}
          </div>
          {filings.length === 0 ? (
            <div
              style={{
                padding: "28px 20px",
                textAlign: "center",
                color: "var(--text-2)",
                fontSize: 13,
              }}
            >
              {t("declarations.filings.empty")}
            </div>
          ) : (
            <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12.5 }}>
              <thead>
                <tr style={{ background: "var(--surface)", color: "var(--text-2)" }}>
                  <th style={{ padding: "8px 16px", textAlign: "left", fontWeight: 600 }}>
                    {t("declarations.filings.colKind")}
                  </th>
                  <th style={{ padding: "8px 16px", textAlign: "left", fontWeight: 600 }}>
                    {t("declarations.filings.colPeriod")}
                  </th>
                  <th style={{ padding: "8px 16px", textAlign: "left", fontWeight: 600 }}>
                    {t("declarations.filings.colFiledAt")}
                  </th>
                  <th style={{ padding: "8px 16px", textAlign: "left", fontWeight: 600 }}>
                    {t("declarations.filings.colRectificative")}
                  </th>
                  <th style={{ padding: "8px 16px", textAlign: "left", fontWeight: 600 }}>
                    {t("declarations.filings.colStatus")}
                  </th>
                  <th style={{ padding: "8px 8px", textAlign: "center", fontWeight: 600, width: 40 }} />
                </tr>
              </thead>
              <tbody>
                {filings.map((f, i) => (
                  <tr
                    key={f.id}
                    style={{
                      borderTop: i > 0 ? "1px solid var(--line)" : undefined,
                    }}
                  >
                    <td style={{ padding: "9px 16px", fontWeight: 600 }}>{f.kind}</td>
                    <td style={{ padding: "9px 16px", fontFamily: "var(--mono)" }}>{f.period}</td>
                    <td style={{ padding: "9px 16px", color: "var(--text-2)" }}>
                      {new Date(f.filedAt * 1000).toLocaleDateString("ro-RO", {
                        day: "2-digit",
                        month: "short",
                        year: "numeric",
                        hour: "2-digit",
                        minute: "2-digit",
                      })}
                    </td>
                    <td style={{ padding: "9px 16px" }}>
                      {f.isRectificative && (
                        <span
                          style={{
                            fontSize: 10.5,
                            fontWeight: 700,
                            background: "var(--amber-bg, #fff3cd)",
                            color: "var(--amber, #856404)",
                            borderRadius: 5,
                            padding: "2px 7px",
                            textTransform: "uppercase",
                            letterSpacing: "0.04em",
                          }}
                        >
                          {t("declarations.filings.rectificativeBadge")}
                        </span>
                      )}
                    </td>
                    <td style={{ padding: "9px 16px", color: "var(--text-2)" }}>{f.anafStatus}</td>
                    <td style={{ padding: "9px 8px", textAlign: "center" }}>
                      <button
                        onClick={() => void handleDeleteFiling(f.id)}
                        title={t("declarations.filings.delete")}
                        style={{
                          background: "none",
                          border: "none",
                          cursor: "pointer",
                          color: "var(--text-3)",
                          fontSize: 15,
                          lineHeight: 1,
                          padding: "2px 4px",
                          borderRadius: 4,
                        }}
                        onMouseEnter={(e) => {
                          (e.currentTarget as HTMLButtonElement).style.color = "var(--danger, #dc3545)";
                        }}
                        onMouseLeave={(e) => {
                          (e.currentTarget as HTMLButtonElement).style.color = "var(--text-3)";
                        }}
                      >
                        🗑
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>

      {/* D300 Submission Modal (export oficial) */}
      {activeCompany && (
        <D300SubmissionModal
          open={showD300Modal}
          onOpenChange={setShowD300Modal}
          company={activeCompany}
          onSubmit={(sub) => void handleExportOfficial(sub)}
          onPreview={(sub) => void handlePreviewD300(sub)}
        />
      )}
    </div>
  );
}
