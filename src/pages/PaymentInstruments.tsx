/**
 * PaymentInstruments page — CEC & Bilete la ordin register.
 *
 * Monografie (OMFP 1802/2014 + Legea 58/1934 + Legea 59/1934):
 *  - CEC primit:  primire D413/C4111 → depunere D5112/C413 → încasare D5121/C5112
 *  - BO primit:   primire D413/C4111 → depunere D5113/C413 → încasare D5121/C5113
 *  - BO scontat:  remitere D5114/C413 → D5121+D667[+D627]/C5114
 *  - Refuz:       D4111/C5112(CEC) sau D4111/C5113(BO)
 *  - Emis:        acceptare D401/C403 → plată D403/C5121
 *
 * Features:
 *  - CRUD (create/edit only when status=registered)
 *  - Tab filter: Toate / Primite / Emise / Active / Finalizate
 *  - Lifecycle actions: depunere, încasare, scontare (BO), refuz, plată
 *  - GL auto-post on every lifecycle transition
 */

import { useCallback, useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { api } from "@/lib/tauri";
import type {
  PaymentInstrument,
  PiKind,
  PiDirection,
  PiStatus,
  CreatePaymentInstrumentArgs,
  UpdatePaymentInstrumentArgs,
} from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";

// ─── Helpers ─────────────────────────────────────────────────────────────────

const RO_MON = ["ian","feb","mar","apr","mai","iun","iul","aug","sep","oct","nov","dec"];
const fmtRoDate = (iso: string | null | undefined) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

const localDateISO = (d = new Date()) => {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
};

const ACTIVE_STATUSES: PiStatus[] = ["registered", "deposited"];
const SETTLED_STATUSES: PiStatus[] = ["collected", "paid", "discounted", "dishonored"];

type TabId = "all" | "received" | "issued" | "active" | "settled";

// ─── Status chip ─────────────────────────────────────────────────────────────

const STATUS_CLS: Record<PiStatus, string> = {
  registered: "sent",
  deposited:  "sent",
  discounted: "paid",
  collected:  "paid",
  paid:       "paid",
  dishonored: "over",
};

function StatusChip({ status }: { status: PiStatus }) {
  const { t } = useTranslation();
  return (
    <span className={`status-chip ${STATUS_CLS[status] ?? "sent"}`}>
      {t(`pi.status.${status}`)}
    </span>
  );
}

// ─── Action button row ────────────────────────────────────────────────────────

function ActionButtons({
  pi,
  onDeposit,
  onCollect,
  onDiscount,
  onDishonor,
  onPay,
  onEdit,
  onDelete,
}: {
  pi: PaymentInstrument;
  onDeposit: () => void;
  onCollect: () => void;
  onDiscount: () => void;
  onDishonor: () => void;
  onPay: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const received = pi.direction === "received";
  const issued = pi.direction === "issued";

  return (
    <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
      {received && pi.status === "registered" && (
        <button className="btn-sm secondary" onClick={onDeposit}>
          {t("pi.row.deposit")}
        </button>
      )}
      {received && pi.status === "deposited" && (
        <>
          <button className="btn-sm secondary" onClick={onCollect}>
            {t("pi.row.collect")}
          </button>
          <button className="btn-sm secondary" onClick={onDishonor}>
            {t("pi.row.dishonor")}
          </button>
        </>
      )}
      {received && pi.kind === "BO" && (pi.status === "registered" || pi.status === "deposited") && (
        <button className="btn-sm secondary" onClick={onDiscount}>
          {t("pi.row.discount")}
        </button>
      )}
      {issued && pi.status === "deposited" && (
        <button className="btn-sm secondary" onClick={onPay}>
          {t("pi.row.pay")}
        </button>
      )}
      {/* Issued accept (registered→deposited) — reuse deposit action */}
      {issued && pi.status === "registered" && (
        <button className="btn-sm secondary" onClick={onDeposit}>
          {t("pi.row.deposit")}
        </button>
      )}
      {pi.status === "registered" && (
        <button className="btn-sm secondary" onClick={onEdit}>
          {t("pi.row.edit")}
        </button>
      )}
      <button className="btn-sm danger" onClick={onDelete}>
        {t("pi.row.delete")}
      </button>
    </div>
  );
}

// ─── Event date modal (deposit / collect / dishonor / pay) ────────────────────

type EventKind = "deposit" | "collect" | "dishonor" | "pay";

function EventModal({
  kind,
  piKind,
  onConfirm,
  onClose,
}: {
  kind: EventKind;
  piKind: PiKind;
  onConfirm: (date: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [date, setDate] = useState(localDateISO());
  const { closing, close: onAnimClose } = useAnimatedClose(onClose);

  const titleKey =
    kind === "deposit"  ? "pi.eventModal.depositTitle" :
    kind === "collect"  ? "pi.eventModal.collectTitle" :
    kind === "dishonor" ? "pi.eventModal.dishonorTitle" :
                          "pi.eventModal.payTitle";

  // For issued instruments "deposit" means "accept"
  const displayTitle =
    kind === "deposit" && piKind !== "CEC"
      ? t("pi.eventModal.depositTitle")
      : t(titleKey);

  return (
    <div className={`modal-backdrop${closing ? " closing" : ""}`} onClick={onAnimClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{displayTitle}</h2>
          <button className="modal-close" onClick={onAnimClose}><Ic name="x" /></button>
        </div>
        <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <label className="field-label">
            {t("pi.eventModal.date")}
            <input
              type="date"
              className="field-input"
              value={date}
              onChange={(e) => setDate(e.target.value)}
            />
          </label>
        </div>
        <div className="modal-footer">
          <button className="btn secondary" onClick={onAnimClose}>
            {t("pi.eventModal.cancel")}
          </button>
          <button
            className="btn primary"
            disabled={!date}
            onClick={() => { if (date) onConfirm(date); }}
          >
            {t("pi.eventModal.confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Discount modal ───────────────────────────────────────────────────────────

function DiscountModal({
  onConfirm,
  onClose,
}: {
  onConfirm: (date: string, discountAmount: string, commissionAmount: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [date, setDate] = useState(localDateISO());
  const [discountAmount, setDiscountAmount] = useState("");
  const [commissionAmount, setCommissionAmount] = useState("");
  const { closing, close: onAnimClose } = useAnimatedClose(onClose);

  const canSubmit = date && discountAmount && Number(discountAmount) > 0;

  return (
    <div className={`modal-backdrop${closing ? " closing" : ""}`} onClick={onAnimClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t("pi.discountModal.title")}</h2>
          <button className="modal-close" onClick={onAnimClose}><Ic name="x" /></button>
        </div>
        <p style={{ margin: "8px 24px 0", fontSize: 13, color: "var(--text-muted)" }}>
          {t("pi.discountModal.sub")}
        </p>
        <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <label className="field-label">
            {t("pi.discountModal.date")}
            <input
              type="date"
              className="field-input"
              value={date}
              onChange={(e) => setDate(e.target.value)}
            />
          </label>
          <label className="field-label">
            {t("pi.discountModal.discountAmount")}
            <input
              type="number"
              step="0.01"
              min="0.01"
              className="field-input"
              placeholder={t("pi.discountModal.discountPlaceholder")}
              value={discountAmount}
              onChange={(e) => setDiscountAmount(e.target.value)}
            />
          </label>
          <label className="field-label">
            {t("pi.discountModal.commission")}
            <input
              type="number"
              step="0.01"
              min="0"
              className="field-input"
              placeholder={t("pi.discountModal.commissionPlaceholder")}
              value={commissionAmount}
              onChange={(e) => setCommissionAmount(e.target.value)}
            />
          </label>
        </div>
        <div className="modal-footer">
          <button className="btn secondary" onClick={onAnimClose}>
            {t("pi.discountModal.cancel")}
          </button>
          <button
            className="btn primary"
            disabled={!canSubmit}
            onClick={() => {
              if (canSubmit) onConfirm(date, discountAmount, commissionAmount);
            }}
          >
            {t("pi.discountModal.confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Create / Edit modal ──────────────────────────────────────────────────────

function PiModal({
  pi,
  companyId,
  onClose,
}: {
  pi: PaymentInstrument | null;
  companyId: string;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { closing, close: onAnimClose } = useAnimatedClose(onClose);

  const [form, setForm] = useState({
    kind:       (pi?.kind       ?? "CEC") as PiKind,
    direction:  (pi?.direction  ?? "received") as PiDirection,
    partnerCui: pi?.partnerCui  ?? "",
    number:     pi?.number      ?? "",
    amount:     pi?.amount      ?? "",
    currency:   pi?.currency    ?? "RON",
    issueDate:  pi?.issueDate   ?? localDateISO(),
    scadenta:   pi?.scadenta    ?? "",
    notes:      pi?.notes       ?? "",
  });
  const [errors, setErrors] = useState<Record<string, string>>({});

  const set = (k: keyof typeof form) => (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>) =>
    setForm((f) => ({ ...f, [k]: e.target.value }));

  function validate() {
    const errs: Record<string, string> = {};
    if (!form.amount || Number(form.amount) <= 0) errs.amount = t("pi.validate.amount");
    if (!form.issueDate) errs.issueDate = t("pi.validate.issueDate");
    if (form.kind === "BO" && !form.scadenta) errs.scadenta = t("pi.validate.scadenta");
    if (form.kind === "CEC" && form.scadenta) errs.scadenta = t("pi.validate.scadentaCec");
    setErrors(errs);
    return Object.keys(errs).length === 0;
  }

  const invalidate = () => void queryClient.invalidateQueries({ queryKey: ["paymentInstruments", companyId] });

  const createMut = useMutation({
    mutationFn: (args: CreatePaymentInstrumentArgs) => api.paymentInstruments.create(args),
    onSuccess: () => { notify.success(t("pi.notify.created")); invalidate(); onAnimClose(); },
    onError: (e) => notify.error(formatError(e) || t("pi.notify.createError")),
  });

  const updateMut = useMutation({
    mutationFn: (args: UpdatePaymentInstrumentArgs) => api.paymentInstruments.update(args),
    onSuccess: () => { notify.success(t("pi.notify.updated")); invalidate(); onAnimClose(); },
    onError: (e) => notify.error(formatError(e) || t("pi.notify.updateError")),
  });

  function handleSubmit() {
    if (!validate()) return;
    const scadenta = form.kind === "CEC" ? null : (form.scadenta || null);
    if (pi) {
      updateMut.mutate({
        id: pi.id,
        companyId,
        partnerCui: form.partnerCui || null,
        number: form.number || null,
        amount: form.amount,
        currency: form.currency || "RON",
        issueDate: form.issueDate,
        scadenta,
        notes: form.notes || null,
      });
    } else {
      createMut.mutate({
        companyId,
        kind: form.kind,
        direction: form.direction,
        partnerCui: form.partnerCui || null,
        number: form.number || null,
        amount: form.amount,
        currency: form.currency || "RON",
        issueDate: form.issueDate,
        scadenta,
        notes: form.notes || null,
      });
    }
  }

  const isBusy = createMut.isPending || updateMut.isPending;

  return (
    <div className={`modal-backdrop${closing ? " closing" : ""}`} onClick={onAnimClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()} style={{ maxWidth: 560 }}>
        <div className="modal-header">
          <h2 className="modal-title">
            {pi ? t("pi.modal.editTitle") : t("pi.modal.createTitle")}
          </h2>
          <button className="modal-close" onClick={onAnimClose}><Ic name="x" /></button>
        </div>
        <p style={{ margin: "8px 24px 0", fontSize: 13, color: "var(--text-muted)" }}>
          {t("pi.modal.sub")}
        </p>
        <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 14 }}>

          {/* Kind + Direction — only editable on create */}
          {!pi && (
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
              <label className="field-label">
                {t("pi.modal.kind")}
                <select className="field-input" value={form.kind} onChange={(e) => setForm((f) => ({ ...f, kind: e.target.value as PiKind, scadenta: "" }))}>
                  <option value="CEC">{t("pi.kind.CEC")}</option>
                  <option value="BO">{t("pi.kind.BO")}</option>
                </select>
              </label>
              <label className="field-label">
                {t("pi.modal.direction")}
                <select className="field-input" value={form.direction} onChange={set("direction") as React.ChangeEventHandler<HTMLSelectElement>}>
                  <option value="received">{t("pi.modal.directionReceived")}</option>
                  <option value="issued">{t("pi.modal.directionIssued")}</option>
                </select>
              </label>
            </div>
          )}

          {/* Number + Partner CUI */}
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
            <label className="field-label">
              {t("pi.modal.number")}
              <input type="text" className="field-input" placeholder={t("pi.modal.numberPlaceholder")} value={form.number} onChange={set("number")} />
            </label>
            <label className="field-label">
              {t("pi.modal.partnerCui")}
              <input type="text" className="field-input" placeholder={t("pi.modal.partnerCuiPlaceholder")} value={form.partnerCui} onChange={set("partnerCui")} />
            </label>
          </div>

          {/* Amount + Currency */}
          <div style={{ display: "grid", gridTemplateColumns: "2fr 1fr", gap: 12 }}>
            <label className="field-label">
              {t("pi.modal.amount")}
              <input
                type="number"
                step="0.01"
                min="0.01"
                className={`field-input${errors.amount ? " field-error" : ""}`}
                placeholder={t("pi.modal.amountPlaceholder")}
                value={form.amount}
                onChange={set("amount")}
              />
              {errors.amount && <span className="field-error-msg">{errors.amount}</span>}
            </label>
            <label className="field-label">
              {t("pi.modal.currency")}
              <select className="field-input" value={form.currency} onChange={set("currency") as React.ChangeEventHandler<HTMLSelectElement>}>
                <option value="RON">RON</option>
                <option value="EUR">EUR</option>
                <option value="USD">USD</option>
              </select>
            </label>
          </div>

          {/* Issue date + Scadenta */}
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
            <label className="field-label">
              {t("pi.modal.issueDate")}
              <input
                type="date"
                className={`field-input${errors.issueDate ? " field-error" : ""}`}
                value={form.issueDate}
                onChange={set("issueDate")}
              />
              {errors.issueDate && <span className="field-error-msg">{errors.issueDate}</span>}
            </label>
            <label className="field-label">
              {t("pi.modal.scadenta")}
              {form.kind === "CEC" ? (
                <input type="date" className="field-input" disabled placeholder="—" value="" onChange={() => {}} />
              ) : (
                <input
                  type="date"
                  className={`field-input${errors.scadenta ? " field-error" : ""}`}
                  value={form.scadenta}
                  onChange={set("scadenta")}
                />
              )}
              {form.kind === "CEC" && (
                <span style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 2 }}>
                  {t("pi.modal.scadentaNote")}
                </span>
              )}
              {errors.scadenta && <span className="field-error-msg">{errors.scadenta}</span>}
            </label>
          </div>

          {/* Notes */}
          <label className="field-label">
            {t("pi.modal.notes")}
            <textarea
              className="field-input"
              rows={2}
              placeholder={t("pi.modal.notesPlaceholder")}
              value={form.notes}
              onChange={set("notes")}
            />
          </label>
        </div>
        <div className="modal-footer">
          <button className="btn secondary" onClick={onAnimClose}>
            {t("pi.modal.close")}
          </button>
          <button className="btn primary" disabled={isBusy} onClick={handleSubmit}>
            {isBusy
              ? t("pi.modal.saving")
              : pi
              ? t("pi.modal.saveChanges")
              : t("pi.modal.create")}
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Main page ────────────────────────────────────────────────────────────────

type PendingEvent =
  | { kind: "event"; type: EventKind; pi: PaymentInstrument }
  | { kind: "discount"; pi: PaymentInstrument };

export function PaymentInstrumentsPage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [tab, setTab] = useState<TabId>("all");
  const [search, setSearch] = useState("");
  const [editing, setEditing] = useState<PaymentInstrument | null | "new">(null);
  const [pending, setPending] = useState<PendingEvent | null>(null);
  const [deleteId, setDeleteId] = useState<string | null>(null);

  const { data: items = [], isLoading, error } = useQuery({
    queryKey: ["paymentInstruments", activeCompanyId],
    queryFn: () => api.paymentInstruments.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const invalidate = useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: ["paymentInstruments", activeCompanyId] });
  }, [queryClient, activeCompanyId]);

  // ── Lifecycle mutations ──────────────────────────────────────────────────────

  const depositMut = useMutation({
    mutationFn: ({ id, date }: { id: string; date: string }) =>
      api.paymentInstruments.deposit(id, activeCompanyId!, date),
    onSuccess: () => { notify.success(t("pi.notify.deposited")); invalidate(); setPending(null); },
    onError: (e) => notify.error(formatError(e) || t("pi.notify.depositError")),
  });

  const collectMut = useMutation({
    mutationFn: ({ id, date }: { id: string; date: string }) =>
      api.paymentInstruments.collect(id, activeCompanyId!, date),
    onSuccess: () => { notify.success(t("pi.notify.collected")); invalidate(); setPending(null); },
    onError: (e) => notify.error(formatError(e) || t("pi.notify.collectError")),
  });

  const discountMut = useMutation({
    mutationFn: (args: { id: string; date: string; discountAmount: string; commissionAmount?: string | null }) =>
      api.paymentInstruments.discount({ ...args, companyId: activeCompanyId! }),
    onSuccess: () => { notify.success(t("pi.notify.discounted")); invalidate(); setPending(null); },
    onError: (e) => notify.error(formatError(e) || t("pi.notify.discountError")),
  });

  const dishonorMut = useMutation({
    mutationFn: ({ id, date }: { id: string; date: string }) =>
      api.paymentInstruments.dishonor(id, activeCompanyId!, date),
    onSuccess: () => { notify.success(t("pi.notify.dishonored")); invalidate(); setPending(null); },
    onError: (e) => notify.error(formatError(e) || t("pi.notify.dishonorError")),
  });

  const payMut = useMutation({
    mutationFn: ({ id, date }: { id: string; date: string }) =>
      api.paymentInstruments.pay(id, activeCompanyId!, date),
    onSuccess: () => { notify.success(t("pi.notify.paid")); invalidate(); setPending(null); },
    onError: (e) => notify.error(formatError(e) || t("pi.notify.payError")),
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => api.paymentInstruments.delete(id, activeCompanyId!),
    onSuccess: () => { notify.success(t("pi.notify.deleted")); invalidate(); setDeleteId(null); },
    onError: (e) => notify.error(formatError(e) || t("pi.notify.deleteError")),
  });

  // ── Filtering ────────────────────────────────────────────────────────────────

  const filtered = useMemo(() => {
    let list = items;
    if (tab === "received") list = list.filter((x) => x.direction === "received");
    else if (tab === "issued") list = list.filter((x) => x.direction === "issued");
    else if (tab === "active") list = list.filter((x) => ACTIVE_STATUSES.includes(x.status));
    else if (tab === "settled") list = list.filter((x) => SETTLED_STATUSES.includes(x.status));
    if (search.trim()) {
      const q = search.toLowerCase();
      list = list.filter(
        (x) =>
          x.number?.toLowerCase().includes(q) ||
          x.partnerCui?.toLowerCase().includes(q) ||
          x.amount.includes(q) ||
          t(`pi.kind.${x.kind}`).toLowerCase().includes(q)
      );
    }
    return list;
  }, [items, tab, search, t]);

  // ── Event dispatch ────────────────────────────────────────────────────────────

  function handleEventConfirm(date: string) {
    if (!pending || pending.kind !== "event") return;
    const id = pending.pi.id;
    if (pending.type === "deposit")  depositMut.mutate({ id, date });
    if (pending.type === "collect")  collectMut.mutate({ id, date });
    if (pending.type === "dishonor") dishonorMut.mutate({ id, date });
    if (pending.type === "pay")      payMut.mutate({ id, date });
  }

  if (!activeCompanyId) {
    return <div className="state-row muted">{t("pi.selectCompany")}</div>;
  }

  const TABS: { id: TabId; label: string }[] = [
    { id: "all",      label: t("pi.tabs.all") },
    { id: "received", label: t("pi.tabs.received") },
    { id: "issued",   label: t("pi.tabs.issued") },
    { id: "active",   label: t("pi.tabs.active") },
    { id: "settled",  label: t("pi.tabs.settled") },
  ];

  return (
    <div className="main-inner">
      {/* Header */}
      <div className="page-head">
        <div>
          <h1 className="page-title">{t("pi.title")}</h1>
          <div className="page-sub">
            {t("pi.sub.items", { count: items.length, context: items.length === 1 ? "one" : items.length < 5 ? "few" : "other" })}
          </div>
        </div>
        <button className="btn-dark" onClick={() => setEditing("new")}>
          <Ic name="plus" />
          {t("pi.head.new")}
        </button>
      </div>

      {/* Info banner */}
      <div className="banner">
        <Ic name="info" />
        <span>{t("pi.banner.info")}</span>
      </div>

      {/* Card: tabs + search + table */}
      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tabs">
            {TABS.map((tb) => (
              <button
                key={tb.id}
                className={"tab" + (tab === tb.id ? " active" : "")}
                onClick={() => setTab(tb.id)}
              >
                {tb.label}
              </button>
            ))}
          </div>
          <div className="spacer" />
          <div className="scr-search">
            <Ic name="search" />
            <input
              placeholder={t("pi.search")}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        </div>

        {/* Error */}
        {error && <QueryErrorBanner label={t("pi.states.errorLabel")} error={error} />}

        {/* Loading */}
        {isLoading && <div className="state-row">{t("pi.states.loading")}</div>}

        {/* Table */}
        {!isLoading && !error && (
          <>
            {filtered.length === 0 ? (
              <div className="state-row muted">
                {items.length === 0 ? t("pi.states.emptyNone") : t("pi.states.emptyFiltered")}
              </div>
            ) : (
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>{t("pi.table.kind")}</th>
                    <th>{t("pi.table.direction")}</th>
                    <th>{t("pi.table.number")}</th>
                    <th>{t("pi.table.partner")}</th>
                    <th style={{ textAlign: "right" }}>{t("pi.table.amount")}</th>
                    <th>{t("pi.table.issueDate")}</th>
                    <th>{t("pi.table.scadenta")}</th>
                    <th>{t("pi.table.status")}</th>
                    <th>{/* actions */}</th>
                  </tr>
                </thead>
                <tbody>
                  {filtered.map((pi) => (
                    <tr key={pi.id}>
                      <td>
                        <strong>{t(`pi.kind.${pi.kind}`)}</strong>
                      </td>
                      <td>{t(`pi.direction.${pi.direction}`)}</td>
                      <td>{pi.number ?? "—"}</td>
                      <td>{pi.partnerCui ?? "—"}</td>
                      <td style={{ textAlign: "right", fontVariantNumeric: "tabular-nums" }}>
                        {pi.amount} {pi.currency}
                      </td>
                      <td>{fmtRoDate(pi.issueDate)}</td>
                      <td>{fmtRoDate(pi.scadenta)}</td>
                      <td><StatusChip status={pi.status} /></td>
                      <td>
                        {deleteId === pi.id ? (
                          <div style={{ display: "flex", gap: 6 }}>
                            <button
                              className="btn-sm danger"
                              onClick={() => deleteMut.mutate(pi.id)}
                              disabled={deleteMut.isPending}
                            >
                              {t("pi.row.confirmDelete")}
                            </button>
                            <button className="btn-sm secondary" onClick={() => setDeleteId(null)}>
                              ✕
                            </button>
                          </div>
                        ) : (
                          <ActionButtons
                            pi={pi}
                            onDeposit={() => setPending({ kind: "event", type: "deposit", pi })}
                            onCollect={() => setPending({ kind: "event", type: "collect", pi })}
                            onDiscount={() => setPending({ kind: "discount", pi })}
                            onDishonor={() => setPending({ kind: "event", type: "dishonor", pi })}
                            onPay={() => setPending({ kind: "event", type: "pay", pi })}
                            onEdit={() => setEditing(pi)}
                            onDelete={() => setDeleteId(pi.id)}
                          />
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </>
        )}
      </div>

      {/* Create/Edit modal */}
      {editing !== null && (
        <PiModal
          pi={editing === "new" ? null : editing}
          companyId={activeCompanyId}
          onClose={() => setEditing(null)}
        />
      )}

      {/* Event modal (deposit/collect/dishonor/pay) */}
      {pending?.kind === "event" && (
        <EventModal
          kind={pending.type}
          piKind={pending.pi.kind}
          onConfirm={handleEventConfirm}
          onClose={() => setPending(null)}
        />
      )}

      {/* Discount modal */}
      {pending?.kind === "discount" && (
        <DiscountModal
          onConfirm={(date, discountAmount, commissionAmount) =>
            discountMut.mutate({
              id: pending.pi.id,
              date,
              discountAmount,
              commissionAmount: commissionAmount || null,
            })
          }
          onClose={() => setPending(null)}
        />
      )}
    </div>
  );
}
