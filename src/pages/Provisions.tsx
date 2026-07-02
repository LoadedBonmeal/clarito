/**
 * Provizioane (class 15x) — OMFP 1802/2014 pct. 374. Lean register: constituire D 6812 / C 15x
 * (after confirming the 3 cumulative conditions), reluare/utilizare D 15x / C 7812.
 */
import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { provisions as api, type Provision, type CreateProvisionInput } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import { fmtRON } from "@/lib/utils";

const thisYM = () => new Date().toISOString().slice(0, 7);
/** Account codes — the human labels live in locales (provisions.accounts.*). */
const ACCT_CODES = ["1511", "1512", "1513", "1514", "1515", "1518"] as const;

export function ProvisionsPage() {
  const { t } = useTranslation();
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
    onSuccess: () => { inval(); setShowCreate(false); notify.success(t("provisions.notify.created")); },
    onError: (e) => notify.error(formatError(e, t("provisions.notify.createError"))),
  });
  const reverseMut = useMutation({
    mutationFn: (id: string) => api.reverse(id, companyId!, period),
    onSuccess: () => { inval(); notify.success(t("provisions.notify.reversed")); },
    onError: (e) => notify.error(formatError(e, t("provisions.notify.reverseError"))),
  });
  const deleteMut = useMutation({
    mutationFn: (id: string) => api.delete(id, companyId!),
    onSuccess: () => { inval(); notify.success(t("provisions.notify.deleted")); },
    onError: (e) => notify.error(formatError(e, t("provisions.notify.deleteError"))),
  });

  if (!companyId) {
    return (
      <div className="main-inner">
        <div className="page-head"><div><h1>{t("provisions.title")}</h1></div></div>
        <div className="empty"><b>{t("provisions.selectCompany")}</b></div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      <div className="page-head">
        <div>
          <h1>{t("provisions.title")}</h1>
          <p className="sub">{t("provisions.sub", { count: rows.length })}</p>
        </div>
        <div className="head-actions" style={{ gap: 8 }}>
          <input className="input" type="month" value={period} onChange={(e) => setPeriod(e.target.value)} style={{ width: 150 }} title={t("provisions.periodTitle")} />
          <button className="btn-dark" onClick={() => setShowCreate(true)}><Ic name="plus" /> {t("provisions.create")}</button>
        </div>
      </div>

      <div className="scr-card">
        {isLoading && <div className="state-row">{t("provisions.loading")}</div>}
        {isError && <QueryErrorBanner error={error} label={t("provisions.errorLabel")} />}
        {!isLoading && !isError && (
          <table className="scr-table">
            <thead>
              <tr><th>{t("provisions.table.account")}</th><th>{t("provisions.table.description")}</th><th className="r">{t("provisions.table.amount")}</th><th>{t("provisions.table.deductible")}</th><th>{t("provisions.table.status")}</th><th></th></tr>
            </thead>
            <tbody>
              {rows.length === 0 ? (
                <tr><td colSpan={6} style={{ padding: 0 }}>
                  <div className="empty">
                    <div className="ei"><Ic name="scale" /></div>
                    <b>{t("provisions.empty.title")}</b>
                    {t("provisions.empty.hint")}
                  </div>
                </td></tr>
              ) : rows.map((p: Provision) => (
                <tr key={p.id}>
                  <td className="num">{p.account15x}</td>
                  <td>{p.description}</td>
                  <td className="r num">{fmtRON(Number(p.amount))}</td>
                  <td>{p.deductible ? <span className="chip paid">{t("provisions.chip.deductible")}</span> : <span className="chip">{t("provisions.chip.nonDeductible")}</span>}</td>
                  <td>{p.status === "reversed" ? <span className="chip">{t("provisions.chip.reversed", { period: p.reversedPeriod })}</span> : <span className="chip sent">{t("provisions.chip.active")}</span>}</td>
                  <td className="r">
                    {p.status === "active" && (
                      <button className="mini-btn" title={t("provisions.row.reverseIn", { period })} disabled={reverseMut.isPending} onClick={() => reverseMut.mutate(p.id)}>
                        <Ic name="undo" />
                      </button>
                    )}
                    <button className="mini-btn" title={t("provisions.row.delete")} onClick={() => deleteMut.mutate(p.id)}><Ic name="trash" /></button>
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
  const { t } = useTranslation();
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
            <div className="mt">{t("provisions.modal.title")}</div>
            <div className="ms">{t("provisions.modal.subtitle")}</div>
          </div>
          <button className="modal-x" onClick={onClose}><Ic name="xMark" /></button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>{t("provisions.modal.account")} <span className="req">*</span></label>
              <select className="select" value={account15x} onChange={(e) => setAccount(e.target.value)}>
                {ACCT_CODES.map((code) => <option key={code} value={code}>{code} — {t(`provisions.accounts.${code}`)}</option>)}
              </select>
            </div>
            <div className="field">
              <label>{t("provisions.modal.amount")} <span className="req">*</span></label>
              <input className="input" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value.replace(/[^0-9.]/g, ""))} placeholder="10000.00" style={{ textAlign: "right" }} />
            </div>
            <div className="field span2">
              <label>{t("provisions.modal.description")} <span className="req">*</span></label>
              <input className="input" value={description} onChange={(e) => setDescription(e.target.value)} placeholder={t("provisions.modal.descriptionPh")} />
            </div>
            <div className="field">
              <label>{t("provisions.modal.probability")}</label>
              <input className="input" value={probability} onChange={(e) => setProbability(e.target.value)} />
            </div>
            <div className="field">
              <label>{t("provisions.modal.createdMonth")}</label>
              <input className="input" type="month" value={createdPeriod} onChange={(e) => setCreatedPeriod(e.target.value)} />
            </div>
            <label className="chk-row span2"><input type="checkbox" checked={deductible} onChange={(e) => setDeductible(e.target.checked)} /> {t("provisions.modal.deductibleLabel")}</label>
          </div>
          <div style={{ marginTop: 14, paddingTop: 12, borderTop: "1px solid var(--line)" }}>
            <div className="ms" style={{ marginBottom: 8 }}>{t("provisions.modal.conditionsTitle")}</div>
            <label className="chk-row"><input type="checkbox" checked={c1} onChange={(e) => setC1(e.target.checked)} /> {t("provisions.modal.c1")}</label>
            <label className="chk-row"><input type="checkbox" checked={c2} onChange={(e) => setC2(e.target.checked)} /> {t("provisions.modal.c2")}</label>
            <label className="chk-row"><input type="checkbox" checked={c3} onChange={(e) => setC3(e.target.checked)} /> {t("provisions.modal.c3")}</label>
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn" onClick={onClose}>{t("provisions.modal.cancel")}</button>
          <button className="btn-dark" disabled={saving || !(c1 && c2 && c3)}
            onClick={() => onSubmit({ companyId, account15x, description, amount, probability, createdPeriod, deductible, obligationPresent: c1, outflowProbable: c2, estimateReliable: c3 })}>
            {saving ? t("provisions.modal.posting") : t("provisions.modal.submit")}
          </button>
        </div>
      </div>
    </div>
  );
}
