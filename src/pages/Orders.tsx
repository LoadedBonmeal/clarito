/**
 * Orders page (Comenzi) — list + create/edit modal with LineItemsEditor.
 * Commercial pre-accounting documents: NO GL, no VAT obligation, no e-Factura.
 * Stock is NOT affected by orders. qty_reserved on order lines is informational only.
 * GL fires only when converting an accepted order to a factura (→ /invoices/:id).
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
import type { Order, CreateOrderInput, UpdateOrderInput } from "@/lib/tauri";
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
  draft:     { cls: "sent",   labelKey: "orders.status.draft" },
  sent:      { cls: "wait",   labelKey: "orders.status.sent" },
  accepted:  { cls: "paid",   labelKey: "orders.status.accepted" },
  invoiced:  { cls: "paid",   labelKey: "orders.status.invoiced" },
  cancelled: { cls: "late",   labelKey: "orders.status.cancelled" },
};

// Modal

interface ModalProps {
  companyId: string;
  order?: Order;
  onClose: () => void;
}

function OrderModal({ companyId, order, onClose }: ModalProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { closing, close: animClose } = useAnimatedClose(onClose);

  const isEdit = !!order;
  const [contactId, setContactId] = useState(order?.contactId ?? "");
  const [series, setSeries] = useState(order?.series ?? "");
  const [orderDate, setOrderDate] = useState(order?.orderDate ?? localDateISO());
  const [expectedDelivery, setExpectedDelivery] = useState(order?.expectedDelivery ?? "");
  const [currency, setCurrency] = useState(order?.currency ?? "RON");
  const [notes, setNotes] = useState(order?.notes ?? "");
  const [lines, setLines] = useState<LineRow[]>(() =>
    order ? [] : [makeEmptyRow()]
  );

  const { data: owl } = useQuery({
    queryKey: queryKeys.orders.detail(order?.id ?? ""),
    queryFn: () => api.orders.get(order!.id, companyId),
    enabled: isEdit && !!order?.id,
    staleTime: 0,
  });

  useMemo(() => {
    if (owl?.lines && owl.lines.length > 0) {
      setLines(linesToRows(owl.lines));
    }
  }, [owl]);

  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId }),
    queryFn: () => api.contacts.list({ companyId }),
    staleTime: 60_000,
  });

  const createMut = useMutation({
    mutationFn: (input: CreateOrderInput) => api.orders.create(input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.orders.list(companyId) });
      notify.success(t("orders.notify.created"));
      animClose();
    },
    onError: (e: unknown) => notify.error(t("orders.notify.createError") + " " + formatError(e)),
  });

  const updateMut = useMutation({
    mutationFn: (input: UpdateOrderInput) => api.orders.update(order!.id, companyId, input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.orders.list(companyId) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.orders.detail(order!.id) });
      notify.success(t("orders.notify.updated"));
      animClose();
    },
    onError: (e: unknown) => notify.error(t("orders.notify.updateError") + " " + formatError(e)),
  });

  const saving = createMut.isPending || updateMut.isPending;

  const handleSubmit = () => {
    if (!orderDate) { notify.error(t("orders.validate.orderDate")); return; }
    if (lines.length === 0) { notify.error(t("orders.validate.lines")); return; }

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
        series: series || null,
        orderDate,
        expectedDelivery: expectedDelivery || null,
        currency: currency || "RON",
        notes: notes || null,
        lines: mappedLines,
      });
    } else {
      createMut.mutate({
        companyId,
        contactId: contactId || null,
        series: series || null,
        orderDate,
        expectedDelivery: expectedDelivery || null,
        currency: currency || "RON",
        notes: notes || null,
        lines: mappedLines,
      });
    }
  };

  return (
    <div className={`modal-overlay${closing ? " closing" : ""}`} onClick={animClose}>
      <div className="modal-panel" style={{ maxWidth: 780, width: "100%" }} onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <span className="modal-title">
            {isEdit ? t("orders.modal.editTitle") : t("orders.modal.createTitle")}
          </span>
          <button className="sq-btn ghost" onClick={animClose} aria-label={t("orders.modal.close")}>
            <svg width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" viewBox="0 0 24 24">
              <path d="M6 18 18 6M6 6l12 12"/>
            </svg>
          </button>
        </div>

        <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 16, padding: "20px 24px" }}>
          <div className="form-row">
            <label className="form-label">{t("orders.modal.client")}</label>
            <select className="select" value={contactId} onChange={(e) => setContactId(e.target.value)}>
              <option value="">{t("orders.modal.clientPick")}</option>
              {contacts.map((c) => (
                <option key={c.id} value={c.id}>{c.legalName}</option>
              ))}
            </select>
          </div>

          <div style={{ display: "flex", gap: 12 }}>
            <div className="form-row" style={{ flex: 1 }}>
              <label className="form-label">{t("orders.modal.orderDate")}</label>
              <input type="date" className="input" value={orderDate} onChange={(e) => setOrderDate(e.target.value)} />
            </div>
            <div className="form-row" style={{ flex: 1 }}>
              <label className="form-label">{t("orders.modal.expectedDelivery")}</label>
              <input type="date" className="input" value={expectedDelivery} onChange={(e) => setExpectedDelivery(e.target.value)} />
            </div>
          </div>

          <div style={{ display: "flex", gap: 12 }}>
            <div className="form-row" style={{ flex: 1 }}>
              <label className="form-label">{t("orders.modal.series")}</label>
              <input type="text" className="input" placeholder={t("orders.modal.seriesPlaceholder")} value={series} onChange={(e) => setSeries(e.target.value)} />
            </div>
            <div className="form-row" style={{ flex: 1 }}>
              <label className="form-label">{t("orders.modal.currency")}</label>
              <select className="select" value={currency} onChange={(e) => setCurrency(e.target.value)}>
                <option value="RON">RON</option>
                <option value="EUR">EUR</option>
                <option value="USD">USD</option>
                <option value="GBP">GBP</option>
              </select>
            </div>
          </div>

          <div>
            <div className="form-label" style={{ marginBottom: 8 }}>{t("orders.modal.lines")}</div>
            <LineItemsEditor
              lines={lines}
              onChange={setLines}
              companyId={companyId}
              currency={currency}
              issueDate={orderDate}
              showTotals
            />
          </div>

          <div className="form-row">
            <label className="form-label">{t("orders.modal.notes")}</label>
            <textarea
              className="input"
              rows={2}
              placeholder={t("orders.modal.notesPlaceholder")}
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              style={{ resize: "vertical" }}
            />
          </div>
        </div>

        <div className="modal-foot" style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: "16px 24px" }}>
          <button className="btn ghost" onClick={animClose}>{t("orders.modal.close")}</button>
          <button className="btn-dark" onClick={handleSubmit} disabled={saving}>
            {saving
              ? t("orders.modal.saving")
              : isEdit ? t("orders.modal.saveChanges") : t("orders.modal.create")}
          </button>
        </div>
      </div>
    </div>
  );
}

interface RowActionsProps {
  order: Order;
  companyId: string;
  onEdit: () => void;
  onClose: () => void;
  anchor: DOMRect | null;
}

function RowActions({ order, companyId, onEdit, onClose, anchor }: RowActionsProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const statusMut = useMutation({
    mutationFn: (status: string) => api.orders.setStatus(order.id, companyId, status),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.orders.list(companyId) });
      notify.success(t("orders.notify.statusUpdated"));
      onClose();
    },
    onError: (e: unknown) => { notify.error(t("orders.notify.statusError") + " " + formatError(e)); onClose(); },
  });

  const convertMut = useMutation({
    mutationFn: () => api.orders.convertToInvoice(companyId, order.id),
    onSuccess: (inv) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.orders.list(companyId) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success(t("orders.notify.converted"));
      onClose();
      void navigate({ to: "/invoices/$id", params: { id: inv.id } });
    },
    onError: (e: unknown) => { notify.error(t("orders.notify.convertError") + " " + formatError(e)); onClose(); },
  });

  const deleteMut = useMutation({
    mutationFn: () => api.orders.delete(order.id, companyId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.orders.list(companyId) });
      notify.success(t("orders.notify.deleted"));
      onClose();
    },
    onError: (e: unknown) => { notify.error(t("orders.notify.deleteError") + " " + formatError(e)); onClose(); },
  });

  const [deleteConfirm, setDeleteConfirm] = useState(false);

  const style: React.CSSProperties = anchor
    ? { position: "fixed", top: anchor.bottom + 4, right: window.innerWidth - anchor.right, zIndex: 9999 }
    : { position: "fixed", top: 0, right: 0, zIndex: 9999 };

  const s = order.status;

  return (
    <div className="pop" style={style}>
      {s === "draft" && <button className="pop-item" onClick={() => { onEdit(); onClose(); }}>{t("orders.actions.edit")}</button>}
      {s === "draft" && <button className="pop-item" onClick={() => statusMut.mutate("sent")}>{t("orders.actions.send")}</button>}
      {(s === "draft" || s === "sent") && <button className="pop-item" onClick={() => statusMut.mutate("accepted")}>{t("orders.actions.accept")}</button>}
      {s === "accepted" && (
        <button className="pop-item" onClick={() => convertMut.mutate()}>
          {t("orders.actions.convertToInvoice")}
        </button>
      )}
      {(s === "draft" || s === "sent" || s === "accepted") && (
        <button className="pop-item" onClick={() => statusMut.mutate("cancelled")}>{t("orders.actions.cancel")}</button>
      )}
      {(s === "draft" || s === "cancelled") && (
        deleteConfirm
          ? <button className="pop-item danger" onClick={() => deleteMut.mutate()}>{t("orders.actions.confirmDelete")}</button>
          : <button className="pop-item danger" onClick={() => setDeleteConfirm(true)}>{t("orders.actions.delete")}</button>
      )}
    </div>
  );
}

export function OrdersPage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const [tab, setTab] = useState<TabFilter>("all");
  const [search, setSearch] = useState("");
  const [modalOpen, setModalOpen] = useState(false);
  const [editOrder, setEditOrder] = useState<Order | undefined>();
  const [menuAnchor, setMenuAnchor] = useState<DOMRect | null>(null);
  const [menuOrder, setMenuOrder] = useState<Order | undefined>();

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  const activeCompany = companies.find((c) => c.id === activeCompanyId) ?? companies[0];

  const { data: orderList = [], isLoading, error } = useQuery({
    queryKey: queryKeys.orders.list(activeCompanyId ?? ""),
    queryFn: () => api.orders.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const { data: contactsList = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
    staleTime: 60_000,
  });

  const contactMap = useMemo(
    () => Object.fromEntries(contactsList.map((c) => [c.id, c])),
    [contactsList],
  );

  const tabCounts = useMemo(() => {
    const all = orderList.length;
    const active = orderList.filter((o) => !["invoiced", "cancelled"].includes(o.status)).length;
    const invoiced = orderList.filter((o) => o.status === "invoiced").length;
    return { all, active, invoiced };
  }, [orderList]);

  const filtered = useMemo(() => {
    let items = orderList;
    if (tab === "active") items = items.filter((o) => !["invoiced","cancelled"].includes(o.status));
    if (tab === "invoiced") items = items.filter((o) => o.status === "invoiced");
    if (search.trim()) {
      const s = search.toLowerCase();
      items = items.filter((o) =>
        (o.fullNumber ?? "").toLowerCase().includes(s) ||
        (o.notes ?? "").toLowerCase().includes(s)
      );
    }
    return items;
  }, [orderList, tab, search]);

  const handleOpenMenu = useCallback((e: React.MouseEvent, o: Order) => {
    e.stopPropagation();
    setMenuAnchor((e.currentTarget as HTMLElement).getBoundingClientRect());
    setMenuOrder(o);
  }, []);

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="banner info">{t("orders.selectCompany")}</div>
      </div>
    );
  }

  const TAB_DEFS: { id: TabFilter; label: string; count: number }[] = [
    { id: "all",      label: t("orders.tabs.all"),      count: tabCounts.all },
    { id: "active",   label: t("orders.tabs.active"),   count: tabCounts.active },
    { id: "invoiced", label: t("orders.tabs.invoiced"), count: tabCounts.invoiced },
  ];

  const colCount = 6;

  return (
    <div className="main-inner wide">
      <div className="page-head">
        <div>
          <h1>{t("orders.title")}</h1>
          <p className="sub">
            {orderList.length} {t("orders.title").toLowerCase()} · {activeCompany?.legalName ?? ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="btn-dark" onClick={() => { setEditOrder(undefined); setModalOpen(true); }}>
            <Ic name="plus" />
            {t("orders.head.new")}
          </button>
        </div>
      </div>

      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tabs">
            {TAB_DEFS.map((tb) => (
              <div
                key={tb.id}
                className={"tab" + (tab === tb.id ? " active" : "")}
                onClick={() => setTab(tb.id)}
              >
                {tb.label}<span className="cnt">{tb.count}</span>
              </div>
            ))}
          </div>
          <div className="spacer" />
          <div className="scr-search">
            <Ic name="lens" />
            <input
              type="text"
              placeholder="Cauta comanda..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        </div>

        {isLoading && <div className="state-row">{t("orders.states.loading")}</div>}
        {error && <QueryErrorBanner label={t("orders.states.errorLabel")} error={error} />}

        {!isLoading && !error && (
          <table className="scr-table">
            <thead>
              <tr>
                <th style={{ width: 140 }}>{t("orders.table.number")}</th>
                <th style={{ width: 130 }}>{t("orders.table.date")}</th>
                <th>{t("orders.table.client") || "Client"}</th>
                <th style={{ width: 150 }}>{t("orders.table.delivery")}</th>
                <th className="r" style={{ width: 130 }}>{t("orders.table.total")}</th>
                <th style={{ width: 120 }}>{t("orders.table.status")}</th>
                <th style={{ width: 40 }}></th>
              </tr>
            </thead>
            {filtered.length === 0 ? (
              <tbody>
                <tr>
                  <td colSpan={colCount + 1} style={{ padding: 0 }}>
                    <div className="empty">
                      <div className="ei"><Ic name="clipboardList" /></div>
                      <b>Nicio comanda.</b>
                      Adaugati o comanda de la un client.
                    </div>
                  </td>
                </tr>
              </tbody>
            ) : (
              <tbody>
                {filtered.map((o) => {
                  const chip = STATUS_CHIP[o.status] ?? { cls: "sent", labelKey: `orders.status.${o.status}` };
                  return (
                    <tr key={o.id}>
                      <td className="num">{o.fullNumber ?? `CMD-${String(o.number).padStart(4, "0")}`}</td>
                      <td>{fmtRoDate(o.orderDate)}</td>
                      <td>{o.contactId ? (contactMap[o.contactId]?.legalName ?? "—") : "—"}</td>
                      <td>{fmtRoDate(o.expectedDelivery)}</td>
                      <td className="r">{fmtRON(o.totalAmount)} {o.currency !== "RON" ? o.currency : ""}</td>
                      <td><span className={`chip ${chip.cls}`}>{t(chip.labelKey)}</span></td>
                      <td>
                        <button
                          className="sq-btn ghost"
                          onClick={(e) => handleOpenMenu(e, o)}
                          aria-label="Actiuni"
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
        {t("orders.banner.info")}
      </div>

      {menuOrder && (
        <RowActions
          order={menuOrder}
          companyId={activeCompanyId}
          anchor={menuAnchor}
          onEdit={() => setEditOrder(menuOrder)}
          onClose={() => { setMenuOrder(undefined); setMenuAnchor(null); }}
        />
      )}

      {(modalOpen || editOrder) && (
        <OrderModal
          companyId={activeCompanyId}
          order={editOrder}
          onClose={() => { setModalOpen(false); setEditOrder(undefined); }}
        />
      )}
    </div>
  );
}
