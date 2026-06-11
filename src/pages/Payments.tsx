/**
 * Urmărire plăți — verbatim port of the design "Urmarire plati.html":
 *   .page-head (title + "Stare plăți pe factură · companie" sub + btn-dark
 *   "Adaugă plată") → .kpis (real stat cards the prototype lacks) → .scr-card →
 *   .scr-toolbar (.tt · .tabs Toate/Neplătite/Parțiale/Plătite/Restanțe ·
 *   .scr-search) → .scr-table (nr. factură link · .cli client · date ·
 *   total/plătit/rest · status chip · .row-acts +/eye) →
 *   .modal-back/.modal "Adaugă plată" (RON + valută cu diferență de curs 665/765).
 *
 * ALL wiring preserved: api.invoices.list, api.contacts.list,
 * api.payments.listSummaries, add → api.payments.add(args) (incl. exchangeRate),
 * delete → api.payments.delete(paymentId, companyId), header-flow
 * InvoicePickerCombobox, filter UNPAID/PARTIAL/PAID/OVERDUE, isOverdue
 * (date-only local), api.bnr.fetchRate for the FX rate.
 */

import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { AddPaymentArgs, Payment } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Invoice } from "@/types";

type PayFilter = "all" | "UNPAID" | "PARTIAL" | "PAID" | "OVERDUE";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string | null | undefined) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

/** Render at most this many rows (plain table, no virtualizer — design parity). */
const MAX_ROWS = 1000;

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

/** "Giannis Auto SRL" → "GA" (prototype .cli-ava initials). */
function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "—";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[1][0]).toUpperCase();
}

/** fmtRON + currency suffix for non-RON rows (prototype: "2.000,00 EUR"). */
const fmtAmt = (v: number, cur: string) => (cur && cur !== "RON" ? `${fmtRON(v)} ${cur}` : fmtRON(v));

/** ro-RO rate display: 5.0712 → "5,0712". */
const fmtRate = (r: number) =>
  r.toLocaleString("ro-RO", { minimumFractionDigits: 4, maximumFractionDigits: 4 });

// Inline SVG paths (icons not in Ic): warning triangle (chip Restanță),
// circle-check (chip Plătită), trending-up (FX banner), trash (șterge plata).
const WARN_PATH =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';
const CIRCLE_CHECK_PATH =
  '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>';
const TREND_PATH =
  '<path d="M2.25 18 9 11.25l4.306 4.306a11.95 11.95 0 0 1 5.814-5.518l2.74-1.22m0 0-5.94-2.281m5.94 2.28-2.28 5.941"/>';
const TRASH_PATH =
  '<path d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>';

// Payment status → design chip (.chip variants, prototype labels + diacritics).
function payChip(payStatus: string, overdue: boolean) {
  if (overdue)
    return (
      <span className="chip late">
        <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_PATH }} />
        Restanță
      </span>
    );
  if (payStatus === "PAID")
    return (
      <span className="chip paid">
        <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: CIRCLE_CHECK_PATH }} />
        Plătită
      </span>
    );
  if (payStatus === "PARTIAL")
    return (
      <span className="chip wait">
        <Ic name="clock" cls="sic" />
        Parțial
      </span>
    );
  return (
    <span className="chip sent">
      <Ic name="dot" cls="sic" />
      Neplătită
    </span>
  );
}

export function PaymentsPage() {
  const navigate = useNavigate();
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
  const [bnrLoading, setBnrLoading] = useState(false);

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

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });
  const activeCompany = companies.find((c) => c.id === activeCompanyId);

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
      setPickedInvoice(null);
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

  const tabs: Array<{ value: PayFilter; label: string; count: number }> = [
    { value: "all",     label: "Toate",     count: counts.all },
    { value: "UNPAID",  label: "Neplătite", count: counts.UNPAID },
    { value: "PARTIAL", label: "Parțiale",  count: counts.PARTIAL },
    { value: "PAID",    label: "Plătite",   count: counts.PAID },
    { value: "OVERDUE", label: "Restanțe",  count: counts.OVERDUE },
  ];

  const visibleRows = list.slice(0, MAX_ROWS);

  // The invoice the modal operates on: row flow stores the id, header flow uses the picker.
  const modalInvoice: Invoice | null = addModal
    ? (addModal.invoiceId
        ? allInvoices.find((i) => i.id === addModal.invoiceId) ?? null
        : pickedInvoice)
    : null;
  const modalSummary = modalInvoice ? summaryMap.get(modalInvoice.id) : undefined;
  const modalRest = modalInvoice
    ? Math.max(0, parseDec(modalInvoice.totalAmount) - parseDec(modalSummary?.paidAmount))
    : 0;
  const modalCurrency = modalInvoice?.currency ?? addModal?.currency ?? "RON";
  const modalIsFx = modalCurrency !== "RON";

  // FX math for the banner (prototype payModalFx): suma × (curs plată − curs emitere).
  const fxAmount = parseFloat(form.amount);
  const fxRate = parseFloat(form.exchangeRate);
  const fxIssueRate = modalInvoice?.exchangeRate ?? null;
  const fxRonEquiv = Number.isFinite(fxAmount) && Number.isFinite(fxRate) && fxRate > 0 ? fxAmount * fxRate : null;
  const fxDiff =
    modalIsFx && fxIssueRate !== null && fxIssueRate > 0 && Number.isFinite(fxAmount) && Number.isFinite(fxRate) && fxRate > 0
      ? fxAmount * (fxRate - fxIssueRate)
      : null;

  function openModalFor(inv: Invoice, rest: number) {
    setPickedInvoice(null);
    setAddModal({ invoiceId: inv.id, totalAmount: inv.totalAmount, currency: inv.currency ?? "RON" });
    setForm({
      amount: rest > 0 ? rest.toFixed(2) : "",
      paidAt: new Date().toISOString().slice(0, 10),
      method: "transfer",
      reference: "",
      exchangeRate: "",
    });
  }

  function closeModal() {
    setAddModal(null);
    setPickedInvoice(null);
  }

  async function handleFetchBnr() {
    if (!modalIsFx || !form.paidAt) return;
    setBnrLoading(true);
    try {
      const rate = await api.bnr.fetchRate(modalCurrency, form.paidAt);
      setForm((f) => ({ ...f, exchangeRate: String(rate) }));
    } catch (e) {
      notify.error(formatError(e, "Nu s-a putut prelua cursul BNR."));
    }
    setBnrLoading(false);
  }

  function handleSave() {
    if (!activeCompanyId || !addModal) return;
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
  }

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>Urmărire Plăți</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Selectați o companie activă pentru a urmări plățile.
        </div>
      </div>
    );
  }

  const existingPayments = modalInvoice ? (summaryMap.get(modalInvoice.id)?.payments ?? []) : [];
  const saveDisabled =
    addMutation.isPending ||
    !form.amount ||
    !form.paidAt ||
    // When opened from header, an invoice must be picked first.
    (addModal !== null && addModal.invoiceId === "" && pickedInvoice === null);

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Urmărire Plăți</h1>
          <p className="sub">
            Stare plăți pe factură
            {activeCompany ? ` · ${activeCompany.legalName}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button
            className="btn-dark"
            onClick={() => {
              setPickedInvoice(null);
              setAddModal({ invoiceId: "", totalAmount: "", currency: "RON" });
              setForm({ amount: "", paidAt: new Date().toISOString().slice(0, 10), method: "transfer", reference: "", exchangeRate: "" });
            }}
          >
            <Ic name="plus" />Adaugă plată
          </button>
        </div>
      </div>

      {/* stat cards — real functionality kept (prototype lacks them), design .kpi */}
      <div className="kpis" style={{ gridTemplateColumns: "repeat(3,1fr)" }}>
        <div className="kpi">
          <div className="top"><span className="klabel">De încasat</span><Ic name="incasat" /></div>
          <div className="val num">{fmtRON(totalDue)}<span className="cur">RON</span></div>
          <div className="delta">{counts.UNPAID + counts.PARTIAL} facturi neachitate</div>
        </div>
        <div className="kpi">
          <div className="top"><span className="klabel">Restante</span><Ic name="clock" /></div>
          <div className="val num" style={totalOverdue > 0 ? { color: "var(--red)" } : undefined}>
            {fmtRON(totalOverdue)}<span className="cur">RON</span>
          </div>
          <div className="delta">{counts.OVERDUE} facturi depășite</div>
        </div>
        <div className="kpi">
          <div className="top"><span className="klabel">Total încasat</span><Ic name="check" /></div>
          <div className="val num">{fmtRON(totalPaid)}<span className="cur">RON</span></div>
          <div className="delta">{counts.PAID} facturi achitate</div>
        </div>
      </div>

      <div className="scr-card pg-payments">
        {/* toolbar */}
        <div className="scr-toolbar">
          <div className="tt">Stare plăți pe factură</div>
          <div className="tabs">
            {tabs.map((t) => (
              <div
                key={t.value}
                className={`tab${filter === t.value ? " active" : ""}`}
                onClick={() => setFilter(t.value)}
              >
                {t.label}<span className="cnt num">{t.count}</span>
              </div>
            ))}
          </div>
          <div className="spacer" />
          <div className="scr-search" style={{ width: 200 }}>
            <Ic name="lens" />
            <input
              type="text"
              placeholder="Caută client sau factură…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
        </div>

        {/* truncation note */}
        {paged && paged.total > paged.items.length && (
          <div style={{ padding: "6px 16px", borderBottom: "1px solid var(--line)", fontSize: 12, color: "var(--amber)" }}>
            Afișate primele {paged.items.length.toLocaleString("ro-RO")} din {paged.total.toLocaleString("ro-RO")} facturi.
          </div>
        )}

        {/* table */}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
        ) : invoicesError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={invoicesErr} label="facturile" onRetry={() => void refetchInvoices()} />
          </div>
        ) : summariesError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={summariesErr} label="plățile" onRetry={() => void refetchSummaries()} />
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {allInvoices.length === 0
              ? "Nicio factură emisă — plățile apar aici după emiterea facturilor."
              : "Nicio înregistrare pentru filtrele aplicate."}
          </div>
        ) : (
          <>
            <table className="scr-table">
              <thead>
                <tr>
                  <th>Nr. Factură</th>
                  <th>Client</th>
                  <th>Data emitere</th>
                  <th>Scadență</th>
                  <th className="r">Total</th>
                  <th className="r">Plătit</th>
                  <th className="r">Rest</th>
                  <th>Status plată</th>
                  <th className="r" style={{ width: 60 }}></th>
                </tr>
              </thead>
              <tbody>
                {visibleRows.map((inv) => {
                  const s = summaryMap.get(inv.id);
                  const paidAmt = parseDec(s?.paidAmount);
                  const totalAmt = parseDec(inv.totalAmount);
                  const rest = Math.max(0, totalAmt - paidAmt);
                  const payStatus = s?.paymentStatus ?? "UNPAID";
                  const overdue = isOverdue(inv.dueDate, payStatus);
                  const clientName = contactMap.get(inv.contactId) ?? "—";
                  const cur = inv.currency ?? "RON";

                  return (
                    <tr key={inv.id}>
                      <td>
                        <a
                          className="link"
                          style={{ fontFamily: "var(--mono)", fontSize: 12, fontWeight: 700, cursor: "pointer" }}
                          onClick={() => void navigate({ to: "/invoices/$id", params: { id: inv.id } })}
                        >
                          {inv.fullNumber}
                        </a>
                      </td>
                      <td><div className="cli"><span className="cli-ava">{initials(clientName)}</span>{clientName}</div></td>
                      <td className="num">{fmtRoDate(inv.issueDate)}</td>
                      <td className="num">{fmtRoDate(inv.dueDate)}</td>
                      <td className="r num">{fmtAmt(totalAmt, cur)}</td>
                      <td className="r num">{fmtRON(paidAmt)}</td>
                      <td className="r num"><b>{fmtAmt(rest, cur)}</b></td>
                      <td>{payChip(payStatus, overdue)}</td>
                      <td>
                        <div className="row-acts">
                          {payStatus === "PAID" ? (
                            <button
                              className="mini-btn"
                              title="Vezi plățile"
                              onClick={() => openModalFor(inv, rest)}
                            >
                              <Ic name="eye" />
                            </button>
                          ) : (
                            <button
                              className="mini-btn"
                              title="Adaugă plată"
                              onClick={() => openModalFor(inv, rest)}
                            >
                              <Ic name="plus" />
                            </button>
                          )}
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>

            {/* totals footer */}
            <div className="tot-foot">
              <span>Total: <b className="num">{list.length}</b> facturi</span>
              <span>Neplătite: <b className="num">{counts.UNPAID}</b></span>
              <span>Restante: <b className="num" style={{ color: counts.OVERDUE > 0 ? "var(--red)" : undefined }}>{counts.OVERDUE}</b></span>
              <span className="spacer" style={{ flex: 1 }} />
              {list.length > MAX_ROWS && (
                <span className="muted">afișate primele {MAX_ROWS.toLocaleString("ro-RO")} din {list.length.toLocaleString("ro-RO")}</span>
              )}
            </div>
          </>
        )}
      </div>

      {/* add-payment modal — design .modal-back/.modal (RON + FX variant) */}
      {addModal && (
        <div
          className="modal-back show"
          style={{ position: "fixed" }}
          onMouseDown={(e) => { if (e.target === e.currentTarget) closeModal(); }}
        >
          <div className="modal">
            <div className="modal-head">
              <div>
                <div className="mt">{modalIsFx ? "Adaugă plată în valută" : "Adaugă plată"}</div>
                <div className="ms num">
                  {modalInvoice
                    ? `${modalInvoice.fullNumber} · ${contactMap.get(modalInvoice.contactId) ?? "—"} · rest ${fmtRON(modalRest)} ${modalCurrency}${
                        modalIsFx && fxIssueRate ? ` (emisă la curs ${fmtRate(fxIssueRate)})` : ""
                      }`
                    : "Selectați factura pentru care înregistrați plata"}
                </div>
              </div>
              <button className="modal-x" onClick={closeModal}>
                <Ic name="xMark" />
              </button>
            </div>
            <div className="modal-body">
              {/* Invoice picker — only shown when modal was opened from the header button */}
              {addModal.invoiceId === "" && (
                <div className="field" style={{ marginBottom: 14 }}>
                  <label>Factura <span className="req">*</span></label>
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

              {/* Existing payments — real feature the prototype lacks (list + delete) */}
              {existingPayments.length > 0 && (
                <div style={{ marginBottom: 14 }}>
                  <div className="col-title" style={{ padding: "0 0 6px" }}>Plăți înregistrate</div>
                  {existingPayments.map((p: Payment) => (
                    <div
                      key={p.id}
                      style={{
                        display: "flex", alignItems: "center", gap: 8,
                        padding: "6px 0", borderBottom: "1px solid var(--line)", fontSize: 12.5,
                      }}
                    >
                      <span className="num" style={{ flex: 1 }}>{fmtRoDate(p.paidAt)}</span>
                      <span>{METHOD_LABELS[p.method] ?? p.method}</span>
                      {p.reference && <span style={{ color: "var(--text-2)" }}>#{p.reference}</span>}
                      <span className="num" style={{ fontWeight: 600, minWidth: 80, textAlign: "right" }}>
                        {fmtRON(parseDec(p.amount))} {modalCurrency}
                      </span>
                      <button
                        className="mini-btn"
                        title="Șterge plata"
                        disabled={deleteMutation.isPending}
                        onClick={() => deleteMutation.mutate({ paymentId: p.id })}
                      >
                        <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: TRASH_PATH }} />
                      </button>
                    </div>
                  ))}
                </div>
              )}

              <div className="fgrid">
                <div className="field">
                  <label>Suma ({modalCurrency}) <span className="req">*</span></label>
                  <input
                    className="input num"
                    type="number"
                    step="0.01"
                    min="0.01"
                    placeholder="0.00"
                    value={form.amount}
                    onChange={(e) => setForm((f) => ({ ...f, amount: e.target.value }))}
                    style={{ textAlign: "right" }}
                  />
                </div>
                <div className="field">
                  <label>Data plății</label>
                  <input
                    className="input num"
                    type="date"
                    value={form.paidAt}
                    onChange={(e) => setForm((f) => ({ ...f, paidAt: e.target.value }))}
                  />
                </div>
                {modalIsFx && (
                  <>
                    <div className="field">
                      <label>Curs BNR la data plății (dif. de curs 665/765)</label>
                      <input
                        className="input num"
                        type="number"
                        step="0.0001"
                        min="0"
                        placeholder="ex. 4.9750"
                        value={form.exchangeRate}
                        onChange={(e) => setForm((f) => ({ ...f, exchangeRate: e.target.value }))}
                        style={{ textAlign: "right" }}
                      />
                      <span className="hint">
                        curs oficial BNR din {fmtRoDate(form.paidAt)} ·{" "}
                        <a
                          className="link"
                          style={{ cursor: "pointer" }}
                          onClick={() => void handleFetchBnr()}
                        >
                          {bnrLoading ? "se preia…" : "preia automat"}
                        </a>
                      </span>
                    </div>
                    <div className="field">
                      <label>Echivalent RON</label>
                      <input
                        className="input num"
                        type="text"
                        value={fxRonEquiv !== null ? fmtRON(fxRonEquiv) : "—"}
                        disabled
                        style={{ textAlign: "right", background: "var(--fill)", color: "var(--text-2)" }}
                      />
                    </div>
                  </>
                )}
                <div className="field">
                  <label>Metoda de plată</label>
                  <select
                    className="select"
                    value={form.method}
                    onChange={(e) => setForm((f) => ({ ...f, method: e.target.value }))}
                  >
                    {Object.entries(METHOD_LABELS).map(([v, l]) => (
                      <option key={v} value={v}>{l}</option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label>Referință / nr. chitanță</label>
                  <input
                    className="input"
                    type="text"
                    placeholder={modalIsFx ? "ex. SWIFT 99-2210" : "ex. OP 2248"}
                    value={form.reference}
                    onChange={(e) => setForm((f) => ({ ...f, reference: e.target.value }))}
                  />
                </div>
              </div>

              {/* FX difference banner — 665/765 (prototype payModalFx) */}
              {fxDiff !== null && (
                <div className={`banner ${fxDiff >= 0 ? "ok" : "warn"}`} style={{ margin: "14px 0 0" }}>
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: TREND_PATH }} />
                  <span>
                    <b>
                      Diferență de curs {fxDiff >= 0 ? "favorabilă" : "nefavorabilă"}: {fxDiff >= 0 ? "+" : "−"}
                      {fmtRON(Math.abs(fxDiff))} RON
                    </b>{" "}
                    · {form.amount} × ({fmtRate(fxRate)} − {fmtRate(fxIssueRate!)}). Se înregistrează automat ca{" "}
                    {fxDiff >= 0 ? (
                      <>venit financiar în contul <b>765</b> (nefavorabilă → cheltuială <b>665</b>)</>
                    ) : (
                      <>cheltuială financiară în contul <b>665</b> (favorabilă → venit <b>765</b>)</>
                    )}.
                  </span>
                </div>
              )}
            </div>
            <div className="modal-foot">
              {modalInvoice && form.amount && Number.isFinite(fxAmount) && (
                <span className="left">
                  {fxAmount >= modalRest - 0.005
                    ? "Plata stinge integral restul facturii"
                    : `Plată parțială — rest după plată: ${fmtAmt(Math.max(0, modalRest - fxAmount), modalCurrency)}`}
                </span>
              )}
              <button className="pill-btn" onClick={closeModal}>Renunță</button>
              <button
                className="btn-dark"
                disabled={saveDisabled}
                style={saveDisabled ? { opacity: 0.5 } : undefined}
                onClick={handleSave}
              >
                <Ic name="check" />
                {addMutation.isPending ? "Se salvează…" : "Salvează plata"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ─── InvoicePickerCombobox ─────────────────────────────────────────────────
// Inline combobox for picking an invoice from the active company.
// Used by the header-level "Adaugă plată" button so users can select which
// invoice to record a payment against. Design classes (.input / .pop / .mini-btn).

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
      e.stopPropagation();
      setOpen(false);
    }
  };

  // Selected state — compact pill (design tokens)
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
          minHeight: 36,
          padding: "4px 6px 4px 11px",
          border: "1px solid var(--line)",
          background: "#fff",
          borderRadius: 8,
        }}
      >
        <div style={{ flex: 1, minWidth: 0, lineHeight: 1.25 }}>
          <div
            style={{
              fontFamily: "var(--mono)",
              fontSize: 12.5,
              fontWeight: 600,
              color: "var(--text)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {value.fullNumber}
          </div>
          <div style={{ fontSize: 11, color: "var(--text-2)" }}>
            {fmtRoDate(value.issueDate)} · {fmtRON(parseDec(value.totalAmount))} {value.currency ?? "RON"}
          </div>
        </div>
        <button
          type="button"
          className="mini-btn"
          onClick={handleClear}
          aria-label="Elimină factura selectată"
          title="Elimină factura selectată"
        >
          <Ic name="xMark" />
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
        className="input"
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
          className="pop show"
          style={{
            top: "calc(100% + 4px)",
            left: 0,
            right: 0,
            zIndex: 70,
            maxHeight: 240,
            overflowY: "auto",
          }}
        >
          {isFetching ? (
            <div style={{ padding: "10px 12px", fontSize: 12, color: "var(--text-2)" }}>
              Se caută…
            </div>
          ) : results.length === 0 ? (
            <div style={{ padding: "10px 12px", fontSize: 12, color: "var(--text-2)" }}>
              {debouncedQuery ? `Nicio factură pentru „${debouncedQuery}”.` : "Nicio factură găsită."}
            </div>
          ) : (
            results.map((inv, idx) => {
              const active = idx === highlight;
              return (
                <button
                  key={inv.id}
                  type="button"
                  role="option"
                  aria-selected={active}
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => handleSelect(inv)}
                  onMouseEnter={() => setHighlight(idx)}
                  style={{
                    display: "block",
                    width: "100%",
                    textAlign: "left",
                    padding: "8px 10px",
                    border: 0,
                    borderRadius: 8,
                    background: active ? "var(--fill)" : "transparent",
                    cursor: "pointer",
                    color: "var(--text)",
                    font: "inherit",
                  }}
                >
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 8 }}>
                    <span style={{ fontFamily: "var(--mono)", fontSize: 12.5, fontWeight: 600 }}>
                      {inv.fullNumber}
                    </span>
                    <span className="num" style={{ fontSize: 12, color: "var(--text-2)", flexShrink: 0 }}>
                      {fmtRON(parseDec(inv.totalAmount))} {inv.currency ?? "RON"}
                    </span>
                  </div>
                  <div style={{ fontSize: 11, color: "var(--text-2)" }}>
                    {fmtRoDate(inv.issueDate)}
                  </div>
                </button>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
