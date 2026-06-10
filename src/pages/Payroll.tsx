/**
 * Payroll — salarizare (nucleul D112): angajați + statul de salarii lunar.
 * Lista angajaților (brut + deducere) + rularea lunară care calculează stările (ratele 2026) și
 * postează nota contabilă agregată (641/421, 4315, 4316, 444, 646/436).
 */

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm, save as saveDialog } from "@tauri-apps/plugin-dialog";

import {
  PageHeader, Btn, IconBtn, Card, Field, Input, Modal, Banner, Segmented, Empty,
} from "@/components/rf";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON, MONTHS_RO_SHORT } from "@/lib/utils";
import type { Employee, CreateEmployeeInput, PayrollRun } from "@/types";

const MONTHS = MONTHS_RO_SHORT;

export function PayrollPage() {
  const companyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();
  const now = new Date();
  const [year, setYear] = useState(now.getFullYear());
  const [month, setMonth] = useState(now.getMonth() + 1);
  const [modal, setModal] = useState<"create" | { edit: Employee } | null>(null);
  const [run, setRun] = useState<PayrollRun | null>(null);
  const [showD112, setShowD112] = useState(false);

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

  const period = useMemo(() => {
    const mm = String(month).padStart(2, "0");
    const last = new Date(year, month, 0).getDate();
    return { from: `${year}-${mm}-01`, to: `${year}-${mm}-${String(last).padStart(2, "0")}` };
  }, [year, month]);

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

  return (
    <div className="rf-page">
      <PageHeader
        title="Salarizare"
        actions={
          <Btn variant="primary" icon="plus" disabled={!companyId} onClick={() => setModal("create")}>
            Angajat nou
          </Btn>
        }
      />
      <div className="rf-page-body">
        <Banner variant="info">
          Calcul salarial 2026: CAS 25%, CASS 10%, impozit 10%, CAM 2,25% (angajator). Rularea
          lunară postează nota contabilă agregată în jurnal; exportați D112 (XML) pentru import în
          aplicația ANAF.
        </Banner>

        <Card>
          <div style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap", padding: 12 }}>
            <Segmented
              options={MONTHS.map((l, i) => ({ value: String(i + 1), label: l }))}
              value={String(month)}
              onChange={(v) => setMonth(Number(v))}
            />
            <Segmented
              options={[year - 1, year, year + 1].map((y) => ({ value: String(y), label: String(y) }))}
              value={String(year)}
              onChange={(v) => setYear(Number(v))}
            />
            <Btn variant="secondary" icon="ledger" disabled={runMut.isPending || !companyId} onClick={() => runMut.mutate()}>
              {runMut.isPending ? "Calculez…" : "Rulează stat salarii"}
            </Btn>
            <Btn variant="secondary" icon="download" disabled={!companyId} onClick={() => setShowD112(true)}>
              D112 XML
            </Btn>
          </div>
        </Card>

        {/* Employee list */}
        <Card>
          {employees.length === 0 ? (
            <Empty icon="users" title="Niciun angajat — adăugați angajați pentru a calcula salariile." />
          ) : (
            <table className="rf-tbl">
              <thead>
                <tr><th>Nume</th><th>CNP</th><th className="right">Brut</th><th className="right">Deducere</th><th></th></tr>
              </thead>
              <tbody>
                {employees.map((e) => (
                  <tr key={e.id} style={{ opacity: e.active ? 1 : 0.5 }}>
                    <td style={{ fontWeight: 500 }}>{e.fullName}</td>
                    <td className="mono">{e.cnp}</td>
                    <td className="right rf-mono">{fmtRON(e.grossSalary)}</td>
                    <td className="right rf-mono">{fmtRON(e.personalDeduction)}</td>
                    <td className="right">
                      <IconBtn icon="edit" onClick={() => setModal({ edit: e })} title="Editează" />
                      <IconBtn icon="trash" onClick={async () => {
                        if (await confirm(`Ștergeți angajatul "${e.fullName}"?`, { kind: "warning" })) del.mutate(e.id);
                      }} title="Șterge" />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>

        {companyId && <SediiManager companyId={companyId} sedii={sedii} />}

        {companyId && (
          <ConcediiManager companyId={companyId} periodYm={period.from.slice(0, 7)} employees={employees} />
        )}

        {/* Payroll register */}
        {run && run.states.length > 0 && (
          <Card>
            <div style={{ padding: "10px 12px", fontWeight: 600 }}>
              Stat de salarii {MONTHS[month - 1]} {year}
            </div>
            <table className="rf-tbl">
              <thead>
                <tr>
                  <th>Angajat</th><th className="right">Brut</th><th className="right">CAS</th>
                  <th className="right">CASS</th><th className="right">Impozit</th>
                  <th className="right">Net</th><th className="right">CAM</th>
                </tr>
              </thead>
              <tbody>
                {run.states.map((s) => (
                  <tr key={s.employeeId}>
                    <td>{s.fullName}</td>
                    <td className="right rf-mono">{fmtRON(s.gross)}</td>
                    <td className="right rf-mono">{fmtRON(s.cas)}</td>
                    <td className="right rf-mono">{fmtRON(s.cass)}</td>
                    <td className="right rf-mono">{fmtRON(s.incomeTax)}</td>
                    <td className="right rf-mono" style={{ fontWeight: 600 }}>{fmtRON(s.net)}</td>
                    <td className="right rf-mono">{fmtRON(s.cam)}</td>
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr style={{ fontWeight: 700, borderTop: "2px solid var(--rf-border)" }}>
                  <td>TOTAL</td>
                  <td className="right rf-mono">{fmtRON(run.totalGross)}</td>
                  <td className="right rf-mono">{fmtRON(run.totalCas)}</td>
                  <td className="right rf-mono">{fmtRON(run.totalCass)}</td>
                  <td className="right rf-mono">{fmtRON(run.totalIncomeTax)}</td>
                  <td className="right rf-mono">{fmtRON(run.totalNet)}</td>
                  <td className="right rf-mono">{fmtRON(run.totalCam)}</td>
                </tr>
              </tfoot>
            </table>
          </Card>
        )}
      </div>

      {modal && companyId && (
        <EmployeeModal
          companyId={companyId}
          employee={modal === "create" ? null : modal.edit}
          sedii={sedii}
          onClose={() => setModal(null)}
          onSaved={() => {
            setModal(null);
            void qc.invalidateQueries({ queryKey: ["employees", companyId] });
          }}
        />
      )}

      {showD112 && (
        <CaenExportModal
          title="Export D112 (XML)"
          onClose={() => setShowD112(false)}
          onExport={runD112}
        />
      )}
    </div>
  );
}

function EmployeeModal({
  companyId, employee, sedii, onClose, onSaved,
}: {
  companyId: string;
  employee: Employee | null;
  sedii: import("@/types").SecondaryOffice[];
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

  return (
    <Modal
      open
      onOpenChange={(o) => { if (!o) onClose(); }}
      title={isEdit ? `Editează: ${employee.fullName}` : "Angajat nou"}
      width={480}
      footer={
        <>
          <Btn variant="secondary" onClick={onClose} disabled={save.isPending}>Anulează</Btn>
          <Btn variant="primary" icon="check" disabled={save.isPending} onClick={() => save.mutate()}>
            {save.isPending ? "Se salvează…" : isEdit ? "Salvează" : "Adaugă"}
          </Btn>
        </>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        <Field label="Nume complet" required><Input placeholder="Ion Popescu" {...field("fullName")} autoFocus /></Field>
        <Field label="CNP"><Input placeholder="1900101..." className="mono" {...field("cnp")} /></Field>
        <div className="rf-grid-2">
          <Field label="Salariu brut (lei)"><Input inputMode="decimal" placeholder="5000" {...field("grossSalary")} /></Field>
          <Field label="Deducere personală (lei)"><Input inputMode="decimal" placeholder="0" {...field("personalDeduction")} /></Field>
        </div>
        <div className="rf-grid-2">
          <Field label="Tip contract (D112)">
            <select className="rf-input" value={form.tipContract}
              onChange={(e) => setForm((f) => ({ ...f, tipContract: e.target.value }))}>
              <option value="N">N — normă întreagă</option>
              <option value="P1">P1 — parțial</option>
              <option value="P2">P2 — parțial</option>
              <option value="P3">P3 — parțial</option>
              <option value="P4">P4 — parțial</option>
              <option value="P5">P5 — parțial</option>
              <option value="P6">P6 — parțial</option>
              <option value="P7">P7 — parțial</option>
            </select>
          </Field>
          <Field label="Ore normă/zi"><Input inputMode="numeric" placeholder="8" {...field("oreNorma")} /></Field>
        </div>
        <label style={{ display: "flex", gap: 8, alignItems: "center", fontSize: 13 }}>
          <input type="checkbox" checked={form.pensionar}
            onChange={(e) => setForm((f) => ({ ...f, pensionar: e.target.checked }))} />
          Pensionar (D112 A_2)
        </label>
        <Field label="Excepție bază minimă CAS/CASS part-time (art. 146 alin. 5⁷)">
          <select className="rf-input" value={form.exceptieCasMin}
            onChange={(e) => setForm((f) => ({ ...f, exceptieCasMin: e.target.value }))}>
            <option value="">Fără excepție (se aplică baza minimă)</option>
            <option value="elev_student">Elev/student până la 26 ani (lit. a)</option>
            <option value="ucenic">Ucenic până la 18 ani (lit. b)</option>
            <option value="dizabilitate">Persoană cu dizabilități / &lt; 8h/zi (lit. c)</option>
            <option value="contracte_multiple">Contracte multiple ≥ salariul minim (lit. e)</option>
          </select>
        </Field>
        <Field label="Sediu (D112) — impozit pe salarii la sediu secundar">
          <select className="rf-input" value={form.sediuCif}
            onChange={(e) => setForm((f) => ({ ...f, sediuCif: e.target.value }))}>
            <option value="">Sediu principal</option>
            {sedii.map((s) => (
              <option key={s.id} value={s.cif}>{s.cif}{s.name ? ` — ${s.name}` : ""}</option>
            ))}
          </select>
        </Field>
        {error && <Banner variant="error">{error}</Banner>}
      </div>
    </Modal>
  );
}

/** Reusable CAEN-only export dialog — replaces window.prompt (a no-op in Tauri's WebView). */
function CaenExportModal({
  title,
  onClose,
  onExport,
}: {
  title: string;
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

  return (
    <Modal open onOpenChange={(o) => { if (!o) onClose(); }} title={title} width={420}
      footer={
        <>
          <Btn variant="secondary" onClick={onClose} disabled={busy}>Anulează</Btn>
          <Btn variant="primary" icon="download" disabled={busy} onClick={() => void submit()}>
            {busy ? "Se exportă…" : "Exportă"}
          </Btn>
        </>
      }
    >
      <Field label="Cod CAEN (4 cifre)" required>
        <Input className="mono" placeholder="6201" value={caen} onChange={(e) => setCaen(e.target.value)} autoFocus />
      </Field>
    </Modal>
  );
}

/** Sedii secundare (D112 angajatorF2) — adăugare/ștergere; salariații se repartizează în formularul
 *  angajatului. CIF doar cifre, unic per companie. */
function SediiManager({
  companyId,
  sedii,
}: {
  companyId: string;
  sedii: import("@/types").SecondaryOffice[];
}) {
  const qc = useQueryClient();
  const [cif, setCif] = useState("");
  const [name, setName] = useState("");

  const add = useMutation({
    mutationFn: () => api.payroll.createSediu(companyId, cif.trim(), name.trim()),
    onSuccess: () => {
      setCif(""); setName("");
      void qc.invalidateQueries({ queryKey: ["sedii", companyId] });
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut adăuga sediul secundar.")),
  });
  const remove = useMutation({
    mutationFn: (id: string) => api.payroll.deleteSediu(id, companyId),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["sedii", companyId] }),
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge sediul secundar.")),
  });

  return (
    <Card>
      <div style={{ padding: "10px 12px", fontWeight: 600 }}>Sedii secundare (D112 angajatorF2)</div>
      <div style={{ display: "flex", gap: 8, alignItems: "center", padding: "0 12px 12px", flexWrap: "wrap" }}>
        <Input className="mono" style={{ maxWidth: 140 }} placeholder="CIF sediu" value={cif}
          onChange={(e) => setCif(e.target.value)} />
        <Input style={{ maxWidth: 220 }} placeholder="Denumire (opțional)" value={name}
          onChange={(e) => setName(e.target.value)} />
        <Btn variant="secondary" size="sm" icon="plus" disabled={add.isPending || !cif.trim()}
          onClick={() => add.mutate()}>Adaugă</Btn>
      </div>
      {sedii.length > 0 && (
        <table className="rf-tbl">
          <thead><tr><th>CIF</th><th>Denumire</th><th></th></tr></thead>
          <tbody>
            {sedii.map((s) => (
              <tr key={s.id}>
                <td className="mono">{s.cif}</td>
                <td>{s.name || "—"}</td>
                <td className="right">
                  <IconBtn icon="trash" onClick={async () => {
                    if (await confirm(`Ștergeți sediul secundar ${s.cif}?`, { kind: "warning" })) remove.mutate(s.id);
                  }} title="Șterge" />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Card>
  );
}

/** Concedii medicale (OUG 158/2005) — registru per lună: certificat (serie/nr/cod), zile (angajator
 *  + FNUASS) și indemnizațiile. Sursa blocului D112 asiguratD; emiterea în XML se validează ulterior
 *  în DUKIntegrator. */
function ConcediiManager({
  companyId,
  periodYm,
  employees,
}: {
  companyId: string;
  periodYm: string;
  employees: Employee[];
}) {
  const qc = useQueryClient();
  const { data: leaves = [] } = useQuery({
    queryKey: ["concedii", companyId, periodYm],
    queryFn: () => api.payroll.listConcedii(companyId, periodYm),
    enabled: !!companyId,
  });
  const [f, setF] = useState({
    employeeId: "", serie: "", numar: "", codIndemnizatie: "01",
    dataInceput: "", dataSfarsit: "", zileAngajator: "", zileFnuass: "",
    sumaAngajator: "", sumaFnuass: "",
  });

  const empName = (id: string) => employees.find((e) => e.id === id)?.fullName ?? id;

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
    onSuccess: () => {
      setF((s) => ({ ...s, serie: "", numar: "", dataInceput: "", dataSfarsit: "", zileAngajator: "", zileFnuass: "", sumaAngajator: "", sumaFnuass: "" }));
      void qc.invalidateQueries({ queryKey: ["concedii", companyId, periodYm] });
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut adăuga concediul medical.")),
  });
  const remove = useMutation({
    mutationFn: (id: string) => api.payroll.deleteConcediu(id, companyId),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["concedii", companyId, periodYm] }),
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge concediul medical.")),
  });

  const num = (k: keyof typeof f) => ({ value: f[k], onChange: (e: React.ChangeEvent<HTMLInputElement>) => setF((s) => ({ ...s, [k]: e.target.value })) });

  return (
    <Card>
      <div style={{ padding: "10px 12px", fontWeight: 600 }}>Concedii medicale (OUG 158/2005) — {periodYm}</div>
      <div style={{ display: "flex", gap: 8, alignItems: "center", padding: "0 12px 12px", flexWrap: "wrap" }}>
        <select className="rf-input" style={{ maxWidth: 180 }} value={f.employeeId}
          onChange={(e) => setF((s) => ({ ...s, employeeId: e.target.value }))}>
          <option value="">Angajat…</option>
          {employees.map((e) => <option key={e.id} value={e.id}>{e.fullName}</option>)}
        </select>
        <Input style={{ maxWidth: 70 }} placeholder="Serie" {...num("serie")} />
        <Input style={{ maxWidth: 90 }} placeholder="Nr." {...num("numar")} />
        <select className="rf-input" style={{ maxWidth: 130 }} value={f.codIndemnizatie}
          onChange={(e) => setF((s) => ({ ...s, codIndemnizatie: e.target.value }))} title="Cod indemnizație (D_9)">
          <option value="01">01 boală obișnuită</option>
          <option value="06">06 sarcină/lăuzie</option>
          <option value="09">09 îngrijire copil</option>
          <option value="15">15 risc maternal</option>
        </select>
        <Input style={{ maxWidth: 130 }} placeholder="Început (AAAA-LL-ZZ)" {...num("dataInceput")} />
        <Input style={{ maxWidth: 130 }} placeholder="Sfârșit" {...num("dataSfarsit")} />
        <Input style={{ maxWidth: 90 }} inputMode="numeric" placeholder="Zile ang." {...num("zileAngajator")} />
        <Input style={{ maxWidth: 90 }} inputMode="numeric" placeholder="Zile FNUASS" {...num("zileFnuass")} />
        <Input style={{ maxWidth: 110 }} inputMode="decimal" placeholder="Indem. ang." {...num("sumaAngajator")} />
        <Input style={{ maxWidth: 110 }} inputMode="decimal" placeholder="Indem. FNUASS" {...num("sumaFnuass")} />
        <Btn variant="secondary" size="sm" icon="plus" disabled={add.isPending || !f.employeeId}
          onClick={() => add.mutate()}>Adaugă</Btn>
      </div>
      {leaves.length > 0 && (
        <table className="rf-tbl">
          <thead><tr><th>Angajat</th><th>Cert.</th><th>Cod</th><th className="right">Zile</th><th className="right">Indem. ang.</th><th className="right">Indem. FNUASS</th><th></th></tr></thead>
          <tbody>
            {leaves.map((l) => (
              <tr key={l.id}>
                <td>{empName(l.employeeId)}</td>
                <td className="mono">{l.serie} {l.numar}</td>
                <td className="mono">{l.codIndemnizatie}</td>
                <td className="right rf-mono">{l.zileAngajator + l.zileFnuass}</td>
                <td className="right rf-mono">{fmtRON(l.sumaAngajator)}</td>
                <td className="right rf-mono">{fmtRON(l.sumaFnuass)}</td>
                <td className="right">
                  <IconBtn icon="trash" onClick={async () => {
                    if (await confirm("Ștergeți acest concediu medical?", { kind: "warning" })) remove.mutate(l.id);
                  }} title="Șterge" />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Card>
  );
}
