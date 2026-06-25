/**
 * Provizioane (class 15x) — OMFP 1802/2014 pct. 374. Lean register: constituire D 6812 / C 15x
 * (after confirming the 3 cumulative conditions), reluare/utilizare D 15x / C 7812.
 */
import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { provisions as api, type Provision, type CreateProvisionInput } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import { fmtRON } from "@/lib/utils";

const thisYM = () => new Date().toISOString().slice(0, 7);
const ACCTS: [string, string][] = [
  ["1511", "Litigii"],
  ["1512", "Garanții acordate clienților"],
  ["1513", "Dezafectare imobilizări"],
  ["1514", "Restructurare"],
  ["1515", "Pensii și obligații similare"],
  ["1518", "Alte provizioane"],
];

export function ProvisionsPage() {
  const companyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();
  const [showCreate, setShowCreate] = useState(false);
  const [period, setPeriod] = useState(thisYM());

  const { data: rows = [], isLoading, isError, error } = useQuery({
    queryKey: ["provisions", companyId],
    queryFn: () => api.list(companyId!),
    enabled: !!companyId,
  });

  const inval = () => void qc.invalidateQueries({ queryKey: ["provisions", companyId] });

  const createMut = useMutation({
    mutationFn: (i: CreateProvisionInput) => api.create(i),
    onSuccess: () => { inval(); setShowCreate(false); notify.success("Provizion constituit (D 6812 / C 15x)."); },
    onError: (e) => notify.error(formatError(e, "Eroare la constituire.")),
  });
  const reverseMut = useMutation({
    mutationFn: (id: string) => api.reverse(id, companyId!, period),
    onSuccess: () => { inval(); notify.success("Provizion reluat (D 15x / C 7812)."); },
    onError: (e) => notify.error(formatError(e, "Eroare la reluare.")),
  });
  const deleteMut = useMutation({
    mutationFn: (id: string) => api.delete(id, companyId!),
    onSuccess: () => { inval(); notify.success("Provizion șters."); },
    onError: (e) => notify.error(formatError(e, "Eroare la ștergere.")),
  });

  if (!companyId) {
    return (
      <div className="main-inner">
        <div className="page-head"><div><h1>Provizioane</h1></div></div>
        <div className="empty"><b>Selectați o companie.</b></div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      <div className="page-head">
        <div>
          <h1>Provizioane</h1>
          <p className="sub">{rows.length} provizioane · class 15x</p>
        </div>
        <div className="head-actions" style={{ gap: 8 }}>
          <input className="input" type="month" value={period} onChange={(e) => setPeriod(e.target.value)} style={{ width: 150 }} title="Luna pentru reluare" />
          <button className="btn-dark" onClick={() => setShowCreate(true)}><Ic name="plus" /> Constituie</button>
        </div>
      </div>

      <div className="scr-card">
        {isLoading && <div className="state-row">Se încarcă…</div>}
        {isError && <QueryErrorBanner error={error} label="provizioanele" />}
        {!isLoading && !isError && (
          <table className="scr-table">
            <thead>
              <tr><th>CONT</th><th>DESCRIERE</th><th className="r">SUMĂ</th><th>DEDUCT.</th><th>STATUS</th><th></th></tr>
            </thead>
            <tbody>
              {rows.length === 0 ? (
                <tr><td colSpan={6} style={{ padding: 0 }}>
                  <div className="empty">
                    <div className="ei"><Ic name="scale" /></div>
                    <b>Niciun provizion.</b>
                    Constituiți un provizion când cele 3 condiții (OMFP 1802 pct. 374) sunt îndeplinite cumulativ.
                  </div>
                </td></tr>
              ) : rows.map((p: Provision) => (
                <tr key={p.id}>
                  <td className="num">{p.account15x}</td>
                  <td>{p.description}</td>
                  <td className="r num">{fmtRON(Number(p.amount))}</td>
                  <td>{p.deductible ? <span className="chip paid">deductibil</span> : <span className="chip">nedeductibil</span>}</td>
                  <td>{p.status === "reversed" ? <span className="chip">reluat {p.reversedPeriod}</span> : <span className="chip sent">activ</span>}</td>
                  <td className="r">
                    {p.status === "active" && (
                      <button className="mini-btn" title={`Reluare în ${period}`} disabled={reverseMut.isPending} onClick={() => reverseMut.mutate(p.id)}>
                        <Ic name="undo" />
                      </button>
                    )}
                    <button className="mini-btn" title="Șterge" onClick={() => deleteMut.mutate(p.id)}><Ic name="trash" /></button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {showCreate && (
        <CreateModal companyId={companyId} onClose={() => setShowCreate(false)} onSubmit={(i) => createMut.mutate(i)} saving={createMut.isPending} />
      )}
    </div>
  );
}

function CreateModal({ companyId, onClose, onSubmit, saving }: {
  companyId: string; onClose: () => void; onSubmit: (i: CreateProvisionInput) => void; saving: boolean;
}) {
  const [account15x, setAccount] = useState("1511");
  const [description, setDescription] = useState("");
  const [amount, setAmount] = useState("");
  const [probability, setProbability] = useState("probabil");
  const [createdPeriod, setCreatedPeriod] = useState(thisYM());
  const [deductible, setDeductible] = useState(false);
  const [c1, setC1] = useState(false);
  const [c2, setC2] = useState(false);
  const [c3, setC3] = useState(false);

  return (
    <div className="modal-back show" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <div>
            <div className="mt">Constituire provizion</div>
            <div className="ms">Se postează D 6812 / C 15x. Cele 3 condiții (pct. 374) sunt obligatorii.</div>
          </div>
          <button className="modal-x" onClick={onClose}><Ic name="xMark" /></button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>Cont provizion <span className="req">*</span></label>
              <select className="select" value={account15x} onChange={(e) => setAccount(e.target.value)}>
                {ACCTS.map(([code, name]) => <option key={code} value={code}>{code} — {name}</option>)}
              </select>
            </div>
            <div className="field">
              <label>Sumă (lei) <span className="req">*</span></label>
              <input className="input" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value.replace(/[^0-9.]/g, ""))} placeholder="10000.00" style={{ textAlign: "right" }} />
            </div>
            <div className="field span2">
              <label>Descriere <span className="req">*</span></label>
              <input className="input" value={description} onChange={(e) => setDescription(e.target.value)} placeholder="ex. Litigiu comercial X" />
            </div>
            <div className="field">
              <label>Probabilitate</label>
              <input className="input" value={probability} onChange={(e) => setProbability(e.target.value)} />
            </div>
            <div className="field">
              <label>Luna constituirii</label>
              <input className="input" type="month" value={createdPeriod} onChange={(e) => setCreatedPeriod(e.target.value)} />
            </div>
            <label className="chk-row span2"><input type="checkbox" checked={deductible} onChange={(e) => setDeductible(e.target.checked)} /> Deductibil fiscal (Cod fiscal art. 26 — de regulă NU)</label>
          </div>
          <div style={{ marginTop: 14, paddingTop: 12, borderTop: "1px solid var(--line)" }}>
            <div className="ms" style={{ marginBottom: 8 }}>Condiții cumulative de recunoaștere (OMFP 1802 pct. 374):</div>
            <label className="chk-row"><input type="checkbox" checked={c1} onChange={(e) => setC1(e.target.checked)} /> Obligație actuală dintr-un eveniment trecut</label>
            <label className="chk-row"><input type="checkbox" checked={c2} onChange={(e) => setC2(e.target.checked)} /> Ieșire probabilă de resurse</label>
            <label className="chk-row"><input type="checkbox" checked={c3} onChange={(e) => setC3(e.target.checked)} /> Estimare credibilă a valorii</label>
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn" onClick={onClose}>Anulează</button>
          <button className="btn-dark" disabled={saving || !(c1 && c2 && c3)}
            onClick={() => onSubmit({ companyId, account15x, description, amount, probability, createdPeriod, deductible, obligationPresent: c1, outflowProbable: c2, estimateReliable: c3 })}>
            {saving ? "Se postează…" : "Constituie"}
          </button>
        </div>
      </div>
    </div>
  );
}
