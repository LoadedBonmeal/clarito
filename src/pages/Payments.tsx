/**
 * Urmărire plăți — per factură, status plată, adăugare/ștergere plăți.
 */

import { useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { AddPaymentArgs, Payment } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";

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

export function PaymentsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [filter, setFilter] = useState<PayFilter>("all");
  const [query, setQuery] = useState("");
  const [addModal, setAddModal] = useState<{ invoiceId: string; totalAmount: string } | null>(null);
  const [form, setForm] = useState({ amount: "", paidAt: new Date().toISOString().slice(0, 10), method: "transfer", reference: "" });

  // Fetch all invoices
  const { data: paged, isLoading } = useQuery({
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

  // Fetch payment summaries for all invoices — single batch query (replaces N+1)
  const { data: summariesArray = [], isError: summariesError, error: summariesErr, refetch: refetchSummaries } = useQuery({
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
    onError: (e) => notify.error("Eroare la adăugarea plății: " + String(e)),
  });

  const deleteMutation = useMutation({
    mutationFn: ({ paymentId }: { paymentId: string }) =>
      api.payments.delete(paymentId, activeCompanyId!),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.payments.summaries(activeCompanyId!) });
      notify.success("Plată ștearsă");
    },
    onError: (e) => notify.error("Eroare la ștergere: " + String(e)),
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

  if (!activeCompanyId) {
    return (
      <div className="content">
        <div className="content-titlebar"><span className="content-title">Urmărire Plăți</span></div>
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
          Urmărire Plăți
        </span>
        <span className="muted" style={{ fontSize: 11 }}>
          {list.length} facturi
        </span>
      </div>

      <div className="views-bar">
        {(["all", "UNPAID", "PARTIAL", "PAID", "OVERDUE"] as PayFilter[]).map((f) => {
          const labels: Record<PayFilter, string> = {
            all: "Toate", UNPAID: "Neplătite", PARTIAL: "Parțiale", PAID: "Plătite", OVERDUE: "Restanțe",
          };
          return (
            <span
              key={f}
              className={"view-tab " + (filter === f ? "active" : "")}
              onClick={() => setFilter(f)}
            >
              {labels[f]} <span className="count">{counts[f]}</span>
            </span>
          );
        })}
      </div>

      <div className="content-toolbar">
        <div className="search">
          <Icon name="search" size={13} />
          <input
            placeholder="Caută după nr. factură sau client…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
      </div>

      <div className="content-body" style={{ overflowY: "auto", flex: 1 }}>
        {isLoading ? (
          <div style={{ padding: 24, color: "var(--text-muted)" }}>Se încarcă…</div>
        ) : summariesError ? (
          <QueryErrorBanner error={summariesErr} label="plățile" onRetry={() => void refetchSummaries()} />
        ) : list.length === 0 ? (
          <div style={{ padding: 40, textAlign: "center", color: "var(--text-muted)" }}>
            Nicio factură corespunzătoare filtrelor selectate.
          </div>
        ) : (
          <table className="dt" style={{ width: "100%" }}>
            <thead>
              <tr>
                <th>Nr. Factură</th>
                <th>Client</th>
                <th>Dată emitere</th>
                <th>Scadență</th>
                <th className="num">Total (RON)</th>
                <th className="num">Plătit (RON)</th>
                <th className="num">Rest (RON)</th>
                <th>Status plată</th>
                <th style={{ width: 80 }}>Acțiuni</th>
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
                  <tr key={inv.id} className={overdue ? "row-error" : ""}>
                    <td className="mono">{inv.fullNumber}</td>
                    <td>{clientName}</td>
                    <td>{inv.issueDate}</td>
                    <td style={{ color: overdue ? "var(--st-rejected-fg)" : undefined }}>
                      {inv.dueDate ?? "—"}
                      {overdue && <span style={{ marginLeft: 4, fontSize: 10, fontWeight: 700 }}>RESTANȚĂ</span>}
                    </td>
                    <td className="num">{fmtRON(totalAmt)}</td>
                    <td className="num" style={{ color: paidAmt > 0 ? "var(--st-validated-fg)" : undefined }}>
                      {fmtRON(paidAmt)}
                    </td>
                    <td className="num" style={{ color: rest > 0 ? "var(--st-rejected-fg)" : undefined }}>
                      {fmtRON(rest)}
                    </td>
                    <td>
                      <StatusBadge status={payStatus} />
                    </td>
                    <td>
                      <button
                        className="btn compact"
                        disabled={payStatus === "PAID"}
                        onClick={() => {
                          setAddModal({ invoiceId: inv.id, totalAmount: inv.totalAmount });
                          setForm({ amount: rest.toFixed(2), paidAt: new Date().toISOString().slice(0, 10), method: "transfer", reference: "" });
                        }}
                        title="Adaugă plată"
                      >
                        <Icon name="plus" size={11} />
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {/* Add payment modal */}
      {addModal && (
        <div
          className="palette-scrim"
          style={{ alignItems: "center", paddingTop: 0 }}
          onClick={() => setAddModal(null)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{
              background: "var(--bg-content)",
              border: "1px solid var(--border)",
              minWidth: 360,
              maxWidth: 440,
              boxShadow: "0 8px 32px rgba(0,0,0,0.18)",
              padding: 20,
            }}
          >
            <div style={{ fontWeight: 700, fontSize: 13, marginBottom: 12 }}>
              Plăți factură
            </div>

            {/* Existing payments */}
            {(() => {
              const existing = summaryMap.get(addModal.invoiceId)?.payments ?? [];
              if (existing.length === 0) return null;
              return (
                <div style={{ marginBottom: 14 }}>
                  <div style={{ fontSize: 10, fontWeight: 600, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: 6 }}>
                    Plăți înregistrate
                  </div>
                  {existing.map((p) => (
                    <div key={p.id} style={{ display: "flex", alignItems: "center", gap: 8, padding: "4px 0", borderBottom: "1px solid var(--border-subtle, var(--border))", fontSize: 11 }}>
                      <span className="mono" style={{ flex: 1 }}>{p.paidAt}</span>
                      <span>{METHOD_LABELS[p.method] ?? p.method}</span>
                      {p.reference && <span style={{ color: "var(--text-muted)" }}>#{p.reference}</span>}
                      <span style={{ fontWeight: 600, minWidth: 70, textAlign: "right" }}>{p.amount} RON</span>
                      <button
                        className="btn compact"
                        title="Șterge plata"
                        disabled={deleteMutation.isPending}
                        onClick={() => deleteMutation.mutate({ paymentId: p.id })}
                        style={{ color: "var(--st-rejected-fg)" }}
                      >
                        <Icon name="trash" size={10} />
                      </button>
                    </div>
                  ))}
                </div>
              );
            })()}

            <div style={{ fontSize: 10, fontWeight: 600, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: 8 }}>
              Adaugă plată nouă
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              <label style={{ fontSize: 11 }}>
                Sumă (RON)
                <input
                  className="field"
                  style={{ display: "block", width: "100%", marginTop: 4 }}
                  type="number"
                  step="0.01"
                  min="0.01"
                  value={form.amount}
                  onChange={(e) => setForm((f) => ({ ...f, amount: e.target.value }))}
                />
              </label>
              <label style={{ fontSize: 11 }}>
                Data plății
                <input
                  className="field"
                  type="date"
                  style={{ display: "block", width: "100%", marginTop: 4 }}
                  value={form.paidAt}
                  onChange={(e) => setForm((f) => ({ ...f, paidAt: e.target.value }))}
                />
              </label>
              <label style={{ fontSize: 11 }}>
                Metodă de plată
                <select
                  className="field"
                  style={{ display: "block", width: "100%", marginTop: 4 }}
                  value={form.method}
                  onChange={(e) => setForm((f) => ({ ...f, method: e.target.value }))}
                >
                  {Object.entries(METHOD_LABELS).map(([v, l]) => (
                    <option key={v} value={v}>{l}</option>
                  ))}
                </select>
              </label>
              <label style={{ fontSize: 11 }}>
                Referință / nr. chitanță
                <input
                  className="field"
                  style={{ display: "block", width: "100%", marginTop: 4 }}
                  placeholder="opțional"
                  value={form.reference}
                  onChange={(e) => setForm((f) => ({ ...f, reference: e.target.value }))}
                />
              </label>
            </div>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 16 }}>
              <button className="btn" onClick={() => setAddModal(null)}>Anulează</button>
              <button
                className="btn primary"
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
                {addMutation.isPending ? "Se salvează…" : "Salvează plata"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
