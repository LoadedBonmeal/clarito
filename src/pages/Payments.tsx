/**
 * Urmărire plăți — re-skinned to rf kit (Wave 4).
 * Preserves: api.payments.listSummaries(activeCompanyId),
 * api.invoices.list, api.contacts.list for context,
 * stat cards (de încasat/restante/încasat),
 * filter (UNPAID/PARTIAL/PAID/OVERDUE),
 * "Adaugă plată" modal → api.payments.add(args),
 * delete payment → api.payments.delete(paymentId, companyId).
 */

import { useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import {
  PageHeader, Btn, IconBtn, Badge, Card, SectionCard, StatCard,
  Field, Input, Select, SearchInput, Segmented, Empty, Modal,
} from "@/components/rf";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { AddPaymentArgs, Payment } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";

type PayFilter = "all" | "UNPAID" | "PARTIAL" | "PAID" | "OVERDUE";

function isOverdue(dueDate: string | null | undefined, status: string): boolean {
  if (!dueDate || status === "PAID") return false;
  return new Date(dueDate) < new Date();
}

const METHOD_LABELS: Record<string, string> = {
  transfer: "Transfer bancar",
  cash: "Numerar",
  card: "Card",
  other: "Altele",
};

const FILTER_LABELS: Record<PayFilter, string> = {
  all: "Toate",
  UNPAID: "Neplătite",
  PARTIAL: "Parțiale",
  PAID: "Plătite",
  OVERDUE: "Restanțe",
};

export function PaymentsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [filter, setFilter] = useState<PayFilter>("all");
  const [query, setQuery] = useState("");
  const [addModal, setAddModal] = useState<{ invoiceId: string; totalAmount: string } | null>(null);
  const [form, setForm] = useState({
    amount: "",
    paidAt: new Date().toISOString().slice(0, 10),
    method: "transfer",
    reference: "",
  });

  // Fetch all invoices
  const {
    data: paged,
    isLoading,
    isError: invoicesError,
    error: invoicesErr,
    refetch: refetchInvoices,
  } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 500 } }),
    queryFn: () => api.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 500 } }),
    enabled: !!activeCompanyId,
  });

  // Fetch contacts for client names
  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const contactMap = useMemo(
    () => new Map(contacts.map((c) => [c.id, c.legalName])),
    [contacts],
  );

  const allInvoices = useMemo(() => paged?.items ?? [], [paged]);

  // Fetch payment summaries — single batch query
  const {
    data: summariesArray = [],
    isError: summariesError,
    error: summariesErr,
    refetch: refetchSummaries,
  } = useQuery({
    queryKey: queryKeys.payments.summaries(activeCompanyId!),
    queryFn: () => api.payments.listSummaries(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const summaryMap = useMemo(() => {
    const m = new Map<string, { paidAmount: string; paymentStatus: string; payments: Payment[] }>();
    for (const s of summariesArray) {
      m.set(s.invoiceId, { paidAmount: s.paidAmount, paymentStatus: s.paymentStatus, payments: s.payments });
    }
    return m;
  }, [summariesArray]);

  const addMutation = useMutation({
    mutationFn: (args: AddPaymentArgs) => api.payments.add(args),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.payments.summaries(activeCompanyId!) });
      notify.success("Plată adăugată cu succes");
      setAddModal(null);
      setForm({ amount: "", paidAt: new Date().toISOString().slice(0, 10), method: "transfer", reference: "" });
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut adăuga plata.")),
  });

  const deleteMutation = useMutation({
    mutationFn: ({ paymentId }: { paymentId: string }) =>
      api.payments.delete(paymentId, activeCompanyId!),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.payments.summaries(activeCompanyId!) });
      notify.success("Plată ștearsă");
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge plata.")),
  });

  // Filter + search
  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return allInvoices
      .filter((inv) => {
        if (filter === "all") return true;
        const summary = summaryMap.get(inv.id);
        const payStatus = summary?.paymentStatus ?? "UNPAID";
        if (filter === "OVERDUE") return isOverdue(inv.dueDate, payStatus);
        return payStatus === filter;
      })
      .filter((inv) => {
        if (!q) return true;
        const clientName = contactMap.get(inv.contactId) ?? "";
        return inv.fullNumber.toLowerCase().includes(q) || clientName.toLowerCase().includes(q);
      });
  }, [allInvoices, filter, query, summaryMap, contactMap]);

  const counts = useMemo(() => {
    const c = { all: allInvoices.length, UNPAID: 0, PARTIAL: 0, PAID: 0, OVERDUE: 0 };
    for (const inv of allInvoices) {
      const s = summaryMap.get(inv.id);
      const ps = s?.paymentStatus ?? "UNPAID";
      if (ps === "UNPAID") c.UNPAID++;
      else if (ps === "PARTIAL") c.PARTIAL++;
      else if (ps === "PAID") c.PAID++;
      if (isOverdue(inv.dueDate, ps)) c.OVERDUE++;
    }
    return c;
  }, [allInvoices, summaryMap]);

  // Stat computations
  const totalDue = useMemo(() => {
    let sum = 0;
    for (const inv of allInvoices) {
      const s = summaryMap.get(inv.id);
      const ps = s?.paymentStatus ?? "UNPAID";
      if (ps !== "PAID") {
        sum += Math.max(0, parseDec(inv.totalAmount) - parseDec(s?.paidAmount));
      }
    }
    return sum;
  }, [allInvoices, summaryMap]);

  const totalOverdue = useMemo(() => {
    let sum = 0;
    for (const inv of allInvoices) {
      const s = summaryMap.get(inv.id);
      const ps = s?.paymentStatus ?? "UNPAID";
      if (isOverdue(inv.dueDate, ps)) {
        sum += Math.max(0, parseDec(inv.totalAmount) - parseDec(s?.paidAmount));
      }
    }
    return sum;
  }, [allInvoices, summaryMap]);

  const totalPaid = useMemo(() => {
    let sum = 0;
    for (const s of summariesArray) sum += parseDec(s.paidAmount);
    return sum;
  }, [summariesArray]);

  if (!activeCompanyId) {
    return (
      <div className="rf-page">
        <PageHeader title="Urmărire Plăți" />
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

  const filterOptions = (["all", "UNPAID", "PARTIAL", "PAID", "OVERDUE"] as PayFilter[]).map((f) => ({
    value: f,
    label: `${FILTER_LABELS[f]} (${counts[f]})`,
  }));

  return (
    <div className="rf-page">
      <PageHeader
        title="Urmărire Plăți"
        sub={<Badge variant="neutral">{list.length} facturi</Badge>}
        actions={
          <Btn
            variant="primary"
            icon="plus"
            size="sm"
            onClick={() => {
              notify.info("Selectați o factură din tabel pentru a adăuga o plată.");
            }}
          >
            Adaugă plată
          </Btn>
        }
      />

      <div className="rf-page-body">
        {/* Stat cards */}
        <div className="rf-grid-3" style={{ marginBottom: 16 }}>
          <StatCard
            icon="wallet"
            label="De încasat"
            value={fmtRON(totalDue)}
            unit="RON"
            ctx={`${counts.UNPAID + counts.PARTIAL} facturi neachitate`}
          />
          <StatCard
            icon="alertTriangle"
            label="Restante"
            value={fmtRON(totalOverdue)}
            unit="RON"
            ctx={`${counts.OVERDUE} facturi depășite`}
          />
          <StatCard
            icon="check"
            label="Total încasat"
            value={fmtRON(totalPaid)}
            unit="RON"
            ctx={`${counts.PAID} facturi achitate`}
          />
        </div>

        <SectionCard icon="wallet" title="Stare plăți pe factură">
          {/* Toolbar */}
          <div className="rf-toolbar-row" style={{ padding: "10px 16px", borderBottom: "1px solid var(--rf-border)" }}>
            <SearchInput
              placeholder="Caută după nr. factură sau client…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              style={{ width: 300 }}
            />
            <Segmented options={filterOptions} value={filter} onChange={(v) => setFilter(v)} />
          </div>

          {/* Table */}
          <div className="rf-tbl-wrap">
            {isLoading ? (
              <Empty icon="wallet" title="Se încarcă…" />
            ) : invoicesError ? (
              <QueryErrorBanner error={invoicesErr} label="facturile" onRetry={() => void refetchInvoices()} />
            ) : summariesError ? (
              <QueryErrorBanner error={summariesErr} label="plățile" onRetry={() => void refetchSummaries()} />
            ) : list.length === 0 ? (
              <Empty icon="wallet" title="Nicio factură corespunzătoare filtrelor" />
            ) : (
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th>Nr. Factură</th>
                    <th>Client</th>
                    <th>Data emitere</th>
                    <th>Scadență</th>
                    <th className="rf-num">Total</th>
                    <th className="rf-num">Plătit</th>
                    <th className="rf-num">Rest</th>
                    <th>Status plată</th>
                    <th style={{ width: 60 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {list.map((inv) => {
                    const s = summaryMap.get(inv.id);
                    const paidAmt = parseDec(s?.paidAmount);
                    const totalAmt = parseDec(inv.totalAmount);
                    const rest = Math.max(0, totalAmt - paidAmt);
                    const payStatus = s?.paymentStatus ?? "UNPAID";
                    const overdue = isOverdue(inv.dueDate, payStatus);
                    const clientName = contactMap.get(inv.contactId) ?? "—";

                    return (
                      <tr key={inv.id}>
                        <td className="mono" style={{ fontWeight: 600 }}>{inv.fullNumber}</td>
                        <td style={{ fontWeight: 500 }}>{clientName}</td>
                        <td style={{ color: "var(--rf-text-muted)" }}>{inv.issueDate}</td>
                        <td style={{ color: overdue ? "var(--rf-error)" : "var(--rf-text-muted)" }}>
                          {inv.dueDate ?? "—"}
                          {overdue && (
                            <span style={{ marginLeft: 4, fontSize: 10, fontWeight: 700, color: "var(--rf-error)" }}>
                              RESTANȚĂ
                            </span>
                          )}
                        </td>
                        <td className="rf-num mono">{fmtRON(totalAmt)}</td>
                        <td
                          className="rf-num mono"
                          style={{ color: paidAmt > 0 ? "var(--rf-success)" : undefined }}
                        >
                          {fmtRON(paidAmt)}
                        </td>
                        <td
                          className="rf-num mono"
                          style={{ fontWeight: 600, color: rest > 0 ? "var(--rf-text)" : "var(--rf-text-dim)" }}
                        >
                          {fmtRON(rest)}
                        </td>
                        <td>
                          <StatusBadge status={payStatus} />
                        </td>
                        <td onClick={(e) => e.stopPropagation()}>
                          <IconBtn
                            icon="plus"
                            title="Adaugă plată"
                            disabled={payStatus === "PAID"}
                            onClick={() => {
                              setAddModal({ invoiceId: inv.id, totalAmount: inv.totalAmount });
                              setForm({
                                amount: rest.toFixed(2),
                                paidAt: new Date().toISOString().slice(0, 10),
                                method: "transfer",
                                reference: "",
                              });
                            }}
                          />
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
          </div>

          {/* Footer */}
          <div className="rf-tbl-footer">
            <span>Total: <b>{list.length}</b> facturi</span>
            <span>Neplătite: <b style={{ color: "var(--rf-warning)" }}>{counts.UNPAID}</b></span>
            <span>Restante: <b style={{ color: "var(--rf-error)" }}>{counts.OVERDUE}</b></span>
          </div>
        </SectionCard>
      </div>

      {/* Add payment modal */}
      {addModal && (
        <Modal
          open
          onOpenChange={(open) => { if (!open) setAddModal(null); }}
          title="Plăți factură"
          width={460}
          footer={
            <>
              <Btn variant="secondary" onClick={() => setAddModal(null)}>Anulează</Btn>
              <Btn
                variant="primary"
                icon="check"
                disabled={addMutation.isPending || !form.amount || !form.paidAt}
                onClick={() => {
                  if (!activeCompanyId) return;
                  addMutation.mutate({
                    invoiceId: addModal.invoiceId,
                    companyId: activeCompanyId,
                    amount: form.amount,
                    paidAt: form.paidAt,
                    method: form.method,
                    reference: form.reference || undefined,
                  });
                }}
              >
                {addMutation.isPending ? "Se salvează…" : "Înregistrează plata"}
              </Btn>
            </>
          }
        >
          {/* Existing payments */}
          {(() => {
            const existing = summaryMap.get(addModal.invoiceId)?.payments ?? [];
            if (existing.length === 0) return null;
            return (
              <div style={{ marginBottom: 16 }}>
                <div
                  style={{
                    fontSize: 11,
                    fontWeight: 600,
                    color: "var(--rf-text-muted)",
                    textTransform: "uppercase",
                    letterSpacing: "0.05em",
                    marginBottom: 8,
                  }}
                >
                  Plăți înregistrate
                </div>
                {existing.map((p: Payment) => (
                  <div
                    key={p.id}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      padding: "6px 0",
                      borderBottom: "1px solid var(--rf-border)",
                      fontSize: 12,
                    }}
                  >
                    <span className="mono" style={{ flex: 1 }}>{p.paidAt}</span>
                    <span>{METHOD_LABELS[p.method] ?? p.method}</span>
                    {p.reference && (
                      <span style={{ color: "var(--rf-text-muted)" }}>#{p.reference}</span>
                    )}
                    <span style={{ fontWeight: 600, minWidth: 80, textAlign: "right" }}>
                      {p.amount} RON
                    </span>
                    <IconBtn
                      icon="trash"
                      title="Șterge plata"
                      disabled={deleteMutation.isPending}
                      onClick={() => deleteMutation.mutate({ paymentId: p.id })}
                    />
                  </div>
                ))}
              </div>
            );
          })()}

          <div
            style={{
              fontSize: 11,
              fontWeight: 600,
              color: "var(--rf-text-muted)",
              textTransform: "uppercase",
              letterSpacing: "0.05em",
              marginBottom: 10,
            }}
          >
            Adaugă plată nouă
          </div>

          <div className="rf-grid-2">
            <Field label="Sumă (RON)" required>
              <Input
                type="number"
                step="0.01"
                min="0.01"
                placeholder="0.00"
                value={form.amount}
                onChange={(e) => setForm((f) => ({ ...f, amount: e.target.value }))}
              />
            </Field>
            <Field label="Data plății">
              <Input
                type="date"
                value={form.paidAt}
                onChange={(e) => setForm((f) => ({ ...f, paidAt: e.target.value }))}
              />
            </Field>
            <Field label="Metodă de plată">
              <Select
                value={form.method}
                onChange={(e) => setForm((f) => ({ ...f, method: e.target.value }))}
              >
                {Object.entries(METHOD_LABELS).map(([v, l]) => (
                  <option key={v} value={v}>{l}</option>
                ))}
              </Select>
            </Field>
            <Field label="Referință / nr. chitanță">
              <Input
                placeholder="opțional"
                value={form.reference}
                onChange={(e) => setForm((f) => ({ ...f, reference: e.target.value }))}
              />
            </Field>
          </div>
        </Modal>
      )}
    </div>
  );
}

