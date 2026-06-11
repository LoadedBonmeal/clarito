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
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { PreflightPanel } from "@/components/shared/PreflightPanel";
import { D300SubmissionModal } from "@/components/modals/D300SubmissionModal";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec, MONTHS_RO } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { queryKeys } from "@/lib/queries";
import type { D300Report, D300Submission } from "@/types";
import type { PreflightIssue } from "@/lib/tauri";
import type { ReportView } from "@/router";

// ─── Helpers ─────────────────────────────────────────────────────────────────

const MONTHS = MONTHS_RO;

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

// Inline icons NOT in Ic (verbatim from prototype).
const WARN_PATH = '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

const numInputStyle: React.CSSProperties = {
  width: "100%", height: 32, fontSize: 12.5, padding: "0 10px",
  border: "1px solid var(--line)", borderRadius: 8, fontFamily: "var(--mono)",
};

// ─── Component ───────────────────────────────────────────────────────────────

export function DeclarationsPage() {
  const navigate = useNavigate();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

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
  const [openPop, setOpenPop] = useState<"" | "period">("");

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

  const yearOptions = buildYearOptions();
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
      notify.warn("Selectați o companie activă.");
      return;
    }
    setComputing(true);
    clearReportState();
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
  const handleExportOfficial = async (submission: D300Submission, override = false) => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setLastSubmission(submission);
    const savePath = await saveDialog({
      title:       "Salvează D300 oficial ANAF (XML)",
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
        notify.error("DUKIntegrator a găsit erori. Corectați-le sau exportați oricum.");
        return;
      }
      setDukBlock(null);
      notify.success(
        res.dukAvailable
          ? `D300 oficial salvat (DUK: valid): ${res.path}`
          : `D300 oficial salvat: ${res.path} (validare DUK indisponibilă local)`,
      );
      try { await openPath(res.path); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta D300 oficial."));
    } finally {
      setExportingOfficial(false);
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
  const termenD101   = toIso(new Date(selectedYear, 5, 25));                 // 25 iun pentru anul fiscal precedent

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
        <div className="page-head"><div><h1>Declarații</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Selectați o companie activă pentru a vedea declarațiile.
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Declarații</h1>
          <p className="sub">
            Perioada de raportare {periodLabel} · D300 se calculează aici; calculele detaliate
            (e-TVA, D390, D394, D100, D101, D406) sunt și în Rapoarte
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
              <div
                className="pop show"
                style={{ right: 0, top: 40, width: 220, maxHeight: 320, overflowY: "auto" }}
                onMouseDown={(e) => e.stopPropagation()}
              >
                <div className="col-title">An</div>
                {yearOptions.map((y) => (
                  <button
                    key={y}
                    className="pop-item"
                    onClick={() => { setSelectedYear(y); clearReportState(); }}
                  >
                    <span style={{ flex: 1 }} className="num">{y}</span>
                    {selectedYear === y && <Ic name="check" cls="co-check" />}
                  </button>
                ))}
                <div className="pop-div" />
                <div className="col-title">Lună</div>
                {MONTHS.map((m, idx) => (
                  <button
                    key={m}
                    className="pop-item"
                    onClick={() => { setSelectedMonth(idx + 1); clearReportState(); setOpenPop(""); }}
                  >
                    <span style={{ flex: 1 }}>{m}</span>
                    {selectedMonth === idx + 1 && <Ic name="check" cls="co-check" />}
                  </button>
                ))}
              </div>
            )}
          </div>
          {/* propunere — neimplementat (fără backend pentru calendar termene) */}
          <button className="pill-btn" onClick={() => notify.info("În curând.")}>
            <Ic name="calendar" />Calendar termene
          </button>
          <button
            className="btn-dark spin-btn"
            disabled={computing}
            onClick={() => { void handleCompute(); void refetchIntrastat(); }}
          >
            <Ic name="sync" />{computing ? "Recalculez…" : "Recalculează toate"}
          </button>
        </div>
      </div>

      <div className="dec-grid">

        {/* ── D300 ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D300</span>Decont de TVA</div>
              <div className="ds">
                Rândurile se completează automat din jurnale; <b>regularizările cote vechi
                (19%/9%/5%)</b> se precompletează pe R16/R30 cu suprascriere manuală. Exportul
                oficial (schema v12) cere datele declarantului, CAEN, banca și trece prin
                validarea DUKIntegrator.
              </div>
            </div>
            {dukBlock ? (
              <span className="chip late">
                <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_PATH }} />
                Erori DUK
              </span>
            ) : report ? (
              <span className="chip wait"><Ic name="clock" cls="sic" />De depus</span>
            ) : (
              <span className="chip sent"><Ic name="dot" cls="sic" />Necalculat</span>
            )}
          </div>
          <div className="dkv">
            <span>Perioada <b>{periodLabel}</b></span>
            <span>Termen <b className="num">{fmtRoDate(termenLunar)}</b></span>
            <span>
              {report && netTvaDePlata < 0 ? "TVA de recuperat" : "TVA de plată"}{" "}
              <b className="num">{report ? `${fmtRON(Math.abs(netTvaDePlata))} RON` : "—"}</b>
            </span>
            <span>
              Verificare preflight{" "}
              {preflightIssues.length === 0
                ? <b className="pos">fără probleme</b>
                : <b className={preflightErrors > 0 ? "neg" : "pos"}>
                    {preflightErrors > 0
                      ? `${preflightErrors} ${preflightErrors === 1 ? "eroare" : "erori"}`
                      : `${preflightIssues.length} ${preflightIssues.length === 1 ? "avertisment" : "avertismente"}`}
                  </b>}
            </span>
          </div>
          <div className="dfoot">
            <button className="pill-btn spin-btn" disabled={computing} onClick={() => void handleCompute()}>
              {calcIcon}{computing ? "Calculez…" : "Calculează"}
            </button>
            <button
              className="pill-btn"
              disabled={exporting || noD300Data}
              style={exporting || noD300Data ? { opacity: 0.5, cursor: "default" } : undefined}
              title="Exportă extras D300 ca fișier XML (document de lucru, nu schema ANAF)"
              onClick={() => void handleExport()}
            >
              {exportXmlIcon}{exporting ? "Export…" : "Extract XML"}
            </button>
            <span className="spacer" />
            <button
              className="btn-dark"
              disabled={exportingOfficial || noD300Data || !activeCompany}
              style={exportingOfficial || noD300Data || !activeCompany ? { opacity: 0.5, cursor: "default" } : undefined}
              title="Exportă D300 conform schemei oficiale ANAF v12"
              onClick={() => setShowD300Modal(true)}
            >
              {exportXmlIcon}{exportingOfficial ? "Export…" : "Export XML oficial"}
            </button>
          </div>
        </div>

        {/* ── D390 ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D390</span>Declarație recapitulativă VIES</div>
              <div className="ds">
                Operațiuni intracomunitare pe tipuri: <b>L</b> livrări · <b>T</b> triunghiulare ·{" "}
                <b>A</b> achiziții · <b>P</b> prestări · <b>S</b> servicii primite ·{" "}
                <b>R</b> regularizări. Partenerii fără cod VIES valid sunt semnalați la calcul.
              </div>
            </div>
            <span className="chip wait"><Ic name="clock" cls="sic" />De depus</span>
          </div>
          <div className="dkv">
            <span>Perioada <b>{periodLabel}</b></span>
            <span>Termen <b className="num">{fmtRoDate(termenLunar)}</b></span>
            <span>Calcul și export <b>în Rapoarte → D390</b></span>
          </div>
          <div className="dfoot">
            <button className="pill-btn spin-btn" onClick={() => goReports("d390")}>
              {calcIcon}Calculează
            </button>
            <span className="spacer" />
            {/* exportul XML real e în vizualizarea D390 din Rapoarte */}
            <button className="btn-dark" onClick={() => goReports("d390")}>
              {exportXmlIcon}Export XML
            </button>
          </div>
        </div>

        {/* ── D394 ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D394</span>Livrări și achiziții naționale</div>
              <div className="ds">
                Declarație informativă per partener, generată din facturile emise și primite cu
                CIF valid RO. Partenerii se agregă automat la calcul.
              </div>
            </div>
            <span className="chip wait"><Ic name="clock" cls="sic" />De depus</span>
          </div>
          <div className="dkv">
            <span>Perioada <b>{periodLabel}</b></span>
            <span>Termen <b className="num">{fmtRoDate(termenLunar)}</b></span>
            <span>Calcul și export <b>în Rapoarte → D394</b></span>
          </div>
          <div className="dfoot">
            <button className="pill-btn spin-btn" onClick={() => goReports("d394")}>
              {calcIcon}Calculează
            </button>
            <span className="spacer" />
            {/* exportul XML real e în vizualizarea D394 din Rapoarte */}
            <button className="btn-dark" onClick={() => goReports("d394")}>
              {exportXmlIcon}Export XML
            </button>
          </div>
        </div>

        {/* ── D406 SAF-T ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D406</span>SAF-T — fișierul standard de audit</div>
              <div className="ds">
                Raportare lunară completă: master files, documente sursă, GL. Generarea durează
                câteva minute; validarea se face cu validatorul DUK SAF-T.
              </div>
            </div>
            <span className="chip wait"><Ic name="clock" cls="sic" />De depus</span>
          </div>
          <div className="dkv">
            <span>Perioada <b>{periodLabel}</b></span>
            <span>Termen <b className="num">{fmtRoDate(termenD406)}</b></span>
            <span>Generare și validare <b>în Rapoarte → D406 SAF-T</b></span>
          </div>
          <div className="dfoot">
            <button className="pill-btn spin-btn" onClick={() => goReports("saft")}>
              {calcIcon}Generează
            </button>
            <button className="pill-btn" onClick={() => goReports("saft")}>
              <Ic name="checkC" />Validează
            </button>
            <span className="spacer" />
            <button className="btn-dark" onClick={() => goReports("saft")}>
              {exportXmlIcon}Export XML
            </button>
          </div>
        </div>

        {/* ── D100 ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D100</span>Obligații de plată — impozit</div>
              <div className="ds">
                Micro: impozit pe venit la <b>poziția 5</b> · Profit: plăți anticipate la{" "}
                <b>poziția 2</b>. Pentru T4 la profit, regularizarea anuală se face prin D101.
              </div>
            </div>
            <span className="chip wait"><Ic name="clock" cls="sic" />Estimată</span>
          </div>
          <div className="dkv">
            <span>Perioada <b>T{quarter} {selectedYear}</b></span>
            <span>Termen <b className="num">{fmtRoDate(termenD100)}</b></span>
            <span>Calcul și export <b>în Rapoarte → D100</b></span>
          </div>
          <div className="dfoot">
            <button className="pill-btn spin-btn" onClick={() => goReports("d100")}>
              {calcIcon}Calculează
            </button>
            <span className="spacer" />
            {/* exportul XML real e în vizualizarea D100 din Rapoarte */}
            <button className="btn-dark" onClick={() => goReports("d100")}>
              {exportXmlIcon}Export XML
            </button>
          </div>
        </div>

        {/* ── D101 ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">D101</span>Impozit pe profit — anual</div>
              <div className="ds">
                Pierderea fiscală reportată se recuperează în limita a <b>70%</b> din profitul
                impozabil. Creditul fiscal pentru sponsorizări se aplică în limita legală.
              </div>
            </div>
            <span className="chip sent"><Ic name="docText" cls="sic" />An {selectedYear - 1}</span>
          </div>
          <div className="dkv">
            <span>Perioada <b>An {selectedYear - 1}</b></span>
            <span>Termen <b className="num">{fmtRoDate(termenD101)}</b></span>
            <span>Fișa de calcul <b>în Rapoarte → D101</b></span>
          </div>
          <div className="dfoot">
            {/* propunere — neimplementat (recipisa ANAF nu are backend) */}
            <button className="pill-btn" onClick={() => notify.info("În curând.")}>
              <Ic name="eye" />Vezi recipisa
            </button>
            <span className="spacer" />
            <button className="pill-btn" onClick={() => goReports("d101")}>
              {exportXmlIcon}Vezi XML
            </button>
          </div>
        </div>

        {/* ── e-TVA ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">e-TVA</span>Reconciliere decont precompletat</div>
              <div className="ds">
                ANAF transmite decontul precompletat P300ETVA (se descarcă din SPV); diferențele{" "}
                <b>≥ 5.000 lei și ≥ 20%</b> sunt semnificative și se justifică prin „Notă
                justificativă”. La TVA la încasare divergențele sunt așteptate.
              </div>
            </div>
            <span className="chip sent"><Ic name="dot" cls="sic" />De reconciliat</span>
          </div>
          <div className="dkv">
            <span>Perioada <b>{periodLabel}</b></span>
            <span>P300 precompletat <b>se descarcă din SPV</b></span>
            <span>Reconciliere <b>în Rapoarte → e-TVA</b></span>
          </div>
          <div className="dfoot">
            <button className="pill-btn spin-btn" onClick={() => goReports("etva")}>
              {calcIcon}Vezi reconcilierea
            </button>
            <span className="spacer" />
            {/* propunere — neimplementat (trimiterea notei justificative nu are backend) */}
            <button className="btn-dark" onClick={() => notify.info("În curând.")}>
              <Ic name="send" />Trimite justificare
            </button>
          </div>
        </div>

        {/* ── Intrastat ── */}
        <div className="dec">
          <div className="dh">
            <div>
              <div className="dt"><span className="doc">Intrastat</span>Statistică schimburi intra-UE</div>
              <div className="ds">
                Obligație doar la depășirea pragurilor anuale (introduceri 1.000.000 lei /
                expedieri 1.000.000 lei). Pragurile se monitorizează automat din facturi.
              </div>
            </div>
            {intrastatLevel === "exceeded" ? (
              <span className="chip late">
                <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_PATH }} />
                Peste prag
              </span>
            ) : intrastatLevel === "approaching" ? (
              <span className="chip wait"><Ic name="clock" cls="sic" />Aproape de prag</span>
            ) : (
              <span className="chip sent"><Ic name="dot" cls="sic" />Sub prag</span>
            )}
          </div>
          <div className="dkv">
            <span>
              Introduceri {currentYear}{" "}
              <b className="num">{intrastat ? `${fmtRON(intrastat.arrivals.ytdRon)} lei` : "—"}</b>
            </span>
            <span>
              Expedieri {currentYear}{" "}
              <b className="num">{intrastat ? `${fmtRON(intrastat.dispatches.ytdRon)} lei` : "—"}</b>
            </span>
            <span>
              Prag <b className="num">{intrastat ? `${fmtRON(intrastat.thresholdRon)} lei` : "1.000.000,00 lei"}</b>
            </span>
          </div>
          <div className="dfoot">
            <button
              className="pill-btn spin-btn"
              disabled={intrastatFetching}
              onClick={() => {
                void refetchIntrastat().then(() => notify.success("Praguri Intrastat actualizate."));
              }}
            >
              {calcIcon}{intrastatFetching ? "Verific…" : "Verifică pragurile"}
            </button>
            <span className="spacer" />
            {/* propunere — neimplementat (export Intrastat fără backend; sub prag nu e cazul) */}
            <button className="pill-btn" disabled style={{ opacity: 0.5, cursor: "default" }}>
              {intrastatLevel === "exceeded" ? "Export — în curând" : "Export — nu e cazul"}
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
            Exportă oricum (ignoră DUK)
          </button>
        </div>
      )}

      {/* ── D300 detail (real compute results — the prototype lacks this) ──── */}
      {(computing || report) && (
        <>
          <div className="col-title" style={{ margin: "20px 0 8px", padding: 0 }}>
            Detaliu D300 — {periodLabel}
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14 }}>

            {/* TVA colectată (vânzări) */}
            <div className="scr-card" style={{ alignSelf: "start" }}>
              <div style={{ padding: "13px 16px 11px", fontSize: 13, fontWeight: 600, borderBottom: "1px solid var(--line)" }}>
                TVA colectată (vânzări)
              </div>
              {computing ? (
                <div style={{ padding: "14px 16px", fontSize: 12.5, color: "var(--text-2)" }}>Se calculează…</div>
              ) : !report || report.invoiceCount === 0 ? (
                <div style={{ padding: "14px 16px", fontSize: 12.5, color: "var(--text-2)" }}>
                  Nu există facturi VALIDATED în perioada selectată.
                </div>
              ) : (
                <>
                  <div style={{ padding: "10px 16px 6px", fontSize: 12, color: "var(--text-2)", display: "flex", gap: 16 }}>
                    <span>CUI: <b className="num" style={{ color: "var(--text)" }}>{report.companyCui}</b></span>
                    <span>Facturi: <b className="num" style={{ color: "var(--text)" }}>{report.invoiceCount}</b></span>
                  </div>
                  <table className="scr-table">
                    <thead>
                      <tr>
                        <th>Cotă</th>
                        <th>Cat.</th>
                        <th className="r">Bază</th>
                        <th className="r">TVA</th>
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
                    <span>Total colectată: bază <b className="num">{fmtRON(totalBase)}</b></span>
                    <span>TVA <b className="num">{fmtRON(totalVat)}</b></span>
                  </div>
                </>
              )}
            </div>

            {/* TVA deductibilă (achiziții) */}
            <div className="scr-card" style={{ alignSelf: "start" }}>
              <div style={{ padding: "13px 16px 11px", fontSize: 13, fontWeight: 600, borderBottom: "1px solid var(--line)" }}>
                TVA deductibilă (achiziții)
              </div>
              <div style={{ padding: "10px 16px 0", fontSize: 12, color: "var(--text-2)", lineHeight: 1.5 }}>
                TVA deductibilă se calculează din facturile primite procesate. Verificați că toate
                facturile lunii au fost descărcate și parsate din SPV pentru un decont corect.
              </div>
              {report && report.purchaseGroups.length > 0 ? (
                <>
                  <table className="scr-table" style={{ marginTop: 8 }}>
                    <thead>
                      <tr>
                        <th>Cotă</th>
                        <th>Cat.</th>
                        <th className="r">Bază</th>
                        <th className="r">TVA</th>
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
                    <span>Total deductibilă: bază <b className="num">{fmtRON(report.totalDeductibleBase)}</b></span>
                    <span>TVA <b className="num">{fmtRON(report.totalDeductibleVat)}</b></span>
                  </div>
                </>
              ) : (
                <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--text-2)" }}>
                  {computing
                    ? "Se calculează…"
                    : !report
                      ? "Calculați D300 pentru a vedea datele."
                      : "Nicio factură primită parsată în perioadă."}
                </div>
              )}

              {/* Unparsed note */}
              {report && report.purchaseUnparsedCount > 0 && (
                <div style={{ margin: "0 16px 4px", padding: "8px 10px", fontSize: 12, color: "var(--amber)", background: "rgba(180,83,9,.07)", border: "1px solid rgba(180,83,9,.18)", borderRadius: 8, lineHeight: 1.5 }}>
                  <b>
                    {report.purchaseUnparsedCount}{" "}
                    {report.purchaseUnparsedCount === 1 ? "factură primită nu are" : "facturi primite nu au"}{" "}
                    încă defalcare TVA
                  </b>{" "}
                  — suma calculată automat poate fi parțială. Introduceți manual valoarea corectă
                  mai jos sau folosiți «Recalculează TVA din XML» în Jurnal cumpărări.
                </div>
              )}

              {/* Manual override input */}
              <div style={{ padding: "8px 16px 16px" }}>
                <label htmlFor="manual-deductible" style={{ display: "block", fontSize: 12, color: "var(--text-2)", marginBottom: 6 }}>
                  Ajustare manuală TVA deductibilă{" "}
                  <span style={{ color: "var(--dim)" }}>· pentru achiziții fără factură SPV parsată</span>
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
                    title="Resetează la valoarea calculată automat"
                  >
                    <Ic name="sync" />Resetează la valoarea calculată
                  </button>
                )}
              </div>
            </div>
          </div>

          {/* ── Regularizări cote vechi (19%/9%/5%) — Wave 8 ───────────────── */}
          {report && (parseDec(report.regColectataTva) !== 0 || parseDec(report.regDedusaTva) !== 0) && (
            <div className="scr-card" style={{ marginTop: 14 }}>
              <div style={{ padding: "13px 16px 11px", fontSize: 13, fontWeight: 600, borderBottom: "1px solid var(--line)" }}>
                Regularizări cote vechi (19%/9%/5%)
              </div>
              <div style={{ padding: "10px 16px 4px", fontSize: 12.5, color: "var(--text-2)", lineHeight: 1.5 }}>
                Operațiunile la cote vechi sunt raportate automat în rândurile de regularizări
                (R16 — taxă colectată; R30 — taxă dedusă). Verificați și ajustați dacă este necesar.
              </div>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 0 }}>
                {/* R16 — regularizări colectată */}
                <div style={{ padding: "8px 16px 14px", borderRight: "1px solid var(--line)" }}>
                  <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 8 }}>
                    R16 — Regularizări taxă colectată
                  </div>
                  <label htmlFor="reg-colectata-baza" style={{ display: "block", fontSize: 12, color: "var(--text-2)", marginBottom: 4 }}>
                    Bază impozabilă (lei)
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
                    TVA colectată (lei)
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
                      title="Resetează la valorile calculate automat"
                    >
                      <Ic name="sync" />Resetează la calculat
                    </button>
                  )}
                </div>
                {/* R30 — regularizări dedusă */}
                <div style={{ padding: "8px 16px 14px" }}>
                  <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 8 }}>
                    R30 — Regularizări taxă dedusă
                  </div>
                  <label htmlFor="reg-dedusa-baza" style={{ display: "block", fontSize: 12, color: "var(--text-2)", marginBottom: 4 }}>
                    Bază impozabilă (lei)
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
                    TVA dedusă (lei)
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
                      title="Resetează la valorile calculate automat"
                    >
                      <Ic name="sync" />Resetează la calculat
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
                background: netTvaDePlata > 0 ? "rgba(180,83,9,.07)" : "rgba(4,120,87,.07)",
                borderRadius: 12,
                border: `1.5px solid ${netTvaDePlata > 0 ? "rgba(180,83,9,.18)" : "rgba(4,120,87,.18)"}`,
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
                  {netTvaDePlata >= 0 ? "TVA de plată" : "TVA de recuperat"}
                </div>
                <div style={{ fontSize: 12.5, color: "var(--text-2)", marginTop: 4 }}>
                  Colectată <b className="num">{fmtRON(totalVat)}</b> RON −
                  Deductibilă <b className="num">{fmtRON(deductibleVat)}</b> RON
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
