/**
 * Facturi recurente — re-skinned to rf kit (Wave 4).
 * Preserves: api.recurring.list(activeCompanyId) + api.contacts.list,
 * "Șablon nou"/edit modal (templateName/clientId/frequency/dayOfMonth/
 * nextIssueDate/series/autoSubmitAnaf/notes + LineItemsEditor)
 * → api.recurring.create / api.recurring.update,
 * delete → api.recurring.delete(id, companyId),
 * Activ toggle → api.recurring.toggleActive(id, companyId, active).
 */

import { useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { LineItemsEditor } from "@/components/shared/LineItemsEditor";
import type { LineRow } from "@/components/shared/LineItemsEditor";
import {
  PageHeader, Btn, IconBtn, Badge, Card, Field, Input, Select,
  Toggle, Banner, Empty, Modal,
} from "@/components/rf";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { CreateRecurringArgs, RecurringInvoice } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";

const FREQ_LABELS: Record<string, string> = {
  monthly:   "Lunar",
  quarterly: "Trimestrial",
  annual:    "Anual",
};

function localDateISO(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function nextDatePreview(freq: string, day: number): string {
  const today = new Date();
  let next = new Date(today.getFullYear(), today.getMonth(), day);
  if (next <= today) {
    if (freq === "monthly") next = new Date(today.getFullYear(), today.getMonth() + 1, day);
    else if (freq === "quarterly") next = new Date(today.getFullYear(), today.getMonth() + 3, day);
    else next = new Date(today.getFullYear() + 1, today.getMonth(), day);
  }
  return localDateISO(next);
}

const DEFAULT_LINE: LineRow = {
  rowId: crypto.randomUUID(),
  name: "Servicii",
  quantity: 1,
  unit: "buc",
  unitPrice: 0,
  vatRate: 19,
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
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [showModal, setShowModal] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form, setForm] = useState({ ...EMPTY_FORM });
  const [lines, setLines] = useState<LineRow[]>(makeEmptyLines);
  const [linesError, setLinesError] = useState<string | null>(null);
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
      notify.success("Factură recurentă creată cu succes");
      setShowModal(false);
      setForm({ ...EMPTY_FORM });
      setLines(makeEmptyLines());
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut crea șablonul recurent.")),
  });

  const updateMutation = useMutation({
    mutationFn: (args: Parameters<typeof api.recurring.update>[0]) =>
      api.recurring.update(args),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.recurring.list(activeCompanyId!) });
      notify.success("Șablon actualizat cu succes");
      setShowModal(false);
      setEditingId(null);
      setForm({ ...EMPTY_FORM });
      setLines(makeEmptyLines());
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut actualiza șablonul recurent.")),
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.recurring.delete(id, activeCompanyId!),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.recurring.list(activeCompanyId!) });
      notify.success("Șablon șters");
      setDeleteConfirm(null);
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge șablonul.")),
  });

  const toggleActive = useMutation({
    mutationFn: ({ id, active }: { id: string; active: boolean }) =>
      api.recurring.toggleActive(id, activeCompanyId!, active),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.recurring.list(activeCompanyId!) });
      notify.success("Status șablon actualizat.");
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut actualiza statusul șablonului.")),
  });

  const handleOpenModal = () => {
    setEditingId(null);
    setForm({ ...EMPTY_FORM });
    setLines(makeEmptyLines());
    setLinesError(null);
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
    setShowModal(true);
  };

  const handleCreate = () => {
    if (!activeCompanyId) return;
    if (!form.templateName.trim()) { notify.warn("Introduceți un nume pentru șablon."); return; }
    if (!editingId && !form.clientId) { notify.warn("Selectați un client."); return; }
    if (!form.series.trim()) { notify.warn("Introduceți seria facturii."); return; }

    if (lines.length === 0) {
      setLinesError("Adăugați cel puțin un articol.");
      return;
    }
    for (const [i, line] of lines.entries()) {
      if (!line.name?.trim()) {
        setLinesError(`Linia ${i + 1}: denumirea produsului/serviciului este obligatorie.`);
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

  if (!activeCompanyId) {
    return (
      <div className="rf-page">
        <PageHeader title="Facturi Recurente" />
        <div className="rf-page-body">
          <Card pad>
            <p style={{ textAlign: "center", color: "var(--rf-text-muted)", padding: "32px 0" }}>
              Selectați o companie activă din Setări.
            </p>
          </Card>
        </div>
      </div>
    );
  }

  return (
    <div className="rf-page">
      <PageHeader
        title="Facturi Recurente"
        sub={<Badge variant="neutral">{recurringList.length} șabloane</Badge>}
        actions={
          <Btn
            variant="primary"
            icon="plus"
            size="sm"
            onClick={handleOpenModal}
          >
            Șablon nou
          </Btn>
        }
      />

      <div className="rf-page-body">
        <Card>
          <div className="rf-tbl-wrap">
            {isLoading ? (
              <Empty icon="refresh" title="Se încarcă…" />
            ) : recurringError ? (
              <QueryErrorBanner error={recurringErr} label="facturile recurente" onRetry={() => void refetchRecurring()} />
            ) : recurringList.length === 0 ? (
              <Empty icon="refresh" title="Niciun șablon recurent">
                <div style={{ marginTop: 12 }}>
                  Creați un șablon pentru a emite automat facturi periodice.
                </div>
                <div style={{ marginTop: 12 }}>
                  <Btn variant="primary" icon="plus" size="sm" onClick={handleOpenModal}>
                    Șablon nou
                  </Btn>
                </div>
              </Empty>
            ) : (
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th>Denumire șablon</th>
                    <th>Client</th>
                    <th>Frecvență</th>
                    <th>Urm. emitere</th>
                    <th>Serie</th>
                    <th style={{ textAlign: "center" }}>Auto ANAF</th>
                    <th style={{ textAlign: "center" }}>Activ</th>
                    <th style={{ width: 160 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {recurringList.map((r) => (
                    <tr key={r.id}>
                      <td>
                        <span style={{ fontWeight: 600 }}>{r.templateName}</span>
                        {r.notes && (
                          <span style={{ display: "block", fontSize: 11, color: "var(--rf-text-muted)" }}>
                            {r.notes}
                          </span>
                        )}
                      </td>
                      <td>{contactMap.get(r.clientId) ?? r.clientId}</td>
                      <td>
                        <Badge variant="info">{FREQ_LABELS[r.frequency] ?? r.frequency}</Badge>
                      </td>
                      <td className="mono" style={{ color: "var(--rf-text-muted)" }}>{r.nextIssueDate}</td>
                      <td className="mono">{r.series}</td>
                      <td style={{ textAlign: "center" }}>
                        {r.autoSubmitAnaf ? (
                          <Icon name="checkCircle" size={16} style={{ color: "var(--rf-success)" }} />
                        ) : (
                          <span className="rf-dim">—</span>
                        )}
                      </td>
                      <td style={{ textAlign: "center" }}>
                        <Toggle
                          checked={r.active}
                          onChange={(checked) =>
                            toggleActive.mutate({ id: r.id, active: checked })
                          }
                          disabled={toggleActive.isPending}
                          aria-label={r.active ? "Dezactivează șablon" : "Activează șablon"}
                        />
                      </td>
                      <td onClick={(e) => e.stopPropagation()}>
                        <div className="rf-cell-actions">
                          {deleteConfirm === r.id ? (
                            <>
                              <Btn
                                variant="danger"
                                size="sm"
                                icon="check"
                                onClick={() => deleteMutation.mutate(r.id)}
                                disabled={deleteMutation.isPending}
                              >
                                Confirma
                              </Btn>
                              <IconBtn
                                icon="x"
                                title="Anulează"
                                onClick={() => setDeleteConfirm(null)}
                              />
                            </>
                          ) : (
                            <>
                              <IconBtn
                                icon="pen"
                                title="Editează"
                                onClick={() => handleOpenEditModal(r)}
                              />
                              <IconBtn
                                icon="trash"
                                title="Șterge"
                                onClick={() => setDeleteConfirm(r.id)}
                              />
                            </>
                          )}
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>

          {/* Footer */}
          {recurringList.length > 0 && (
            <div className="rf-tbl-footer">
              <span>Total: <b>{recurringList.length}</b> șabloane</span>
              <span>
                Active:{" "}
                <b style={{ color: "var(--rf-success)" }}>
                  {recurringList.filter((r) => r.active).length}
                </b>
              </span>
            </div>
          )}
        </Card>
      </div>

      {/* Create / Edit modal */}
      {showModal && (
        <Modal
          open
          onOpenChange={(open) => {
            if (!open) { setShowModal(false); setEditingId(null); }
          }}
          title={editingId ? "Editează șablon recurent" : "Șablon factură recurentă"}
          width={720}
          footer={
            <>
              <Btn
                variant="secondary"
                onClick={() => { setShowModal(false); setEditingId(null); }}
                disabled={createMutation.isPending || updateMutation.isPending}
              >
                Anulează
              </Btn>
              <Btn
                variant="primary"
                icon="check"
                disabled={createMutation.isPending || updateMutation.isPending}
                onClick={handleCreate}
              >
                {(createMutation.isPending || updateMutation.isPending)
                  ? "Se salvează…"
                  : editingId
                  ? "Salvează modificările"
                  : "Creează șablon"}
              </Btn>
            </>
          }
        >
          <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            {/* Template name */}
            <Field label="Nume șablon" required>
              <Input
                placeholder="ex: Abonament lunar hosting"
                value={form.templateName}
                onChange={(e) => setForm((f) => ({ ...f, templateName: e.target.value }))}
                autoFocus
              />
            </Field>

            {/* Client — read-only in edit mode */}
            <Field label="Client" required>
              <Select
                value={form.clientId}
                disabled={!!editingId}
                onChange={(e) => setForm((f) => ({ ...f, clientId: e.target.value }))}
              >
                <option value="">— Selectați client —</option>
                {contacts.map((c) => (
                  <option key={c.id} value={c.id}>{c.legalName}</option>
                ))}
              </Select>
              {editingId && (
                <span style={{ fontSize: 11, color: "var(--rf-text-muted)", marginTop: 2 }}>
                  Clientul nu poate fi modificat după creare.
                </span>
              )}
            </Field>

            {/* Frequency + Day */}
            <div className="rf-grid-2">
              <Field label="Frecvență" required>
                <Select
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
                  <option value="monthly">Lunar</option>
                  <option value="quarterly">Trimestrial</option>
                  <option value="annual">Anual</option>
                </Select>
              </Field>
              <Field label="Ziua lunii (1–28)">
                <Input
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
                {form.frequency === "monthly" && form.dayOfMonth > 28 && (
                  <span style={{ fontSize: 11, color: "var(--rf-warning)", marginTop: 3, display: "block" }}>
                    Lunile cu mai puține zile (feb., etc.) pot decala sau sări emiterea.
                  </span>
                )}
              </Field>
            </div>

            {/* Next issue date + Series */}
            <div className="rf-grid-2">
              <Field label="Prima / urm. emitere">
                <Input
                  type="date"
                  value={form.nextIssueDate}
                  onChange={(e) => setForm((f) => ({ ...f, nextIssueDate: e.target.value }))}
                />
              </Field>
              <Field label="Serie factură" required>
                <Input
                  placeholder="ex: FCT"
                  value={form.series}
                  onChange={(e) =>
                    setForm((f) => ({ ...f, series: e.target.value.toUpperCase() }))
                  }
                />
              </Field>
            </div>

            {/* Auto submit ANAF */}
            <label
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                fontSize: 13,
                cursor: "pointer",
              }}
            >
              <Toggle
                checked={form.autoSubmitAnaf}
                onChange={(checked) => setForm((f) => ({ ...f, autoSubmitAnaf: checked }))}
              />
              <span>Trimitere automată la ANAF după emitere</span>
            </label>

            {/* Line items */}
            <div>
              <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>
                Articole <span style={{ color: "var(--rf-error)" }}>*</span>
              </div>
              <LineItemsEditor
                lines={lines}
                onChange={(updated) => { setLines(updated); setLinesError(null); }}
                showTotals={false}
                companyId={activeCompanyId ?? undefined}
              />
              {linesError && (
                <span style={{ fontSize: 11, color: "var(--rf-error)", marginTop: 4, display: "block" }}>
                  {linesError}
                </span>
              )}
            </div>

            {/* Notes */}
            <Field label="Notițe (opțional)">
              <Input
                placeholder="Informații suplimentare"
                value={form.notes}
                onChange={(e) => setForm((f) => ({ ...f, notes: e.target.value }))}
              />
            </Field>

            <Banner variant="info">
              Puteți crea un șablon și direct dintr-o factură existentă, prin opțiunea „Salvează ca șablon".
            </Banner>
          </div>
        </Modal>
      )}
    </div>
  );
}
