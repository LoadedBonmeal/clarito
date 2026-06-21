/**
 * Contracts page — list + create/edit modal.
 *
 * A contract is a commercial/legal driver record (NOT a document justificativ,
 * OMFP 3512/2008). Signing/terminating a contract creates NO GL entries.
 * The value field is informational only (no class 8036 commitment tracking).
 *
 * Features:
 *  - CRUD with status lifecycle (draft → active → expired/terminated)
 *  - Partner, titlu/obiect, valoare, start/end date, auto-renew, payment terms
 *  - Expiry indicator (days remaining) + color-coded urgency
 *  - Linked recurring invoices panel (read-only within modal)
 *  - Tabbed filter: All / Active / Draft / Expired / Terminated
 */

import { useCallback, useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { Contract, ContractStatus, CreateContractArgs, UpdateContractArgs } from "@/lib/tauri";
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

/** Days from today until the end_date (positive = future, negative = past). */
function daysUntil(endDate: string | null | undefined): number | null {
  if (!endDate) return null;
  const end = new Date(endDate);
  const now = new Date();
  now.setHours(0, 0, 0, 0);
  end.setHours(0, 0, 0, 0);
  return Math.round((end.getTime() - now.getTime()) / 86_400_000);
}

// ─── Status chip ─────────────────────────────────────────────────────────────

const STATUS_CLS: Record<ContractStatus, string> = {
  draft: "sent",
  active: "paid",
  expired: "over",
  terminated: "over",
};

function StatusChip({ status }: { status: ContractStatus }) {
  const { t } = useTranslation();
  return (
    <span className={`status-chip ${STATUS_CLS[status] ?? "sent"}`}>
      {t(`contracts.status.${status}`)}
    </span>
  );
}

// ─── Expiry indicator ─────────────────────────────────────────────────────────

function ExpiryBadge({
  endDate,
  renewalNoticeDays,
  status,
}: {
  endDate: string | null | undefined;
  renewalNoticeDays: number;
  status: ContractStatus;
}) {
  const { t } = useTranslation();
  if (!endDate || status === "terminated") return <span className="text-muted">—</span>;

  const days = daysUntil(endDate);
  if (days === null) return <span className="text-muted">—</span>;

  if (days < 0) {
    return <span style={{ color: "var(--color-error, #e53)" }}>{t("contracts.expiry.expired")}</span>;
  }
  if (days <= renewalNoticeDays) {
    const urgentColor = days <= 7 ? "var(--color-error, #e53)" : "var(--color-warning, #f90)";
    return (
      <span style={{ color: urgentColor, fontWeight: 600 }}>
        {t("contracts.expiry.days", { count: days })}
      </span>
    );
  }
  return (
    <span className="text-muted">
      {t("contracts.expiry.days", { count: days })}
    </span>
  );
}

// ─── Tab filter ───────────────────────────────────────────────────────────────

type TabFilter = "all" | "active" | "draft" | "expired" | "terminated";

// ─── Modal form state ─────────────────────────────────────────────────────────

const EMPTY_FORM = {
  number: "",
  title: "",
  object: "",
  contactId: "",
  value: "",
  currency: "RON",
  startDate: localDateISO(),
  endDate: "",
  status: "active" as ContractStatus,
  paymentTermsDays: "",
  autoRenew: false,
  renewalNoticeDays: "30",
  notes: "",
};

type FormState = typeof EMPTY_FORM;

function contractToForm(c: Contract): FormState {
  return {
    number: c.number ?? "",
    title: c.title,
    object: c.object ?? "",
    contactId: c.contactId ?? "",
    value: c.value ?? "",
    currency: c.currency,
    startDate: c.startDate,
    endDate: c.endDate ?? "",
    status: c.status,
    paymentTermsDays: c.paymentTermsDays != null ? String(c.paymentTermsDays) : "",
    autoRenew: c.autoRenew,
    renewalNoticeDays: String(c.renewalNoticeDays),
    notes: c.notes ?? "",
  };
}

// ─── SVG icons ───────────────────────────────────────────────────────────────

const SVG_TRASH =
  '<path d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>';
const SVG_INFO =
  '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';
const SVG_X = '<path d="M6 18 18 6M6 6l12 12"/>';

// ─── Main page ────────────────────────────────────────────────────────────────

export function ContractsPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const [tab, setTab] = useState<TabFilter>("all");
  const [search, setSearch] = useState("");
  const [editingContract, setEditingContract] = useState<Contract | null>(null);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);
  const [showRecurring, setShowRecurring] = useState<string | null>(null); // contractId

  // ── Queries ────────────────────────────────────────────────────────────────

  const {
    data: contractsData,
    isLoading,
    error,
  } = useQuery({
    queryKey: queryKeys.contracts.list(activeCompanyId ?? ""),
    queryFn: () => api.contracts.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const { data: contactsData } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const { data: linkedRecurring } = useQuery({
    queryKey: queryKeys.contracts.recurring(showRecurring ?? ""),
    queryFn: () => api.contracts.listRecurring(showRecurring!, activeCompanyId!),
    enabled: !!showRecurring && !!activeCompanyId,
  });

  // ── Mutations ──────────────────────────────────────────────────────────────

  const invalidate = useCallback(() => {
    qc.invalidateQueries({ queryKey: queryKeys.contracts.all });
  }, [qc]);

  const createMut = useMutation({
    mutationFn: (args: CreateContractArgs) => api.contracts.create(args),
    onSuccess: () => {
      invalidate();
      notify.success(t("contracts.notify.created"));
      closeModal();
    },
    onError: (e) => notify.error(formatError(e) || t("contracts.notify.createError")),
  });

  const updateMut = useMutation({
    mutationFn: (args: UpdateContractArgs) => api.contracts.update(args),
    onSuccess: () => {
      invalidate();
      notify.success(t("contracts.notify.updated"));
      closeModal();
    },
    onError: (e) => notify.error(formatError(e) || t("contracts.notify.updateError")),
  });

  const deleteMut = useMutation({
    mutationFn: ({ id, companyId }: { id: string; companyId: string }) =>
      api.contracts.delete(id, companyId),
    onSuccess: () => {
      invalidate();
      notify.success(t("contracts.notify.deleted"));
      setDeleteConfirmId(null);
    },
    onError: (e) => notify.error(formatError(e) || t("contracts.notify.deleteError")),
  });

  const setStatusMut = useMutation({
    mutationFn: ({
      id,
      companyId,
      status,
    }: {
      id: string;
      companyId: string;
      status: ContractStatus;
    }) => api.contracts.setStatus(id, companyId, status),
    onSuccess: () => {
      invalidate();
      notify.success(t("contracts.notify.statusChanged"));
    },
    onError: (e) => notify.error(formatError(e) || t("contracts.notify.statusError")),
  });

  // ── Modal ──────────────────────────────────────────────────────────────────

  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [formErrors, setFormErrors] = useState<Partial<Record<keyof FormState, string>>>({});
  const { closing, close: animClose } = useAnimatedClose(() => setIsModalOpen(false));

  const openCreate = useCallback(() => {
    setEditingContract(null);
    setForm(EMPTY_FORM);
    setFormErrors({});
    setIsModalOpen(true);
  }, []);

  const openEdit = useCallback((c: Contract) => {
    setEditingContract(c);
    setForm(contractToForm(c));
    setFormErrors({});
    setIsModalOpen(true);
  }, []);

  const closeModal = useCallback(() => {
    animClose();
    setShowRecurring(null);
  }, [animClose]);

  const setField = useCallback(
    <K extends keyof FormState>(key: K, val: FormState[K]) => {
      setForm((prev) => ({ ...prev, [key]: val }));
      setFormErrors((prev) => ({ ...prev, [key]: undefined }));
    },
    []
  );

  const validate = useCallback((): boolean => {
    const errs: Partial<Record<keyof FormState, string>> = {};
    if (!form.title.trim()) errs.title = t("contracts.validate.title");
    if (!form.startDate.trim()) errs.startDate = t("contracts.validate.startDate");
    setFormErrors(errs);
    return Object.keys(errs).length === 0;
  }, [form, t]);

  const handleSubmit = useCallback(() => {
    if (!validate() || !activeCompanyId) return;

    const base = {
      companyId: activeCompanyId,
      contactId: form.contactId || null,
      number: form.number || null,
      title: form.title,
      object: form.object || null,
      value: form.value || null,
      currency: form.currency || "RON",
      startDate: form.startDate,
      endDate: form.endDate || null,
      paymentTermsDays: form.paymentTermsDays ? Number(form.paymentTermsDays) : null,
      autoRenew: form.autoRenew,
      renewalNoticeDays: form.renewalNoticeDays ? Number(form.renewalNoticeDays) : 30,
      notes: form.notes || null,
    };

    if (editingContract) {
      updateMut.mutate({ ...base, id: editingContract.id });
    } else {
      createMut.mutate({ ...base, status: form.status });
    }
  }, [validate, form, activeCompanyId, editingContract, createMut, updateMut]);

  // ── Filtered list ──────────────────────────────────────────────────────────

  const filtered = useMemo(() => {
    const all = contractsData ?? [];
    const q = search.trim().toLowerCase();
    return all.filter((c) => {
      if (tab !== "all" && c.status !== tab) return false;
      if (q) {
        const haystack = [c.title, c.number, c.object, c.notes]
          .filter(Boolean)
          .join(" ")
          .toLowerCase();
        if (!haystack.includes(q)) return false;
      }
      return true;
    });
  }, [contractsData, tab, search]);

  // ── Stats ──────────────────────────────────────────────────────────────────

  const stats = useMemo(() => {
    const all = contractsData ?? [];
    const active = all.filter((c) => c.status === "active").length;
    return { total: all.length, active };
  }, [contractsData]);

  const contactMap = useMemo(() => {
    const m: Record<string, string> = {};
    for (const c of contactsData ?? []) m[c.id] = c.legalName;
    return m;
  }, [contactsData]);

  // ── Render helpers ────────────────────────────────────────────────────────

  const isSaving = createMut.isPending || updateMut.isPending;

  if (!activeCompanyId) {
    return (
      <div className="page-empty-state">
        <p>{t("contracts.selectCompany")}</p>
      </div>
    );
  }

  return (
    <div className="page-root">
      {/* Page header */}
      <div className="page-head">
        <div>
          <h1 className="page-title">{t("contracts.title")}</h1>
          <p className="page-sub">
            {t("contracts.sub.contracts", { count: stats.total })}
            {stats.active > 0 && (
              <> · {t("contracts.sub.active", { n: stats.active })}</>
            )}
          </p>
        </div>
        <button className="btn-dark" onClick={openCreate}>
          <Ic name="plus" />
          {t("contracts.head.new")}
        </button>
      </div>

      {/* Card */}
      <div className="scr-card">
        {/* Toolbar */}
        <div className="scr-toolbar">
          <div className="tabs">
            {(["all", "active", "draft", "expired", "terminated"] as TabFilter[]).map((tb) => (
              <button
                key={tb}
                className={`tab-btn${tab === tb ? " active" : ""}`}
                onClick={() => setTab(tb)}
              >
                {t(`contracts.tabs.${tb}`)}
              </button>
            ))}
          </div>
          <span className="spacer" />
          <div className="scr-search">
            <Ic name="search" />
            <input
              type="search"
              placeholder={t("contracts.search")}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        </div>

        {/* Error */}
        {error && <QueryErrorBanner error={error} label={t("contracts.states.errorLabel")} />}

        {/* Loading */}
        {isLoading && <p className="table-loading">{t("contracts.states.loading")}</p>}

        {/* Table */}
        {!isLoading && !error && (
          <>
            {filtered.length === 0 ? (
              <p className="table-empty">
                {(contractsData?.length ?? 0) === 0
                  ? t("contracts.states.emptyNone")
                  : t("contracts.states.emptyFiltered")}
              </p>
            ) : (
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>{t("contracts.table.number")}</th>
                    <th>{t("contracts.table.title")}</th>
                    <th>{t("contracts.table.partner")}</th>
                    <th>{t("contracts.table.period")}</th>
                    <th className="col-right">{t("contracts.table.value")}</th>
                    <th>{t("contracts.table.status")}</th>
                    <th>{t("contracts.table.expiry")}</th>
                    <th className="col-acts" />
                  </tr>
                </thead>
                <tbody>
                  {filtered.map((c) => (
                    <ContractRow
                      key={c.id}
                      contract={c}
                      contactName={contactMap[c.contactId ?? ""] ?? "—"}
                      deleteConfirmId={deleteConfirmId}
                      onEdit={openEdit}
                      onDelete={(id) => {
                        if (deleteConfirmId === id) {
                          deleteMut.mutate({ id, companyId: activeCompanyId });
                        } else {
                          setDeleteConfirmId(id);
                        }
                      }}
                      onCancelDelete={() => setDeleteConfirmId(null)}
                      onSetStatus={(id, status) =>
                        setStatusMut.mutate({ id, companyId: activeCompanyId, status })
                      }
                    />
                  ))}
                </tbody>
              </table>
            )}
          </>
        )}
      </div>

      {/* Info banner */}
      <div className="info-banner">
        <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_INFO }} />
        <span>{t("contracts.banner.info")}</span>
      </div>

      {/* Modal */}
      {isModalOpen && (
        <div className={`modal-back ${closing ? "closing" : "show"}`} onClick={closeModal}>
          <div
            className="modal-panel modal-lg"
            onClick={(e) => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-labelledby="contract-modal-title"
          >
            {/* Modal header */}
            <div className="modal-head">
              <div>
                <h2 id="contract-modal-title">
                  {editingContract
                    ? t("contracts.modal.editTitle")
                    : t("contracts.modal.createTitle")}
                </h2>
                <p className="modal-sub">{t("contracts.modal.sub")}</p>
              </div>
              <button className="modal-close" onClick={closeModal} aria-label={t("contracts.modal.close")}>
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_X }} />
              </button>
            </div>

            {/* Modal body */}
            <div className="modal-body">
              {/* Row 1: Number + Title */}
              <div className="form-row form-row-2">
                <div className="form-group">
                  <label className="form-label">{t("contracts.modal.number")}</label>
                  <input
                    className="form-input"
                    type="text"
                    value={form.number}
                    onChange={(e) => setField("number", e.target.value)}
                    placeholder={t("contracts.modal.numberPlaceholder")}
                  />
                </div>
                <div className="form-group">
                  <label className="form-label required">{t("contracts.modal.title")}</label>
                  <input
                    className={`form-input${formErrors.title ? " input-error" : ""}`}
                    type="text"
                    value={form.title}
                    onChange={(e) => setField("title", e.target.value)}
                    placeholder={t("contracts.modal.titlePlaceholder")}
                  />
                  {formErrors.title && <p className="form-error">{formErrors.title}</p>}
                </div>
              </div>

              {/* Row 2: Object */}
              <div className="form-group">
                <label className="form-label">{t("contracts.modal.object")}</label>
                <input
                  className="form-input"
                  type="text"
                  value={form.object}
                  onChange={(e) => setField("object", e.target.value)}
                  placeholder={t("contracts.modal.objectPlaceholder")}
                />
              </div>

              {/* Row 3: Partner */}
              <div className="form-group">
                <label className="form-label">{t("contracts.modal.partner")}</label>
                <select
                  className="form-select"
                  value={form.contactId}
                  onChange={(e) => setField("contactId", e.target.value)}
                >
                  <option value="">{t("contracts.modal.partnerPick")}</option>
                  {(contactsData ?? []).map((ct) => (
                    <option key={ct.id} value={ct.id}>
                      {ct.legalName}
                    </option>
                  ))}
                </select>
              </div>

              {/* Row 4: Value + Currency */}
              <div className="form-row form-row-2">
                <div className="form-group">
                  <label className="form-label">{t("contracts.modal.value")}</label>
                  <input
                    className="form-input"
                    type="text"
                    inputMode="decimal"
                    value={form.value}
                    onChange={(e) => setField("value", e.target.value)}
                    placeholder={t("contracts.modal.valuePlaceholder")}
                  />
                </div>
                <div className="form-group">
                  <label className="form-label">{t("contracts.modal.currency")}</label>
                  <select
                    className="form-select"
                    value={form.currency}
                    onChange={(e) => setField("currency", e.target.value)}
                  >
                    {["RON","EUR","USD","GBP","CHF"].map((c) => (
                      <option key={c} value={c}>{c}</option>
                    ))}
                  </select>
                </div>
              </div>

              {/* Row 5: Start date + End date */}
              <div className="form-row form-row-2">
                <div className="form-group">
                  <label className="form-label required">{t("contracts.modal.startDate")}</label>
                  <input
                    className={`form-input${formErrors.startDate ? " input-error" : ""}`}
                    type="date"
                    value={form.startDate}
                    onChange={(e) => setField("startDate", e.target.value)}
                  />
                  {formErrors.startDate && (
                    <p className="form-error">{formErrors.startDate}</p>
                  )}
                </div>
                <div className="form-group">
                  <label className="form-label">{t("contracts.modal.endDate")}</label>
                  <input
                    className="form-input"
                    type="date"
                    value={form.endDate}
                    onChange={(e) => setField("endDate", e.target.value)}
                  />
                </div>
              </div>

              {/* Row 6: Status (create only) + Payment terms */}
              <div className="form-row form-row-2">
                {!editingContract && (
                  <div className="form-group">
                    <label className="form-label">{t("contracts.modal.status")}</label>
                    <select
                      className="form-select"
                      value={form.status}
                      onChange={(e) => setField("status", e.target.value as ContractStatus)}
                    >
                      {(["draft", "active"] as ContractStatus[]).map((s) => (
                        <option key={s} value={s}>
                          {t(`contracts.status.${s}`)}
                        </option>
                      ))}
                    </select>
                  </div>
                )}
                <div className="form-group">
                  <label className="form-label">{t("contracts.modal.paymentTerms")}</label>
                  <input
                    className="form-input"
                    type="number"
                    min={0}
                    value={form.paymentTermsDays}
                    onChange={(e) => setField("paymentTermsDays", e.target.value)}
                    placeholder={t("contracts.modal.paymentTermsPlaceholder")}
                  />
                </div>
              </div>

              {/* Row 7: Auto-renew + Notice days */}
              <div className="form-row form-row-2">
                <div className="form-group form-group-check">
                  <label className="toggle-label">
                    <span className="toggle-track">
                      <input
                        type="checkbox"
                        checked={form.autoRenew}
                        onChange={(e) => setField("autoRenew", e.target.checked)}
                      />
                      <span className="toggle-thumb" />
                    </span>
                    {t("contracts.modal.autoRenew")}
                  </label>
                </div>
                <div className="form-group">
                  <label className="form-label">{t("contracts.modal.renewalNoticeDays")}</label>
                  <input
                    className="form-input"
                    type="number"
                    min={1}
                    value={form.renewalNoticeDays}
                    onChange={(e) => setField("renewalNoticeDays", e.target.value)}
                    placeholder={t("contracts.modal.renewalNoticePlaceholder")}
                  />
                </div>
              </div>

              {/* Notes */}
              <div className="form-group">
                <label className="form-label">{t("contracts.modal.notes")}</label>
                <textarea
                  className="form-input form-textarea"
                  value={form.notes}
                  onChange={(e) => setField("notes", e.target.value)}
                  placeholder={t("contracts.modal.notesPlaceholder")}
                  rows={3}
                />
              </div>

              {/* Linked recurring invoices (edit mode only) */}
              {editingContract && (
                <div className="form-group">
                  <div className="section-label-row">
                    <label className="form-label">{t("contracts.modal.recurringSection")}</label>
                    {showRecurring !== editingContract.id && (
                      <button
                        className="btn-link"
                        onClick={() => setShowRecurring(editingContract.id)}
                      >
                        {t("contracts.row.viewRecurring")}
                      </button>
                    )}
                  </div>
                  {showRecurring === editingContract.id && (
                    <div className="recurring-linked-list">
                      {!linkedRecurring ? (
                        <p className="text-muted">{t("contracts.states.loading")}</p>
                      ) : linkedRecurring.length === 0 ? (
                        <p className="text-muted">{t("contracts.modal.recurringEmpty")}</p>
                      ) : (
                        <table className="scr-table scr-table-sm">
                          <thead>
                            <tr>
                              <th>{t("recurring.table.name")}</th>
                              <th>{t("recurring.table.frequency")}</th>
                              <th>{t("recurring.table.nextIssue")}</th>
                              <th>{t("recurring.table.active")}</th>
                            </tr>
                          </thead>
                          <tbody>
                            {linkedRecurring.map((r) => (
                              <tr key={r.id}>
                                <td>{r.templateName}</td>
                                <td>{r.frequency}</td>
                                <td>{fmtRoDate(r.nextIssueDate)}</td>
                                <td>
                                  <span className={`status-chip ${r.active ? "paid" : "over"}`}>
                                    {r.active ? t("recurring.table.active") : t("recurring.table.inactive")}
                                  </span>
                                </td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      )}
                    </div>
                  )}
                </div>
              )}
            </div>

            {/* Modal footer */}
            <div className="modal-foot">
              <button className="btn-ghost" onClick={closeModal} disabled={isSaving}>
                {t("contracts.cancel")}
              </button>
              <button className="btn-dark" onClick={handleSubmit} disabled={isSaving}>
                {isSaving
                  ? t("contracts.modal.saving")
                  : editingContract
                  ? t("contracts.modal.saveChanges")
                  : t("contracts.modal.create")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ─── Row component ────────────────────────────────────────────────────────────

function ContractRow({
  contract: c,
  contactName,
  deleteConfirmId,
  onEdit,
  onDelete,
  onCancelDelete,
  onSetStatus,
}: {
  contract: Contract;
  contactName: string;
  deleteConfirmId: string | null;
  onEdit: (c: Contract) => void;
  onDelete: (id: string) => void;
  onCancelDelete: () => void;
  onSetStatus: (id: string, status: ContractStatus) => void;
}) {
  const { t } = useTranslation();
  const isConfirming = deleteConfirmId === c.id;

  const periodLabel =
    c.startDate && c.endDate
      ? `${c.startDate.slice(0, 7)} — ${c.endDate.slice(0, 7)}`
      : c.startDate
      ? `${c.startDate.slice(0, 7)} —`
      : "—";

  const valueLabel =
    c.value
      ? `${Number(c.value).toLocaleString("ro-RO", { minimumFractionDigits: 2, maximumFractionDigits: 2 })} ${c.currency}`
      : "—";

  return (
    <tr>
      <td className="col-mono">{c.number ?? "—"}</td>
      <td>
        <span className="doc-name">{c.title}</span>
        {c.object && <span className="doc-sub">{c.object}</span>}
      </td>
      <td>{contactName}</td>
      <td className="text-muted">{periodLabel}</td>
      <td className="col-right text-mono">{valueLabel}</td>
      <td>
        <StatusChip status={c.status} />
      </td>
      <td>
        <ExpiryBadge
          endDate={c.endDate}
          renewalNoticeDays={c.renewalNoticeDays}
          status={c.status}
        />
      </td>
      <td className="col-acts">
        <div className="row-acts">
          {/* Quick status actions */}
          {c.status === "active" && (
            <button
              className="act-btn act-warn"
              title={t("contracts.row.setTerminated")}
              onClick={() => onSetStatus(c.id, "terminated")}
            >
              <Ic name="xMark" />
            </button>
          )}
          {c.status === "draft" && (
            <button
              className="act-btn act-ok"
              title={t("contracts.row.setActive")}
              onClick={() => onSetStatus(c.id, "active")}
            >
              <Ic name="check" />
            </button>
          )}

          {/* Edit */}
          <button className="act-btn" title={t("contracts.row.edit")} onClick={() => onEdit(c)}>
            <Ic name="pen" />
          </button>

          {/* Delete */}
          {isConfirming ? (
            <>
              <button
                className="act-btn act-danger"
                title={t("contracts.row.confirmDelete")}
                onClick={() => onDelete(c.id)}
              >
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_TRASH }} />
              </button>
              <button className="act-btn" title={t("contracts.cancel")} onClick={onCancelDelete}>
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_X }} />
              </button>
            </>
          ) : (
            <button
              className="act-btn"
              title={t("contracts.row.delete")}
              onClick={() => onDelete(c.id)}
            >
              <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_TRASH }} />
            </button>
          )}
        </div>
      </td>
    </tr>
  );
}
