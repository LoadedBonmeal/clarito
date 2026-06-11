/**
 * Chitanțe — verbatim port of the design "Chitante.html":
 *   .page-head (title + count/serie/company sub + sq-btn refresh + btn-dark "Chitanță nouă")
 *   .scr-card → .scr-table (număr · dată · plătitor cu .cli-ava · factură asociată link ·
 *   sumă · monedă · .row-acts PDF + ștergere) → .pager (paginare client-side)
 *   + .modal-back/.modal "Chitanță nouă" (fgrid: serie/dată/sumă/monedă/plătitor
 *   contact|text liber/factură asociată/observații).
 *
 * ALL wiring preserved: api.receipts.list(activeCompanyId) (company guard),
 * create → api.receipts.create(companyId, input), PDF → api.receipts.generatePdf
 * + openPath, delete → api.receipts.delete with confirm, ContactCombobox,
 * InvoiceCombobox (picker, not free-text UUID — fix R3 / issue #4).
 */

import { useEffect, useId, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { confirm } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { ContactCombobox } from "@/components/shared/ContactCombobox";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { Contact, Invoice, Receipt, ReceiptInput } from "@/types";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

/** Initials for the .cli-ava chip (design parity with Dashboard/Invoices). */
const ini = (name?: string | null) =>
  (name ?? "")
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("") || "—";

/** Client-side page size for the design .pager. */
const PAGE_SIZE = 25;

// Trash icon is not in Ic — inlined verbatim from the prototype.
const TRASH_PATH =
  '<path d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>';

export function ReceiptsPage() {
  const navigate = useNavigate();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [showModal, setShowModal] = useState(false);
  const [page, setPage] = useState(1);

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

  // Invoice numbers for the "Factură asociată" link column.
  const { data: invoicePage } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    queryFn: () => api.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    enabled: !!activeCompanyId,
  });

  const contactMap = useMemo(() => {
    const m = new Map<string, string>();
    for (const c of contacts) m.set(c.id, c.legalName);
    return m;
  }, [contacts]);

  const invoiceMap = useMemo(() => {
    const m = new Map<string, string>();
    for (const inv of invoicePage?.items ?? []) m.set(inv.id, inv.fullNumber);
    return m;
  }, [invoicePage]);

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

  // ── pagination (design .pager, client-side) ────────────────────────────────
  const totalPages = Math.max(1, Math.ceil(receiptList.length / PAGE_SIZE));
  const curPage = Math.min(page, totalPages);
  const pageRows = receiptList.slice((curPage - 1) * PAGE_SIZE, curPage * PAGE_SIZE);
  const from = receiptList.length === 0 ? 0 : (curPage - 1) * PAGE_SIZE + 1;
  const to = Math.min(curPage * PAGE_SIZE, receiptList.length);
  const pageNums = useMemo(() => {
    const WIN = 5;
    const start = Math.max(1, Math.min(curPage - 2, totalPages - WIN + 1));
    const end = Math.min(totalPages, start + WIN - 1);
    const out: number[] = [];
    for (let p = start; p <= end; p++) out.push(p);
    return out;
  }, [curPage, totalPages]);

  // Sub-line: count · serie (when a single series is in use) · company.
  const seriesSet = useMemo(() => {
    const s = new Set<string>();
    for (const r of receiptList) s.add(r.series);
    return Array.from(s);
  }, [receiptList]);
  const sub =
    `${receiptList.length.toLocaleString("ro-RO")} chitanțe emise` +
    (seriesSet.length === 1 ? ` · seria ${seriesSet[0]}` : "") +
    (activeCompany ? ` · ${activeCompany.legalName}` : "");

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>Chitanțe</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Selectați o companie activă pentru a vedea chitanțele.
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Chitanțe</h1>
          <p className="sub">{sub}</p>
        </div>
        <div className="head-actions">
          <button
            className="sq-btn spin-btn"
            title="Reîmprospătează"
            onClick={() => void queryClient.invalidateQueries({ queryKey: queryKeys.receipts.all })}
          >
            <Ic name="sync" />
          </button>
          <button className="btn-dark" onClick={() => setShowModal(true)}>
            <Ic name="plus" />Chitanță nouă
          </button>
        </div>
      </div>

      <div className="scr-card">
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
        ) : isError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={error} label="chitanțele" onRetry={() => void refetch()} />
          </div>
        ) : receiptList.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            Nicio chitanță. Apăsați „Chitanță nouă” pentru a emite prima chitanță.
          </div>
        ) : (
          <>
            <table className="scr-table">
              <thead>
                <tr>
                  <th>Număr</th>
                  <th>Data</th>
                  <th>Plătitor</th>
                  <th>Factură asociată</th>
                  <th className="r">Sumă</th>
                  <th>Monedă</th>
                  <th className="r" style={{ width: 90 }}></th>
                </tr>
              </thead>
              <tbody>
                {pageRows.map((r) => {
                  const contactName = r.contactId ? contactMap.get(r.contactId) : undefined;
                  const invoiceNumber = r.invoiceId ? invoiceMap.get(r.invoiceId) : undefined;
                  return (
                    <tr key={r.id}>
                      <td><span className="doc" style={{ fontWeight: 700, color: "var(--text)" }}>{r.series}-{r.number}</span></td>
                      <td className="num">{fmtRoDate(r.issueDate)}</td>
                      <td>
                        {contactName ? (
                          <div className="cli"><span className="cli-ava">{ini(contactName)}</span>{contactName}</div>
                        ) : r.payerName ? (
                          <>{r.payerName} <span className="muted">(text liber)</span></>
                        ) : (
                          <span className="muted">—</span>
                        )}
                      </td>
                      <td>
                        {r.invoiceId ? (
                          <a
                            className="link"
                            style={{ fontFamily: "var(--mono)", fontSize: 12 }}
                            onClick={() => void navigate({ to: "/invoices/$id", params: { id: r.invoiceId! } })}
                          >
                            {invoiceNumber ?? "factură"}
                          </a>
                        ) : (
                          <span className="muted">—</span>
                        )}
                      </td>
                      <td className="r num"><b>{fmtRON(parseDec(r.amount))}</b></td>
                      <td>{r.currency}</td>
                      <td>
                        <div className="row-acts">
                          <button
                            className="mini-btn"
                            title="Generează PDF"
                            disabled={pdfMutation.isPending}
                            onClick={() => pdfMutation.mutate(r.id)}
                          >
                            <Ic name="dl" />
                          </button>
                          <button
                            className="mini-btn"
                            title="Șterge"
                            onClick={() => void handleDelete(r)}
                          >
                            <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: TRASH_PATH }} />
                          </button>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>

            {/* pager */}
            <div className="pager">
              <span>Afișezi <b>{from}–{to}</b> din <b>{receiptList.length.toLocaleString("ro-RO")}</b> chitanțe</span>
              <div className="pg-btns">
                <button
                  className="pg-btn"
                  disabled={curPage === 1}
                  onClick={() => setPage(curPage - 1)}
                >
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: '<path d="M15.75 19.5 8.25 12l7.5-7.5"/>' }} />
                </button>
                {pageNums.map((p) => (
                  <button
                    key={p}
                    className={`pg-btn${p === curPage ? " cur" : ""}`}
                    onClick={() => setPage(p)}
                  >
                    {p}
                  </button>
                ))}
                <button
                  className="pg-btn"
                  disabled={curPage === totalPages}
                  onClick={() => setPage(curPage + 1)}
                >
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: '<path d="m8.25 4.5 7.5 7.5-7.5 7.5"/>' }} />
                </button>
              </div>
            </div>
          </>
        )}
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

// ─── Modal — design .modal-back/.modal "Chitanță nouă" ──────────────────────

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
  const [invoice, setInvoice] = useState<Invoice | null>(null);
  const [formError, setFormError] = useState<string | null>(null);

  const createMutation = useMutation({
    mutationFn: (input: ReceiptInput) => api.receipts.create(companyId, input),
    onSuccess: () => {
      notify.success("Chitanță emisă.");
      onSaved();
    },
    onError: (e) => setFormError(formatError(e, "Eroare la emitere.")),
  });

  // Esc closes the modal.
  useEffect(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", h);
    return () => document.removeEventListener("keydown", h);
  }, [onClose]);

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
      // Use the UUID from the picked invoice, not a free-text value.
      invoiceId: invoice?.id ?? undefined,
    };
    createMutation.mutate(input);
  };

  return createPortal(
    <div
      className="modal-back show"
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="modal">
        <div className="modal-head">
          <div>
            <div className="mt">Chitanță nouă</div>
            <div className="ms">Numărul se alocă automat în serie</div>
          </div>
          <button className="modal-x" onClick={onClose}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field">
              <label>Serie</label>
              <input
                className="input num"
                type="text"
                value={form.series ?? "CH"}
                onChange={(e) => setForm((f) => ({ ...f, series: e.target.value }))}
                placeholder="CH"
              />
            </div>
            <div className="field">
              <label>Data emiterii <span className="req">*</span></label>
              <input
                className="input num"
                type="date"
                value={form.issueDate}
                onChange={(e) => setForm((f) => ({ ...f, issueDate: e.target.value }))}
              />
            </div>
            <div className="field">
              <label>Sumă <span className="req">*</span></label>
              <input
                className="input num"
                type="number"
                step="0.01"
                min="0.01"
                placeholder="0,00"
                style={{ textAlign: "right" }}
                value={form.amount}
                onChange={(e) => setForm((f) => ({ ...f, amount: e.target.value }))}
                autoFocus
              />
            </div>
            <div className="field">
              <label>Monedă</label>
              <select
                className="select"
                value={form.currency ?? "RON"}
                onChange={(e) => setForm((f) => ({ ...f, currency: e.target.value }))}
              >
                <option value="RON">RON</option>
                <option value="EUR">EUR</option>
                <option value="USD">USD</option>
              </select>
            </div>
            <div className="field span2">
              <label>Plătitor (contact)</label>
              <ContactCombobox
                value={contact}
                onChange={setContact}
                companyId={companyId}
                placeholder="Caută plătitor (opțional)…"
                width="100%"
              />
            </div>
            <div className="field span2">
              <label>Plătitor (text liber)</label>
              <input
                className="input"
                type="text"
                placeholder="Nume plătitor (dacă nu e în contacte)"
                value={form.payerName ?? ""}
                onChange={(e) => setForm((f) => ({ ...f, payerName: e.target.value }))}
              />
            </div>
            <div className="field span2">
              <label>Factură asociată (opțional)</label>
              <InvoiceCombobox
                companyId={companyId}
                value={invoice}
                onChange={setInvoice}
              />
            </div>
            <div className="field span2">
              <label>Observații</label>
              <textarea
                className="input"
                placeholder="opțional"
                value={form.notes ?? ""}
                onChange={(e) => setForm((f) => ({ ...f, notes: e.target.value }))}
              />
            </div>
          </div>
          {formError && (
            <div className="field" style={{ marginTop: 12 }}>
              <span className="err">
                <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: '<path d="M6 18 18 6M6 6l12 12"/>' }} />
                {formError}
              </span>
            </div>
          )}
        </div>
        <div className="modal-foot">
          <button className="pill-btn" onClick={onClose} disabled={createMutation.isPending}>
            Renunță
          </button>
          <button
            className="btn-dark"
            disabled={createMutation.isPending}
            style={createMutation.isPending ? { opacity: 0.6 } : undefined}
            onClick={handleSubmit}
          >
            <Ic name="check" />
            {createMutation.isPending ? "Se salvează…" : "Emite chitanță"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ─── InvoiceCombobox ────────────────────────────────────────────────────────
// Inline combobox for picking an invoice by fullNumber + client name.
// Mirrors ContactCombobox conventions, restyled with design classes.

function InvoiceCombobox({
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
    queryKey: ["invoices", "picker", companyId, debouncedQuery],
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
            {fmtRoDate(value.issueDate)}
          </div>
        </div>
        <button
          type="button"
          className="mini-btn"
          onClick={handleClear}
          aria-label="Elimină factura asociată"
          title="Elimină factura asociată"
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
                      {fmtRON(parseDec(inv.totalAmount))} {inv.currency}
                    </span>
                  </div>
                  <div style={{ fontSize: 11, color: "var(--text-2)" }}>
                    {fmtRoDate(inv.issueDate)} · {inv.status}
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
