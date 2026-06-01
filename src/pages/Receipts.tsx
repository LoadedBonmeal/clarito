/**
 * Chitanțe (cash receipts) — re-skinned to rf kit (Wave 4).
 * Preserves: api.receipts.list(activeCompanyId) (company guard),
 * "Chitanță nouă" modal → api.receipts.create(companyId, input)
 * (serie/data/sumă/monedă/plătitor contact|text/factură asociată/observații),
 * Generează/Vizualizare PDF → api.receipts.generatePdf(id, companyId) + openPath,
 * delete → api.receipts.delete(id, companyId) with confirm.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Icon } from "@/components/shared/Icon";
import { ContactCombobox } from "@/components/shared/ContactCombobox";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import {
  PageHeader, Btn, IconBtn, Badge, Card, Field, Input, Select, Empty, Modal,
} from "@/components/rf";
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
      <div className="rf-page">
        <PageHeader title="Chitanțe" />
        <div className="rf-page-body">
          <Card pad>
            <p style={{ textAlign: "center", color: "var(--rf-text-muted)", padding: "32px 0" }}>
              Selectați o companie activă pentru a vedea chitanțele.
            </p>
          </Card>
        </div>
      </div>
    );
  }

  return (
    <div className="rf-page">
      <PageHeader
        title="Chitanțe"
        sub={<Badge variant="neutral">{receiptList.length} chitanțe</Badge>}
        actions={
          <>
            <IconBtn
              icon="refresh"
              title="Reîmprospătează"
              onClick={() => void queryClient.invalidateQueries({ queryKey: queryKeys.receipts.all })}
            />
            <Btn
              variant="primary"
              icon="plus"
              size="sm"
              onClick={() => setShowModal(true)}
            >
              Chitanță nouă
            </Btn>
          </>
        }
      />

      <div className="rf-page-body">
        <Card>
          <div className="rf-tbl-wrap">
            {isLoading ? (
              <Empty icon="file" title="Se încarcă…" />
            ) : isError ? (
              <QueryErrorBanner
                error={error}
                label="chitanțele"
                onRetry={() => void refetch()}
              />
            ) : receiptList.length === 0 ? (
              <Empty icon="file" title="Nicio chitanță">
                Apăsați „Chitanță nouă" pentru a emite prima chitanță.
              </Empty>
            ) : (
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th>Număr</th>
                    <th>Data</th>
                    <th>Plătitor</th>
                    <th>Factură asociată</th>
                    <th className="rf-num">Sumă</th>
                    <th>Monedă</th>
                    <th style={{ width: 90 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {receiptList.map((r: Receipt) => (
                    <tr key={r.id}>
                      <td className="mono" style={{ fontWeight: 600 }}>{r.series}-{r.number}</td>
                      <td style={{ color: "var(--rf-text-muted)" }}>{r.issueDate}</td>
                      <td style={{ fontWeight: 500 }}>{r.payerName ?? <span className="rf-dim">—</span>}</td>
                      <td className="mono">
                        {r.invoiceId
                          ? <span title={r.invoiceId} style={{ color: "var(--rf-accent)", cursor: "default" }}>factură</span>
                          : <span className="rf-dim">—</span>}
                      </td>
                      <td className="rf-num mono" style={{ fontWeight: 600 }}>{fmtRON(parseDec(r.amount))}</td>
                      <td className="mono" style={{ color: "var(--rf-text-muted)" }}>{r.currency}</td>
                      <td onClick={(e) => e.stopPropagation()}>
                        <div className="rf-cell-actions">
                          <IconBtn
                            icon="file"
                            title="Generează PDF"
                            disabled={pdfMutation.isPending}
                            onClick={() => pdfMutation.mutate(r.id)}
                          />
                          <IconBtn
                            icon="trash"
                            title="Șterge"
                            onClick={() => void handleDelete(r)}
                          />
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>

          {/* Footer */}
          <div className="rf-tbl-footer">
            <span>Total: <b>{receiptList.length}</b> chitanțe</span>
          </div>
        </Card>
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
  const [formError, setFormError] = useState<string | null>(null);

  const createMutation = useMutation({
    mutationFn: (input: ReceiptInput) => api.receipts.create(companyId, input),
    onSuccess: () => {
      notify.success("Chitanță emisă.");
      onSaved();
    },
    onError: (e) => setFormError(formatError(e, "Eroare la emitere.")),
  });

  const handleSubmit = () => {
    if (createMutation.isPending) return;
    setFormError(null);
    if (!form.amount?.trim() || parseDec(form.amount) <= 0) {
      setFormError("Suma trebuie să fie pozitivă.");
      return;
    }
    if (!form.issueDate?.trim()) {
      setFormError("Data emiterii este obligatorie.");
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
    <Modal
      open
      onOpenChange={(open) => { if (!open) onClose(); }}
      title="Chitanță nouă"
      width={480}
      footer={
        <>
          <Btn variant="secondary" onClick={onClose} disabled={createMutation.isPending}>
            Anulează
          </Btn>
          <Btn
            variant="primary"
            icon="file"
            disabled={createMutation.isPending}
            onClick={handleSubmit}
          >
            {createMutation.isPending ? "Se salvează…" : "Emite chitanță"}
          </Btn>
        </>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        {/* Serie + Data */}
        <div className="rf-grid-2">
          <Field label="Serie">
            <Input
              className="mono"
              value={form.series ?? "CH"}
              onChange={(e) => setForm((f) => ({ ...f, series: e.target.value }))}
              placeholder="CH"
            />
          </Field>
          <Field label="Data emiterii" required>
            <Input
              type="date"
              value={form.issueDate}
              onChange={(e) => setForm((f) => ({ ...f, issueDate: e.target.value }))}
            />
          </Field>
        </div>

        {/* Sumă + Monedă */}
        <div className="rf-grid-2">
          <Field label="Sumă" required>
            <Input
              type="number"
              step="0.01"
              min="0.01"
              placeholder="0.00"
              value={form.amount}
              onChange={(e) => setForm((f) => ({ ...f, amount: e.target.value }))}
              autoFocus
            />
          </Field>
          <Field label="Monedă">
            <Select
              value={form.currency ?? "RON"}
              onChange={(e) => setForm((f) => ({ ...f, currency: e.target.value }))}
            >
              <option value="RON">RON</option>
              <option value="EUR">EUR</option>
              <option value="USD">USD</option>
            </Select>
          </Field>
        </div>

        {/* Plătitor (contact) */}
        <Field label="Plătitor (contact)">
          <ContactCombobox
            value={contact}
            onChange={setContact}
            companyId={companyId}
            placeholder="Caută plătitor (opțional)…"
            width="100%"
          />
        </Field>

        {/* Plătitor (text liber) */}
        <Field label="Plătitor (text liber)">
          <Input
            placeholder="Nume plătitor (dacă nu e în contacte)"
            value={form.payerName ?? ""}
            onChange={(e) => setForm((f) => ({ ...f, payerName: e.target.value }))}
          />
        </Field>

        {/* Factură asociată */}
        <Field label="Nr. factură asociată (opțional)">
          <Input
            className="mono"
            placeholder="ex. FACT-0001 — ID intern, verificat server-side"
            value={form.invoiceId ?? ""}
            onChange={(e) => setForm((f) => ({ ...f, invoiceId: e.target.value || undefined }))}
          />
        </Field>

        {/* Observații */}
        <Field label="Observații">
          <Input
            placeholder="opțional"
            value={form.notes ?? ""}
            onChange={(e) => setForm((f) => ({ ...f, notes: e.target.value }))}
          />
        </Field>

        {formError && (
          <div className="rf-banner rf-banner--error">
            <Icon name="xCircle" size={16} />
            <span>{formError}</span>
          </div>
        )}
      </div>
    </Modal>
  );
}
