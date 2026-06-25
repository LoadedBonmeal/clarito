/**
 * Quotes page (Oferte & Devize) — list + create/edit modal with LineItemsEditor.
 * Commercial pre-accounting documents: NO GL, no VAT obligation, no e-Factura.
 * GL fires only when converting an accepted quote to a factură (→ /invoices/:id).
 */

import { useCallback, useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useNavigate } from "@tanstack/react-router";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { LineItemsEditor } from "@/components/shared/LineItemsEditor";
import type { LineRow } from "@/components/shared/LineItemsEditor";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { Quote, CreateQuoteInput, UpdateQuoteInput, QuoteKind } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";
import type { CreateLineInput } from "@/types";

const RO_MON = ["ian","feb","mar","apr","mai","iun","iul","aug","sep","oct","nov","dec"];
const fmtRoDate = (iso: string | null | undefined) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

const localDateISO = (d = new Date()) => {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
};

const newRowId = () => Math.random().toString(36).slice(2);

function makeEmptyRow(): LineRow {
  return {
    rowId: newRowId(),
    name: "",
    description: undefined,
    quantity: 1,
    unit: "buc",
    unitPrice: 0,
    vatRate: 21,
    vatCategory: "S",
    cpvCode: undefined,
    art331Code: undefined,
    revenueKind: "goods",
  };
}

function linesToRows(lines: { name: string; description?: string | null; quantity: string; unit?: string | null; unitPrice: string; vatRate: string; vatCategory?: string | null; revenueKind?: string | null }[]): LineRow[] {
  return lines.map((l) => ({
    rowId: newRowId(),
    name: l.name,
    description: l.description ?? undefined,
    quantity: parseFloat(l.quantity) || 1,
    unit: l.unit ?? "buc",
    unitPrice: parseFloat(l.unitPrice) || 0,
    vatRate: parseFloat(l.vatRate) || 21,
    vatCategory: (l.vatCategory ?? "S") as CreateLineInput["vatCategory"],
    cpvCode: undefined,
    art331Code: undefined,
    revenueKind: l.revenueKind ?? "goods",
  }));
}

type TabFilter = "all" | "active" | "invoiced";

const STATUS_CHIP: Record<string, { cls: string; labelKey: string }> = {
  draft:     { cls: "sent",   labelKey: "quotes.status.draft" },
  sent:      { cls: "wait",   labelKey: "quotes.status.sent" },
  accepted:  { cls: "paid",   labelKey: "quotes.status.accepted" },
  invoiced:  { cls: "paid",   labelKey: "quotes.status.invoiced" },
  cancelled: { cls: "late",   labelKey: "quotes.status.cancelled" },
  expired:   { cls: "wait",   labelKey: "quotes.status.expired" },
};

// ─── Modal ─────────────────────────────────────────────────────────────────

interface ModalProps {
  companyId: string;
  quote?: Quote;
  onClose: () => void;
}

function QuoteModal({ companyId, quote, onClose }: ModalProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { closing, close: animClose } = useAnimatedClose(onClose);

  const isEdit = !!quote;
  const [kind, setKind] = useState<QuoteKind>(quote?.kind ?? "quote");
  const [contactId, setContactId] = useState(quote?.contactId ?? "");
  const [series, setSeries] = useState(quote?.series ?? "");
  const [issueDate, setIssueDate] = useState(quote?.issueDate ?? localDateISO());
  const [validUntil, setValidUntil] = useState(quote?.validUntil ?? "");
  const [currency, setCurrency] = useState(quote?.currency ?? "RON");
  const [notes, setNotes] = useState(quote?.notes ?? "");
  const [lines, setLines] = useState<LineRow[]>(() =>
    quote ? [] : [makeEmptyRow()]
  );

  // Load lines for edit
  const { data: qwl } = useQuery({
    queryKey: queryKeys.quotes.detail(quote?.id ?? ""),
    queryFn: () => api.quotes.get(quote!.id, companyId),
    enabled: isEdit && !!quote?.id,
    staleTime: 0,
  });

  // Populate lines when loaded
  useMemo(() => {
    if (qwl?.lines && qwl.lines.length > 0) {
      setLines(linesToRows(qwl.lines));
    }
  }, [qwl]);

  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId }),
    queryFn: () => api.contacts.list({ companyId }),
    staleTime: 60_000,
  });

  const createMut = useMutation({
    mutationFn: (input: CreateQuoteInput) => api.quotes.create(input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.quotes.list(companyId) });
      notify.success(t("quotes.notify.created"));
      animClose();
    },
    onError: (e: unknown) => notify.error(t("quotes.notify.createError") + " " + formatError(e)),
  });

  const updateMut = useMutation({
    mutationFn: (input: UpdateQuoteInput) => api.quotes.update(quote!.id, companyId, input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.quotes.list(companyId) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.quotes.detail(quote!.id) });
      notify.success(t("quotes.notify.updated"));
      animClose();
    },
    onError: (e: unknown) => notify.error(t("quotes.notify.updateError") + " " + formatError(e)),
  });

  const saving = createMut.isPending || updateMut.isPending;

  const handleSubmit = () => {
    if (!issueDate) { notify.error(t("quotes.validate.issueDate")); return; }
    if (lines.length === 0) { notify.error(t("quotes.validate.lines")); return; }

    const mappedLines = lines.map((r) => ({
      name: r.name,
      description: r.description || undefined,
      quantity: r.quantity,
      unit: r.unit || undefined,
      unitPrice: r.unitPrice,
      vatRate: r.vatRate,
      vatCategory: r.vatCategory,
      revenueKind: r.revenueKind || undefined,
    }));

    if (isEdit) {
      updateMut.mutate({
        contactId: contactId || null,
        kind,
        series: series || null,
        issueDate,
        validUntil: validUntil || null,
        currency: currency || "RON",
        notes: notes || null,
        lines: mappedLines,
      });
    } else {
      createMut.mutate({
        companyId,
        contactId: contactId || null,
        kind,
        series: series || null,
        issueDate,
        validUntil: validUntil || null,
        currency: currency || "RON",
        notes: notes || null,
        lines: mappedLines,
      });
    }
  };

  const title = isEdit
    ? (kind === "deviz" ? t("quotes.modal.editDevizTitle") : t("quotes.modal.editTitle"))
    : (kind === "deviz" ? t("quotes.modal.createDevizTitle") : t("quotes.modal.createTitle"));

  return (
    <div className={`modal-back ${closing ? "closing" : "show"}`} onClick={animClose}>
      <div className="modal lg" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <div>
            <div className="mt">{title}</div>
          </div>
          <button className="modal-x" onClick={animClose} aria-label={t("quotes.modal.close")}>
            <svg width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" viewBox="0 0 24 24">
              <path d="M6 18 18 6M6 6l12 12"/>
            </svg>
          </button>
        </div>

        <div className="modal-body">
          {/* Kind */}
          <div className="field">
            <label>{t("quotes.modal.kind")}</label>
            <div style={{ display: "flex", gap: 8 }}>
              {(["quote","deviz"] as QuoteKind[]).map((k) => (
                <button
                  key={k}
                  type="button"
                  className={`chip${kind === k ? " active" : ""}`}
                  onClick={() => setKind(k)}
                  style={{ cursor: "pointer" }}
                >
                  {t(`quotes.kind.${k}`)}
                </button>
              ))}
            </div>
          </div>

          {/* Client */}
          <div className="field">
            <label>{t("quotes.modal.client")}</label>
            <select className="select" value={contactId} onChange={(e) => setContactId(e.target.value)}>
              <option value="">{t("quotes.modal.clientPick")}</option>
              {contacts.map((c) => (
                <option key={c.id} value={c.id}>{c.legalName}</option>
              ))}
            </select>
          </div>

          {/* Date row */}
          <div className="fgrid">
            <div className="field">
              <label>{t("quotes.modal.issueDate")}</label>
              <input type="date" className="input" value={issueDate} onChange={(e) => setIssueDate(e.target.value)} />
            </div>
            <div className="field">
              <label>{t("quotes.modal.validUntil")}</label>
              <input type="date" className="input" value={validUntil} onChange={(e) => setValidUntil(e.target.value)} />
            </div>
          </div>

          {/* Series + Currency */}
          <div className="fgrid">
            <div className="field">
              <label>{t("quotes.modal.series")}</label>
              <input type="text" className="input" placeholder={t("quotes.modal.seriesPlaceholder")} value={series} onChange={(e) => setSeries(e.target.value)} />
            </div>
            <div className="field">
              <label>{t("quotes.modal.currency")}</label>
              <select className="select" value={currency} onChange={(e) => setCurrency(e.target.value)}>
                <option value="RON">RON</option>
                <option value="EUR">EUR</option>
                <option value="USD">USD</option>
                <option value="GBP">GBP</option>
              </select>
            </div>
          </div>

          {/* Lines */}
          <div className="field">
            <label>{t("quotes.modal.lines")}</label>
            <LineItemsEditor
              lines={lines}
              onChange={setLines}
              companyId={companyId}
              currency={currency}
              issueDate={issueDate}
              showTotals
            />
          </div>

          {/* Notes */}
          <div className="field">
            <label>{t("quotes.modal.notes")}</label>
            <textarea
              className="input"
              rows={2}
              placeholder={t("quotes.modal.notesPlaceholder")}
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              style={{ resize: "vertical" }}
            />
          </div>
        </div>

        <div className="modal-foot">
          <button type="button" className="pill-btn" onClick={animClose}>{t("quotes.modal.close")}</button>
          <button className="btn-dark" onClick={handleSubmit} disabled={saving}>
            {saving
              ? t("quotes.modal.saving")
              : isEdit ? t("quotes.modal.saveChanges") : t("quotes.modal.create")}
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Row Actions ───────────────────────────────────────────────────────────

interface RowActionsProps {
  quote: Quote;
  companyId: string;
  onEdit: () => void;
  onClose: () => void;
  anchor: DOMRect | null;
}

function RowActions({ quote, companyId, onEdit, onClose, anchor }: RowActionsProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const statusMut = useMutation({
    mutationFn: (status: string) => api.quotes.setStatus(quote.id, companyId, status),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.quotes.list(companyId) });
      notify.success(t("quotes.notify.statusUpdated"));
      onClose();
    },
    onError: (e: unknown) => { notify.error(t("quotes.notify.statusError") + " " + formatError(e)); onClose(); },
  });

  const convertMut = useMutation({
    mutationFn: () => api.quotes.convertToInvoice(companyId, quote.id),
    onSuccess: (inv) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.quotes.list(companyId) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success(t("quotes.notify.converted"));
      onClose();
      void navigate({ to: "/invoices/$id", params: { id: inv.id } });
    },
    onError: (e: unknown) => { notify.error(t("quotes.notify.convertError") + " " + formatError(e)); onClose(); },
  });

  const deleteMut = useMutation({
    mutationFn: () => api.quotes.delete(quote.id, companyId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.quotes.list(companyId) });
      notify.success(t("quotes.notify.deleted"));
      onClose();
    },
    onError: (e: unknown) => { notify.error(t("quotes.notify.deleteError") + " " + formatError(e)); onClose(); },
  });

  const [deleteConfirm, setDeleteConfirm] = useState(false);

  const style: React.CSSProperties = anchor
    ? { position: "fixed", top: anchor.bottom + 4, right: window.innerWidth - anchor.right, zIndex: 9999 }
    : { position: "fixed", top: 0, right: 0, zIndex: 9999 };

  const s = quote.status;

  return (
    <div className="pop" style={style}>
      {s === "draft" && <button className="pop-item" onClick={() => { onEdit(); onClose(); }}>{t("quotes.actions.edit")}</button>}
      {s === "draft" && <button className="pop-item" onClick={() => statusMut.mutate("sent")}>{t("quotes.actions.send")}</button>}
      {(s === "draft" || s === "sent") && <button className="pop-item" onClick={() => statusMut.mutate("accepted")}>{t("quotes.actions.accept")}</button>}
      {s === "accepted" && (
        <button className="pop-item" onClick={() => convertMut.mutate()}>
          {t("quotes.actions.convertToInvoice")}
        </button>
      )}
      {(s === "sent" || s === "accepted") && <button className="pop-item" onClick={() => statusMut.mutate("expired")}>{t("quotes.actions.expire")}</button>}
      {(s === "draft" || s === "sent" || s === "accepted") && (
        <button className="pop-item" onClick={() => statusMut.mutate("cancelled")}>{t("quotes.actions.cancel")}</button>
      )}
      {(s === "draft" || s === "cancelled" || s === "expired") && (
        deleteConfirm
          ? <button className="pop-item danger" onClick={() => deleteMut.mutate()}>{t("quotes.actions.confirmDelete")}</button>
          : <button className="pop-item danger" onClick={() => setDeleteConfirm(true)}>{t("quotes.actions.delete")}</button>
      )}
    </div>
  );
}

// ─── Page ──────────────────────────────────────────────────────────────────

export function QuotesPage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const [tab, setTab] = useState<TabFilter>("all");
  const [search, setSearch] = useState("");
  const [modalOpen, setModalOpen] = useState(false);
  const [editQuote, setEditQuote] = useState<Quote | undefined>();
  const [menuAnchor, setMenuAnchor] = useState<DOMRect | null>(null);
  const [menuQuote, setMenuQuote] = useState<Quote | undefined>();

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
    staleTime: 60_000,
  });

  const activeCompany = companies.find((c) => c.id === activeCompanyId);

  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
    staleTime: 60_000,
  });

  const contactMap = useMemo(
    () => new Map(contacts.map((c) => [c.id, c.legalName])),
    [contacts],
  );

  const { data: quotes = [], isLoading, error } = useQuery({
    queryKey: queryKeys.quotes.list(activeCompanyId ?? ""),
    queryFn: () => api.quotes.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const allCount = quotes.length;
  const activeCount = useMemo(
    () => quotes.filter((q) => !["invoiced", "cancelled", "expired"].includes(q.status)).length,
    [quotes],
  );
  const invoicedCount = useMemo(
    () => quotes.filter((q) => q.status === "invoiced").length,
    [quotes],
  );

  const filtered = useMemo(() => {
    let items = quotes;
    if (tab === "active") items = items.filter((q) => !["invoiced","cancelled","expired"].includes(q.status));
    if (tab === "invoiced") items = items.filter((q) => q.status === "invoiced");
    if (search.trim()) {
      const s = search.toLowerCase();
      items = items.filter((q) =>
        (q.fullNumber ?? "").toLowerCase().includes(s) ||
        (q.notes ?? "").toLowerCase().includes(s)
      );
    }
    return items;
  }, [quotes, tab, search]);

  const handleOpenMenu = useCallback((e: React.MouseEvent, q: Quote) => {
    e.stopPropagation();
    setMenuAnchor((e.currentTarget as HTMLElement).getBoundingClientRect());
    setMenuQuote(q);
  }, []);

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="banner info">{t("quotes.selectCompany")}</div>
      </div>
    );
  }

  const tabCounts: Record<TabFilter, number> = {
    all: allCount,
    active: activeCount,
    invoiced: invoicedCount,
  };

  return (
    <div className="main-inner wide">
      <div className="page-head">
        <div>
          <h1>{t("quotes.title")}</h1>
          <p className="sub">
            {allCount} {t("quotes.title").toLowerCase()} &middot; {activeCompany?.legalName ?? ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="btn-dark" onClick={() => { setEditQuote(undefined); setModalOpen(true); }}>
            <Ic name="plus" />
            {t("quotes.head.new")}
          </button>
        </div>
      </div>

      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tabs">
            {(["all", "active", "invoiced"] as TabFilter[]).map((tb) => (
              <div
                key={tb}
                className={"tab" + (tab === tb ? " active" : "")}
                onClick={() => setTab(tb)}
                style={{ cursor: "pointer" }}
              >
                {t(`quotes.tabs.${tb}`)}<span className="cnt">{tabCounts[tb]}</span>
              </div>
            ))}
          </div>
          <div className="spacer" />
          <div className="scr-search">
            <Ic name="lens" />
            <input
              type="text"
              placeholder="Cauta oferta..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        </div>

        {isLoading && <div className="state-row">{t("quotes.states.loading")}</div>}
        {error && <QueryErrorBanner label={t("quotes.states.errorLabel")} error={error} />}

        {!isLoading && !error && (
          <table className="scr-table">
            <thead>
              <tr>
                <th style={{ width: 140 }}>{t("quotes.table.number")}</th>
                <th style={{ width: 120 }}>{t("quotes.table.date")}</th>
                <th>{t("quotes.table.client") || "Client"}</th>
                <th style={{ width: 110 }}>{t("quotes.table.kind")}</th>
                <th style={{ width: 130 }}>{t("quotes.table.validUntil")}</th>
                <th className="r" style={{ width: 130 }}>{t("quotes.table.total")}</th>
                <th style={{ width: 120 }}>{t("quotes.table.status")}</th>
                <th style={{ width: 40 }}></th>
              </tr>
            </thead>
            {filtered.length === 0 ? (
              <tbody>
                <tr>
                  <td colSpan={8} style={{ padding: 0 }}>
                    <div className="empty">
                      <div className="ei"><Ic name="calc" /></div>
                      <b>Nicio oferta.</b>
                      Creati o oferta cu butonul Oferta noua.
                    </div>
                  </td>
                </tr>
              </tbody>
            ) : (
              <tbody>
                {filtered.map((q) => {
                  const chip = STATUS_CHIP[q.status] ?? { cls: "sent", labelKey: `quotes.status.${q.status}` };
                  return (
                    <tr key={q.id}>
                      <td>{q.fullNumber ?? `${q.series ?? "OFR"}-${String(q.number).padStart(4, "0")}`}</td>
                      <td>{fmtRoDate(q.issueDate)}</td>
                      <td>{(q.contactId && contactMap.get(q.contactId)) ?? "—"}</td>
                      <td><span className="chip">{t(`quotes.kind.${q.kind}`)}</span></td>
                      <td>{fmtRoDate(q.validUntil)}</td>
                      <td className="r">{fmtRON(q.totalAmount)} {q.currency !== "RON" ? q.currency : ""}</td>
                      <td><span className={`chip ${chip.cls}`}>{t(chip.labelKey)}</span></td>
                      <td>
                        <button
                          className="sq-btn ghost"
                          onClick={(e) => handleOpenMenu(e, q)}
                          aria-label="Acțiuni"
                        >
                          <svg width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" viewBox="0 0 24 24">
                            <path d="M6.75 12a.75.75 0 1 1-1.5 0 .75.75 0 0 1 1.5 0ZM12.75 12a.75.75 0 1 1-1.5 0 .75.75 0 0 1 1.5 0ZM18.75 12a.75.75 0 1 1-1.5 0 .75.75 0 0 1 1.5 0Z"/>
                          </svg>
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            )}
          </table>
        )}
      </div>

      <div className="banner info" style={{ marginTop: 12 }}>
        {t("quotes.banner.info")}
      </div>

      {menuQuote && (
        <RowActions
          quote={menuQuote}
          companyId={activeCompanyId}
          anchor={menuAnchor}
          onEdit={() => setEditQuote(menuQuote)}
          onClose={() => { setMenuQuote(undefined); setMenuAnchor(null); }}
        />
      )}

      {(modalOpen || editQuote) && (
        <QuoteModal
          companyId={activeCompanyId}
          quote={editQuote}
          onClose={() => { setModalOpen(false); setEditQuote(undefined); }}
        />
      )}
    </div>
  );
}
