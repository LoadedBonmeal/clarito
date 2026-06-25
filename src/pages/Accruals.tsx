/**
 * Cheltuieli / venituri în avans (471/472) — OMFP 1802/2014 pct. 351.
 * Lean register: schedule a deferral (auto-posts the constituire), recognize the monthly slice
 * at month-end (one balanced journal per period, idempotent).
 */
import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { accruals as api, type Accrual, type CreateAccrualInput } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import { fmtRON } from "@/lib/utils";

const thisYM = () => new Date().toISOString().slice(0, 7);

export function AccrualsPage() {
  const companyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();

  const [showCreate, setShowCreate] = useState(false);
  const [runPeriod, setRunPeriod] = useState(thisYM());

  const { data: rows = [], isLoading, isError, error } = useQuery({
    queryKey: ["accruals", companyId],
    queryFn: () => api.list(companyId!),
    enabled: !!companyId,
  });

  const createMut = useMutation({
    mutationFn: (input: CreateAccrualInput) => api.create(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["accruals", companyId] });
      setShowCreate(false);
      notify.success("Accrual înregistrat — constituirea a fost postată.");
    },
    onError: (e) => notify.error(formatError(e, "Eroare la înregistrarea accrual-ului.")),
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => api.delete(id, companyId!),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["accruals", companyId] });
      notify.success("Accrual șters.");
    },
    onError: (e) => notify.error(formatError(e, "Eroare la ștergere.")),
  });

  const runMut = useMutation({
    mutationFn: () => api.run(companyId!, runPeriod),
    onSuccess: (r) => {
      notify.success(
        r.posted
          ? `Recunoaștere ${runPeriod}: ${fmtRON(Number(r.total))} RON postați.`
          : `Nicio tranșă activă în ${runPeriod}.`,
      );
    },
    onError: (e) => notify.error(formatError(e, "Eroare la recunoaștere.")),
  });

  if (!companyId) {
    return (
      <div className="main-inner">
        <div className="page-head"><div><h1>Cheltuieli / venituri în avans</h1></div></div>
        <div className="empty"><b>Selectați o companie.</b></div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      <div className="page-head">
        <div>
          <h1>Cheltuieli / venituri în avans</h1>
          <p className="sub">{rows.length} înregistrări</p>
        </div>
        <div className="head-actions" style={{ gap: 8 }}>
          <input className="input" type="month" value={runPeriod}
            onChange={(e) => setRunPeriod(e.target.value)} style={{ width: 150 }} />
          <button className="btn btn-outline" disabled={runMut.isPending} onClick={() => runMut.mutate()}>
            <Ic name="sync" /> Recunoaște luna
          </button>
          <button className="btn-dark" onClick={() => setShowCreate(true)}>
            <Ic name="plus" /> Adaugă
          </button>
        </div>
      </div>

      <div className="scr-card">
        {isLoading && <div className="state-row">Se încarcă…</div>}
        {isError && <QueryErrorBanner error={error} label="accrual-urile" />}
        {!isLoading && !isError && (
          <table className="scr-table">
            <thead>
              <tr>
                <th>TIP</th><th>DESCRIERE</th><th>CONT</th>
                <th className="r">TOTAL</th><th>START</th><th className="r">LUNI</th><th></th>
              </tr>
            </thead>
            <tbody>
              {rows.length === 0 ? (
                <tr><td colSpan={7} style={{ padding: 0 }}>
                  <div className="empty">
                    <div className="ei"><Ic name="calc" /></div>
                    <b>Nicio cheltuială/venit în avans.</b>
                    Înregistrați o sumă plătită/încasată în avans pentru a o eșalona pe luni.
                  </div>
                </td></tr>
              ) : rows.map((a: Accrual) => (
                <tr key={a.id}>
                  <td><span className="chip">{a.kind === "prepaid" ? "Cheltuială" : "Venit"}</span></td>
                  <td>{a.description}</td>
                  <td className="num">{a.counterAcct}</td>
                  <td className="r num">{fmtRON(Number(a.totalAmount))}</td>
                  <td className="num">{a.startPeriod}</td>
                  <td className="r num">{a.months}</td>
                  <td className="r">
                    <button className="mini-btn" title="Șterge" onClick={() => deleteMut.mutate(a.id)}>
                      <Ic name="trash" />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {showCreate && (
        <CreateModal
          companyId={companyId}
          onClose={() => setShowCreate(false)}
          onSubmit={(input) => createMut.mutate(input)}
          saving={createMut.isPending}
        />
      )}
    </div>
  );
}

function CreateModal({
  companyId, onClose, onSubmit, saving,
}: {
  companyId: string;
  onClose: () => void;
  onSubmit: (i: CreateAccrualInput) => void;
  saving: boolean;
}) {
  const [kind, setKind] = useState<"prepaid" | "deferred">("prepaid");
  const [description, setDescription] = useState("");
  const [counterAcct, setCounterAcct] = useState("");
  const [totalAmount, setTotalAmount] = useState("");
  const [startPeriod, setStartPeriod] = useState(thisYM());
  const [months, setMonths] = useState("12");

  return (
    <div className="modal-back show" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <div>
            <div className="mt">Cheltuială / venit în avans</div>
            <div className="ms">Constituirea (471/472) se postează automat; recunoașterea se face lunar.</div>
          </div>
          <button className="modal-x" onClick={onClose}><Ic name="xMark" /></button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>Tip</label>
              <select className="select" value={kind} onChange={(e) => setKind(e.target.value as "prepaid" | "deferred")}>
                <option value="prepaid">Cheltuială în avans (471)</option>
                <option value="deferred">Venit în avans (472)</option>
              </select>
            </div>
            <div className="field">
              <label>Cont {kind === "prepaid" ? "cheltuială (6xx)" : "venit (7xx)"} <span className="req">*</span></label>
              <input className="input" value={counterAcct} onChange={(e) => setCounterAcct(e.target.value)} placeholder={kind === "prepaid" ? "ex. 613" : "ex. 706"} />
            </div>
            <div className="field span2">
              <label>Descriere <span className="req">*</span></label>
              <input className="input" value={description} onChange={(e) => setDescription(e.target.value)} placeholder="ex. Asigurare RCA 12 luni" />
            </div>
            <div className="field">
              <label>Sumă totală (lei) <span className="req">*</span></label>
              <input className="input" inputMode="decimal" value={totalAmount} onChange={(e) => setTotalAmount(e.target.value.replace(/[^0-9.]/g, ""))} placeholder="1200.00" style={{ textAlign: "right" }} />
            </div>
            <div className="field">
              <label>Lună de start</label>
              <input className="input" type="month" value={startPeriod} onChange={(e) => setStartPeriod(e.target.value)} />
            </div>
            <div className="field">
              <label>Eșalonare (luni) <span className="req">*</span></label>
              <input className="input" type="number" min={1} value={months} onChange={(e) => setMonths(e.target.value)} />
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn" onClick={onClose}>Anulează</button>
          <button className="btn-dark" disabled={saving}
            onClick={() => onSubmit({ companyId, kind, description, counterAcct, totalAmount, startPeriod, months: parseInt(months) || 1 })}>
            {saving ? "Se salvează…" : "Înregistrează"}
          </button>
        </div>
      </div>
    </div>
  );
}
