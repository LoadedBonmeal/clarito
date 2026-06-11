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
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm, save as saveDialog } from "@tauri-apps/plugin-dialog";

import { Ic } from "@/components/shared/Ic";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON, parseDec, MONTHS_RO_SHORT } from "@/lib/utils";
import type { Employee, CreateEmployeeInput, PayrollRun, SecondaryOffice } from "@/types";

const MONTHS = MONTHS_RO_SHORT;
const MONTHS_FULL = [
  "Ianuarie", "Februarie", "Martie", "Aprilie", "Mai", "Iunie",
  "Iulie", "August", "Septembrie", "Octombrie", "Noiembrie", "Decembrie",
];

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

/** Excepții art. 146 (5⁷) — etichete mini-chip ok (stil prototip). */
const EXCEPTIE_LABEL: Record<string, string> = {
  elev_student: "elev_student · art. 146(5⁷)",
  ucenic: "ucenic · art. 146(5⁷)",
  dizabilitate: "dizabilități · art. 146(5⁷)",
  contracte_multiple: "contracte multiple · art. 146(5⁷)",
};

/** Cod indemnizație CM (D_9) — etichete chip. */
const COD_CM_LABEL: Record<string, string> = {
  "01": "boală obișnuită",
  "06": "sarcină și lăuzie",
  "09": "îngrijire copil",
  "15": "risc maternal",
};

// triunghi avertisment (nu există în Ic)
const WARN_TRIANGLE =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

export function PayrollPage() {
  const companyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();
  const now = new Date();
  const [year, setYear] = useState(now.getFullYear());
  const [month, setMonth] = useState(now.getMonth() + 1);
  const [tab, setTab] = useState(0);
  const [empQuery, setEmpQuery] = useState("");
  const [modal, setModal] = useState<"create" | { edit: Employee } | null>(null);
  const [showD112, setShowD112] = useState(false);
  const [showSediu, setShowSediu] = useState(false);
  const [showConcediu, setShowConcediu] = useState(false);
  const [run, setRun] = useState<PayrollRun | null>(null);
  const [openPop, setOpenPop] = useState<"" | "period">("");

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
        ? notify.success(`Stat de salarii postat — net total ${r.totalNet} lei.`)
        : notify.info("Niciun angajat activ — nimic de calculat.");
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut rula statul de salarii.")),
  });

  const del = useMutation({
    mutationFn: (id: string) => api.payroll.delete(id, companyId!),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["employees", companyId] }),
    onError: (e) => notify.error(formatError(e, "Eroare la ștergere.")),
  });

  const delSediu = useMutation({
    mutationFn: (id: string) => api.payroll.deleteSediu(id, companyId!),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["sedii", companyId] }),
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge sediul secundar.")),
  });

  const delConcediu = useMutation({
    mutationFn: (id: string) => api.payroll.deleteConcediu(id, companyId!),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["concedii", companyId, periodYm] }),
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge concediul medical.")),
  });

  const runD112 = async (caen: string) => {
    if (!companyId) return;
    // Noul model D112 (Ordin comun 605/95/928/2.314/2026, M.Of. 463/02.06.2026) se aplică
    // veniturilor lunii IULIE 2026+; aplicația emite structura v7 (valabilă ≤ iunie 2026).
    if (year > 2026 || (year === 2026 && month >= 7)) {
      notify.warn(
        `Pentru ${MONTHS[month - 1]} ${year} se aplică NOUL model D112 (OPANAF 605/2026), ` +
        "neimplementat încă — fișierul exportat folosește structura veche (≤ iunie 2026) și " +
        "poate fi respins de DUKIntegrator. Verificați înainte de depunere.",
      );
    }
    const dest = await saveDialog({
      title: "Salvează D112 (XML)",
      defaultPath: `d112-${year}-${String(month).padStart(2, "0")}.xml`,
      filters: [{ name: "XML", extensions: ["xml"] }],
    });
    if (!dest) return;
    try {
      await api.payroll.exportD112Xml(companyId, year, month, caen, dest);
      notify.success(`D112 (XML) exportat — antet + obligații angajator + ${employees.filter((e) => e.active).length} ` +
        `asigurați. Importați-l în aplicația D112 (PDF inteligent), validați (DUKIntegrator) și ` +
        `completați declarantul + blocurile speciale înainte de depunere.`);
      setShowD112(false);
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta D112."));
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
        <div className="page-head"><div><h1>Salarizare</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Selectați o companie activă pentru a gestiona salarizarea.
        </div>
      </div>
    );
  }

  const tabs: Array<{ label: string; count: number | null }> = [
    { label: "Angajați", count: employees.length },
    { label: "Stat de salarii", count: null },
    { label: "Concedii medicale", count: leaves.length },
    { label: "Sedii secundare", count: sedii.length + 1 },
  ];

  return (
    <div className="main-inner wide pg-payroll">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Salarizare</h1>
          <p className="sub">
            {MONTHS_FULL[month - 1]} {year} · {activeEmployees.length === 1 ? "1 angajat activ" : `${activeEmployees.length} angajați activi`} · fond brut {fmtRON(fondBrut)} RON
          </p>
        </div>
        <div className="head-actions">
          {/* perioadă — funcționalitate reală (prototipul are luna fixă) */}
          <div className="nou-wrap" style={{ position: "relative" }}>
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
              <div className="pop show" style={{ right: 0, top: 40, width: 220, maxHeight: 320, overflowY: "auto" }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="col-title" style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                  <button className="mini-btn" aria-label="Anul precedent" onClick={() => setYear(year - 1)}>‹</button>
                  <span className="num">{year}</span>
                  <button className="mini-btn" aria-label="Anul următor" onClick={() => setYear(year + 1)}>›</button>
                </div>
                {MONTHS_FULL.map((m, i) => (
                  <button key={m} className="pop-item" onClick={() => { setMonth(i + 1); setOpenPop(""); }}>
                    <span style={{ flex: 1 }}>{m} {year}</span>
                    {month === i + 1 && <Ic name="check" cls="co-check" />}
                  </button>
                ))}
              </div>
            )}
          </div>
          <button className="pill-btn" onClick={() => setModal("create")}>
            <Ic name="plus" />Angajat nou
          </button>
          <button className="btn-dark" onClick={() => setShowD112(true)}>
            <Ic name="code" />Export D112 (XML)
          </button>
        </div>
      </div>

      {/* banner D112 model nou */}
      <div className="banner warn">
        <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_TRIANGLE }} />
        <span>
          <b>D112 — model nou OPANAF 605/2026</b> se aplică pentru lunile de raportare <b>≥ iulie 2026</b>.
          Clarito emite deocamdată modelul curent (≤ iunie 2026); pentru iulie 2026+ fișierul exportat
          folosește încă structura veche — validați în DUKIntegrator înainte de depunere.
        </span>
      </div>

      {/* tabs */}
      <div className="tabs" style={{ display: "inline-flex", marginBottom: 16 }}>
        {tabs.map((t, i) => (
          <div key={t.label} className={`tab${tab === i ? " active" : ""}`} onClick={() => setTab(i)}>
            {t.label}
            {t.count !== null && <span className="cnt">{t.count}</span>}
          </div>
        ))}
      </div>

      {/* ── ANGAJAȚI ─────────────────────────────────────────────────────── */}
      <div className={`panel${tab === 0 ? " show" : ""}`}>
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">Angajați</div>
            <div className="spacer" />
            <div className="scr-search" style={{ width: 190 }}>
              <Ic name="lens" />
              <input
                type="text"
                placeholder="Caută angajat…"
                value={empQuery}
                onChange={(e) => setEmpQuery(e.target.value)}
              />
            </div>
          </div>
          {filteredEmployees.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {employees.length === 0
                ? "Niciun angajat — adăugați angajați pentru a calcula salariile."
                : "Niciun angajat pentru căutarea aplicată."}
            </div>
          ) : (
            <table className="scr-table">
              <thead>
                <tr><th>Nume</th><th>CNP</th><th className="r">Brut</th><th className="r">Deducere</th><th className="r" style={{ width: 90 }}></th></tr>
              </thead>
              <tbody>
                {filteredEmployees.map((e) => (
                  <tr key={e.id} style={{ opacity: e.active ? 1 : 0.5 }}>
                    <td>
                      <div className="cli">
                        <span className="cli-ava">{initials(e.fullName)}</span>
                        {e.fullName}
                        <span className="chips">
                          <span className="mini-chip">{e.tipContract} · {e.oreNorma}h</span>
                          {e.exceptieCasMin && EXCEPTIE_LABEL[e.exceptieCasMin] && (
                            <span className="mini-chip ok">{EXCEPTIE_LABEL[e.exceptieCasMin]}</span>
                          )}
                          {e.pensionar && <span className="mini-chip ok">pensionar · art. 146(5⁷)</span>}
                          {e.tipContract !== "N" && !e.exceptieCasMin && !e.pensionar && (
                            <span className="mini-chip warn">bază minimă CAS/CASS</span>
                          )}
                          {!e.active && <span className="mini-chip">inactiv</span>}
                        </span>
                      </div>
                    </td>
                    <td><span className="doc">{e.cnp || "—"}</span></td>
                    <td className="r num">{fmtRON(e.grossSalary)}</td>
                    <td className="r num">{fmtRON(e.personalDeduction)}</td>
                    <td>
                      <div className="row-acts">
                        <button className="mini-btn" title="Editează" onClick={() => setModal({ edit: e })}>
                          <Ic name="pen" />
                        </button>
                        <button
                          className="mini-btn"
                          title="Șterge"
                          onClick={async () => {
                            if (await confirm(`Ștergeți angajatul "${e.fullName}"?`, { kind: "warning" })) del.mutate(e.id);
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
              Excepții de la baza minimă part-time — art. 146(5⁷): <b>elev/student sub 26 de ani, ucenic
              sub 18, persoane cu dizabilități, pensionari, contracte multiple însumate ≥ norma întreagă</b>
            </span>
            <span></span>
          </div>
        </div>
      </div>

      {/* ── STAT DE SALARII ──────────────────────────────────────────────── */}
      <div className={`panel${tab === 1 ? " show" : ""}`}>
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">Stat de salarii — {MONTHS_FULL[month - 1]} {year}</div>
            <div className="spacer" />
            <button className="pill-btn" disabled={runMut.isPending} onClick={() => runMut.mutate()}>
              <Ic name="calc" />{runMut.isPending ? "Calculez…" : "Rulează stat salarii"}
            </button>
            {/* propunere — neimplementat (prototipul are Export XLSX fără echivalent backend) */}
            <button className="pill-btn" onClick={() => notify.info("În curând.")}>
              <Ic name="dl" />Export XLSX
            </button>
            <button className="pill-btn" onClick={() => window.print()}>
              <Ic name="printer" />Printează
            </button>
          </div>
          {!run ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {runMut.isPending
                ? "Se calculează statul de salarii…"
                : "Apăsați „Rulează stat salarii” pentru a calcula contribuțiile lunii și a posta nota contabilă."}
            </div>
          ) : run.states.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              Niciun angajat activ — nimic de calculat.
            </div>
          ) : (
            <table className="scr-table">
              <thead>
                <tr>
                  <th>Angajat</th><th className="r">Brut</th><th className="r">CAS 25%</th>
                  <th className="r">CASS 10%</th><th className="r">Impozit 10%</th>
                  <th className="r">Net</th><th className="r">CAM 2,25%</th>
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
                <tr style={{ background: "#FCFCFD", fontWeight: 600 }}>
                  <td>Total</td>
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
              * CAS și CASS calculate la baza salariului minim brut pentru contractele part-time fără
              excepție — diferența este suportată de angajator conform art. 146(5⁶) Cod fiscal.
              {run?.posted ? <> Nota contabilă agregată (641/421, 4315, 4316, 444, 646/436) a fost postată în jurnal la <b>{fmtRoDate(run.entryDate)}</b>.</> : null}
            </span>
            <span></span>
          </div>
        </div>
      </div>

      {/* ── CONCEDII MEDICALE ────────────────────────────────────────────── */}
      <div className={`panel${tab === 2 ? " show" : ""}`}>
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">Concedii medicale — {MONTHS_FULL[month - 1]} {year}</div>
            <div className="spacer" />
            <button className="pill-btn" onClick={() => setShowConcediu(true)}>
              <Ic name="plus" />Adaugă certificat
            </button>
          </div>
          {leaves.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              Niciun concediu medical înregistrat pentru {MONTHS_FULL[month - 1]} {year}.
            </div>
          ) : (
            <table className="scr-table">
              <thead>
                <tr>
                  <th>Angajat</th><th>Certificat</th><th>Cod indemnizație</th><th>Perioada</th>
                  <th className="r">Zile angajator</th><th className="r">Zile FNUASS</th>
                  <th className="r">Suma angajator</th><th className="r">Suma FNUASS</th>
                  <th className="r" style={{ width: 50 }}></th>
                </tr>
              </thead>
              <tbody>
                {leaves.map((l) => {
                  const emp = employees.find((e) => e.id === l.employeeId);
                  const name = emp?.fullName ?? l.employeeId;
                  return (
                    <tr key={l.id}>
                      <td><div className="cli"><span className="cli-ava">{initials(name)}</span>{name}</div></td>
                      <td><span className="doc">{l.serie || l.numar ? `CM seria ${l.serie || "—"} nr. ${l.numar || "—"}` : "—"}</span></td>
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
                            title="Șterge"
                            onClick={async () => {
                              if (await confirm("Ștergeți acest concediu medical?", { kind: "warning" })) delConcediu.mutate(l.id);
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
              Indemnizațiile FNUASS se recuperează prin <b>cererea de restituire</b> depusă la CAS după
              D112 · cod 01: primele 5 zile la angajator, restul din FNUASS · sursa blocului D112 asiguratD
            </span>
            <span></span>
          </div>
        </div>
      </div>

      {/* ── SEDII SECUNDARE ──────────────────────────────────────────────── */}
      <div className={`panel${tab === 3 ? " show" : ""}`}>
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">Sedii secundare înregistrate fiscal</div>
            <div className="spacer" />
            <button className="pill-btn" onClick={() => setShowSediu(true)}>
              <Ic name="plus" />Adaugă sediu
            </button>
          </div>
          <table className="scr-table">
            <thead>
              <tr><th>Sediu</th><th>CIF sediu</th><th className="r">Angajați repartizați</th><th className="r" style={{ width: 50 }}></th></tr>
            </thead>
            <tbody>
              <tr>
                <td>
                  <div className="cli">
                    <span className="cli-ava">{activeCompany ? initials(activeCompany.legalName) : "SP"}</span>
                    <b>Sediu principal{activeCompany ? ` — ${activeCompany.legalName}` : ""}</b>
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
                      <span className="cli-ava">{initials(s.name || "Punct lucru")}</span>
                      {s.name || "Punct de lucru"}
                    </div>
                  </td>
                  <td><span className="doc">{s.cif}</span></td>
                  <td className="r num">{sediuCount(s.cif)}</td>
                  <td>
                    <div className="row-acts">
                      <button
                        className="mini-btn"
                        title="Șterge"
                        onClick={async () => {
                          if (await confirm(`Ștergeți sediul secundar ${s.cif}?`, { kind: "warning" })) delSediu.mutate(s.id);
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
              Sediile secundare cu <b>minimum 5 salariați</b> au obligația înregistrării fiscale (CIF
              propriu) și a declarării impozitului pe salarii separat în D112 — secțiunea F.
            </span>
            <span></span>
          </div>
        </div>
      </div>

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
          onClose={() => setShowD112(false)}
          onExport={runD112}
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
  });
  const [error, setError] = useState<string | null>(null);

  const save = useMutation({
    mutationFn: () => {
      if (!form.fullName.trim()) throw new Error("Numele e obligatoriu.");
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
      };
      if (isEdit) {
        return api.payroll.update(employee!.id, companyId, payload);
      }
      const input: CreateEmployeeInput = { companyId, ...payload };
      return api.payroll.create(input);
    },
    onSuccess: onSaved,
    onError: (e) => setError(formatError(e, "Eroare la salvare.")),
  });

  type StrKey = "cnp" | "fullName" | "grossSalary" | "personalDeduction" | "oreNorma";
  const field = (k: StrKey) => ({
    value: form[k],
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => setForm((f) => ({ ...f, [k]: e.target.value })),
  });

  return createPortal(
    <div
      className="modal-back show"
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="modal lg">
        <div className="modal-head">
          <div>
            <div className="mt">{isEdit ? `Editează: ${employee.fullName}` : "Angajat nou"}</div>
            <div className="ms">Datele alimentează statul de salarii și D112</div>
          </div>
          <button className="modal-x" onClick={onClose} aria-label="Închide">
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>Nume complet <span className="req">*</span></label>
              <input className="input" type="text" placeholder="Ion Popescu" {...field("fullName")} autoFocus />
            </div>
            <div className="field">
              <label>CNP</label>
              <input className="input num" type="text" placeholder="1900101…" {...field("cnp")} />
            </div>
            <div className="field">
              <label>Salariu brut <span className="req">*</span></label>
              <input className="input num" type="text" inputMode="decimal" placeholder="5000" style={{ textAlign: "right" }} {...field("grossSalary")} />
            </div>
            <div className="field">
              <label>Deducere personală</label>
              <input className="input num" type="text" inputMode="decimal" placeholder="0" style={{ textAlign: "right" }} {...field("personalDeduction")} />
            </div>
            <div className="field">
              <label>Tip contract</label>
              <select
                className="select"
                value={form.tipContract}
                onChange={(e) => setForm((f) => ({ ...f, tipContract: e.target.value }))}
              >
                <option value="N">N — normă întreagă</option>
                <option value="P1">P1 — part-time 1h</option>
                <option value="P2">P2 — part-time 2h</option>
                <option value="P3">P3 — part-time 3h</option>
                <option value="P4">P4 — part-time 4h</option>
                <option value="P5">P5 — part-time 5h</option>
                <option value="P6">P6 — part-time 6h</option>
                <option value="P7">P7 — part-time 7h</option>
              </select>
            </div>
            <div className="field">
              <label>Ore normă / zi</label>
              <input className="input num" type="text" inputMode="numeric" placeholder="8" {...field("oreNorma")} />
            </div>
            <div className="field">
              <label>Pensionar (D112 A_2)</label>
              <select
                className="select"
                value={form.pensionar ? "da" : "nu"}
                onChange={(e) => setForm((f) => ({ ...f, pensionar: e.target.value === "da" }))}
              >
                <option value="da">Da</option>
                <option value="nu">Nu</option>
              </select>
            </div>
            <div className="field">
              <label>Excepție bază minimă — art. 146(5⁷)</label>
              <select
                className="select"
                value={form.exceptieCasMin}
                onChange={(e) => setForm((f) => ({ ...f, exceptieCasMin: e.target.value }))}
              >
                <option value="">— fără excepție —</option>
                <option value="elev_student">Elev / student până la 26 de ani (lit. a)</option>
                <option value="ucenic">Ucenic până la 18 ani (lit. b)</option>
                <option value="dizabilitate">Persoană cu dizabilități / &lt; 8h/zi (lit. c)</option>
                <option value="contracte_multiple">Contracte multiple ≥ salariul minim (lit. e)</option>
              </select>
            </div>
            <div className="field span2">
              <label>Sediu (CIF) — D112 angajatorF2</label>
              <select
                className="select"
                value={form.sediuCif}
                onChange={(e) => setForm((f) => ({ ...f, sediuCif: e.target.value }))}
              >
                <option value="">Sediu principal{mainCui ? ` · ${mainCui}` : ""}</option>
                {sedii.map((s) => (
                  <option key={s.id} value={s.cif}>{s.name ? `${s.name} · ${s.cif}` : s.cif}</option>
                ))}
              </select>
            </div>
            {error && (
              <div className="field span2">
                <div className="banner danger" style={{ marginBottom: 0 }}>
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_TRIANGLE }} />
                  <span>{error}</span>
                </div>
              </div>
            )}
          </div>
        </div>
        <div className="modal-foot">
          <button className="pill-btn" onClick={onClose} disabled={save.isPending}>Renunță</button>
          <button className="btn-dark" disabled={save.isPending} onClick={() => save.mutate()}>
            <Ic name="check" />{save.isPending ? "Se salvează…" : "Salvează angajat"}
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
  const [f, setF] = useState({
    employeeId: "", serie: "", numar: "", codIndemnizatie: "01",
    dataInceput: "", dataSfarsit: "", zileAngajator: "", zileFnuass: "",
    sumaAngajator: "", sumaFnuass: "",
  });
  const [error, setError] = useState<string | null>(null);

  const add = useMutation({
    mutationFn: () => {
      if (!f.employeeId) throw new Error("Selectați angajatul.");
      return api.payroll.createConcediu({
        companyId, employeeId: f.employeeId, periodYm,
        serie: f.serie, numar: f.numar, codIndemnizatie: f.codIndemnizatie,
        dataInceput: f.dataInceput, dataSfarsit: f.dataSfarsit,
        zileAngajator: Number(f.zileAngajator) || 0, zileFnuass: Number(f.zileFnuass) || 0,
        sumaAngajator: f.sumaAngajator || "0", sumaFnuass: f.sumaFnuass || "0",
      });
    },
    onSuccess: onSaved,
    onError: (e) => setError(formatError(e, "Nu s-a putut adăuga concediul medical.")),
  });

  const num = (k: keyof typeof f) => ({
    value: f[k],
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => setF((s) => ({ ...s, [k]: e.target.value })),
  });

  return createPortal(
    <div
      className="modal-back show"
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="modal lg">
        <div className="modal-head">
          <div>
            <div className="mt">Adaugă certificat de concediu medical</div>
            <div className="ms">{monthLabel} · OUG 158/2005 — sursa blocului D112 asiguratD</div>
          </div>
          <button className="modal-x" onClick={onClose} aria-label="Închide">
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field span2">
              <label>Angajat <span className="req">*</span></label>
              <select
                className="select"
                value={f.employeeId}
                onChange={(e) => setF((s) => ({ ...s, employeeId: e.target.value }))}
                autoFocus
              >
                <option value="">— selectați angajatul —</option>
                {employees.map((e) => <option key={e.id} value={e.id}>{e.fullName}</option>)}
              </select>
            </div>
            <div className="field">
              <label>Serie certificat</label>
              <input className="input num" type="text" placeholder="CCMAH" {...num("serie")} />
            </div>
            <div className="field">
              <label>Număr certificat</label>
              <input className="input num" type="text" placeholder="8841220" {...num("numar")} />
            </div>
            <div className="field span2">
              <label>Cod indemnizație (D_9)</label>
              <select
                className="select"
                value={f.codIndemnizatie}
                onChange={(e) => setF((s) => ({ ...s, codIndemnizatie: e.target.value }))}
              >
                <option value="01">01 — boală obișnuită</option>
                <option value="06">06 — sarcină și lăuzie</option>
                <option value="09">09 — îngrijire copil</option>
                <option value="15">15 — risc maternal</option>
              </select>
            </div>
            <div className="field">
              <label>Data început</label>
              <input className="input num" type="date" {...num("dataInceput")} />
            </div>
            <div className="field">
              <label>Data sfârșit</label>
              <input className="input num" type="date" {...num("dataSfarsit")} />
            </div>
            <div className="field">
              <label>Zile angajator</label>
              <input className="input num" type="text" inputMode="numeric" placeholder="5" style={{ textAlign: "right" }} {...num("zileAngajator")} />
            </div>
            <div className="field">
              <label>Zile FNUASS</label>
              <input className="input num" type="text" inputMode="numeric" placeholder="0" style={{ textAlign: "right" }} {...num("zileFnuass")} />
            </div>
            <div className="field">
              <label>Indemnizație angajator</label>
              <input className="input num" type="text" inputMode="decimal" placeholder="0,00" style={{ textAlign: "right" }} {...num("sumaAngajator")} />
            </div>
            <div className="field">
              <label>Indemnizație FNUASS</label>
              <input className="input num" type="text" inputMode="decimal" placeholder="0,00" style={{ textAlign: "right" }} {...num("sumaFnuass")} />
            </div>
            {error && (
              <div className="field span2">
                <div className="banner danger" style={{ marginBottom: 0 }}>
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_TRIANGLE }} />
                  <span>{error}</span>
                </div>
              </div>
            )}
          </div>
        </div>
        <div className="modal-foot">
          <span className="left">cod 01: primele 5 zile la angajator, restul din FNUASS</span>
          <button className="pill-btn" onClick={onClose} disabled={add.isPending}>Renunță</button>
          <button className="btn-dark" disabled={add.isPending || !f.employeeId} onClick={() => add.mutate()}>
            <Ic name="check" />{add.isPending ? "Se salvează…" : "Salvează certificat"}
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
  const [cif, setCif] = useState("");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);

  const add = useMutation({
    mutationFn: () => api.payroll.createSediu(companyId, cif.trim(), name.trim()),
    onSuccess: onSaved,
    onError: (e) => setError(formatError(e, "Nu s-a putut adăuga sediul secundar.")),
  });

  return createPortal(
    <div
      className="modal-back show"
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="modal">
        <div className="modal-head">
          <div>
            <div className="mt">Adaugă sediu secundar</div>
            <div className="ms">CIF propriu (doar cifre, unic per companie) — D112 angajatorF2 / secțiunea F</div>
          </div>
          <button className="modal-x" onClick={onClose} aria-label="Închide">
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>CIF sediu <span className="req">*</span></label>
              <input className="input num" type="text" placeholder="49102337" value={cif} onChange={(e) => setCif(e.target.value)} autoFocus />
            </div>
            <div className="field">
              <label>Denumire (opțional)</label>
              <input className="input" type="text" placeholder="Punct de lucru Cluj-Napoca" value={name} onChange={(e) => setName(e.target.value)} />
            </div>
            {error && (
              <div className="field span2">
                <div className="banner danger" style={{ marginBottom: 0 }}>
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_TRIANGLE }} />
                  <span>{error}</span>
                </div>
              </div>
            )}
          </div>
        </div>
        <div className="modal-foot">
          <button className="pill-btn" onClick={onClose} disabled={add.isPending}>Renunță</button>
          <button className="btn-dark" disabled={add.isPending || !cif.trim()} onClick={() => add.mutate()}>
            <Ic name="check" />{add.isPending ? "Se salvează…" : "Salvează sediu"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ─── D112Modal — export XML cu CAEN (înlocuiește window.prompt, no-op în Tauri) ─

function D112Modal({
  monthLabel, newModel, onClose, onExport,
}: {
  monthLabel: string;
  newModel: boolean;
  onClose: () => void;
  onExport: (caen: string) => Promise<void>;
}) {
  const [caen, setCaen] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (!/^\d{4}$/.test(caen.trim())) { notify.error("Cod CAEN invalid — 4 cifre (ex. 6201)."); return; }
    setBusy(true);
    try {
      await onExport(caen.trim());
    } finally {
      setBusy(false);
    }
  };

  return createPortal(
    <div
      className="modal-back show"
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="modal">
        <div className="modal-head">
          <div>
            <div className="mt">Export D112 (XML)</div>
            <div className="ms">
              {monthLabel} · {newModel
                ? "se aplică modelul nou OPANAF 605/2026 — exportul folosește încă structura veche (verificați în DUKIntegrator)"
                : "modelul curent; modelul nou OPANAF 605/2026 se aplică de la luna de raportare iulie 2026"}
            </div>
          </div>
          <button className="modal-x" onClick={onClose} aria-label="Închide">
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>Cod CAEN <span className="req">*</span></label>
              <input
                className="input num"
                type="text"
                placeholder="6201"
                value={caen}
                onChange={(e) => setCaen(e.target.value)}
                autoFocus
              />
              <span className="hint">4 cifre — activitatea principală a angajatorului</span>
            </div>
            <div className="field">
              <label>Luna de raportare</label>
              <input
                className="input num"
                type="text"
                value={monthLabel}
                disabled
                style={{ background: "var(--fill)", color: "var(--text-2)" }}
              />
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <span className="left">Include secțiunea F (sedii secundare) și asiguratD (concedii medicale)</span>
          <button className="pill-btn" onClick={onClose} disabled={busy}>Renunță</button>
          <button className="btn-dark" disabled={busy} onClick={() => void submit()}>
            <Ic name="code" />{busy ? "Se exportă…" : "Generează XML"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
