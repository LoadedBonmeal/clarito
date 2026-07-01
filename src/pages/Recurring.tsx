/**
 * Facturi recurente — verbatim port of the design "Facturi recurente.html":
 *   .page-head (title + "N șabloane · M active · următoarea emitere …" sub +
 *   btn-dark "Șablon nou") → .scr-card → .scr-toolbar (.tabs Toate/Active/
 *   Inactive · .spacer · .scr-search) → .scr-table (cli-ava+denumire · client ·
 *   frecvență · urm. emitere · serie .doc · Auto ANAF .tog · Activ .tog ·
 *   .row-acts pen/trash) → info .banner below the card.
 *
 * ALL wiring preserved: api.recurring.list(activeCompanyId) + api.contacts.list,
 * "Șablon nou"/edit modal (templateName/clientId/frequency/dayOfMonth/
 * nextIssueDate/series/autoSubmitAnaf/notes + LineItemsEditor)
 * → api.recurring.create / api.recurring.update,
 * delete (confirm in-row) → api.recurring.delete(id, companyId),
 * Activ toggle → api.recurring.toggleActive(id, companyId, active),
 * Auto ANAF toggle in table → api.recurring.update (flips autoSubmitAnaf).
 */

import { useCallback, useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { LineItemsEditor } from "@/components/shared/LineItemsEditor";
import type { LineRow } from "@/components/shared/LineItemsEditor";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { CreateRecurringArgs, RecurringInvoice } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";

type TabFilter = "all" | "active" | "inactive";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

// Frequency → table label (prototype shows "Lunar · ziua 1" for monthly).
function freqLabel(frequency: string, dayOfMonth: number, t: TFunction): string {
  if (frequency === "monthly") return t("recurring.freq.monthlyDay", { n: dayOfMonth });
  if (frequency === "quarterly") return t("recurring.freq.quarterly");
  if (frequency === "annual") return t("recurring.freq.annual");
  return frequency;
}

// Template name → two-letter avatar initials (prototype: "AB" for "Abonament Cloud").
const avaInitials = (name: string) => name.trim().slice(0, 2).toUpperCase() || "··";

// Icons not in Ic.tsx — inlined verbatim from the prototype.
const SVG_TRASH =
  '<path d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>';
const SVG_INFO =
  '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';
const SVG_X = '<path d="M6 18 18 6M6 6l12 12"/>';

function localDateISO(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function nextDatePreview(freq: string, day: number): string {
  const today = new Date();
  // Mirror the backend scheduler (db/recurring.rs::advance_date): the day-of-month is
  // clamped to 28 so it is valid in every month (incl. February) and never overflows
  // into the next month — keeps the preview truthful about the scheduled date.
  const d = Math.min(Math.max(day, 1), 28);
  const mk = (y: number, m: number) => new Date(y, m, d);
  let next = mk(today.getFullYear(), today.getMonth());
  if (next <= today) {
    if (freq === "monthly") next = mk(today.getFullYear(), today.getMonth() + 1);
    else if (freq === "quarterly") next = mk(today.getFullYear(), today.getMonth() + 3);
    else next = mk(today.getFullYear() + 1, today.getMonth());
  }
  return localDateISO(next);
}

const DEFAULT_LINE: LineRow = {
  rowId: crypto.randomUUID(),
  name: "Servicii",
  quantity: 1,
  unit: "buc",
  unitPrice: 0,
  vatRate: 21,
  vatCategory: "S",
};

function makeEmptyLines(): LineRow[] {
  return [{ ...DEFAULT_LINE, rowId: crypto.randomUUID() }];
}

const EMPTY_FORM = {
  templateName: "",
  clientId: "",
  frequency: "monthly",
  dayOfMonth: 1,
  nextIssueDate: localDateISO(new Date()),
  series: "FCT",
  autoSubmitAnaf: false,
  notes: "",
};

export function RecurringPage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [tab, setTab] = useState<TabFilter>("all");
  const [query, setQuery] = useState("");
  const [showModal, setShowModal] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form, setForm] = useState({ ...EMPTY_FORM });
  const [lines, setLines] = useState<LineRow[]>(makeEmptyLines);
  const [linesError, setLinesError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);

  // Fetch recurring invoices
  const {
    data: recurringList = [],
    isLoading,
    isError: recurringError,
    error: recurringErr,
    refetch: refetchRecurring,
  } = useQuery({
    queryKey: queryKeys.recurring.list(activeCompanyId!),
    queryFn: () => api.recurring.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  // Fetch contacts for client picker
  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const contactMap = useMemo(
    () => new Map(contacts.map((c) => [c.id, c.legalName])),
    [contacts],
  );

  const createMutation = useMutation({
    mutationFn: (args: CreateRecurringArgs) => api.recurring.create(args),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.recurring.list(activeCompanyId!) });
      notify.success(t("recurring.notify.created"));
      setShowModal(false);
      setForm({ ...EMPTY_FORM });
      setLines(makeEmptyLines());
    },
    onError: (e) => notify.error(formatError(e, t("recurring.notify.createError"))),
  });

  const updateMutation = useMutation({
    mutationFn: (args: Parameters<typeof api.recurring.update>[0]) =>
      api.recurring.update(args),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.recurring.list(activeCompanyId!) });
      notify.success(t("recurring.notify.updated"));
      setShowModal(false);
      setEditingId(null);
      setForm({ ...EMPTY_FORM });
      setLines(makeEmptyLines());
    },
    onError: (e) => notify.error(formatError(e, t("recurring.notify.updateError"))),
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.recurring.delete(id, activeCompanyId!),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.recurring.list(activeCompanyId!) });
      notify.success(t("recurring.notify.deleted"));
      setDeleteConfirm(null);
    },
    onError: (e) => notify.error(formatError(e, t("recurring.notify.deleteError"))),
  });

  const toggleActive = useMutation({
    mutationFn: ({ id, active }: { id: string; active: boolean }) =>
      api.recurring.toggleActive(id, activeCompanyId!, active),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.recurring.list(activeCompanyId!) });
      notify.success(t("recurring.notify.toggled"));
    },
    onError: (e) => notify.error(formatError(e, t("recurring.notify.toggleError"))),
  });

  // Design table exposes Auto ANAF as a toggle → flip via api.recurring.update.
  const toggleAutoAnaf = useMutation({
    mutationFn: (r: RecurringInvoice) =>
      api.recurring.update({
        id: r.id,
        companyId: activeCompanyId!,
        templateName: r.templateName,
        frequency: r.frequency,
        nextIssueDate: r.nextIssueDate,
        dayOfMonth: r.dayOfMonth,
        autoSubmitAnaf: !r.autoSubmitAnaf,
        active: r.active,
        series: r.series,
        linesJson: r.linesJson,
        notes: r.notes ?? null,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.recurring.list(activeCompanyId!) });
      notify.success(t("recurring.notify.autoAnafSaved"));
    },
    onError: (e) => notify.error(formatError(e, t("recurring.notify.autoAnafError"))),
  });

  const handleOpenModal = () => {
    setEditingId(null);
    setForm({ ...EMPTY_FORM });
    setLines(makeEmptyLines());
    setLinesError(null);
    setFormError(null);
    setShowModal(true);
  };

  const handleOpenEditModal = (r: RecurringInvoice) => {
    setEditingId(r.id);
    setForm({
      templateName: r.templateName,
      clientId: r.clientId,
      frequency: r.frequency,
      dayOfMonth: r.dayOfMonth,
      nextIssueDate: r.nextIssueDate,
      series: r.series,
      autoSubmitAnaf: r.autoSubmitAnaf,
      notes: r.notes ?? "",
    });
    try {
      const parsed = JSON.parse(r.linesJson) as Omit<LineRow, "rowId">[];
      setLines(parsed.map((l) => ({ ...l, rowId: crypto.randomUUID() })));
    } catch {
      setLines(makeEmptyLines());
    }
    setLinesError(null);
    setFormError(null);
    setShowModal(true);
  };

  const { closing, close: closeModal } = useAnimatedClose(
    useCallback(() => {
      setShowModal(false);
      setEditingId(null);
    }, []),
  );

  const handleCreate = () => {
    if (!activeCompanyId) return;
    if (!form.templateName.trim()) { setFormError(t("recurring.validate.name")); notify.warn(t("recurring.validate.name")); return; }
    if (!editingId && !form.clientId) { setFormError(t("recurring.validate.client")); notify.warn(t("recurring.validate.client")); return; }
    if (!form.series.trim()) { setFormError(t("recurring.validate.series")); notify.warn(t("recurring.validate.series")); return; }
    setFormError(null);

    if (lines.length === 0) {
      setLinesError(t("recurring.validate.lines"));
      return;
    }
    for (const [i, line] of lines.entries()) {
      if (!line.name?.trim()) {
        setLinesError(t("recurring.validate.lineName", { n: i + 1 }));
        return;
      }
    }
    setLinesError(null);

    const linesJson = JSON.stringify(
      lines.map(({ rowId: _rowId, ...rest }) => rest),
    );

    if (editingId) {
      const current = recurringList.find((r) => r.id === editingId);
      updateMutation.mutate({
        id: editingId,
        companyId: activeCompanyId,
        templateName: form.templateName.trim(),
        frequency: form.frequency,
        nextIssueDate: form.nextIssueDate,
        dayOfMonth: form.dayOfMonth,
        autoSubmitAnaf: form.autoSubmitAnaf,
        active: current?.active ?? true,
        series: form.series.trim(),
        linesJson,
        notes: form.notes.trim() || null,
      });
    } else {
      createMutation.mutate({
        companyId: activeCompanyId,
        templateName: form.templateName.trim(),
        clientId: form.clientId,
        frequency: form.frequency,
        nextIssueDate: form.nextIssueDate,
        dayOfMonth: form.dayOfMonth,
        autoSubmitAnaf: form.autoSubmitAnaf,
        series: form.series.trim(),
        linesJson,
        notes: form.notes.trim() || undefined,
      });
    }
  };

  const activeCount = recurringList.filter((r) => r.active).length;
  const inactiveCount = recurringList.length - activeCount;

  // Next scheduled issue date across active templates (page-head sub line).
  const nextEmitere = useMemo(() => {
    const dates = recurringList.filter((r) => r.active && r.nextIssueDate).map((r) => r.nextIssueDate);
    if (dates.length === 0) return null;
    return dates.sort((a, b) => a.localeCompare(b))[0];
  }, [recurringList]);

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return recurringList
      .filter((r) => (tab === "all" ? true : tab === "active" ? r.active : !r.active))
      .filter((r) => {
        if (!q) return true;
        const client = contactMap.get(r.clientId) ?? "";
        return (
          r.templateName.toLowerCase().includes(q) ||
          client.toLowerCase().includes(q) ||
          r.series.toLowerCase().includes(q)
        );
      });
  }, [recurringList, tab, query, contactMap]);

  const tabs: Array<{ value: TabFilter; label: string; count: number }> = [
    { value: "all",      label: t("recurring.tabs.all"),      count: recurringList.length },
    { value: "active",   label: t("recurring.tabs.active"),   count: activeCount },
    { value: "inactive", label: t("recurring.tabs.inactive"), count: inactiveCount },
  ];

  const saving = createMutation.isPending || updateMutation.isPending;

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide page-recurring">
        <div className="page-head"><div><h1>{t("recurring.title")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("recurring.selectCompany")}
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide page-recurring">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("recurring.title")}</h1>
          <p className="sub">
            {t("recurring.sub.templates", { count: recurringList.length })} · {t("recurring.sub.active", { n: activeCount })}
            {nextEmitere ? ` · ${t("recurring.sub.next", { date: fmtRoDate(nextEmitere) })}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="btn-dark" onClick={handleOpenModal}>
            <Ic name="plus" />{t("recurring.head.new")}
          </button>
        </div>
      </div>

      <div className="scr-card">
        {/* toolbar */}
        <div className="scr-toolbar">
          <div className="tabs">
            {tabs.map((t) => (
              <div
                key={t.value}
                className={`tab${tab === t.value ? " active" : ""}`}
                onClick={() => setTab(t.value)}
              >
                {t.label}<span className="cnt num">{t.count}</span>
              </div>
            ))}
          </div>
          <div className="spacer" />
          <div className="scr-search">
            <Ic name="lens" />
            <input
              type="text"
              placeholder={t("recurring.search")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
        </div>

        {/* table */}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("recurring.states.loading")}</div>
        ) : recurringError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={recurringErr} label={t("recurring.states.errorLabel")} onRetry={() => void refetchRecurring()} />
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {recurringList.length === 0
              ? t("recurring.states.emptyNone")
              : t("recurring.states.emptyFiltered")}
          </div>
        ) : (
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("recurring.table.name")}</th>
                <th>{t("recurring.table.client")}</th>
                <th>{t("recurring.table.frequency")}</th>
                <th>{t("recurring.table.nextIssue")}</th>
                <th>{t("recurring.table.series")}</th>
                <th style={{ textAlign: "center" }}>{t("recurring.table.autoAnaf")}</th>
                <th style={{ textAlign: "center" }}>{t("recurring.table.active")}</th>
                <th className="r" style={{ width: 120 }}></th>
              </tr>
            </thead>
            <tbody>
              {list.map((r) => (
                <tr key={r.id} style={!r.active ? { background: "var(--bg-table-header)" } : undefined}>
                  <td>
                    <div className="cli">
                      <span className="cli-ava">{avaInitials(r.templateName)}</span>
                      <span>
                        <b>{r.templateName}</b>
                        {r.notes && (
                          <span style={{ display: "block", fontSize: 11, color: "var(--text-2)", fontWeight: 400 }}>
                            {r.notes}
                          </span>
                        )}
                      </span>
                    </div>
                  </td>
                  <td>{contactMap.get(r.clientId) ?? r.clientId}</td>
                  <td>{freqLabel(r.frequency, r.dayOfMonth, t)}</td>
                  {r.active ? (
                    <td className="num">{fmtRoDate(r.nextIssueDate)}</td>
                  ) : (
                    <td className="num muted">{t("recurring.table.inactive")}</td>
                  )}
                  <td><span className="doc">{r.series}</span></td>
                  <td style={{ textAlign: "center" }}>
                    <span
                      className={`tog${r.autoSubmitAnaf ? " on" : ""}`}
                      role="switch"
                      aria-checked={r.autoSubmitAnaf}
                      aria-label={t("recurring.row.autoAnafAria")}
                      tabIndex={0}
                      style={toggleAutoAnaf.isPending ? { opacity: 0.5, pointerEvents: "none" } : undefined}
                      onClick={() => toggleAutoAnaf.mutate(r)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          e.preventDefault();
                          toggleAutoAnaf.mutate(r);
                        }
                      }}
                    />
                  </td>
                  <td style={{ textAlign: "center" }}>
                    <span
                      className={`tog${r.active ? " on" : ""}`}
                      role="switch"
                      aria-checked={r.active}
                      aria-label={r.active ? t("recurring.row.deactivate") : t("recurring.row.activate")}
                      tabIndex={0}
                      style={toggleActive.isPending ? { opacity: 0.5, pointerEvents: "none" } : undefined}
                      onClick={() => toggleActive.mutate({ id: r.id, active: !r.active })}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          e.preventDefault();
                          toggleActive.mutate({ id: r.id, active: !r.active });
                        }
                      }}
                    />
                  </td>
                  <td>
                    {deleteConfirm === r.id ? (
                      <div className="row-acts" style={{ alignItems: "center", gap: 6 }}>
                        <span style={{ fontSize: 12, color: "var(--red)", whiteSpace: "nowrap" }}>{t("recurring.row.deleteQ")}</span>
                        <button
                          className="mini-btn"
                          title={t("recurring.row.confirmDelete")}
                          style={{ color: "var(--red)", opacity: 1 }}
                          disabled={deleteMutation.isPending}
                          onClick={() => deleteMutation.mutate(r.id)}
                        >
                          <Ic name="check" />
                        </button>
                        <button
                          className="mini-btn"
                          title={t("recurring.cancel")}
                          style={{ opacity: 1 }}
                          onClick={() => setDeleteConfirm(null)}
                        >
                          <Ic name="xMark" />
                        </button>
                      </div>
                    ) : (
                      <div className="row-acts">
                        <button className="mini-btn" title={t("recurring.row.edit")} onClick={() => handleOpenEditModal(r)}>
                          <Ic name="pen" />
                        </button>
                        <button className="mini-btn" title={t("recurring.row.delete")} onClick={() => setDeleteConfirm(r.id)}>
                          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_TRASH }} />
                        </button>
                      </div>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {/* info banner */}
      <div className="banner" style={{ marginTop: 14 }}>
        <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_INFO }} />
        <span>
          {t("recurring.banner.part1")} <b>{t("recurring.banner.drafts")}</b> {t("recurring.banner.part2")}{" "}
          <b>{t("recurring.banner.autoAnaf")}</b> {t("recurring.banner.part3")}
        </span>
      </div>

      {/* Create / Edit modal — design .modal-back + .modal pattern */}
      {showModal && (
        <div
          className={`modal-back ${closing ? "closing" : "show"}`}
          style={{ position: "fixed", zIndex: 80 }}
          onMouseDown={(e) => { if (e.target === e.currentTarget && !saving) closeModal(); }}
        >
          <div className="modal lg" style={{ width: 720 }}>
            <div className="modal-head">
              <div>
                <div className="mt">{editingId ? t("recurring.modal.editTitle") : t("recurring.modal.createTitle")}</div>
                <div className="ms">
                  {t("recurring.modal.sub")}
                </div>
              </div>
              <button className="modal-x" onClick={closeModal} aria-label={t("recurring.modal.close")}>
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_X }} />
              </button>
            </div>
            <div className="modal-body">
              <div className="fgrid">
                {/* Template name */}
                <div className="field span2">
                  <label>{t("recurring.modal.name")} <span className="req">*</span></label>
                  <input
                    className={`input${formError && !form.templateName.trim() ? " invalid" : ""}`}
                    placeholder={t("recurring.modal.namePlaceholder")}
                    value={form.templateName}
                    onChange={(e) => setForm((f) => ({ ...f, templateName: e.target.value }))}
                    autoFocus
                  />
                </div>

                {/* Client — read-only in edit mode */}
                <div className="field span2">
                  <label>{t("recurring.modal.client")} <span className="req">*</span></label>
                  <select
                    className={`select${formError && !editingId && !form.clientId ? " invalid" : ""}`}
                    value={form.clientId}
                    disabled={!!editingId}
                    onChange={(e) => setForm((f) => ({ ...f, clientId: e.target.value }))}
                  >
                    <option value="">{t("recurring.modal.clientPick")}</option>
                    {contacts.map((c) => (
                      <option key={c.id} value={c.id}>{c.legalName}</option>
                    ))}
                  </select>
                  {editingId && (
                    <span style={{ fontSize: 11, color: "var(--text-2)" }}>
                      {t("recurring.modal.clientLocked")}
                    </span>
                  )}
                </div>

                {/* Frequency + Day */}
                <div className="field">
                  <label>{t("recurring.modal.frequency")} <span className="req">*</span></label>
                  <select
                    className="select"
                    value={form.frequency}
                    onChange={(e) => {
                      const freq = e.target.value;
                      setForm((f) => ({
                        ...f,
                        frequency: freq,
                        nextIssueDate: nextDatePreview(freq, f.dayOfMonth),
                      }));
                    }}
                  >
                    <option value="monthly">{t("recurring.freq.monthly")}</option>
                    <option value="quarterly">{t("recurring.freq.quarterly")}</option>
                    <option value="annual">{t("recurring.freq.annual")}</option>
                  </select>
                </div>
                <div className="field">
                  <label>{t("recurring.modal.dayOfMonth")}</label>
                  <input
                    className="input"
                    type="number"
                    min={1}
                    max={28}
                    value={form.dayOfMonth}
                    onChange={(e) => {
                      const day = Math.max(1, Math.min(28, Number(e.target.value)));
                      setForm((f) => ({
                        ...f,
                        dayOfMonth: day,
                        nextIssueDate: nextDatePreview(f.frequency, day),
                      }));
                    }}
                  />
                </div>

                {/* Next issue date + Series */}
                <div className="field">
                  <label>{t("recurring.modal.nextIssue")}</label>
                  <input
                    className="input"
                    type="date"
                    value={form.nextIssueDate}
                    onChange={(e) => setForm((f) => ({ ...f, nextIssueDate: e.target.value }))}
                  />
                </div>
                <div className="field">
                  <label>{t("recurring.modal.series")} <span className="req">*</span></label>
                  <input
                    className={`input${formError && !form.series.trim() ? " invalid" : ""}`}
                    placeholder={t("recurring.modal.seriesPlaceholder")}
                    value={form.series}
                    onChange={(e) =>
                      setForm((f) => ({ ...f, series: e.target.value.toUpperCase() }))
                    }
                  />
                </div>

                {/* Auto submit ANAF */}
                <label
                  className="span2"
                  style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 13, cursor: "pointer", userSelect: "none" }}
                >
                  <span
                    className={`tog${form.autoSubmitAnaf ? " on" : ""}`}
                    role="switch"
                    aria-checked={form.autoSubmitAnaf}
                    aria-label={t("recurring.modal.autoSubmit")}
                    tabIndex={0}
                    onClick={() => setForm((f) => ({ ...f, autoSubmitAnaf: !f.autoSubmitAnaf }))}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        setForm((f) => ({ ...f, autoSubmitAnaf: !f.autoSubmitAnaf }));
                      }
                    }}
                  />
                  <span>{t("recurring.modal.autoSubmit")}</span>
                </label>

                {/* Line items */}
                <div className="field span2">
                  <label>{t("recurring.modal.lines")} <span className="req">*</span></label>
                  <LineItemsEditor
                    lines={lines}
                    onChange={(updated) => { setLines(updated); setLinesError(null); }}
                    showTotals={false}
                    companyId={activeCompanyId ?? undefined}
                  />
                  {linesError && <span className="err">{linesError}</span>}
                </div>

                {/* Notes */}
                <div className="field span2">
                  <label>{t("recurring.modal.notes")}</label>
                  <input
                    className="input"
                    placeholder={t("recurring.modal.notesPlaceholder")}
                    value={form.notes}
                    onChange={(e) => setForm((f) => ({ ...f, notes: e.target.value }))}
                  />
                </div>

                <div className="banner span2" style={{ marginBottom: 0 }}>
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_INFO }} />
                  <span>
                    {t("recurring.modal.tip")}
                  </span>
                </div>
              </div>
            </div>
            <div className="modal-foot">
              <button type="button" className="pill-btn" onClick={closeModal} disabled={saving}>
                {t("recurring.cancel")}
              </button>
              <button type="button" className="btn-dark" disabled={saving} onClick={handleCreate}>
                <Ic name="check" />
                {saving ? t("recurring.modal.saving") : editingId ? t("recurring.modal.saveChanges") : t("recurring.modal.create")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
