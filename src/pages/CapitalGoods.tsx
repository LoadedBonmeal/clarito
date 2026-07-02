/**
 * Registrul bunurilor de capital — ajustarea TVA (Cod fiscal art. 305).
 * Movables = 5-year adjustment period, immovables = 20-year. On a use-change year, 1/N of the
 * deducted VAT is adjusted by the change in deduction %, posted to the GL (D 635/C 4426 clawback,
 * D 4426/C 758 positive) and surfaced for the D300 deductible-adjustment row.
 */
import { Fragment, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

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
  const { t } = useTranslation();
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
    onSuccess: () => { inval(); setShowCreate(false); notify.success(t("capitalGoods.notify.created")); },
    onError: (e) => notify.error(formatError(e, t("capitalGoods.notify.createError"))),
  });
  const deleteMut = useMutation({
    mutationFn: (id: string) => api.delete(id, companyId!),
    onSuccess: () => { inval(); notify.success(t("capitalGoods.notify.deleted")); },
    onError: (e) => notify.error(formatError(e, t("capitalGoods.notify.deleteError"))),
  });

  if (!companyId) {
    return (
      <div className="main-inner">
        <div className="page-head"><div><h1>{t("capitalGoods.shortTitle")}</h1></div></div>
        <div className="empty"><b>{t("capitalGoods.selectCompany")}</b></div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      <div className="page-head">
        <div>
          <h1>{t("capitalGoods.title")}</h1>
          <p className="sub">{t("capitalGoods.sub", { count: rows.length })}</p>
        </div>
        <div className="head-actions">
          <button className="btn-dark" onClick={() => setShowCreate(true)}><Ic name="plus" /> {t("capitalGoods.add")}</button>
        </div>
      </div>

      <div className="scr-card">
        {isLoading && <div className="state-row">{t("capitalGoods.loading")}</div>}
        {isError && <QueryErrorBanner error={error} label={t("capitalGoods.errorLabel")} />}
        {!isLoading && !isError && (
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("capitalGoods.table.description")}</th><th>{t("capitalGoods.table.kind")}</th><th>{t("capitalGoods.table.acquisition")}</th>
                <th className="r">{t("capitalGoods.table.vatDeducted")}</th><th className="r">{t("capitalGoods.table.initialPct")}</th><th>{t("capitalGoods.table.status")}</th><th></th>
              </tr>
            </thead>
            <tbody>
              {rows.length === 0 ? (
                <tr><td colSpan={7} style={{ padding: 0 }}>
                  <div className="empty">
                    <div className="ei"><Ic name="building" /></div>
                    <b>{t("capitalGoods.empty.title")}</b>
                    {t("capitalGoods.empty.hint")}
                  </div>
                </td></tr>
              ) : rows.map((g: CapitalGood) => (
                <Fragment key={g.id}>
                  <tr>
                    <td>
                      <button className="btn-plain" style={{ display: "inline-flex", alignItems: "center", gap: 6, background: "none", border: "none", cursor: "pointer", padding: 0, font: "inherit", color: "inherit" }} onClick={() => setExpanded(expanded === g.id ? null : g.id)} title={t("capitalGoods.row.viewAdjustments")}>
                        <Ic name={expanded === g.id ? "chevD" : "chevR"} /> {g.description}
                      </button>
                    </td>
                    <td><span className="chip">{g.kind === "immovable" ? t("capitalGoods.kind.immovable") : t("capitalGoods.kind.movable")}</span></td>
                    <td className="num">{g.acquisitionDate}</td>
                    <td className="r num">{fmtRON(Number(g.vatDeducted))}</td>
                    <td className="r num">{g.initialDeductionPct}%</td>
                    <td>{g.status === "disposed" ? <span className="chip">{t("capitalGoods.status.disposed")}</span> : <span className="chip sent">{t("capitalGoods.status.active")}</span>}</td>
                    <td className="r">
                      <button className="mini-btn" title={t("capitalGoods.row.delete")} onClick={() => deleteMut.mutate(g.id)}><Ic name="xMark" /></button>
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
  const { t } = useTranslation();
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
      notify.success(t("capitalGoods.notify.adjRecorded"));
    },
    onError: (e) => notify.error(formatError(e, t("capitalGoods.notify.adjError"))),
  });

  return (
    <div style={{ padding: "12px 16px" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 8 }}>
        <div className="ms">
          {t("capitalGoods.panel.meta", {
            vat: fmtRON(Number(good.vatDeducted)),
            years: good.adjustmentYears,
            slice: fmtRON(Number(good.vatDeducted) / good.adjustmentYears),
          })}
        </div>
        <button className="btn" onClick={() => setShowRecord(true)}><Ic name="plus" /> {t("capitalGoods.panel.record")}</button>
      </div>
      {isLoading ? <div className="state-row">{t("capitalGoods.loading")}</div> : adjs.length === 0 ? (
        <div className="ms" style={{ padding: "8px 0" }}>{t("capitalGoods.panel.none")}</div>
      ) : (
        <table className="scr-table">
          <thead><tr><th>{t("capitalGoods.panel.table.year")}</th><th className="r">{t("capitalGoods.panel.table.newPct")}</th><th className="r">{t("capitalGoods.panel.table.adjustment")}</th><th>{t("capitalGoods.panel.table.period")}</th><th>{t("capitalGoods.panel.table.gl")}</th></tr></thead>
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
                  <td>{a.posted ? <span className="chip paid">{t("capitalGoods.panel.posted")}</span> : <span className="chip">—</span>}</td>
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
  const { t } = useTranslation();
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
            <div className="mt">{t("capitalGoods.createModal.title")}</div>
            <div className="ms">{t("capitalGoods.createModal.subtitle")}</div>
          </div>
          <button className="modal-x" onClick={onClose}><Ic name="xMark" /></button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field span2">
              <label>{t("capitalGoods.createModal.description")} <span className="req">*</span></label>
              <input className="input" value={description} onChange={(e) => setDescription(e.target.value)} placeholder={t("capitalGoods.createModal.descriptionPh")} />
            </div>
            <div className="field">
              <label>{t("capitalGoods.createModal.kind")}</label>
              <select className="select" value={kind} onChange={(e) => setKind(e.target.value as "movable" | "immovable")}>
                <option value="immovable">{t("capitalGoods.createModal.kindImmovable")}</option>
                <option value="movable">{t("capitalGoods.createModal.kindMovable")}</option>
              </select>
            </div>
            <div className="field">
              <label>{t("capitalGoods.createModal.acquisitionDate")}</label>
              <input className="input" type="date" value={acquisitionDate} onChange={(e) => setAcq(e.target.value)} />
            </div>
            <div className="field">
              <label>{t("capitalGoods.createModal.baseValue")}</label>
              <input className="input" inputMode="decimal" value={baseValue} onChange={(e) => setBase(e.target.value.replace(/[^0-9.]/g, ""))} placeholder="1000000.00" style={{ textAlign: "right" }} />
            </div>
            <div className="field">
              <label>{t("capitalGoods.createModal.vatDeducted")} <span className="req">*</span></label>
              <input className="input" inputMode="decimal" value={vatDeducted} onChange={(e) => setVat(e.target.value.replace(/[^0-9.]/g, ""))} placeholder="210000.00" style={{ textAlign: "right" }} />
            </div>
            <div className="field">
              <label>{t("capitalGoods.createModal.initialPct")}</label>
              <input className="input" inputMode="decimal" value={initialDeductionPct} onChange={(e) => setPct(e.target.value.replace(/[^0-9.]/g, ""))} style={{ textAlign: "right" }} />
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn" onClick={onClose}>{t("capitalGoods.createModal.cancel")}</button>
          <button className="btn-dark" disabled={!canSave}
            onClick={() => onSubmit({ companyId, description, kind, acquisitionDate, baseValue: baseValue || "0", vatDeducted, initialDeductionPct: Number(initialDeductionPct) || 100 })}>
            {saving ? t("capitalGoods.createModal.saving") : t("capitalGoods.createModal.save")}
          </button>
        </div>
      </div>
    </div>
  );
}

function RecordModal({ good, companyId, onClose, onSubmit, saving }: {
  good: CapitalGood; companyId: string; onClose: () => void; onSubmit: (i: RecordAdjustmentInput) => void; saving: boolean;
}) {
  const { t } = useTranslation();
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
            <div className="mt">{t("capitalGoods.recordModal.title")}</div>
            <div className="ms">{t("capitalGoods.recordModal.subtitle", { description: good.description, years: good.adjustmentYears, pct: good.initialDeductionPct })}</div>
          </div>
          <button className="modal-x" onClick={onClose}><Ic name="xMark" /></button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>{t("capitalGoods.recordModal.yearLabel", { years: good.adjustmentYears })}</label>
              <input className="input" inputMode="numeric" value={year} onChange={(e) => setYear(e.target.value.replace(/[^0-9]/g, ""))} style={{ textAlign: "right" }} />
            </div>
            <div className="field">
              <label>{t("capitalGoods.recordModal.newPct")}</label>
              <input className="input" inputMode="decimal" value={newDeductionPct} onChange={(e) => setNewPct(e.target.value.replace(/[^0-9.]/g, ""))} style={{ textAlign: "right" }} />
            </div>
            <div className="field">
              <label>{t("capitalGoods.recordModal.period")}</label>
              <input className="input" type="month" value={period} onChange={(e) => setPeriod(e.target.value)} />
            </div>
          </div>
          <div style={{ marginTop: 14, padding: "12px 14px", borderRadius: 8, background: "var(--bg-subtle, #f5f5f5)", border: "1px solid var(--line)" }}>
            <div className="ms" style={{ marginBottom: 4 }}>{t("capitalGoods.recordModal.computed", { years: good.adjustmentYears })}</div>
            <div style={{ fontSize: 22, fontWeight: 700, color: preview < 0 ? "var(--danger, #c0392b)" : preview > 0 ? "var(--ok, #1e7e34)" : undefined }}>
              {preview > 0 ? "+" : ""}{fmtRON(preview)}
            </div>
            <div className="ms" style={{ marginTop: 4 }}>
              {preview < 0 ? t("capitalGoods.recordModal.clawback") : preview > 0 ? t("capitalGoods.recordModal.extra") : t("capitalGoods.recordModal.noChange")}
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn" onClick={onClose}>{t("capitalGoods.recordModal.cancel")}</button>
          <button className="btn-dark" disabled={!canSave}
            onClick={() => onSubmit({ companyId, capitalGoodId: good.id, year: yearN, newDeductionPct: Number(newDeductionPct) || 0, period })}>
            {saving ? t("capitalGoods.recordModal.posting") : t("capitalGoods.recordModal.submit")}
          </button>
        </div>
      </div>
    </div>
  );
}
