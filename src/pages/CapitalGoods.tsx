/**
 * Registrul bunurilor de capital — ajustarea TVA (Cod fiscal art. 305).
 * Movables = 5-year adjustment period, immovables = 20-year. On a use-change year, 1/N of the
 * deducted VAT is adjusted by the change in deduction %, posted to the GL (D 635/C 4426 clawback,
 * D 4426/C 758 positive) and surfaced for the D300 deductible-adjustment row.
 */
import { Fragment, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import {
  capitalGoods as api,
  type CapitalGood,
  type CapitalGoodAdjustment,
  type CreateCapitalGoodInput,
  type RecordAdjustmentInput,
} from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import { fmtRON } from "@/lib/utils";

const thisYM = () => new Date().toISOString().slice(0, 7);
const today = () => new Date().toISOString().slice(0, 10);

/** Client-side preview of the art. 305 annual adjustment (mirrors the Rust compute_adjustment). */
function previewAdjustment(vatDeducted: string, n: number, initialPct: number, newPct: number): number {
  const v = Number(vatDeducted);
  if (!Number.isFinite(v) || n <= 0) return 0;
  return Math.round(((v / n) * (newPct - initialPct)) / 100 * 100) / 100;
}

export function CapitalGoodsPage() {
  const companyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();
  const [showCreate, setShowCreate] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);

  const { data: rows = [], isLoading, isError, error } = useQuery({
    queryKey: ["capital-goods", companyId],
    queryFn: () => api.list(companyId!),
    enabled: !!companyId,
  });

  const inval = () => void qc.invalidateQueries({ queryKey: ["capital-goods", companyId] });

  const createMut = useMutation({
    mutationFn: (i: CreateCapitalGoodInput) => api.create(i),
    onSuccess: () => { inval(); setShowCreate(false); notify.success("Bun de capital înregistrat."); },
    onError: (e) => notify.error(formatError(e, "Eroare la înregistrare.")),
  });
  const deleteMut = useMutation({
    mutationFn: (id: string) => api.delete(id, companyId!),
    onSuccess: () => { inval(); notify.success("Bun de capital șters."); },
    onError: (e) => notify.error(formatError(e, "Eroare la ștergere.")),
  });

  if (!companyId) {
    return (
      <div className="main-inner">
        <div className="page-head"><div><h1>Bunuri de capital</h1></div></div>
        <div className="empty"><b>Selectați o companie.</b></div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      <div className="page-head">
        <div>
          <h1>Registrul bunurilor de capital</h1>
          <p className="sub">{rows.length} bunuri · ajustare TVA art. 305 (5 ani mobil / 20 ani imobil)</p>
        </div>
        <div className="head-actions">
          <button className="btn-dark" onClick={() => setShowCreate(true)}><Ic name="plus" /> Adaugă bun</button>
        </div>
      </div>

      <div className="scr-card">
        {isLoading && <div className="state-row">Se încarcă…</div>}
        {isError && <QueryErrorBanner error={error} label="bunurile de capital" />}
        {!isLoading && !isError && (
          <table className="scr-table">
            <thead>
              <tr>
                <th>DESCRIERE</th><th>TIP</th><th>ACHIZIȚIE</th>
                <th className="r">TVA DEDUSĂ</th><th className="r">% INIȚIAL</th><th>STATUS</th><th></th>
              </tr>
            </thead>
            <tbody>
              {rows.length === 0 ? (
                <tr><td colSpan={7} style={{ padding: 0 }}>
                  <div className="empty">
                    <div className="ei"><Ic name="building" /></div>
                    <b>Niciun bun de capital.</b>
                    Înregistrați imobilizările cu TVA dedusă supusă ajustării (art. 305).
                  </div>
                </td></tr>
              ) : rows.map((g: CapitalGood) => (
                <Fragment key={g.id}>
                  <tr>
                    <td>
                      <button className="btn-plain" style={{ display: "inline-flex", alignItems: "center", gap: 6, background: "none", border: "none", cursor: "pointer", padding: 0, font: "inherit", color: "inherit" }} onClick={() => setExpanded(expanded === g.id ? null : g.id)} title="Vezi ajustările">
                        <Ic name={expanded === g.id ? "chevD" : "chevR"} /> {g.description}
                      </button>
                    </td>
                    <td><span className="chip">{g.kind === "immovable" ? "imobil · 20 ani" : "mobil · 5 ani"}</span></td>
                    <td className="num">{g.acquisitionDate}</td>
                    <td className="r num">{fmtRON(Number(g.vatDeducted))}</td>
                    <td className="r num">{g.initialDeductionPct}%</td>
                    <td>{g.status === "disposed" ? <span className="chip">cesionat</span> : <span className="chip sent">activ</span>}</td>
                    <td className="r">
                      <button className="mini-btn" title="Șterge" onClick={() => deleteMut.mutate(g.id)}><Ic name="xMark" /></button>
                    </td>
                  </tr>
                  {expanded === g.id && (
                    <tr>
                      <td colSpan={7} style={{ padding: 0, background: "var(--bg-subtle, #fafafa)" }}>
                        <AdjustmentPanel good={g} companyId={companyId} />
                      </td>
                    </tr>
                  )}
                </Fragment>
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

function AdjustmentPanel({ good, companyId }: { good: CapitalGood; companyId: string }) {
  const qc = useQueryClient();
  const [showRecord, setShowRecord] = useState(false);

  const { data: adjs = [], isLoading } = useQuery({
    queryKey: ["cg-adjustments", good.id],
    queryFn: () => api.listAdjustments(good.id, companyId),
  });

  const recordMut = useMutation({
    mutationFn: (i: RecordAdjustmentInput) => api.recordAdjustment(i),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["cg-adjustments", good.id] });
      setShowRecord(false);
      notify.success("Ajustare înregistrată și postată în GL.");
    },
    onError: (e) => notify.error(formatError(e, "Eroare la înregistrarea ajustării.")),
  });

  return (
    <div style={{ padding: "12px 16px" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 8 }}>
        <div className="ms">
          Bază TVA {fmtRON(Number(good.vatDeducted))} · perioada de ajustare {good.adjustmentYears} ani ·
          tranșă anuală {fmtRON(Number(good.vatDeducted) / good.adjustmentYears)}
        </div>
        <button className="btn" onClick={() => setShowRecord(true)}><Ic name="plus" /> Înregistrează ajustare</button>
      </div>
      {isLoading ? <div className="state-row">Se încarcă…</div> : adjs.length === 0 ? (
        <div className="ms" style={{ padding: "8px 0" }}>Nicio ajustare — bunul își păstrează utilizarea inițială.</div>
      ) : (
        <table className="scr-table">
          <thead><tr><th>AN</th><th className="r">% NOU</th><th className="r">AJUSTARE</th><th>PERIOADĂ</th><th>GL</th></tr></thead>
          <tbody>
            {adjs.map((a: CapitalGoodAdjustment) => {
              const amt = Number(a.adjustmentAmount);
              return (
                <tr key={a.id}>
                  <td className="num">{a.year}/{good.adjustmentYears}</td>
                  <td className="r num">{a.newDeductionPct}%</td>
                  <td className="r num" style={{ color: amt < 0 ? "var(--danger, #c0392b)" : amt > 0 ? "var(--ok, #1e7e34)" : undefined }}>
                    {amt > 0 ? "+" : ""}{fmtRON(amt)}
                  </td>
                  <td className="num">{a.period}</td>
                  <td>{a.posted ? <span className="chip paid">postat</span> : <span className="chip">—</span>}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      {showRecord && (
        <RecordModal good={good} companyId={companyId} onClose={() => setShowRecord(false)} onSubmit={(i) => recordMut.mutate(i)} saving={recordMut.isPending} />
      )}
    </div>
  );
}

function CreateModal({ companyId, onClose, onSubmit, saving }: {
  companyId: string; onClose: () => void; onSubmit: (i: CreateCapitalGoodInput) => void; saving: boolean;
}) {
  const [description, setDescription] = useState("");
  const [kind, setKind] = useState<"movable" | "immovable">("immovable");
  const [acquisitionDate, setAcq] = useState(today());
  const [baseValue, setBase] = useState("");
  const [vatDeducted, setVat] = useState("");
  const [initialDeductionPct, setPct] = useState("100");

  const canSave = description.trim() !== "" && vatDeducted !== "" && !saving;

  return (
    <div className="modal-back show" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <div>
            <div className="mt">Adaugă bun de capital</div>
            <div className="ms">Perioada de ajustare: 5 ani (mobil) / 20 ani (imobil) — Cod fiscal art. 305.</div>
          </div>
          <button className="modal-x" onClick={onClose}><Ic name="xMark" /></button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field span2">
              <label>Descriere <span className="req">*</span></label>
              <input className="input" value={description} onChange={(e) => setDescription(e.target.value)} placeholder="ex. Hală producție" />
            </div>
            <div className="field">
              <label>Tip</label>
              <select className="select" value={kind} onChange={(e) => setKind(e.target.value as "movable" | "immovable")}>
                <option value="immovable">Imobil (20 ani)</option>
                <option value="movable">Mobil / servicii (5 ani)</option>
              </select>
            </div>
            <div className="field">
              <label>Data achiziției</label>
              <input className="input" type="date" value={acquisitionDate} onChange={(e) => setAcq(e.target.value)} />
            </div>
            <div className="field">
              <label>Valoare de bază (lei)</label>
              <input className="input" inputMode="decimal" value={baseValue} onChange={(e) => setBase(e.target.value.replace(/[^0-9.]/g, ""))} placeholder="1000000.00" style={{ textAlign: "right" }} />
            </div>
            <div className="field">
              <label>TVA dedusă (lei) <span className="req">*</span></label>
              <input className="input" inputMode="decimal" value={vatDeducted} onChange={(e) => setVat(e.target.value.replace(/[^0-9.]/g, ""))} placeholder="210000.00" style={{ textAlign: "right" }} />
            </div>
            <div className="field">
              <label>% deducere inițial</label>
              <input className="input" inputMode="decimal" value={initialDeductionPct} onChange={(e) => setPct(e.target.value.replace(/[^0-9.]/g, ""))} style={{ textAlign: "right" }} />
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn" onClick={onClose}>Anulează</button>
          <button className="btn-dark" disabled={!canSave}
            onClick={() => onSubmit({ companyId, description, kind, acquisitionDate, baseValue: baseValue || "0", vatDeducted, initialDeductionPct: Number(initialDeductionPct) || 100 })}>
            {saving ? "Se salvează…" : "Adaugă"}
          </button>
        </div>
      </div>
    </div>
  );
}

function RecordModal({ good, companyId, onClose, onSubmit, saving }: {
  good: CapitalGood; companyId: string; onClose: () => void; onSubmit: (i: RecordAdjustmentInput) => void; saving: boolean;
}) {
  const [year, setYear] = useState("2");
  const [newDeductionPct, setNewPct] = useState("0");
  const [period, setPeriod] = useState(thisYM());

  const preview = previewAdjustment(good.vatDeducted, good.adjustmentYears, good.initialDeductionPct, Number(newDeductionPct) || 0);
  const yearN = Number(year);
  const canSave = yearN >= 1 && yearN <= good.adjustmentYears && !saving;

  return (
    <div className="modal-back show" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <div>
            <div className="mt">Înregistrează ajustare TVA</div>
            <div className="ms">{good.description} · {good.adjustmentYears} ani · deducere inițială {good.initialDeductionPct}%</div>
          </div>
          <button className="modal-x" onClick={onClose}><Ic name="xMark" /></button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>An din perioadă (1–{good.adjustmentYears})</label>
              <input className="input" inputMode="numeric" value={year} onChange={(e) => setYear(e.target.value.replace(/[^0-9]/g, ""))} style={{ textAlign: "right" }} />
            </div>
            <div className="field">
              <label>% deducere nou</label>
              <input className="input" inputMode="decimal" value={newDeductionPct} onChange={(e) => setNewPct(e.target.value.replace(/[^0-9.]/g, ""))} style={{ textAlign: "right" }} />
            </div>
            <div className="field">
              <label>Perioadă (decont)</label>
              <input className="input" type="month" value={period} onChange={(e) => setPeriod(e.target.value)} />
            </div>
          </div>
          <div style={{ marginTop: 14, padding: "12px 14px", borderRadius: 8, background: "var(--bg-subtle, #f5f5f5)", border: "1px solid var(--line)" }}>
            <div className="ms" style={{ marginBottom: 4 }}>Ajustare calculată (1/{good.adjustmentYears} × Δ% × TVA):</div>
            <div style={{ fontSize: 22, fontWeight: 700, color: preview < 0 ? "var(--danger, #c0392b)" : preview > 0 ? "var(--ok, #1e7e34)" : undefined }}>
              {preview > 0 ? "+" : ""}{fmtRON(preview)}
            </div>
            <div className="ms" style={{ marginTop: 4 }}>
              {preview < 0 ? "Clawback — D 635 / C 4426 (TVA dedusă devine cost)." : preview > 0 ? "Deducere suplimentară — D 4426 / C 758." : "Fără ajustare (utilizare neschimbată)."}
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn" onClick={onClose}>Anulează</button>
          <button className="btn-dark" disabled={!canSave}
            onClick={() => onSubmit({ companyId, capitalGoodId: good.id, year: yearN, newDeductionPct: Number(newDeductionPct) || 0, period })}>
            {saving ? "Se postează…" : "Înregistrează + postează"}
          </button>
        </div>
      </div>
    </div>
  );
}
