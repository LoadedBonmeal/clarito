/**
 * Salarizare — verbatim port of the design "Salarizare.html":
 *   .page-head (title + "Luna · N angajați activi · fond brut" sub + period pop +
 *   pill-btn "Angajat nou" + btn-dark "Export D112 (XML)") → .banner warn (D112
 *   model nou OPANAF 605/2026 ≥ iulie 2026) → .tabs (Angajați · Stat de salarii ·
 *   Concedii medicale · Sedii secundare) → .panel × 4 (.scr-card + .scr-toolbar +
 *   .scr-table + .pager footnotes) → modale .modal-back/.modal (angajat, certificat
 *   CM, sediu, export D112 cu CAEN).
 *
 * ALL wiring preserved: api.payroll.list/create/update/delete,
 * listSedii/createSediu/deleteSediu, listConcedii/createConcediu/deleteConcediu,
 * api.payroll.run (stat de salarii + nota contabilă 641/421, 4315, 4316, 444,
 * 646/436), api.payroll.exportD112Xml (+ avertisment model nou ≥ 07/2026),
 * selector lună/an, confirm() la ștergeri.
 */

import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm, save as saveDialog } from "@tauri-apps/plugin-dialog";

import { Ic } from "@/components/shared/Ic";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { useOpenXml } from "@/hooks/use-open-xml";
import { MonthPicker } from "@/components/shared/MonthPicker";
import { PreflightPanel } from "@/components/shared/PreflightPanel";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { PreflightIssue } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON, parseDec } from "@/lib/utils";
import type { Employee, CreateEmployeeInput, PayrollRun, SecondaryOffice } from "@/types";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};
/** "03 – 09 iun 2026" / "25 mai – 28 iun 2026" (stil prototip). */
const fmtRange = (a: string, b: string) => {
  if (!a || !b) return [a, b].filter(Boolean).map(fmtRoDate).join(" – ") || "—";
  const [ya, ma, da] = a.split("-");
  const [yb, mb, db] = b.split("-");
  if (ya === yb && ma === mb) return `${da} – ${db} ${RO_MON[Number(mb) - 1] ?? mb} ${yb}`;
  if (ya === yb) return `${da} ${RO_MON[Number(ma) - 1] ?? ma} – ${db} ${RO_MON[Number(mb) - 1] ?? mb} ${yb}`;
  return `${fmtRoDate(a)} – ${fmtRoDate(b)}`;
};

const initials = (name: string) =>
  name.trim().split(/\s+/).slice(0, 2).map((w) => w[0]?.toUpperCase() ?? "").join("") || "—";


export function PayrollPage() {
  const { t } = useTranslation();
  const companyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();
  const now = new Date();
  const [year, setYear] = useState(now.getFullYear());
  const [month, setMonth] = useState(now.getMonth() + 1);
  const [tab, setTab] = useState(0);
  const [empQuery, setEmpQuery] = useState("");
  const [modal, setModal] = useState<"create" | { edit: Employee } | null>(null);
  const [showD112, setShowD112] = useState(false);
  const [dukBlock, setDukBlock] = useState<PreflightIssue[] | null>(null);
  const [d112Rectif, setD112Rectif] = useState(false);
  const [showSediu, setShowSediu] = useState(false);
  const [showConcediu, setShowConcediu] = useState(false);
  const [run, setRun] = useState<PayrollRun | null>(null);
  const [openPop, setOpenPop] = useState<"" | "period">("");

  const MONTHS_FULL = useMemo(() => [
    t("payroll.months.jan"), t("payroll.months.feb"), t("payroll.months.mar"),
    t("payroll.months.apr"), t("payroll.months.may"), t("payroll.months.jun"),
    t("payroll.months.jul"), t("payroll.months.aug"), t("payroll.months.sep"),
    t("payroll.months.oct"), t("payroll.months.nov"), t("payroll.months.dec"),
  ], [t]);

  /** Excepții art. 146 (5⁷) — etichete mini-chip ok (stil prototip). */
  const EXCEPTIE_LABEL: Record<string, string> = useMemo(() => ({
    elev_student: t("payroll.emp.chips.exc.elev_student"),
    ucenic: t("payroll.emp.chips.exc.ucenic"),
    dizabilitate: t("payroll.emp.chips.exc.dizabilitate"),
    contracte_multiple: t("payroll.emp.chips.exc.contracte_multiple"),
  }), [t]);

  /** Cod indemnizație CM (D_9) — etichete chip. */
  const COD_CM_LABEL: Record<string, string> = useMemo(() => ({
    "01": t("payroll.cm.codes.c01"),
    "06": t("payroll.cm.codes.c06"),
    "09": t("payroll.cm.codes.c09"),
    "15": t("payroll.cm.codes.c15"),
  }), [t]);

  // închide pop-urile la click în afară (model Invoices)
  useEffect(() => {
    if (!openPop) return;
    const h = () => setOpenPop("");
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [openPop]);

  const { data: employees = [] } = useQuery({
    queryKey: ["employees", companyId],
    queryFn: () => api.payroll.list(companyId!),
    enabled: !!companyId,
  });

  const { data: sedii = [] } = useQuery({
    queryKey: ["sedii", companyId],
    queryFn: () => api.payroll.listSedii(companyId!),
    enabled: !!companyId,
  });

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });
  const activeCompany = companies.find((c) => c.id === companyId);

  const period = useMemo(() => {
    const mm = String(month).padStart(2, "0");
    const last = new Date(year, month, 0).getDate();
    return { from: `${year}-${mm}-01`, to: `${year}-${mm}-${String(last).padStart(2, "0")}` };
  }, [year, month]);
  const periodYm = period.from.slice(0, 7);

  const { data: leaves = [] } = useQuery({
    queryKey: ["concedii", companyId, periodYm],
    queryFn: () => api.payroll.listConcedii(companyId!, periodYm),
    enabled: !!companyId,
  });

  // statul calculat aparține lunii — la schimbarea perioadei se recalculează
  useEffect(() => { setRun(null); }, [periodYm]);

  const runMut = useMutation({
    mutationFn: () => api.payroll.run(companyId!, period.from, period.to),
    onSuccess: (r) => {
      setRun(r);
      r.posted
        ? notify.success(t("payroll.notify.runPosted", { net: r.totalNet }))
        : notify.info(t("payroll.stat.noActive"));
    },
    onError: (e) => notify.error(formatError(e, t("payroll.notify.runError"))),
  });

  const del = useMutation({
    mutationFn: (id: string) => api.payroll.delete(id, companyId!),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["employees", companyId] }),
    onError: (e) => notify.error(formatError(e, t("payroll.notify.deleteError"))),
  });

  const delSediu = useMutation({
    mutationFn: (id: string) => api.payroll.deleteSediu(id, companyId!),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["sedii", companyId] }),
    onError: (e) => notify.error(formatError(e, t("payroll.notify.deleteSediuError"))),
  });

  const delConcediu = useMutation({
    mutationFn: (id: string) => api.payroll.deleteConcediu(id, companyId!),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["concedii", companyId, periodYm] }),
    onError: (e) => notify.error(formatError(e, t("payroll.notify.deleteConcediuError"))),
  });

  const openXml = useOpenXml();

  /** Construiește XML-ul D112 al lunii și îl deschide în vizualizatorul/editorul XML (cu re-validare DUK). */
  const runD112Preview = async (caen: string, isRectificative: boolean) => {
    if (!companyId) return;
    try {
      const xml = await api.payroll.previewD112Xml(companyId, year, month, caen, isRectificative);
      openXml({
        xml,
        name: `d112-${year}-${String(month).padStart(2, "0")}.xml`,
        declKind: "D112",
      });
    } catch (err) {
      notify.error(formatError(err, t("payroll.d112.previewFailed")));
    }
  };

  const runD112 = async (caen: string, isRectificative: boolean, override = false) => {
    if (!companyId) return;
    // Noul model D112 (Ordin comun 605/95/928/2.314/2026, M.Of. 463/02.06.2026) se aplică
    // veniturilor lunii IULIE 2026+; aplicația emite structura v7 (valabilă ≤ iunie 2026).
    if (year > 2026 || (year === 2026 && month >= 7)) {
      notify.warn(t("payroll.notify.newModelWarn", { month: MONTHS_FULL[month - 1], year }));
    }
    const dest = await saveDialog({
      title: t("payroll.d112.saveTitle"),
      defaultPath: `d112-${year}-${String(month).padStart(2, "0")}.xml`,
      filters: [{ name: "XML", extensions: ["xml"] }],
    });
    if (!dest) return;
    try {
      // Gate DUK: validatorul OFICIAL `D112Validator.jar` rulează înainte de scriere. Dacă raportează
      // ERORI, fișierul NU se scrie (written=false) — afișăm issues + buton „exportă oricum".
      const res = await api.payroll.exportD112Xml(companyId, year, month, caen, dest, override, isRectificative);
      if (!res.written) {
        setDukBlock(res.issues);
        notify.error(t("declarations.notify.dukErrors"));
        return;
      }
      setDukBlock(null);
      notify.success(t("payroll.notify.exported", {
        insured: t("payroll.d112.insured", { count: employees.filter((e) => e.active).length }),
      }));
      setShowD112(false);
    } catch (err) {
      notify.error(formatError(err, t("payroll.notify.exportError")));
    }
  };

  const activeEmployees = employees.filter((e) => e.active);
  const fondBrut = activeEmployees.reduce((s, e) => s + parseDec(e.grossSalary), 0);

  const filteredEmployees = useMemo(() => {
    const q = empQuery.trim().toLowerCase();
    if (!q) return employees;
    return employees.filter((e) => e.fullName.toLowerCase().includes(q) || e.cnp.toLowerCase().includes(q));
  }, [employees, empQuery]);

  const sediuCount = (cif: string) => employees.filter((e) => e.sediuCif === cif).length;

  if (!companyId) {
    return (
      <div className="main-inner wide pg-payroll">
        <div className="page-head"><div><h1>{t("payroll.title")}</h1></div></div>
        <div className="pf-nocompany">
          {t("payroll.noCompany")}
        </div>
      </div>
    );
  }

  const tabs: Array<{ label: string; count: number | null }> = [
    { label: t("payroll.tabs.employees"), count: employees.length },
    { label: t("payroll.tabs.payslip"), count: null },
    { label: t("payroll.tabs.medicalLeaves"), count: leaves.length },
    { label: t("payroll.tabs.offices"), count: sedii.length + 1 },
    { label: t("payroll.sim.tabLabel"), count: null },
    { label: t("payroll.tabs.pontaj"), count: null },
  ];

  return (
    <div className="main-inner wide pg-payroll">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("payroll.title")}</h1>
          <p className="sub">
            {MONTHS_FULL[month - 1]} {year} · {t("payroll.head.activeEmployees", { count: activeEmployees.length })} · {t("payroll.head.grossFund", { amount: fmtRON(fondBrut) })}
          </p>
        </div>
        <div className="head-actions">
          {/* perioadă — funcționalitate reală (prototipul are luna fixă) */}
          <div className="nou-wrap">
            <button
              className="pill-btn"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => setOpenPop(openPop === "period" ? "" : "period")}
            >
              <Ic name="calendar" />
              {MONTHS_FULL[month - 1]} {year}
              <Ic name="chevD" cls="ic" />
            </button>
            {openPop === "period" && (
              <MonthPicker
                year={year}
                month={month}
                monthsFull={MONTHS_FULL}
                prevYearLabel={t("payroll.actions.prevYear")}
                nextYearLabel={t("payroll.actions.nextYear")}
                onPrevYear={() => setYear(year - 1)}
                onNextYear={() => setYear(year + 1)}
                onPick={(m) => { setMonth(m); setOpenPop(""); }}
              />
            )}
          </div>
          <button className="pill-btn" onClick={() => setModal("create")}>
            <Ic name="plus" />{t("payroll.actions.newEmployee")}
          </button>
          <button className="btn-dark" onClick={() => setShowD112(true)}>
            <Ic name="code" />{t("payroll.actions.exportD112")}
          </button>
        </div>
      </div>

      {/* banner D112 model nou */}
      <div className="banner warn">
        <Ic name="triangle" />
        <span>
          <b>{t("payroll.banner.strong")}</b> {t("payroll.banner.applies")} <b>{t("payroll.banner.from")}</b>.{" "}
          {t("payroll.banner.body")}
        </span>
      </div>

      {/* tabs */}
      <div className="tabs">
        {tabs.map((tb, i) => (
          <div key={tb.label} className={`tab${tab === i ? " active" : ""}`} onClick={() => setTab(i)}>
            {tb.label}
            {tb.count !== null && <span className="cnt">{tb.count}</span>}
          </div>
        ))}
      </div>

      {/* ── ANGAJAȚI ─────────────────────────────────────────────────────── */}
      <div className={`panel${tab === 0 ? " show" : ""}`}>
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">{t("payroll.tabs.employees")}</div>
            <div className="spacer" />
            <div className="scr-search scr-search-sm">
              <Ic name="lens" />
              <input
                type="text"
                placeholder={t("payroll.emp.searchPlaceholder")}
                value={empQuery}
                onChange={(e) => setEmpQuery(e.target.value)}
              />
            </div>
          </div>
          {filteredEmployees.length === 0 ? (
            <div className="pf-empty">
              {employees.length === 0
                ? t("payroll.emp.emptyNone")
                : t("payroll.emp.emptySearch")}
            </div>
          ) : (
            <table className="scr-table">
              <thead>
                <tr><th>{t("payroll.emp.th.name")}</th><th>{t("payroll.emp.th.cnp")}</th><th className="r">{t("payroll.emp.th.gross")}</th><th className="r">{t("payroll.emp.th.deduction")}</th><th className="r w-acts"></th></tr>
              </thead>
              <tbody>
                {filteredEmployees.map((e) => (
                  <tr key={e.id} style={{ opacity: e.active ? 1 : 0.5 }}>
                    <td>
                      <div className="cli">
                        <span className="cli-ava">{initials(e.fullName)}</span>
                        {e.fullName}
                        <span className="chips">
                          <span className="mini-chip">{t("payroll.emp.chips.contractHours", { tip: e.tipContract, hours: e.oreNorma })}</span>
                          {e.exceptieCasMin && EXCEPTIE_LABEL[e.exceptieCasMin] && (
                            <span className="mini-chip ok">{EXCEPTIE_LABEL[e.exceptieCasMin]}</span>
                          )}
                          {e.pensionar && <span className="mini-chip ok">{t("payroll.emp.chips.pensioner")}</span>}
                          {e.tipContract !== "N" && !e.exceptieCasMin && !e.pensionar && (
                            <span className="mini-chip warn">{t("payroll.emp.chips.minBase")}</span>
                          )}
                          {!e.active && <span className="mini-chip">{t("payroll.emp.chips.inactive")}</span>}
                        </span>
                      </div>
                    </td>
                    <td><span className="doc">{e.cnp || "—"}</span></td>
                    <td className="r num">{fmtRON(e.grossSalary)}</td>
                    <td className="r num">{fmtRON(e.personalDeduction)}</td>
                    <td>
                      <div className="row-acts">
                        <button className="mini-btn" title={t("payroll.actions.edit")} onClick={() => setModal({ edit: e })}>
                          <Ic name="pen" />
                        </button>
                        <button
                          className="mini-btn"
                          title={t("payroll.actions.delete")}
                          onClick={async () => {
                            if (await confirm(t("payroll.emp.confirmDelete", { name: e.fullName }), { kind: "warning" })) del.mutate(e.id);
                          }}
                        >
                          <Ic name="xMark" />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <div className="pager">
            <span>
              {t("payroll.emp.footnoteLabel")} <b>{t("payroll.emp.footnoteBold")}</b>
            </span>
            <span></span>
          </div>
        </div>
      </div>

      {/* ── STAT DE SALARII ──────────────────────────────────────────────── */}
      <div className={`panel${tab === 1 ? " show" : ""}`}>
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">{t("payroll.stat.title", { period: `${MONTHS_FULL[month - 1]} ${year}` })}</div>
            <div className="spacer" />
            <button className="pill-btn" disabled={runMut.isPending} onClick={() => runMut.mutate()}>
              <Ic name="calc" />{runMut.isPending ? t("payroll.stat.running") : t("payroll.stat.run")}
            </button>
            {/* propunere — neimplementat (prototipul are Export XLSX fără echivalent backend) */}
            <button className="pill-btn" onClick={() => notify.info(t("payroll.notify.soon"))}>
              <Ic name="dl" />{t("payroll.stat.exportXlsx")}
            </button>
            <button className="pill-btn" onClick={() => window.print()}>
              <Ic name="printer" />{t("payroll.stat.print")}
            </button>
          </div>
          {!run ? (
            <div className="pf-empty">
              {runMut.isPending
                ? t("payroll.stat.calculating")
                : t("payroll.stat.emptyPrompt")}
            </div>
          ) : run.states.length === 0 ? (
            <div className="pf-empty">
              {t("payroll.stat.noActive")}
            </div>
          ) : (
            <table className="scr-table">
              <thead>
                <tr>
                  <th>{t("payroll.stat.th.employee")}</th><th className="r">{t("payroll.emp.th.gross")}</th><th className="r">{t("payroll.stat.th.cas")}</th>
                  <th className="r">{t("payroll.stat.th.cass")}</th><th className="r">{t("payroll.stat.th.tax")}</th>
                  <th className="r">{t("payroll.stat.th.net")}</th><th className="r">{t("payroll.stat.th.cam")}</th>
                  {/* CCI 0,85% column removed — abolished OUG 79/2017, no legal basis post-2018 */}
                </tr>
              </thead>
              <tbody>
                {run.states.map((s) => (
                  <tr key={s.employeeId}>
                    <td><div className="cli"><span className="cli-ava">{initials(s.fullName)}</span>{s.fullName}</div></td>
                    <td className="r num">{fmtRON(s.gross)}</td>
                    <td className="r num">{fmtRON(s.cas)}</td>
                    <td className="r num">{fmtRON(s.cass)}</td>
                    <td className="r num">{fmtRON(s.incomeTax)}</td>
                    <td className="r num"><b>{fmtRON(s.net)}</b></td>
                    <td className="r num">{fmtRON(s.cam)}</td>
                  </tr>
                ))}
                <tr className="total-row">
                  <td>{t("payroll.stat.total")}</td>
                  <td className="r num">{fmtRON(run.totalGross)}</td>
                  <td className="r num">{fmtRON(run.totalCas)}</td>
                  <td className="r num">{fmtRON(run.totalCass)}</td>
                  <td className="r num">{fmtRON(run.totalIncomeTax)}</td>
                  <td className="r num">{fmtRON(run.totalNet)}</td>
                  <td className="r num">{fmtRON(run.totalCam)}</td>
                </tr>
              </tbody>
            </table>
          )}
          <div className="pager">
            <span>
              {t("payroll.stat.footnote")}
              {run?.posted ? <> {t("payroll.stat.postedNote")} <b>{fmtRoDate(run.entryDate)}</b>.</> : null}
            </span>
            <span></span>
          </div>
        </div>
      </div>

      {/* ── CONCEDII MEDICALE ────────────────────────────────────────────── */}
      <div className={`panel${tab === 2 ? " show" : ""}`}>
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">{t("payroll.cm.title", { period: `${MONTHS_FULL[month - 1]} ${year}` })}</div>
            <div className="spacer" />
            <button className="pill-btn" onClick={() => setShowConcediu(true)}>
              <Ic name="plus" />{t("payroll.cm.add")}
            </button>
          </div>
          {leaves.length === 0 ? (
            <div className="pf-empty">
              {t("payroll.cm.empty", { period: `${MONTHS_FULL[month - 1]} ${year}` })}
            </div>
          ) : (
            <table className="scr-table">
              <thead>
                <tr>
                  <th>{t("payroll.stat.th.employee")}</th><th>{t("payroll.cm.th.certificate")}</th><th>{t("payroll.cm.th.code")}</th><th>{t("payroll.cm.th.period")}</th>
                  <th className="r">{t("payroll.cm.th.daysEmployer")}</th><th className="r">{t("payroll.cm.th.daysFnuass")}</th>
                  <th className="r">{t("payroll.cm.th.amountEmployer")}</th><th className="r">{t("payroll.cm.th.amountFnuass")}</th>
                  <th className="r w-del"></th>
                </tr>
              </thead>
              <tbody>
                {leaves.map((l) => {
                  const emp = employees.find((e) => e.id === l.employeeId);
                  const name = emp?.fullName ?? l.employeeId;
                  return (
                    <tr key={l.id}>
                      <td><div className="cli"><span className="cli-ava">{initials(name)}</span>{name}</div></td>
                      <td><span className="doc">{l.serie || l.numar ? t("payroll.cm.certLabel", { serie: l.serie || "—", numar: l.numar || "—" }) : "—"}</span></td>
                      <td>
                        <span className="chip sent">
                          {l.codIndemnizatie}{COD_CM_LABEL[l.codIndemnizatie] ? ` · ${COD_CM_LABEL[l.codIndemnizatie]}` : ""}
                        </span>
                      </td>
                      <td className="num">{fmtRange(l.dataInceput, l.dataSfarsit)}</td>
                      <td className="r num">{l.zileAngajator}</td>
                      <td className="r num">{l.zileFnuass}</td>
                      <td className="r num">{fmtRON(l.sumaAngajator)}</td>
                      <td className="r num">{fmtRON(l.sumaFnuass)}</td>
                      <td>
                        <div className="row-acts">
                          <button
                            className="mini-btn"
                            title={t("payroll.actions.delete")}
                            onClick={async () => {
                              if (await confirm(t("payroll.cm.confirmDelete"), { kind: "warning" })) delConcediu.mutate(l.id);
                            }}
                          >
                            <Ic name="xMark" />
                          </button>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          <div className="pager">
            <span>
              {t("payroll.cm.footnote1")} <b>{t("payroll.cm.footnoteBold")}</b> {t("payroll.cm.footnote2")}
            </span>
            <span></span>
          </div>
        </div>
      </div>

      {/* ── SEDII SECUNDARE ──────────────────────────────────────────────── */}
      <div className={`panel${tab === 3 ? " show" : ""}`}>
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">{t("payroll.sedii.title")}</div>
            <div className="spacer" />
            <button className="pill-btn" onClick={() => setShowSediu(true)}>
              <Ic name="plus" />{t("payroll.sedii.add")}
            </button>
          </div>
          <table className="scr-table">
            <thead>
              <tr><th>{t("payroll.sedii.th.office")}</th><th>{t("payroll.sedii.th.cif")}</th><th className="r">{t("payroll.sedii.th.assigned")}</th><th className="r w-del"></th></tr>
            </thead>
            <tbody>
              <tr>
                <td>
                  <div className="cli">
                    <span className="cli-ava">{initials(activeCompany ? activeCompany.legalName : t("payroll.sedii.main"))}</span>
                    <b>{t("payroll.sedii.main")}{activeCompany ? ` — ${activeCompany.legalName}` : ""}</b>
                  </div>
                </td>
                <td><span className="doc">{activeCompany?.cui ?? "—"}</span></td>
                <td className="r num">{sediuCount("")}</td>
                <td></td>
              </tr>
              {sedii.map((s) => (
                <tr key={s.id}>
                  <td>
                    <div className="cli">
                      <span className="cli-ava">{initials(s.name || t("payroll.sedii.defaultName"))}</span>
                      {s.name || t("payroll.sedii.defaultName")}
                    </div>
                  </td>
                  <td><span className="doc">{s.cif}</span></td>
                  <td className="r num">{sediuCount(s.cif)}</td>
                  <td>
                    <div className="row-acts">
                      <button
                        className="mini-btn"
                        title={t("payroll.actions.delete")}
                        onClick={async () => {
                          if (await confirm(t("payroll.sedii.confirmDelete", { cif: s.cif }), { kind: "warning" })) delSediu.mutate(s.id);
                        }}
                      >
                        <Ic name="xMark" />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="pager">
            <span>
              {t("payroll.sedii.footnote1")} <b>{t("payroll.sedii.footnoteBold")}</b> {t("payroll.sedii.footnote2")}
            </span>
            <span></span>
          </div>
        </div>
      </div>

      {/* ── SIMULATOR SALARIU ────────────────────────────────────────────── */}
      {tab === 4 && <SalarySimPanel />}

      {/* ── PONTAJ (condică de prezență — CM art. 119) ──────────────────── */}
      {tab === 5 && <PontajPanel companyId={companyId} period={periodYm} employees={employees} />}

      {/* modale */}
      {modal && (
        <EmployeeModal
          companyId={companyId}
          employee={modal === "create" ? null : modal.edit}
          sedii={sedii}
          mainCui={activeCompany?.cui ?? ""}
          onClose={() => setModal(null)}
          onSaved={() => {
            setModal(null);
            void qc.invalidateQueries({ queryKey: ["employees", companyId] });
          }}
        />
      )}

      {showConcediu && (
        <ConcediuModal
          companyId={companyId}
          periodYm={periodYm}
          monthLabel={`${MONTHS_FULL[month - 1]} ${year}`}
          employees={employees}
          onClose={() => setShowConcediu(false)}
          onSaved={() => {
            setShowConcediu(false);
            void qc.invalidateQueries({ queryKey: ["concedii", companyId, periodYm] });
          }}
        />
      )}

      {showSediu && (
        <SediuModal
          companyId={companyId}
          onClose={() => setShowSediu(false)}
          onSaved={() => {
            setShowSediu(false);
            void qc.invalidateQueries({ queryKey: ["sedii", companyId] });
          }}
        />
      )}

      {showD112 && (
        <D112Modal
          monthLabel={`${MONTHS_FULL[month - 1]} ${year}`}
          newModel={year > 2026 || (year === 2026 && month >= 7)}
          dukBlock={dukBlock}
          isRectificative={d112Rectif}
          onRectificativeChange={setD112Rectif}
          onClose={() => { setShowD112(false); setDukBlock(null); }}
          onExport={runD112}
          onPreview={runD112Preview}
        />
      )}
    </div>
  );
}

// ─── EmployeeModal — design .modal-back/.modal.lg with .fgrid fields ──────────

function EmployeeModal({
  companyId, employee, sedii, mainCui, onClose, onSaved,
}: {
  companyId: string;
  employee: Employee | null;
  sedii: SecondaryOffice[];
  mainCui: string;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const isEdit = employee !== null;
  const [form, setForm] = useState({
    cnp: employee?.cnp ?? "",
    fullName: employee?.fullName ?? "",
    grossSalary: employee?.grossSalary ?? "",
    personalDeduction: employee?.personalDeduction ?? "0",
    tipContract: employee?.tipContract ?? "N",
    oreNorma: employee ? String(employee.oreNorma) : "8",
    pensionar: employee?.pensionar ?? false,
    exceptieCasMin: employee?.exceptieCasMin ?? "",
    sediuCif: employee?.sediuCif ?? "",
    beneficiarSumaNetaxabila: employee?.beneficiarSumaNetaxabila ?? false,
    employmentDate: employee?.employmentDate ?? "",
    contractEndDate: employee?.contractEndDate ?? "",
  });
  const [error, setError] = useState<string | null>(null);

  const save = useMutation({
    mutationFn: () => {
      if (!form.fullName.trim()) throw new Error(t("payroll.empModal.nameRequired"));
      const payload = {
        cnp: form.cnp,
        fullName: form.fullName,
        grossSalary: form.grossSalary,
        personalDeduction: form.personalDeduction,
        tipContract: form.tipContract,
        oreNorma: Number(form.oreNorma) || 8,
        pensionar: form.pensionar,
        exceptieCasMin: form.exceptieCasMin,
        sediuCif: form.sediuCif,
        beneficiarSumaNetaxabila: form.beneficiarSumaNetaxabila,
        employmentDate: form.employmentDate || undefined,
        contractEndDate: form.contractEndDate || undefined,
      };
      if (isEdit) {
        return api.payroll.update(employee!.id, companyId, payload);
      }
      const input: CreateEmployeeInput = { companyId, ...payload };
      return api.payroll.create(input);
    },
    onSuccess: onSaved,
    onError: (e) => setError(formatError(e, t("payroll.notify.saveError"))),
  });

  const { closing, close } = useAnimatedClose(onClose);

  type StrKey = "cnp" | "fullName" | "grossSalary" | "personalDeduction" | "oreNorma";
  const field = (k: StrKey) => ({
    value: form[k],
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => setForm((f) => ({ ...f, [k]: e.target.value })),
  });

  return createPortal(
    <div
      className={`modal-back ${closing ? "closing" : "show"}`}
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) close(); }}
    >
      <div className="modal lg">
        <div className="modal-head">
          <div>
            <div className="mt">{isEdit ? t("payroll.empModal.editTitle", { name: employee.fullName }) : t("payroll.actions.newEmployee")}</div>
            <div className="ms">{t("payroll.empModal.subtitle")}</div>
          </div>
          <button className="modal-x" onClick={close} aria-label={t("payroll.common.close")}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>{t("payroll.empModal.fullName")} <span className="req">*</span></label>
              <input className="input" type="text" placeholder={t("payroll.empModal.namePlaceholder")} {...field("fullName")} autoFocus />
            </div>
            <div className="field">
              <label>{t("payroll.empModal.cnp")}</label>
              <input className="input num" type="text" placeholder="1900101…" {...field("cnp")} />
            </div>
            <div className="field">
              <label>{t("payroll.empModal.gross")} <span className="req">*</span></label>
              <input className="input num num-r" type="text" inputMode="decimal" placeholder="5000" {...field("grossSalary")} />
            </div>
            <div className="field">
              <label>{t("payroll.empModal.deduction")}</label>
              <input className="input num num-r" type="text" inputMode="decimal" placeholder="0" {...field("personalDeduction")} />
            </div>
            <div className="field">
              <label>{t("payroll.empModal.contractType")}</label>
              <select
                className="select"
                value={form.tipContract}
                onChange={(e) => setForm((f) => ({ ...f, tipContract: e.target.value }))}
              >
                <option value="N">{t("payroll.empModal.contractFull")}</option>
                {[1, 2, 3, 4, 5, 6, 7].map((n) => (
                  <option key={n} value={`P${n}`}>{t("payroll.empModal.contractPart", { n })}</option>
                ))}
              </select>
            </div>
            <div className="field">
              <label>{t("payroll.empModal.hoursPerDay")}</label>
              <input className="input num" type="text" inputMode="numeric" placeholder="8" {...field("oreNorma")} />
            </div>
            <div className="field">
              <label>{t("payroll.empModal.employmentDate")}</label>
              <input
                className="input"
                type="date"
                value={form.employmentDate}
                onChange={(e) => setForm((f) => ({ ...f, employmentDate: e.target.value }))}
              />
            </div>
            <div className="field">
              <label>{t("payroll.empModal.contractEndDate")}</label>
              <input
                className="input"
                type="date"
                value={form.contractEndDate}
                onChange={(e) => setForm((f) => ({ ...f, contractEndDate: e.target.value }))}
              />
              <div className="hint">{t("payroll.empModal.contractEndDateHint")}</div>
            </div>
            <div className="field">
              <label>{t("payroll.empModal.pensioner")}</label>
              <select
                className="select"
                value={form.pensionar ? "da" : "nu"}
                onChange={(e) => setForm((f) => ({ ...f, pensionar: e.target.value === "da" }))}
              >
                <option value="da">{t("payroll.common.yes")}</option>
                <option value="nu">{t("payroll.common.no")}</option>
              </select>
            </div>
            <div className="field">
              <label>{t("payroll.empModal.exception")}</label>
              <select
                className="select"
                value={form.exceptieCasMin}
                onChange={(e) => setForm((f) => ({ ...f, exceptieCasMin: e.target.value }))}
              >
                <option value="">{t("payroll.empModal.excNone")}</option>
                <option value="elev_student">{t("payroll.empModal.excElevStudent")}</option>
                <option value="ucenic">{t("payroll.empModal.excUcenic")}</option>
                <option value="dizabilitate">{t("payroll.empModal.excDizabilitate")}</option>
                <option value="contracte_multiple">{t("payroll.empModal.excContracteMultiple")}</option>
              </select>
            </div>
            <div className="field span2">
              <label>{t("payroll.empModal.office")}</label>
              <select
                className="select"
                value={form.sediuCif}
                onChange={(e) => setForm((f) => ({ ...f, sediuCif: e.target.value }))}
              >
                <option value="">{t("payroll.sedii.main")}{mainCui ? ` · ${mainCui}` : ""}</option>
                {sedii.map((s) => (
                  <option key={s.id} value={s.cif}>{s.name ? `${s.name} · ${s.cif}` : s.cif}</option>
                ))}
              </select>
            </div>
            <div className="field span2">
              <label className="chk-row">
                <input
                  type="checkbox"
                  checked={form.beneficiarSumaNetaxabila}
                  onChange={(e) => setForm((f) => ({ ...f, beneficiarSumaNetaxabila: e.target.checked }))}
                />
                <span>{t("payroll.empModal.beneficiar200")}</span>
              </label>
              <div className="hint">{t("payroll.empModal.beneficiar200Hint")}</div>
            </div>
            {error && (
              <div className="field span2">
                <div className="banner danger no-mb">
                  <Ic name="triangle" />
                  <span>{error}</span>
                </div>
              </div>
            )}
          </div>
        </div>
        <div className="modal-foot">
          <button className="pill-btn" onClick={close} disabled={save.isPending}>{t("payroll.common.cancel")}</button>
          <button className="btn-dark" disabled={save.isPending} onClick={() => save.mutate()}>
            <Ic name="check" />{save.isPending ? t("payroll.common.saving") : t("payroll.empModal.save")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ─── ConcediuModal — certificat CM (OUG 158/2005, D112 asiguratD) ─────────────

function ConcediuModal({
  companyId, periodYm, monthLabel, employees, onClose, onSaved,
}: {
  companyId: string;
  periodYm: string;
  monthLabel: string;
  employees: Employee[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [f, setF] = useState({
    employeeId: "", serie: "", numar: "", codIndemnizatie: "01",
    dataAcordare: "", dataInceput: "", dataSfarsit: "", zileAngajator: "", zileFnuass: "",
    bazaCalcul: "", zileBaza: "", sumaAngajator: "", sumaFnuass: "", procent: "",
    // procent (D_28): 55/65/75 per scala OUG 91/2025 — introdus de utilizator
    locPrescriere: "1", codBoala: "",
  });
  const [error, setError] = useState<string | null>(null);
  // D_23: risc maternal (cod 15) is always "RM" — the input below locks to it.
  const codBoala = f.codIndemnizatie === "15" ? "RM" : f.codBoala;

  const add = useMutation({
    mutationFn: () => {
      if (!f.employeeId) throw new Error(t("payroll.cmModal.selectRequired"));
      return api.payroll.createConcediu({
        companyId, employeeId: f.employeeId, periodYm,
        serie: f.serie, numar: f.numar, codIndemnizatie: f.codIndemnizatie,
        dataAcordare: f.dataAcordare, dataInceput: f.dataInceput, dataSfarsit: f.dataSfarsit,
        zileAngajator: Number(f.zileAngajator) || 0, zileFnuass: Number(f.zileFnuass) || 0,
        bazaCalcul: f.bazaCalcul || "0", zileBaza: Number(f.zileBaza) || 0,
        sumaAngajator: f.sumaAngajator || "0", sumaFnuass: f.sumaFnuass || "0",
        // Procentul (D_28) se trimite EXACT cum a fost introdus — fără default tăcut de 75%. Gol → câmp
        // omis (undefined → None în backend), astfel încât validarea pentru cod 01 (OUG 91/2025) să ceară
        // o alegere conștientă 55/65/75 în loc să accepte un 75% injectat tăcut.
        procent: f.procent.trim() === "" ? undefined : Number(f.procent),
        locPrescriere: Number(f.locPrescriere) || 1, codBoala,
      });
    },
    onSuccess: onSaved,
    onError: (e) => setError(formatError(e, t("payroll.notify.addConcediuError"))),
  });

  const { closing, close } = useAnimatedClose(onClose);

  const num = (k: keyof typeof f) => ({
    value: f[k],
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => setF((s) => ({ ...s, [k]: e.target.value })),
  });

  return createPortal(
    <div
      className={`modal-back ${closing ? "closing" : "show"}`}
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) close(); }}
    >
      <div className="modal lg">
        <div className="modal-head">
          <div>
            <div className="mt">{t("payroll.cmModal.title")}</div>
            <div className="ms">{t("payroll.cmModal.subtitle", { month: monthLabel })}</div>
          </div>
          <button className="modal-x" onClick={close} aria-label={t("payroll.common.close")}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field span2">
              <label>{t("payroll.stat.th.employee")} <span className="req">*</span></label>
              <select
                className="select"
                value={f.employeeId}
                onChange={(e) => setF((s) => ({ ...s, employeeId: e.target.value }))}
                autoFocus
              >
                <option value="">{t("payroll.cmModal.selectEmployee")}</option>
                {employees.map((e) => <option key={e.id} value={e.id}>{e.fullName}</option>)}
              </select>
            </div>
            <div className="field">
              <label>{t("payroll.cmModal.serie")}</label>
              <input className="input num" type="text" placeholder="CCMAH" {...num("serie")} />
            </div>
            <div className="field">
              <label>{t("payroll.cmModal.numar")}</label>
              <input className="input num" type="text" placeholder="8841220" {...num("numar")} />
            </div>
            <div className="field span2">
              <label>{t("payroll.cmModal.code")}</label>
              <select
                className="select"
                value={f.codIndemnizatie}
                onChange={(e) => setF((s) => ({ ...s, codIndemnizatie: e.target.value }))}
              >
                <option value="01">01 — {t("payroll.cm.codes.c01")}</option>
                <option value="06">06 — {t("payroll.cm.codes.c06")}</option>
                <option value="09">09 — {t("payroll.cm.codes.c09")}</option>
                <option value="15">15 — {t("payroll.cm.codes.c15")}</option>
              </select>
            </div>
            <div className="field">
              <label>{t("payroll.cmModal.locPrescriere")}</label>
              <select
                className="select"
                value={f.locPrescriere}
                onChange={(e) => setF((s) => ({ ...s, locPrescriere: e.target.value }))}
              >
                <option value="1">1 — {t("payroll.cm.locPrescriere.l1")}</option>
                <option value="2">2 — {t("payroll.cm.locPrescriere.l2")}</option>
                <option value="3">3 — {t("payroll.cm.locPrescriere.l3")}</option>
                <option value="4">4 — {t("payroll.cm.locPrescriere.l4")}</option>
              </select>
            </div>
            <div className="field">
              <label>{t("payroll.cmModal.codBoala")}</label>
              <input
                className="input num uppercase"
                type="text"
                maxLength={3}
                placeholder="A09"
                value={codBoala}
                disabled={f.codIndemnizatie === "15"}
                onChange={(e) => setF((s) => ({ ...s, codBoala: e.target.value.toUpperCase() }))}
              />
            </div>
            <div className="field">
              <label>{t("payroll.cmModal.dateGranted")}</label>
              <input className="input num" type="date" {...num("dataAcordare")} />
            </div>
            <div className="field">
              <label>{t("payroll.cmModal.dateStart")}</label>
              <input className="input num" type="date" {...num("dataInceput")} />
            </div>
            <div className="field">
              <label>{t("payroll.cmModal.dateEnd")}</label>
              <input className="input num" type="date" {...num("dataSfarsit")} />
            </div>
            <div className="field">
              <label>{t("payroll.cm.th.daysEmployer")}</label>
              <input className="input num num-r" type="text" inputMode="numeric" placeholder="5" {...num("zileAngajator")} />
            </div>
            <div className="field">
              <label>{t("payroll.cm.th.daysFnuass")}</label>
              <input className="input num num-r" type="text" inputMode="numeric" placeholder="0" {...num("zileFnuass")} />
            </div>
            <div className="field">
              <label>{t("payroll.cmModal.amountEmployer")}</label>
              <input className="input num num-r" type="text" inputMode="decimal" placeholder={t("payroll.common.zeroAmount")} {...num("sumaAngajator")} />
            </div>
            <div className="field">
              <label>{t("payroll.cmModal.amountFnuass")}</label>
              <input className="input num num-r" type="text" inputMode="decimal" placeholder={t("payroll.common.zeroAmount")} {...num("sumaFnuass")} />
            </div>
            <div className="field">
              <label>{t("payroll.cmModal.baza")}</label>
              <input className="input num num-r" type="text" inputMode="decimal" placeholder={t("payroll.common.zeroAmount")} {...num("bazaCalcul")} />
            </div>
            <div className="field">
              <label>{t("payroll.cmModal.zileBaza")}</label>
              <input className="input num num-r" type="text" inputMode="numeric" placeholder="0" {...num("zileBaza")} />
            </div>
            <div className="field">
              <label>{t("payroll.cmModal.procent")}</label>
              {/* procent D_28: 55/65/75 per OUG 91/2025 — utilizatorul alege */}
              <input className="input num num-r" type="text" inputMode="numeric" placeholder="55/65/75" {...num("procent")} />
            </div>
            {error && (
              <div className="field span2">
                <div className="banner danger no-mb">
                  <Ic name="triangle" />
                  <span>{error}</span>
                </div>
              </div>
            )}
          </div>
        </div>
        <div className="modal-foot">
          <span className="left">{t("payroll.cm.cod01Note")}</span>
          <button className="pill-btn" onClick={close} disabled={add.isPending}>{t("payroll.common.cancel")}</button>
          <button className="btn-dark" disabled={add.isPending || !f.employeeId} onClick={() => add.mutate()}>
            <Ic name="check" />{add.isPending ? t("payroll.common.saving") : t("payroll.cmModal.save")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ─── SediuModal — sediu secundar (D112 angajatorF2) ───────────────────────────

function SediuModal({
  companyId, onClose, onSaved,
}: {
  companyId: string;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [cif, setCif] = useState("");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);

  const add = useMutation({
    mutationFn: () => api.payroll.createSediu(companyId, cif.trim(), name.trim()),
    onSuccess: onSaved,
    onError: (e) => setError(formatError(e, t("payroll.notify.addSediuError"))),
  });

  const { closing, close } = useAnimatedClose(onClose);

  return createPortal(
    <div
      className={`modal-back ${closing ? "closing" : "show"}`}
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) close(); }}
    >
      <div className="modal">
        <div className="modal-head">
          <div>
            <div className="mt">{t("payroll.sediuModal.title")}</div>
            <div className="ms">{t("payroll.sediuModal.subtitle")}</div>
          </div>
          <button className="modal-x" onClick={close} aria-label={t("payroll.common.close")}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>{t("payroll.sediuModal.cif")} <span className="req">*</span></label>
              <input className="input num" type="text" placeholder="49102337" value={cif} onChange={(e) => setCif(e.target.value)} autoFocus />
            </div>
            <div className="field">
              <label>{t("payroll.sediuModal.name")}</label>
              <input className="input" type="text" placeholder={t("payroll.sediuModal.namePlaceholder")} value={name} onChange={(e) => setName(e.target.value)} />
            </div>
            {error && (
              <div className="field span2">
                <div className="banner danger no-mb">
                  <Ic name="triangle" />
                  <span>{error}</span>
                </div>
              </div>
            )}
          </div>
        </div>
        <div className="modal-foot">
          <button className="pill-btn" onClick={close} disabled={add.isPending}>{t("payroll.common.cancel")}</button>
          <button className="btn-dark" disabled={add.isPending || !cif.trim()} onClick={() => add.mutate()}>
            <Ic name="check" />{add.isPending ? t("payroll.common.saving") : t("payroll.sediuModal.save")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ─── D112Modal — export XML cu CAEN (înlocuiește window.prompt, no-op în Tauri) ─

function D112Modal({
  monthLabel, newModel, dukBlock, isRectificative, onRectificativeChange, onClose, onExport, onPreview,
}: {
  monthLabel: string;
  newModel: boolean;
  dukBlock: PreflightIssue[] | null;
  isRectificative: boolean;
  onRectificativeChange: (v: boolean) => void;
  onClose: () => void;
  onExport: (caen: string, isRectificative: boolean, override?: boolean) => Promise<void>;
  onPreview: (caen: string, isRectificative: boolean) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [caen, setCaen] = useState("");
  const [busy, setBusy] = useState(false);
  const [previewing, setPreviewing] = useState(false);

  const { closing, close } = useAnimatedClose(onClose);

  const submit = async (override = false) => {
    if (!/^\d{4}$/.test(caen.trim())) { notify.error(t("payroll.d112.invalidCaen")); return; }
    setBusy(true);
    try {
      await onExport(caen.trim(), isRectificative, override);
    } finally {
      setBusy(false);
    }
  };

  const preview = async () => {
    if (!/^\d{4}$/.test(caen.trim())) { notify.error(t("payroll.d112.invalidCaen")); return; }
    setPreviewing(true);
    try {
      await onPreview(caen.trim(), isRectificative);
    } finally {
      setPreviewing(false);
    }
  };

  return createPortal(
    <div
      className={`modal-back ${closing ? "closing" : "show"}`}
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) close(); }}
    >
      <div className="modal">
        <div className="modal-head">
          <div>
            <div className="mt">{t("payroll.actions.exportD112")}</div>
            <div className="ms">
              {monthLabel} · {newModel
                ? t("payroll.d112.subNew")
                : t("payroll.d112.subOld")}
            </div>
          </div>
          <button className="modal-x" onClick={close} aria-label={t("payroll.common.close")}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>{t("payroll.d112.caen")} <span className="req">*</span></label>
              <input
                className="input num"
                type="text"
                placeholder="6201"
                value={caen}
                onChange={(e) => setCaen(e.target.value)}
                autoFocus
              />
              <span className="hint">{t("payroll.d112.caenHint")}</span>
            </div>
            <div className="field">
              <label>{t("payroll.d112.reportMonth")}</label>
              <input
                className="input num display-only"
                type="text"
                value={monthLabel}
                disabled
              />
            </div>
          </div>

          {/* Declarație rectificativă — toggle per export, în aceeași sesiune cu CAEN. */}
          <div className="field" style={{ marginTop: 14 }}>
            <label className="chk-row">
              <input
                type="checkbox"
                checked={isRectificative}
                onChange={(e) => onRectificativeChange(e.target.checked)}
              />
              <span>{t("payroll.d112.rectificative")}</span>
            </label>
            <div className="hint">{t("payroll.d112.rectificativeHint")}</div>
          </div>

          {/* DUK block panel — validatorul oficial ANAF a raportat erori; oferă „exportă oricum". */}
          {dukBlock && (
            <div style={{ marginTop: 14 }}>
              <PreflightPanel issues={dukBlock} />
              <button
                className="pill-btn"
                style={{ marginTop: 8, color: "var(--red)", borderColor: "rgba(220,38,38,.35)" }}
                disabled={busy}
                onClick={() => void submit(true)}
              >
                {t("declarations.common.exportAnyway")}
              </button>
            </div>
          )}
        </div>
        <div className="modal-foot">
          <span className="left">{t("payroll.d112.foot")}</span>
          <button className="pill-btn" onClick={close} disabled={busy || previewing}>{t("payroll.common.cancel")}</button>
          <button className="pill-btn" disabled={busy || previewing} onClick={() => void preview()}>
            <Ic name="eye" />{previewing ? t("payroll.d112.previewing") : t("payroll.d112.previewXml")}
          </button>
          <button className="btn-dark" disabled={busy || previewing} onClick={() => void submit()}>
            <Ic name="code" />{busy ? t("payroll.d112.exporting") : t("payroll.d112.generate")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ─── Simulator salariu (tab 4) ─────────────────────────────────────────────────

import type { SalarySimResult, SalarySimOpts } from "@/types";

function SalarySimPanel() {
  const { t } = useTranslation();
  const now = new Date();
  const [simMode, setSimMode] = useState<"gross" | "net">("gross");
  const [simInput, setSimInput] = useState("");
  const [simDependents, setSimDependents] = useState(0);
  const [simBeneficiar, setSimBeneficiar] = useState(false);
  const [simMonth, setSimMonth] = useState(now.getMonth() + 1);
  const [simYear, setSimYear] = useState(now.getFullYear());
  const [simResult, setSimResult] = useState<SalarySimResult | null>(null);
  const [simError, setSimError] = useState<string | null>(null);
  const [simLoading, setSimLoading] = useState(false);

  const simOpts: SalarySimOpts = {
    dependents: simDependents,
    beneficiarSumaNetaxabila: simBeneficiar,
    month: simMonth,
    year: simYear,
  };

  const handleCalc = async () => {
    const v = simInput.trim();
    if (!v) { setSimError(t("payroll.sim.errorEmpty")); return; }
    if (!/^[\d.,]+$/.test(v)) { setSimError(t("payroll.sim.errorInvalid")); return; }
    const num = parseFloat(v.replace(",", "."));
    if (isNaN(num)) { setSimError(t("payroll.sim.errorInvalid")); return; }
    if (num < 0) { setSimError(t("payroll.sim.errorNegative")); return; }
    setSimError(null);
    setSimLoading(true);
    try {
      const r = simMode === "gross"
        ? await api.payroll.simulateSalary(String(num), simOpts)
        : await api.payroll.simulateSalaryFromNet(String(num), simOpts);
      setSimResult(r);
    } catch (e: unknown) {
      setSimError(e instanceof Error ? e.message : String(e));
    } finally {
      setSimLoading(false);
    }
  };

  const fmtAmt = (s: string) => {
    const n = parseFloat(s);
    if (isNaN(n)) return s;
    return n.toLocaleString("ro-RO", { minimumFractionDigits: 2, maximumFractionDigits: 2 }) + " RON";
  };

  const SIM_MONTHS = [
    t("payroll.months.jan"), t("payroll.months.feb"), t("payroll.months.mar"),
    t("payroll.months.apr"), t("payroll.months.may"), t("payroll.months.jun"),
    t("payroll.months.jul"), t("payroll.months.aug"), t("payroll.months.sep"),
    t("payroll.months.oct"), t("payroll.months.nov"), t("payroll.months.dec"),
  ];

  return (
    <div className="panel show">
      <div className="scr-card" style={{ maxWidth: 680 }}>
        <div className="scr-toolbar">
          <div>
            <div className="tt">{t("payroll.sim.title")}</div>
            <div style={{ fontSize: 12, color: "var(--txt-sub)", marginTop: 2 }}>{t("payroll.sim.subtitle")}</div>
          </div>
        </div>

        {/* mode toggle */}
        <div style={{ display: "flex", gap: 8, padding: "12px 16px 0" }}>
          <button
            className={`pill-btn${simMode === "gross" ? "" : " secondary"}`}
            onClick={() => { setSimMode("gross"); setSimResult(null); }}
          >{t("payroll.sim.modeGross")}</button>
          <button
            className={`pill-btn${simMode === "net" ? "" : " secondary"}`}
            onClick={() => { setSimMode("net"); setSimResult(null); }}
          >{t("payroll.sim.modeNet")}</button>
        </div>

        {/* inputs */}
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12, padding: "12px 16px" }}>
          <div>
            <label style={{ fontSize: 12, fontWeight: 600, display: "block", marginBottom: 4 }}>
              {simMode === "gross" ? t("payroll.sim.labelGross") : t("payroll.sim.labelNet")}
            </label>
            <input
              className="form-input"
              type="text"
              inputMode="decimal"
              placeholder="ex. 5000"
              value={simInput}
              onChange={(e) => { setSimInput(e.target.value); setSimResult(null); setSimError(null); }}
              onKeyDown={(e) => { if (e.key === "Enter") void handleCalc(); }}
              style={{ width: "100%", boxSizing: "border-box" }}
            />
          </div>
          <div>
            <label style={{ fontSize: 12, fontWeight: 600, display: "block", marginBottom: 4 }}>
              {t("payroll.sim.labelDependents")}
            </label>
            <select
              className="form-input"
              value={simDependents}
              onChange={(e) => { setSimDependents(Number(e.target.value)); setSimResult(null); }}
              style={{ width: "100%" }}
            >
              {[0, 1, 2, 3, 4].map((n) => (
                <option key={n} value={n}>{n === 4 ? "≥4" : n}</option>
              ))}
            </select>
          </div>
          <div>
            <label style={{ fontSize: 12, fontWeight: 600, display: "block", marginBottom: 4 }}>
              {t("payroll.sim.labelMonth")}
            </label>
            <select
              className="form-input"
              value={simMonth}
              onChange={(e) => { setSimMonth(Number(e.target.value)); setSimResult(null); }}
              style={{ width: "100%" }}
            >
              {SIM_MONTHS.map((m, i) => <option key={i + 1} value={i + 1}>{m}</option>)}
            </select>
          </div>
          <div>
            <label style={{ fontSize: 12, fontWeight: 600, display: "block", marginBottom: 4 }}>
              {t("payroll.sim.labelYear")}
            </label>
            <input
              className="form-input"
              type="number"
              value={simYear}
              onChange={(e) => { setSimYear(Number(e.target.value)); setSimResult(null); }}
              style={{ width: "100%", boxSizing: "border-box" }}
            />
          </div>
          <div style={{ gridColumn: "1 / -1", display: "flex", alignItems: "flex-start", gap: 8 }}>
            <input
              type="checkbox"
              id="sim-beneficiar"
              checked={simBeneficiar}
              onChange={(e) => { setSimBeneficiar(e.target.checked); setSimResult(null); }}
              style={{ marginTop: 3 }}
            />
            <label htmlFor="sim-beneficiar" style={{ fontSize: 13, cursor: "pointer" }}>
              <b>{t("payroll.sim.labelBeneficiar")}</b>
              <span style={{ color: "var(--txt-sub)", fontSize: 11, display: "block" }}>
                {t("payroll.sim.labelBeneficiarHint")}
              </span>
            </label>
          </div>
        </div>

        {simError && (
          <div style={{ margin: "0 16px 8px", color: "var(--red)", fontSize: 13 }}>{simError}</div>
        )}

        <div style={{ padding: "0 16px 16px", display: "flex", gap: 8, alignItems: "center" }}>
          <button className="btn-dark" onClick={() => void handleCalc()} disabled={simLoading}>
            {simLoading ? "…" : t("payroll.sim.btnCalc")}
          </button>
          <span style={{ fontSize: 12, color: "var(--txt-sub)" }}>{t("payroll.sim.calcHint")}</span>
        </div>

        {simResult && (
          <div style={{ borderTop: "1px solid var(--brd)", padding: "12px 16px 16px" }}>
            <div style={{ fontWeight: 700, fontSize: 12, textTransform: "uppercase", letterSpacing: "0.05em", color: "var(--txt-sub)", marginBottom: 6 }}>
              {t("payroll.sim.sectionEmployee")}
            </div>
            <table style={{ width: "100%", fontSize: 14, borderCollapse: "collapse" }}>
              <tbody>
                <SimRow label={simMode === "net" ? `${t("payroll.sim.labelGross")} (calculat)` : t("payroll.sim.labelGross")} value={fmtAmt(simResult.gross)} bold />
                <SimRow label={t("payroll.sim.rowCas")} value={`− ${fmtAmt(simResult.cas)}`} />
                <SimRow label={t("payroll.sim.rowCass")} value={`− ${fmtAmt(simResult.cass)}`} />
                {parseFloat(simResult.nonTaxable) > 0 && (
                  <SimRow label={t("payroll.sim.rowNonTaxable")} value={`− ${fmtAmt(simResult.nonTaxable)}`} note />
                )}
                {parseFloat(simResult.deducereEfectiva) > 0 && (
                  <SimRow label={t("payroll.sim.rowDeducere")} value={`− ${fmtAmt(simResult.deducereEfectiva)}`} note />
                )}
                <SimRow label={t("payroll.sim.rowImpozitBase")} value={fmtAmt(simResult.impozitBase)} />
                <SimRow label={t("payroll.sim.rowImpozit")} value={`− ${fmtAmt(simResult.impozit)}`} />
                <SimRow label={t("payroll.sim.rowNet")} value={fmtAmt(simResult.net)} bold highlight />
              </tbody>
            </table>

            <div style={{ fontWeight: 700, fontSize: 12, textTransform: "uppercase", letterSpacing: "0.05em", color: "var(--txt-sub)", margin: "14px 0 6px" }}>
              {t("payroll.sim.sectionEmployer")}
            </div>
            <table style={{ width: "100%", fontSize: 14, borderCollapse: "collapse" }}>
              <tbody>
                <SimRow label={t("payroll.sim.labelGross")} value={fmtAmt(simResult.gross)} />
                <SimRow label={t("payroll.sim.rowCam")} value={`+ ${fmtAmt(simResult.cam)}`} />
                <SimRow label={t("payroll.sim.rowTotalCost")} value={fmtAmt(simResult.totalEmployerCost)} bold highlight />
              </tbody>
            </table>

            <div style={{ marginTop: 12, fontSize: 11, color: "var(--txt-sub)", display: "flex", flexDirection: "column", gap: 3 }}>
              {parseFloat(simResult.deducereEfectiva) > 0 && (
                <span>ℹ {t("payroll.sim.noteDedApplied", { amount: simResult.deducereTabel, n: simDependents, persoane: t("payroll.sim.persoane", { count: simDependents }) })}</span>
              )}
              {parseFloat(simResult.deducereTabel) > 0 && parseFloat(simResult.deducereEfectiva) === 0 && (
                <span>ℹ {t("payroll.sim.noteDeducereZero")}</span>
              )}
              {simResult.carveoutApplied && (
                <span>ℹ {t("payroll.sim.noteCarveout")}</span>
              )}
              <span>ℹ {t("payroll.sim.noteNoCci")}</span>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function SimRow({ label, value, bold, note, highlight }: { label: string; value: string; bold?: boolean; note?: boolean; highlight?: boolean }) {
  return (
    <tr style={{ borderBottom: "1px solid var(--brd)" }}>
      <td style={{ padding: "5px 0", color: note ? "var(--txt-sub)" : undefined, fontStyle: note ? "italic" : undefined }}>
        {label}
      </td>
      <td style={{
        padding: "5px 0",
        textAlign: "right",
        fontWeight: bold ? 700 : undefined,
        color: highlight ? "var(--accent)" : undefined,
        fontVariantNumeric: "tabular-nums",
      }}>
        {value}
      </td>
    </tr>
  );
}

// ─── Pontaj tab (tab 5) ─────────────────────────────────────────────────────

interface PontajPanelProps {
  companyId: string;
  period: string;  // YYYY-MM
  employees: import("@/types").Employee[];
}

function PontajPanel({ companyId, period, employees }: PontajPanelProps) {
  const { t } = useTranslation();
  const [pontaje, setPontaje] = useState<import("@/types").Pontaj[]>([]);
  const [editing, setEditing] = useState<{ empId: string; pontaj: import("@/types").Pontaj | null } | null>(null);
  const [saving, setSaving] = useState(false);
  const [form, setForm] = useState({ workedDays: "0", overtimeHours: "0", nightHours: "0", absenceDays: "0", leaveDays: "0", notes: "" });

  const activeEmps = employees.filter(e => e.active);

  useEffect(() => {
    if (!companyId || !period) return;
    api.payroll.listPontaje(companyId, period).then(setPontaje).catch(() => {});
  }, [companyId, period]);

  // Map employee_id → Pontaj
  const pontajMap = useMemo(() => {
    const m: Record<string, import("@/types").Pontaj> = {};
    for (const p of pontaje) m[p.employeeId] = p;
    return m;
  }, [pontaje]);

  function openEdit(emp: import("@/types").Employee) {
    const pj = pontajMap[emp.id] ?? null;
    setEditing({ empId: emp.id, pontaj: pj });
    setForm({
      workedDays: pj ? String(pj.workedDays) : "0",
      overtimeHours: pj ? pj.overtimeHours : "0",
      nightHours: pj ? pj.nightHours : "0",
      absenceDays: pj ? String(pj.absenceDays) : "0",
      leaveDays: pj ? String(pj.leaveDays) : "0",
      notes: pj ? pj.notes : "",
    });
  }

  async function save() {
    if (!editing) return;
    setSaving(true);
    try {
      const wdNum = parseInt(form.workedDays, 10) || 0;
      const abNum = parseInt(form.absenceDays, 10) || 0;
      const lvNum = parseInt(form.leaveDays, 10) || 0;
      let updated: import("@/types").Pontaj;
      if (editing.pontaj) {
        const input: import("@/types").UpdatePontajInput = {
          workedDays: wdNum,
          overtimeHours: form.overtimeHours || "0",
          nightHours: form.nightHours || "0",
          absenceDays: abNum,
          leaveDays: lvNum,
          notes: form.notes,
        };
        updated = await api.payroll.updatePontaj(editing.pontaj.id, companyId, input);
      } else {
        const input: import("@/types").CreatePontajInput = {
          companyId,
          employeeId: editing.empId,
          period,
          workedDays: wdNum,
          overtimeHours: form.overtimeHours || "0",
          nightHours: form.nightHours || "0",
          absenceDays: abNum,
          leaveDays: lvNum,
          notes: form.notes,
        };
        updated = await api.payroll.createPontaj(input);
      }
      setPontaje(prev => {
        const without = prev.filter(p => p.employeeId !== editing.empId);
        return [...without, updated];
      });
      setEditing(null);
    } catch (e) {
      console.error(e);
    } finally {
      setSaving(false);
    }
  }

  async function removePontaj(pj: import("@/types").Pontaj) {
    const empName = activeEmps.find(e => e.id === pj.employeeId)?.fullName ?? pj.employeeId;
    if (!window.confirm(t("payroll.pontaj.confirmDelete", { name: empName }))) return;
    await api.payroll.deletePontaj(pj.id, companyId).catch(() => {});
    setPontaje(prev => prev.filter(p => p.id !== pj.id));
  }

  return (
    <div className="pontaj-panel" style={{ padding: "16px 0" }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
        <div>
          <div className="tt">{t("payroll.pontaj.title", { period })}</div>
          <div style={{ fontSize: 12, color: "var(--txt-sub)", marginTop: 2 }}>{t("payroll.pontaj.subtitle")}</div>
        </div>
        <button className="btn-secondary" onClick={() => window.print()} style={{ fontSize: 12 }}>
          {t("payroll.pontaj.print")}
        </button>
      </div>

      {activeEmps.length === 0 ? (
        <div style={{ color: "var(--txt-sub)", fontSize: 13 }}>{t("payroll.pontaj.empty", { period })}</div>
      ) : (
        <table className="scr-table">
          <thead>
            <tr>
              <th>{t("payroll.pontaj.th.employee")}</th>
              <th style={{ textAlign: "center" }}>{t("payroll.pontaj.th.workedDays")}</th>
              <th style={{ textAlign: "center" }}>{t("payroll.pontaj.th.overtimeHours")}</th>
              <th style={{ textAlign: "center" }}>{t("payroll.pontaj.th.nightHours")}</th>
              <th style={{ textAlign: "center" }}>{t("payroll.pontaj.th.absenceDays")}</th>
              <th style={{ textAlign: "center" }}>{t("payroll.pontaj.th.leaveDays")}</th>
              <th>{t("payroll.pontaj.th.notes")}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {activeEmps.map(emp => {
              const pj = pontajMap[emp.id];
              const isEditing = editing?.empId === emp.id;
              if (isEditing) {
                return (
                  <tr key={emp.id} style={{ background: "var(--surface-2, var(--bg-card))" }}>
                    <td style={{ fontWeight: 500 }}>{emp.fullName}</td>
                    <td style={{ textAlign: "center" }}>
                      <input type="number" min={0} value={form.workedDays}
                        onChange={ev => setForm(f => ({ ...f, workedDays: ev.target.value }))}
                        style={{ width: 60, textAlign: "center" }} />
                    </td>
                    <td style={{ textAlign: "center" }}>
                      <input type="text" value={form.overtimeHours}
                        onChange={ev => setForm(f => ({ ...f, overtimeHours: ev.target.value }))}
                        style={{ width: 60, textAlign: "center" }} />
                    </td>
                    <td style={{ textAlign: "center" }}>
                      <input type="text" value={form.nightHours}
                        onChange={ev => setForm(f => ({ ...f, nightHours: ev.target.value }))}
                        style={{ width: 60, textAlign: "center" }} />
                    </td>
                    <td style={{ textAlign: "center" }}>
                      <input type="number" min={0} value={form.absenceDays}
                        onChange={ev => setForm(f => ({ ...f, absenceDays: ev.target.value }))}
                        style={{ width: 60, textAlign: "center" }} />
                    </td>
                    <td style={{ textAlign: "center" }}>
                      <input type="number" min={0} value={form.leaveDays}
                        onChange={ev => setForm(f => ({ ...f, leaveDays: ev.target.value }))}
                        style={{ width: 60, textAlign: "center" }} />
                    </td>
                    <td>
                      <input type="text" value={form.notes}
                        onChange={ev => setForm(f => ({ ...f, notes: ev.target.value }))}
                        style={{ width: "100%" }} />
                    </td>
                    <td style={{ display: "flex", gap: 6, alignItems: "center" }}>
                      <button className="btn-primary" onClick={save} disabled={saving} style={{ fontSize: 12 }}>
                        {saving ? t("payroll.common.saving") : t("payroll.pontaj.save")}
                      </button>
                      <button className="btn-secondary" onClick={() => setEditing(null)} style={{ fontSize: 12 }}>
                        {t("payroll.common.cancel")}
                      </button>
                    </td>
                  </tr>
                );
              }
              return (
                <tr key={emp.id}>
                  <td style={{ fontWeight: 500 }}>{emp.fullName}</td>
                  <td style={{ textAlign: "center" }}>{pj ? pj.workedDays : "—"}</td>
                  <td style={{ textAlign: "center" }}>{pj ? pj.overtimeHours : "—"}</td>
                  <td style={{ textAlign: "center" }}>{pj ? pj.nightHours : "—"}</td>
                  <td style={{ textAlign: "center" }}>{pj ? pj.absenceDays : "—"}</td>
                  <td style={{ textAlign: "center" }}>{pj ? pj.leaveDays : "—"}</td>
                  <td style={{ color: "var(--txt-sub)", fontSize: 12 }}>{pj?.notes ?? ""}</td>
                  <td style={{ display: "flex", gap: 6 }}>
                    <button className="btn-secondary" onClick={() => openEdit(emp)} style={{ fontSize: 12 }}>
                      {t("payroll.actions.edit")}
                    </button>
                    {pj && (
                      <button className="btn-danger" onClick={() => removePontaj(pj)} style={{ fontSize: 12 }}>
                        {t("payroll.actions.delete")}
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      <p style={{ fontSize: 11, color: "var(--txt-sub)", marginTop: 12 }}>
        {t("payroll.pontaj.footnote")}
      </p>
    </div>
  );
}
