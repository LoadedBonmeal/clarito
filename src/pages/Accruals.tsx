/**
 * Cheltuieli / venituri în avans (471/472) — OMFP 1802/2014 pct. 351.
 * Lean register: schedule a deferral (auto-posts the constituire), recognize the monthly slice
 * at month-end (one balanced journal per period, idempotent).
 */
import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { accruals as api, type Accrual, type CreateAccrualInput } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import { fmtRON } from "@/lib/utils";

const thisYM = () => new Date().toISOString().slice(0, 7);

export function AccrualsPage() {
  const { t } = useTranslation();
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
      notify.success(t("accruals.notify.created"));
    },
    onError: (e) => notify.error(formatError(e, t("accruals.notify.createError"))),
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => api.delete(id, companyId!),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["accruals", companyId] });
      notify.success(t("accruals.notify.deleted"));
    },
    onError: (e) => notify.error(formatError(e, t("accruals.notify.deleteError"))),
  });

  const runMut = useMutation({
    mutationFn: () => api.run(companyId!, runPeriod),
    onSuccess: (r) => {
      notify.success(
        r.posted
          ? t("accruals.notify.runPosted", { period: runPeriod, amount: fmtRON(Number(r.total)) })
          : t("accruals.notify.runNone", { period: runPeriod }),
      );
    },
    onError: (e) => notify.error(formatError(e, t("accruals.notify.runError"))),
  });

  if (!companyId) {
    return (
      <div className="main-inner">
        <div className="page-head"><div><h1>{t("accruals.title")}</h1></div></div>
        <div className="empty"><b>{t("accruals.selectCompany")}</b></div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      <div className="page-head">
        <div>
          <h1>{t("accruals.title")}</h1>
          <p className="sub">{t("accruals.sub", { count: rows.length })}</p>
        </div>
        <div className="head-actions" style={{ gap: 8 }}>
          <input className="input" type="month" value={runPeriod}
            onChange={(e) => setRunPeriod(e.target.value)} style={{ width: 150 }} />
          <button className="btn btn-outline" disabled={runMut.isPending} onClick={() => runMut.mutate()}>
            <Ic name="sync" /> {t("accruals.recognizeMonth")}
          </button>
          <button className="btn-dark" onClick={() => setShowCreate(true)}>
            <Ic name="plus" /> {t("accruals.add")}
          </button>
        </div>
      </div>

      <div className="scr-card">
        {isLoading && <div className="state-row">{t("accruals.loading")}</div>}
        {isError && <QueryErrorBanner error={error} label={t("accruals.errorLabel")} />}
        {!isLoading && !isError && (
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("accruals.table.kind")}</th><th>{t("accruals.table.description")}</th><th>{t("accruals.table.account")}</th>
                <th className="r">{t("accruals.table.total")}</th><th>{t("accruals.table.start")}</th><th className="r">{t("accruals.table.months")}</th><th></th>
              </tr>
            </thead>
            <tbody>
              {rows.length === 0 ? (
                <tr><td colSpan={7} style={{ padding: 0 }}>
                  <div className="empty">
                    <div className="ei"><Ic name="calc" /></div>
                    <b>{t("accruals.empty.title")}</b>
                    {t("accruals.empty.hint")}
                  </div>
                </td></tr>
              ) : rows.map((a: Accrual) => (
                <tr key={a.id}>
                  <td><span className="chip">{a.kind === "prepaid" ? t("accruals.kind.prepaid") : t("accruals.kind.deferred")}</span></td>
                  <td>{a.description}</td>
                  <td className="num">{a.counterAcct}</td>
                  <td className="r num">{fmtRON(Number(a.totalAmount))}</td>
                  <td className="num">{a.startPeriod}</td>
                  <td className="r num">{a.months}</td>
                  <td className="r">
                    <button className="mini-btn" title={t("accruals.row.delete")} onClick={() => deleteMut.mutate(a.id)}>
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
  const { t } = useTranslation();
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
            <div className="mt">{t("accruals.modal.title")}</div>
            <div className="ms">{t("accruals.modal.subtitle")}</div>
          </div>
          <button className="modal-x" onClick={onClose}><Ic name="xMark" /></button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>{t("accruals.modal.kind")}</label>
              <select className="select" value={kind} onChange={(e) => setKind(e.target.value as "prepaid" | "deferred")}>
                <option value="prepaid">{t("accruals.modal.kindPrepaid")}</option>
                <option value="deferred">{t("accruals.modal.kindDeferred")}</option>
              </select>
            </div>
            <div className="field">
              <label>{kind === "prepaid" ? t("accruals.modal.accountExpense") : t("accruals.modal.accountIncome")} <span className="req">*</span></label>
              <input className="input" value={counterAcct} onChange={(e) => setCounterAcct(e.target.value)} placeholder={kind === "prepaid" ? t("accruals.modal.accountPhExpense") : t("accruals.modal.accountPhIncome")} />
            </div>
            <div className="field span2">
              <label>{t("accruals.modal.description")} <span className="req">*</span></label>
              <input className="input" value={description} onChange={(e) => setDescription(e.target.value)} placeholder={t("accruals.modal.descriptionPh")} />
            </div>
            <div className="field">
              <label>{t("accruals.modal.total")} <span className="req">*</span></label>
              <input className="input" inputMode="decimal" value={totalAmount} onChange={(e) => setTotalAmount(e.target.value.replace(/[^0-9.]/g, ""))} placeholder="1200.00" style={{ textAlign: "right" }} />
            </div>
            <div className="field">
              <label>{t("accruals.modal.startMonth")}</label>
              <input className="input" type="month" value={startPeriod} onChange={(e) => setStartPeriod(e.target.value)} />
            </div>
            <div className="field">
              <label>{t("accruals.modal.months")} <span className="req">*</span></label>
              <input className="input" type="number" min={1} value={months} onChange={(e) => setMonths(e.target.value)} />
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn" onClick={onClose}>{t("accruals.modal.cancel")}</button>
          <button className="btn-dark" disabled={saving}
            onClick={() => onSubmit({ companyId, kind, description, counterAcct, totalAmount, startPeriod, months: parseInt(months) || 1 })}>
            {saving ? t("accruals.modal.saving") : t("accruals.modal.save")}
          </button>
        </div>
      </div>
    </div>
  );
}
