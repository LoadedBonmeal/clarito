/**
 * Payroll — salarizare (nucleul D112): angajați + statul de salarii lunar.
 * Lista angajaților (brut + deducere) + rularea lunară care calculează stările (ratele 2026) și
 * postează nota contabilă agregată (641/421, 4315, 4316, 444, 646/436).
 */

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import {
  PageHeader, Btn, IconBtn, Card, Field, Input, Modal, Banner, Segmented, Empty,
} from "@/components/rf";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";
import type { Employee, CreateEmployeeInput, PayrollRun } from "@/types";

const MONTHS = ["Ian","Feb","Mar","Apr","Mai","Iun","Iul","Aug","Sep","Oct","Nov","Dec"];

export function PayrollPage() {
  const companyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();
  const now = new Date();
  const [year, setYear] = useState(now.getFullYear());
  const [month, setMonth] = useState(now.getMonth() + 1);
  const [modal, setModal] = useState<"create" | { edit: Employee } | null>(null);
  const [run, setRun] = useState<PayrollRun | null>(null);

  const { data: employees = [] } = useQuery({
    queryKey: ["employees", companyId],
    queryFn: () => api.payroll.list(companyId!),
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
          lunară postează nota contabilă agregată în jurnal. D112 (XML) urmează.
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
          onClose={() => setModal(null)}
          onSaved={() => {
            setModal(null);
            void qc.invalidateQueries({ queryKey: ["employees", companyId] });
          }}
        />
      )}
    </div>
  );
}

function EmployeeModal({
  companyId, employee, onClose, onSaved,
}: {
  companyId: string;
  employee: Employee | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const isEdit = employee !== null;
  const [form, setForm] = useState({
    cnp: employee?.cnp ?? "",
    fullName: employee?.fullName ?? "",
    grossSalary: employee?.grossSalary ?? "",
    personalDeduction: employee?.personalDeduction ?? "0",
  });
  const [error, setError] = useState<string | null>(null);

  const save = useMutation({
    mutationFn: () => {
      if (!form.fullName.trim()) throw new Error("Numele e obligatoriu.");
      if (isEdit) {
        return api.payroll.update(employee!.id, companyId, form);
      }
      const input: CreateEmployeeInput = { companyId, ...form };
      return api.payroll.create(input);
    },
    onSuccess: onSaved,
    onError: (e) => setError(formatError(e, "Eroare la salvare.")),
  });

  const field = (k: keyof typeof form) => ({
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
        {error && <Banner variant="error">{error}</Banner>}
      </div>
    </Modal>
  );
}
