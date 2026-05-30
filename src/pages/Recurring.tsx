/**
 * Facturi recurente — creare, listare, ștergere șabloane de facturare automată.
 */

import { useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { CreateRecurringArgs } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";

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

const EMPTY_FORM = {
  templateName: "",
  clientId: "",
  frequency: "monthly",
  dayOfMonth: 1,
  nextIssueDate: localDateISO(new Date()),
  series: "FCT",
  autoSubmitAnaf: false,
  linesJson: JSON.stringify([
    { description: "Servicii", quantity: 1, unitPrice: "0.00", vatRate: 21 }
  ], null, 2),
  notes: "",
};

export function RecurringPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [showModal, setShowModal] = useState(false);
  const [form, setForm] = useState({ ...EMPTY_FORM });
  const [linesError, setLinesError] = useState<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);

  // Fetch recurring invoices
  const { data: recurringList = [], isLoading, isError: recurringError, error: recurringErr, refetch: refetchRecurring } = useQuery({
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
    },
    onError: (e) => notify.error("Eroare la creare: " + String(e)),
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.recurring.delete(id, activeCompanyId!),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.recurring.list(activeCompanyId!) });
      notify.success("Șablon șters");
      setDeleteConfirm(null);
    },
    onError: (e) => notify.error("Eroare la ștergere: " + String(e)),
  });

  const toggleActive = useMutation({
    mutationFn: ({ id, active }: { id: string; active: boolean }) =>
      api.recurring.toggleActive(id, active),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.recurring.list(activeCompanyId!) });
      notify.success("Status șablon actualizat.");
    },
    onError: (e) => notify.error("Eroare: " + String(e)),
  });

  const handleOpenModal = () => {
    setForm({ ...EMPTY_FORM });
    setLinesError(null);
    setShowModal(true);
  };

  const handleCreate = () => {
    if (!activeCompanyId) return;
    if (!form.templateName.trim()) { notify.warn("Introduceți un nume pentru șablon."); return; }
    if (!form.clientId) { notify.warn("Selectați un client."); return; }
    if (!form.series.trim()) { notify.warn("Introduceți seria facturii."); return; }

    // Validate JSON lines
    try {
      const parsed = JSON.parse(form.linesJson);
      if (!Array.isArray(parsed) || parsed.length === 0) throw new Error("Cel puțin un articol necesar");
      setLinesError(null);
    } catch (e) {
      setLinesError("JSON invalid: " + String(e));
      return;
    }

    createMutation.mutate({
      companyId: activeCompanyId,
      templateName: form.templateName.trim(),
      clientId: form.clientId,
      frequency: form.frequency,
      nextIssueDate: form.nextIssueDate,
      dayOfMonth: form.dayOfMonth,
      autoSubmitAnaf: form.autoSubmitAnaf,
      series: form.series.trim(),
      linesJson: form.linesJson,
      notes: form.notes.trim() || undefined,
    });
  };

  if (!activeCompanyId) {
    return (
      <div className="content">
        <div className="content-titlebar">
          <span className="content-title">Facturi Recurente</span>
        </div>
        <div style={{ padding: 40, textAlign: "center", color: "var(--text-muted)" }}>
          Selectați o companie activă din Setări.
        </div>
      </div>
    );
  }

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">Financiar</span>
          Facturi Recurente
        </span>
        <span style={{ marginLeft: "auto" }}>
          <button className="btn primary" onClick={handleOpenModal}>
            <Icon name="plus" size={12} /> Șablon nou
          </button>
        </span>
      </div>

      <div className="content-body" style={{ overflowY: "auto", flex: 1 }}>
        {isLoading ? (
          <div style={{ padding: 24, color: "var(--text-muted)" }}>Se încarcă…</div>
        ) : recurringError ? (
          <QueryErrorBanner error={recurringErr} label="facturile recurente" onRetry={() => void refetchRecurring()} />
        ) : recurringList.length === 0 ? (
          <div style={{ padding: 48, textAlign: "center", color: "var(--text-muted)" }}>
            <Icon name="refresh" size={32} />
            <div style={{ marginTop: 12, fontSize: 13, fontWeight: 600 }}>Niciun șablon recurent</div>
            <div style={{ marginTop: 6, fontSize: 11 }}>
              Creați un șablon pentru a emite automat facturi periodice.
            </div>
            <button className="btn primary" style={{ marginTop: 16 }} onClick={handleOpenModal}>
              <Icon name="plus" size={12} /> Șablon nou
            </button>
          </div>
        ) : (
          <table className="dt" style={{ width: "100%" }}>
            <thead>
              <tr>
                <th>Șablon</th>
                <th>Client</th>
                <th style={{ width: 110 }}>Frecvență</th>
                <th style={{ width: 90 }}>Ziua lunii</th>
                <th style={{ width: 110 }}>Urm. emitere</th>
                <th style={{ width: 60 }}>Serie</th>
                <th style={{ width: 90 }}>Auto ANAF</th>
                <th style={{ width: 70 }}>Stare</th>
                <th style={{ width: 180 }}>Acțiuni</th>
              </tr>
            </thead>
            <tbody>
              {recurringList.map((r) => (
                <tr key={r.id}>
                  <td>
                    <span style={{ fontWeight: 600 }}>{r.templateName}</span>
                    {r.notes && (
                      <span style={{ display: "block", fontSize: 10, color: "var(--text-muted)" }}>
                        {r.notes}
                      </span>
                    )}
                  </td>
                  <td>{contactMap.get(r.clientId) ?? r.clientId}</td>
                  <td>{FREQ_LABELS[r.frequency] ?? r.frequency}</td>
                  <td style={{ textAlign: "center" }}>{r.dayOfMonth}</td>
                  <td className="mono">{r.nextIssueDate}</td>
                  <td className="mono">{r.series}</td>
                  <td style={{ textAlign: "center" }}>
                    {r.autoSubmitAnaf ? (
                      <Icon name="check" size={13} style={{ color: "var(--st-validated-fg)" }} />
                    ) : (
                      <Icon name="minus" size={13} style={{ color: "var(--text-muted)" }} />
                    )}
                  </td>
                  <td>
                    <StatusBadge status={r.active ? "ACTIVE" : "INACTIVE"} />
                  </td>
                  <td>
                    {deleteConfirm === r.id ? (
                      <span style={{ display: "flex", gap: 4 }}>
                        <button
                          className="btn compact"
                          style={{ color: "var(--st-rejected-fg)" }}
                          onClick={() => deleteMutation.mutate(r.id)}
                          disabled={deleteMutation.isPending}
                          title="Confirmare ștergere"
                        >
                          <Icon name="check" size={11} />
                        </button>
                        <button
                          className="btn compact"
                          onClick={() => setDeleteConfirm(null)}
                          title="Anulează"
                        >
                          <Icon name="x" size={11} />
                        </button>
                      </span>
                    ) : (
                      <span style={{ display: "flex", gap: 4 }}>
                        <button
                          type="button"
                          className="btn compact"
                          onClick={() => toggleActive.mutate({ id: r.id, active: !r.active })}
                          disabled={toggleActive.isPending}
                          title={r.active ? "Pune pe pauză șablonul" : "Reia șablonul"}
                        >
                          {r.active ? "Pauză" : "Reia"}
                        </button>
                        {/* TODO MISS-03: full edit modal — for now use delete + create */}
                        <button
                          type="button"
                          className="btn compact"
                          disabled
                          title="În curând: editare completă"
                        >
                          Editează
                        </button>
                        <button
                          className="btn compact"
                          onClick={() => setDeleteConfirm(r.id)}
                          title="Șterge șablon"
                        >
                          <Icon name="trash" size={11} />
                        </button>
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {/* Create recurring modal */}
      {showModal && (
        <div
          className="palette-scrim"
          style={{ alignItems: "center", paddingTop: 0 }}
          onClick={() => setShowModal(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{
              background: "var(--bg-content)",
              border: "1px solid var(--border)",
              minWidth: 420,
              maxWidth: 520,
              maxHeight: "90vh",
              overflowY: "auto",
              boxShadow: "0 8px 32px rgba(0,0,0,0.18)",
              padding: 20,
            }}
          >
            <div style={{ fontWeight: 700, fontSize: 13, marginBottom: 16 }}>
              Șablon factură recurentă
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              {/* Template name */}
              <label style={{ fontSize: 11 }}>
                Nume șablon *
                <input
                  className="field"
                  style={{ display: "block", width: "100%", marginTop: 4 }}
                  placeholder="ex: Abonament lunar hosting"
                  value={form.templateName}
                  onChange={(e) => setForm((f) => ({ ...f, templateName: e.target.value }))}
                />
              </label>

              {/* Client */}
              <label style={{ fontSize: 11 }}>
                Client *
                <select
                  className="field"
                  style={{ display: "block", width: "100%", marginTop: 4 }}
                  value={form.clientId}
                  onChange={(e) => setForm((f) => ({ ...f, clientId: e.target.value }))}
                >
                  <option value="">— Selectați client —</option>
                  {contacts.map((c) => (
                    <option key={c.id} value={c.id}>{c.legalName}</option>
                  ))}
                </select>
              </label>

              {/* Frequency + Day */}
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
                <label style={{ fontSize: 11 }}>
                  Frecvență *
                  <select
                    className="field"
                    style={{ display: "block", width: "100%", marginTop: 4 }}
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
                  </select>
                </label>
                <label style={{ fontSize: 11 }}>
                  Ziua lunii (1–28)
                  <input
                    className="field"
                    type="number"
                    min={1}
                    max={28}
                    style={{ display: "block", width: "100%", marginTop: 4 }}
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
                </label>
              </div>

              {/* Next issue date + Series */}
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
                <label style={{ fontSize: 11 }}>
                  Prima / urm. emitere
                  <input
                    className="field"
                    type="date"
                    style={{ display: "block", width: "100%", marginTop: 4 }}
                    value={form.nextIssueDate}
                    onChange={(e) => setForm((f) => ({ ...f, nextIssueDate: e.target.value }))}
                  />
                </label>
                <label style={{ fontSize: 11 }}>
                  Serie factură *
                  <input
                    className="field"
                    style={{ display: "block", width: "100%", marginTop: 4 }}
                    placeholder="ex: FCT"
                    value={form.series}
                    onChange={(e) => setForm((f) => ({ ...f, series: e.target.value.toUpperCase() }))}
                  />
                </label>
              </div>

              {/* Auto submit ANAF */}
              <label style={{ fontSize: 11, display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
                <input
                  type="checkbox"
                  checked={form.autoSubmitAnaf}
                  onChange={(e) => setForm((f) => ({ ...f, autoSubmitAnaf: e.target.checked }))}
                />
                Trimitere automată la ANAF după emitere
              </label>

              {/* Lines JSON */}
              <label style={{ fontSize: 11 }}>
                Articole (JSON) *
                <textarea
                  className="field"
                  style={{
                    display: "block",
                    width: "100%",
                    marginTop: 4,
                    minHeight: 100,
                    fontFamily: "monospace",
                    fontSize: 11,
                    resize: "vertical",
                  }}
                  value={form.linesJson}
                  onChange={(e) => {
                    setForm((f) => ({ ...f, linesJson: e.target.value }));
                    setLinesError(null);
                  }}
                />
                {linesError && (
                  <span style={{ color: "var(--st-rejected-fg)", fontSize: 10 }}>{linesError}</span>
                )}
                <span style={{ color: "var(--text-muted)", fontSize: 10, display: "block", marginTop: 2 }}>
                  Format: [{"{"}"description":"…","quantity":1,"unitPrice":"100.00","vatRate":19{"}"}]
                </span>
              </label>

              {/* Notes */}
              <label style={{ fontSize: 11 }}>
                Notițe (opțional)
                <input
                  className="field"
                  style={{ display: "block", width: "100%", marginTop: 4 }}
                  placeholder="Informații suplimentare"
                  value={form.notes}
                  onChange={(e) => setForm((f) => ({ ...f, notes: e.target.value }))}
                />
              </label>
            </div>

            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 16 }}>
              <button className="btn" onClick={() => setShowModal(false)}>Anulează</button>
              <button
                className="btn primary"
                disabled={createMutation.isPending}
                onClick={handleCreate}
              >
                {createMutation.isPending ? "Se salvează…" : "Creează șablon"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
