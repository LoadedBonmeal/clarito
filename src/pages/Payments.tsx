/**
 * Urmărire plăți — re-skinned to rf kit (Wave 4).
 * Preserves: api.payments.listSummaries(activeCompanyId),
 * api.invoices.list, api.contacts.list for context,
 * stat cards (de încasat/restante/încasat),
 * filter (UNPAID/PARTIAL/PAID/OVERDUE),
 * "Adaugă plată" modal → api.payments.add(args),
 * delete payment → api.payments.delete(paymentId, companyId).
 */

import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import {
  PageHeader, Btn, IconBtn, Badge, Card, SectionCard, StatCard,
  Field, Input, Select, SearchInput, Segmented, Empty, Modal,
} from "@/components/rf";
import { Icon } from "@/components/shared/Icon";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { AddPaymentArgs, Payment } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Invoice } from "@/types";

type PayFilter = "all" | "UNPAID" | "PARTIAL" | "PAID" | "OVERDUE";

function isOverdue(dueDate: string | null | undefined, status: string): boolean {
  if (!dueDate || status === "PAID") return false;
  // Compare DATE-ONLY in local time to avoid UTC-midnight mis-flagging in EET
  // (e.g. 2026-06-15 parsed as UTC becomes 2026-06-14T22:00 EET → wrongly overdue).
  const today = new Date();
  const todayISO = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
  return dueDate < todayISO;
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
  // addModal.invoiceId === "" means opened from header — user must pick an invoice first.
  const [addModal, setAddModal] = useState<{ invoiceId: string; totalAmount: string; currency: string } | null>(null);
  // Invoice picked via the header-triggered combobox (only used when addModal.invoiceId is "").
  const [pickedInvoice, setPickedInvoice] = useState<Invoice | null>(null);
  const [form, setForm] = useState({
    amount: "",
    paidAt: new Date().toISOString().slice(0, 10),
    method: "transfer",
    reference: "",
    exchangeRate: "",
  });

  // Fetch all invoices
  const {
    data: paged,
    isLoading,
    isError: invoicesError,
    error: invoicesErr,
    refetch: refetchInvoices,
  } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    queryFn: () => api.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
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
      void queryClient.invalidateQueries({ queryKey: ["payments", "summary"] });
      notify.success("Plată adăugată cu succes");
      setAddModal(null);
      setForm({ amount: "", paidAt: new Date().toISOString().slice(0, 10), method: "transfer", reference: "", exchangeRate: "" });
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut adăuga plata.")),
  });

  const deleteMutation = useMutation({
    mutationFn: ({ paymentId }: { paymentId: string }) =>
      api.payments.delete(paymentId, activeCompanyId!),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.payments.summaries(activeCompanyId!) });
      void queryClient.invalidateQueries({ queryKey: ["payments", "summary"] });
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
              setPickedInvoice(null);
              setAddModal({ invoiceId: "", totalAmount: "", currency: "RON" });
              setForm({ amount: "", paidAt: new Date().toISOString().slice(0, 10), method: "transfer", reference: "", exchangeRate: "" });
            }}
          >
            Adaugă plată
          </Btn>
        }
      />

      {/* Truncation warning */}
      {paged && paged.total > paged.items.length && (
        <div
          style={{
            padding: "6px 32px",
            background: "var(--rf-warning-bg, #fffbeb)",
            borderBottom: "1px solid var(--rf-border)",
            fontSize: 12,
            color: "var(--rf-warning, #92400e)",
          }}
        >
          Afișate primele {paged.items.length.toLocaleString("ro-RO")} din {paged.total.toLocaleString("ro-RO")} facturi — restrânge filtrele pentru a vedea toate înregistrările.
        </div>
      )}

      <div className="rf-page-body">
        {/* Stat cards */}
        <div className="rf-grid-3">
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
                              setAddModal({ invoiceId: inv.id, totalAmount: inv.totalAmount, currency: inv.currency ?? "RON" });
                              setForm({
                                amount: rest.toFixed(2),
                                paidAt: new Date().toISOString().slice(0, 10),
                                method: "transfer",
                                reference: "",
                                exchangeRate: "",
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
          onOpenChange={(open) => { if (!open) { setAddModal(null); setPickedInvoice(null); } }}
          title="Plăți factură"
          width={460}
          footer={
            <>
              <Btn variant="secondary" onClick={() => { setAddModal(null); setPickedInvoice(null); }}>Anulează</Btn>
              <Btn
                variant="primary"
                icon="check"
                disabled={
                  addMutation.isPending ||
                  !form.amount ||
                  !form.paidAt ||
                  // When opened from header, an invoice must be picked first.
                  (addModal.invoiceId === "" && pickedInvoice === null)
                }
                onClick={() => {
                  if (!activeCompanyId) return;
                  // Resolve the invoiceId: row-level sets it directly; header flow uses pickedInvoice.
                  const resolvedInvoiceId = addModal.invoiceId || pickedInvoice?.id;
                  if (!resolvedInvoiceId) return;
                  const rate = parseFloat(form.exchangeRate);
                  addMutation.mutate({
                    invoiceId: resolvedInvoiceId,
                    companyId: activeCompanyId,
                    amount: form.amount,
                    paidAt: form.paidAt,
                    method: form.method,
                    reference: form.reference || undefined,
                    exchangeRate: Number.isFinite(rate) && rate > 0 ? rate : undefined,
                  });
                }}
              >
                {addMutation.isPending ? "Se salvează…" : "Înregistrează plata"}
              </Btn>
            </>
          }
        >
          {/* Invoice picker — only shown when modal was opened from the header button */}
          {addModal.invoiceId === "" && activeCompanyId && (
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
                Selectează factura
              </div>
              <InvoicePickerCombobox
                companyId={activeCompanyId}
                value={pickedInvoice}
                onChange={(inv) => {
                  setPickedInvoice(inv);
                  if (inv) {
                    const s = summaryMap.get(inv.id);
                    const paid = parseDec(s?.paidAmount ?? "0");
                    const rest = Math.max(0, parseDec(inv.totalAmount) - paid);
                    setForm((f) => ({ ...f, amount: rest > 0 ? rest.toFixed(2) : "" }));
                  } else {
                    setForm((f) => ({ ...f, amount: "" }));
                  }
                }}
              />
            </div>
          )}

          {/* Existing payments */}
          {(() => {
            const invoiceIdForPayments = addModal.invoiceId || pickedInvoice?.id || "";
            const existing = summaryMap.get(invoiceIdForPayments)?.payments ?? [];
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
                      {p.amount} {pickedInvoice?.currency ?? addModal.currency}
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
            <Field label={`Sumă (${pickedInvoice?.currency ?? addModal.currency})`} required>
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
            {(pickedInvoice?.currency ?? addModal.currency) !== "RON" && (
              <Field label="Curs BNR la data plății (dif. de curs 665/765)">
                <Input
                  type="number"
                  step="0.0001"
                  min="0"
                  placeholder="ex. 4.9750"
                  value={form.exchangeRate}
                  onChange={(e) => setForm((f) => ({ ...f, exchangeRate: e.target.value }))}
                />
              </Field>
            )}
          </div>
        </Modal>
      )}
    </div>
  );
}

// ─── InvoicePickerCombobox ─────────────────────────────────────────────────
// Inline combobox for picking an invoice from the active company.
// Used by the header-level "Adaugă plată" button so users can select which
// invoice to record a payment against. Mirrors the InvoiceCombobox in Receipts.tsx.

function InvoicePickerCombobox({
  companyId,
  value,
  onChange,
}: {
  companyId: string;
  value: Invoice | null;
  onChange: (inv: Invoice | null) => void;
}) {
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listboxId = useId();

  useEffect(() => {
    const t = setTimeout(() => setDebouncedQuery(query.trim()), 250);
    return () => clearTimeout(t);
  }, [query]);

  const { data: page, isFetching } = useQuery({
    queryKey: ["invoices", "payments-picker", companyId, debouncedQuery],
    queryFn: () =>
      api.invoices.list({
        companyId,
        query: debouncedQuery || undefined,
        page: { offset: 0, limit: 30 },
      }),
    enabled: open && !!companyId,
    staleTime: 30_000,
  });

  const results: Invoice[] = page?.items ?? [];

  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, []);

  useEffect(() => {
    setHighlight(0);
  }, [results.length]);

  const handleSelect = (inv: Invoice) => {
    onChange(inv);
    setQuery("");
    setOpen(false);
    inputRef.current?.blur();
  };

  const handleClear = () => {
    onChange(null);
    setQuery("");
    setDebouncedQuery("");
    requestAnimationFrame(() => inputRef.current?.focus());
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (!open) {
      if (e.key === "ArrowDown" || e.key === "Enter") {
        e.preventDefault();
        setOpen(true);
      }
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setHighlight((h) => Math.min(h + 1, Math.max(results.length - 1, 0)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setHighlight((h) => Math.max(h - 1, 0));
    } else if (e.key === "Enter") {
      if (results[highlight]) {
        e.preventDefault();
        handleSelect(results[highlight]);
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
    }
  };

  if (value) {
    return (
      <div
        ref={containerRef}
        style={{
          position: "relative",
          display: "inline-flex",
          alignItems: "center",
          gap: 8,
          width: "100%",
          minHeight: 40,
          padding: "4px 8px 4px 12px",
          border: "1px solid var(--rf-border-strong)",
          background: "var(--rf-content)",
          borderRadius: "var(--rf-radius-sm)",
        }}
      >
        <div style={{ flex: 1, minWidth: 0, lineHeight: 1.25 }}>
          <div
            className="mono"
            style={{
              fontSize: 13,
              fontWeight: 600,
              color: "var(--rf-accent)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {value.fullNumber}
          </div>
          <div style={{ fontSize: 11, color: "var(--rf-text-muted)" }}>
            {value.issueDate} · {value.totalAmount} {value.currency ?? "RON"}
          </div>
        </div>
        <button
          type="button"
          onClick={handleClear}
          className="rf-icon-btn rf-icon-btn--ghost"
          style={{ width: 26, height: 26 }}
          aria-label="Elimină factura selectată"
          title="Elimină factura selectată"
        >
          <Icon name="x" size={12} />
        </button>
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      style={{ position: "relative", display: "inline-block", width: "100%" }}
    >
      <input
        ref={inputRef}
        id={listboxId + "-input"}
        className="rf-input"
        type="text"
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={onKeyDown}
        placeholder="Caută factură (număr sau client)…"
        autoComplete="off"
        aria-autocomplete="list"
        aria-expanded={open}
        aria-controls={listboxId}
        role="combobox"
        style={{ width: "100%" }}
      />
      {open && (
        <div
          id={listboxId}
          role="listbox"
          style={{
            position: "absolute",
            top: "calc(100% + 4px)",
            left: 0,
            right: 0,
            zIndex: 50,
            background: "var(--rf-content)",
            border: "1px solid var(--rf-border-strong)",
            borderRadius: "var(--rf-radius-sm)",
            boxShadow: "var(--rf-shadow-md)",
            maxHeight: 240,
            overflowY: "auto",
          }}
        >
          {isFetching ? (
            <div style={{ padding: "10px 12px", fontSize: 12, color: "var(--rf-text-muted)" }}>
              Se caută…
            </div>
          ) : results.length === 0 ? (
            <div style={{ padding: "10px 12px", fontSize: 12, color: "var(--rf-text-muted)" }}>
              {debouncedQuery ? `Nicio factură pentru „${debouncedQuery}".` : "Nicio factură găsită."}
            </div>
          ) : (
            results.map((inv, idx) => (
              <div
                key={inv.id}
                role="option"
                aria-selected={idx === highlight}
                onMouseDown={(e) => { e.preventDefault(); handleSelect(inv); }}
                onMouseEnter={() => setHighlight(idx)}
                style={{
                  padding: "8px 12px",
                  cursor: "pointer",
                  background: idx === highlight ? "var(--rf-accent-tint)" : "transparent",
                  display: "flex",
                  flexDirection: "column",
                  gap: 2,
                }}
              >
                <span className="mono" style={{ fontSize: 13, fontWeight: 600, color: "var(--rf-accent)" }}>
                  {inv.fullNumber}
                </span>
                <span style={{ fontSize: 11, color: "var(--rf-text-muted)" }}>
                  {inv.issueDate} · {inv.totalAmount} {inv.currency ?? "RON"}
                </span>
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
}

