/**
 * Chitanțe (cash receipts) — company-scoped.
 *
 * Listează chitanțele companiei active, permite emitere via modal
 * și ștergere cu confirmare. Generează PDF per chitanță.
 * Dacă nicio companie nu e activă, afișează mesajul "selectați o companie".
 */

import { useId, isValidElement, cloneElement, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Icon } from "@/components/shared/Icon";
import { ContactCombobox } from "@/components/shared/ContactCombobox";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { Contact, Receipt, ReceiptInput } from "@/types";

export function ReceiptsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [showModal, setShowModal] = useState(false);

  const {
    data: receiptList = [],
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: queryKeys.receipts.list(activeCompanyId ?? ""),
    queryFn: () => api.receipts.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => {
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă."));
      return api.receipts.delete(id, activeCompanyId);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.receipts.all });
      notify.success("Chitanță ștearsă.");
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge chitanța.")),
  });

  const pdfMutation = useMutation({
    mutationFn: (id: string) => {
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă."));
      return api.receipts.generatePdf(id, activeCompanyId);
    },
    onSuccess: async (path) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.receipts.all });
      notify.success("PDF generat.");
      try {
        await openPath(path);
      } catch {
        /* best-effort reveal */
      }
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut genera PDF-ul.")),
  });

  const handleDelete = async (r: Receipt) => {
    const ok = await confirm(
      `Șterge chitanța "${r.series}-${r.number}"? Această acțiune nu poate fi anulată.`,
      { title: "Confirmare ștergere", kind: "warning" },
    );
    if (!ok) return;
    deleteMutation.mutate(r.id);
  };

  if (!activeCompanyId) {
    return (
      <div className="content">
        <div className="content-titlebar">
          <span className="content-title">
            <span className="crumb">Operativ</span>
            Chitanțe
          </span>
        </div>
        <div style={{ padding: 40, textAlign: "center", fontSize: 12, color: "var(--text-muted)" }}>
          Selectați o companie activă pentru a vedea chitanțele.
        </div>
      </div>
    );
  }

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">Operativ</span>
          Chitanțe
        </span>
        <span className="muted" style={{ fontSize: 11 }}>
          {receiptList.length} chitanțe
        </span>
        <span style={{ marginLeft: "auto" }}>
          <button
            type="button"
            className="btn primary"
            onClick={() => setShowModal(true)}
          >
            <Icon name="plus" size={12} /> Chitanță nouă
          </button>
        </span>
      </div>

      <div className="content-toolbar">
        <span style={{ marginLeft: "auto" }}>
          <button
            type="button"
            className="btn-icon"
            title="Reîmprospătează"
            onClick={() =>
              void queryClient.invalidateQueries({ queryKey: queryKeys.receipts.all })
            }
          >
            <Icon name="refresh" size={14} />
          </button>
        </span>
      </div>

      <div className="content-body">
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 12, color: "var(--text-muted)" }}>
            Se încarcă…
          </div>
        ) : isError ? (
          <QueryErrorBanner
            error={error}
            label="chitanțele"
            onRetry={() => void refetch()}
          />
        ) : receiptList.length === 0 ? (
          <div style={{ padding: 40, textAlign: "center", fontSize: 12, color: "var(--text-muted)" }}>
            Nicio chitanță. Apăsați „Chitanță nouă" pentru a emite prima chitanță.
          </div>
        ) : (
          <table className="dt">
            <thead>
              <tr>
                <th style={{ width: 100 }}>Număr</th>
                <th style={{ width: 110 }}>Data</th>
                <th>Plătitor</th>
                <th style={{ width: 120 }}>Leg. de factură</th>
                <th style={{ width: 120 }} className="num">Sumă</th>
                <th style={{ width: 70 }}>Monedă</th>
                <th style={{ width: 110 }}>Acțiuni</th>
              </tr>
            </thead>
            <tbody>
              {receiptList.map((r: Receipt) => (
                <tr key={r.id}>
                  <td className="mono">{r.series}-{r.number}</td>
                  <td>{r.issueDate}</td>
                  <td>{r.payerName ?? <span className="dim">—</span>}</td>
                  <td className="mono">{r.invoiceId ? <span title={r.invoiceId}>factură</span> : <span className="dim">—</span>}</td>
                  <td className="num tnum">{fmtRON(parseDec(r.amount))}</td>
                  <td className="mono">{r.currency}</td>
                  <td onClick={(e) => e.stopPropagation()}>
                    <button
                      type="button"
                      className="btn-icon"
                      title="Generează PDF"
                      disabled={pdfMutation.isPending}
                      onClick={() => pdfMutation.mutate(r.id)}
                    >
                      <Icon name="file" size={13} />
                    </button>
                    <button
                      type="button"
                      className="btn-icon"
                      title="Șterge"
                      onClick={() => void handleDelete(r)}
                    >
                      <Icon name="x" size={13} />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <div
        style={{
          padding: "6px 14px",
          borderTop: "1px solid var(--border)",
          background: "var(--bg)",
          display: "flex",
          gap: 16,
          fontSize: 11,
          color: "var(--text-muted)",
        }}
      >
        <span>
          Total: <b style={{ color: "var(--text)" }}>{receiptList.length}</b> chitanțe
        </span>
      </div>

      {showModal && (
        <ReceiptModal
          companyId={activeCompanyId}
          onClose={() => setShowModal(false)}
          onSaved={() => {
            void queryClient.invalidateQueries({ queryKey: queryKeys.receipts.all });
            setShowModal(false);
          }}
        />
      )}
    </div>
  );
}

// ─── Modal ──────────────────────────────────────────────────────────────────

function ReceiptModal({
  companyId,
  onClose,
  onSaved,
}: {
  companyId: string;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [form, setForm] = useState<ReceiptInput>({
    amount: "",
    currency: "RON",
    issueDate: new Date().toISOString().slice(0, 10),
    series: "CH",
    payerName: "",
    notes: "",
    contactId: undefined,
    invoiceId: undefined,
  });
  const [contact, setContact] = useState<Contact | null>(null);
  const [error, setError] = useState<string | null>(null);

  const createMutation = useMutation({
    mutationFn: (input: ReceiptInput) => api.receipts.create(companyId, input),
    onSuccess: () => {
      notify.success("Chitanță emisă.");
      onSaved();
    },
    onError: (e) => setError(formatError(e, "Eroare la emitere.")),
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    if (!form.amount?.trim() || parseDec(form.amount) <= 0) {
      setError("Suma trebuie să fie pozitivă.");
      return;
    }
    if (!form.issueDate?.trim()) {
      setError("Data emiterii este obligatorie.");
      return;
    }
    const input: ReceiptInput = {
      ...form,
      amount: form.amount.trim(),
      payerName: form.payerName?.trim() || undefined,
      notes: form.notes?.trim() || undefined,
      series: form.series?.trim() || "CH",
      currency: form.currency || "RON",
      contactId: contact?.id ?? undefined,
      invoiceId: form.invoiceId?.trim() || undefined,
    };
    createMutation.mutate(input);
  };

  return (
    <div
      className="palette-scrim"
      style={{ alignItems: "center", paddingTop: 0 }}
      onClick={onClose}
    >
      <div
        style={{
          width: 440,
          background: "var(--bg-content)",
          border: "1px solid var(--border-strong)",
          boxShadow: "0 4px 24px rgba(0,0,0,0.12)",
          padding: "20px 24px 18px",
          maxHeight: "90vh",
          overflowY: "auto",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: 16,
          }}
        >
          <h3 style={{ fontSize: 14, fontWeight: 700, margin: 0 }}>Chitanță nouă</h3>
          <button type="button" className="btn-icon" onClick={onClose}>
            <Icon name="x" size={14} />
          </button>
        </div>

        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: 9 }}>

          <div style={{ display: "flex", gap: 9 }}>
            <MField label="Serie" style={{ flex: "0 0 80px" }}>
              <input
                className="field mono"
                value={form.series ?? "CH"}
                onChange={(e) => setForm((f) => ({ ...f, series: e.target.value }))}
                placeholder="CH"
              />
            </MField>
            <MField label="Data emiterii *" style={{ flex: 1 }}>
              <input
                className="field"
                type="date"
                value={form.issueDate}
                onChange={(e) => setForm((f) => ({ ...f, issueDate: e.target.value }))}
                required
              />
            </MField>
          </div>

          <div style={{ display: "flex", gap: 9 }}>
            <MField label="Sumă *" style={{ flex: 1 }}>
              <input
                className="field num"
                type="number"
                step="0.01"
                min="0.01"
                placeholder="0.00"
                value={form.amount}
                onChange={(e) => setForm((f) => ({ ...f, amount: e.target.value }))}
                autoFocus
              />
            </MField>
            <MField label="Monedă" style={{ flex: "0 0 90px" }}>
              <select
                className="field"
                value={form.currency ?? "RON"}
                onChange={(e) => setForm((f) => ({ ...f, currency: e.target.value }))}
              >
                <option value="RON">RON</option>
                <option value="EUR">EUR</option>
                <option value="USD">USD</option>
              </select>
            </MField>
          </div>

          <MField label="Plătitor (contact)">
            <ContactCombobox
              value={contact}
              onChange={setContact}
              companyId={companyId}
              placeholder="Caută plătitor (opțional)…"
              width="100%"
            />
          </MField>

          <MField label="Plătitor (text liber)">
            <input
              className="field"
              placeholder="Nume plătitor (dacă nu e în contacte)"
              value={form.payerName ?? ""}
              onChange={(e) => setForm((f) => ({ ...f, payerName: e.target.value }))}
            />
          </MField>

          <MField label="Nr. factură asociată (opțional)">
            <input
              className="field mono"
              placeholder="ex. FACT-0001"
              value={form.invoiceId ?? ""}
              onChange={(e) => setForm((f) => ({ ...f, invoiceId: e.target.value }))}
            />
          </MField>

          <MField label="Observații">
            <input
              className="field"
              placeholder="opțional"
              value={form.notes ?? ""}
              onChange={(e) => setForm((f) => ({ ...f, notes: e.target.value }))}
            />
          </MField>

          {error && (
            <div
              style={{
                padding: "6px 10px",
                background: "#FEE2E2",
                border: "1px solid #FECACA",
                fontSize: 11,
                color: "#991B1B",
              }}
            >
              {error}
            </div>
          )}

          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 4 }}>
            <button type="button" className="btn" onClick={onClose}>
              Anulează
            </button>
            <button
              type="submit"
              className="btn primary"
              disabled={createMutation.isPending}
            >
              {createMutation.isPending ? "Se salvează…" : "Emite chitanță"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

// ─── MField helper ─────────────────────────────────────────────────────────

function MField({
  label,
  children,
  style,
}: {
  label: string;
  children: React.ReactNode;
  style?: React.CSSProperties;
}) {
  const fieldId = useId();
  const child = isValidElement(children)
    ? cloneElement(children as React.ReactElement<{ id?: string }>, { id: fieldId })
    : children;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 3, ...style }}>
      <label
        htmlFor={fieldId}
        style={{ fontSize: 11, fontWeight: 600, color: "var(--text-muted)" }}
      >
        {label}
      </label>
      {child}
    </div>
  );
}
