/**
 * Dezmembrări page — component recovery / dismantling records.
 * List + create form + post (fires GL 607 debit + 371 credit per component + 7588 diff).
 * Shows a GL preview before posting so the accountant can verify.
 */

import { useCallback, useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";
import type {
  Dezmembrare,
  DezmembrareWithLines,
  CreateDezmembrareInput,
  DezmembrareLineInput,
  Product,
} from "@/types";

// ─── helpers ──────────────────────────────────────────────────────────────────

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
const newId = () => Math.random().toString(36).slice(2);

interface CompRow {
  rowId: string;
  productId: string;
  qty: number;
  unitFairValue: number;
}
const emptyCompRow = (): CompRow => ({ rowId: newId(), productId: "", qty: 1, unitFairValue: 0 });

type TabFilter = "all" | "draft" | "posted";

const STATUS_CHIP: Record<string, { cls: string }> = {
  DRAFT:  { cls: "sent" },
  POSTED: { cls: "paid" },
};

// ─── GL preview helper (client-side approximation before posting) ─────────────

function buildGlPreviewRows(
  dismantledProduct: Product | undefined,
  _dismantledQty: number,
  compRows: CompRow[],
  products: Product[],
): { account: string; label: string; debit: number; credit: number }[] {
  if (!dismantledProduct) return [];
  // Carrying cost = stockQty present only if product has stock tracked; we can't know without
  // querying actual cost layers. We show the structure with "?" for the 607 row.
  const compTotal = compRows.reduce(
    (sum, r) => sum + r.qty * r.unitFairValue,
    0,
  );
  const rows: { account: string; label: string; debit: number; credit: number }[] = [];
  // 607 — debit (actual cost from stock, shown as computed by backend)
  rows.push({ account: "607", label: dismantledProduct.name, debit: 0, credit: 0 });
  // 371 — credit (from dismantled product cost, debit for components)
  for (const r of compRows) {
    if (!r.productId) continue;
    const prod = products.find((p) => p.id === r.productId);
    const total = r.qty * r.unitFairValue;
    rows.push({ account: "371", label: prod?.name ?? r.productId, debit: total, credit: 0 });
  }
  // 7588 — difference (credit)
  if (compTotal > 0) {
    rows.push({ account: "7588", label: "Diferență valoare", debit: 0, credit: compTotal });
  }
  return rows;
}

// ─── Create modal ─────────────────────────────────────────────────────────────

interface CreateModalProps {
  companyId: string;
  onClose: () => void;
}

function CreateModal({ companyId, onClose }: CreateModalProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { closing, close: animClose } = useAnimatedClose(onClose);

  const [date, setDate] = useState(localDateISO());
  const [gestiuneId, setGestiuneId] = useState("");
  const [dismantledProductId, setDismantledProductId] = useState("");
  const [dismantledQty, setDismantledQty] = useState<number>(1);
  const [notes, setNotes] = useState("");
  const [compRows, setCompRows] = useState<CompRow[]>([emptyCompRow()]);
  const [showGlPreview, setShowGlPreview] = useState(false);

  const { data: gestiuni = [] } = useQuery({
    queryKey: ["gestiuni", "list", companyId],
    queryFn: () => api.gestiuni.list(companyId),
    staleTime: 60_000,
  });

  const { data: products = [] } = useQuery({
    queryKey: ["products", "list", companyId, undefined],
    queryFn: () => api.products.list(companyId),
    staleTime: 60_000,
  });

  // filter to non-service (stocabile) only
  const stockProducts = useMemo(
    () => products.filter((p: Product) => !p.isService),
    [products],
  );

  const dismantledProduct = stockProducts.find((p: Product) => p.id === dismantledProductId);

  const glRows = useMemo(
    () => buildGlPreviewRows(dismantledProduct, dismantledQty, compRows, stockProducts),
    [dismantledProduct, dismantledQty, compRows, stockProducts],
  );

  const createMut = useMutation({
    mutationFn: (input: CreateDezmembrareInput) => api.dezmembrari.create(input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["dezmembrari", "list", companyId] });
      notify.success(t("dezmembrari.notify.created"));
      animClose();
    },
    onError: (e: unknown) => notify.error(t("dezmembrari.notify.createError") + " " + formatError(e)),
  });

  const handleSubmit = () => {
    if (!dismantledProductId) { notify.error(t("dezmembrari.validate.product")); return; }
    if (!dismantledQty || dismantledQty <= 0) { notify.error(t("dezmembrari.validate.qty")); return; }
    const validComps = compRows.filter((r) => r.productId && r.qty > 0 && r.unitFairValue >= 0);
    if (validComps.length === 0) { notify.error(t("dezmembrari.validate.components")); return; }

    const lines: DezmembrareLineInput[] = validComps.map((r) => ({
      productId: r.productId,
      qty: r.qty,
      unitFairValue: r.unitFairValue,
    }));

    createMut.mutate({
      companyId,
      gestiuneId: gestiuneId || null,
      dismantledProductId,
      dismantledQty,
      dezmembrareDate: date,
      notes: notes || null,
      lines,
    });
  };

  const updateComp = (rowId: string, field: keyof CompRow, value: string | number) => {
    setCompRows((rows) =>
      rows.map((r) => r.rowId === rowId ? { ...r, [field]: value } : r)
    );
  };

  const removeComp = (rowId: string) => {
    setCompRows((rows) => rows.filter((r) => r.rowId !== rowId));
  };

  const compTotal = compRows.reduce((s, r) => s + r.qty * r.unitFairValue, 0);

  return (
    <div className={`modal-overlay${closing ? " closing" : ""}`} onClick={animClose}>
      <div
        className="modal-panel"
        style={{ maxWidth: 780, width: "100%" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-head">
          <span className="modal-title">{t("dezmembrari.modal.createTitle")}</span>
          <button className="sq-btn ghost" onClick={animClose} aria-label={t("dezmembrari.modal.close")}>
            <svg width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" viewBox="0 0 24 24">
              <path d="M6 18 18 6M6 6l12 12"/>
            </svg>
          </button>
        </div>

        <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 16, padding: "20px 24px" }}>
          {/* Date + Gestiune */}
          <div style={{ display: "flex", gap: 12 }}>
            <div className="form-row" style={{ flex: 1 }}>
              <label className="form-label">{t("dezmembrari.modal.date")}</label>
              <input
                type="date"
                className="input"
                value={date}
                onChange={(e) => setDate(e.target.value)}
              />
            </div>
            <div className="form-row" style={{ flex: 2 }}>
              <label className="form-label">{t("dezmembrari.modal.gestiune")}</label>
              <select className="select" value={gestiuneId} onChange={(e) => setGestiuneId(e.target.value)}>
                <option value="">{t("dezmembrari.modal.gestiunePick")}</option>
                {gestiuni.map((g) => (
                  <option key={g.id} value={g.id}>{g.denumire}</option>
                ))}
              </select>
            </div>
          </div>

          {/* Dismantled product + qty */}
          <div style={{ display: "flex", gap: 12 }}>
            <div className="form-row" style={{ flex: 3 }}>
              <label className="form-label">{t("dezmembrari.modal.dismantledProduct")}</label>
              <select
                className="select"
                value={dismantledProductId}
                onChange={(e) => setDismantledProductId(e.target.value)}
              >
                <option value="">{t("dezmembrari.modal.dismantledProductPick")}</option>
                {stockProducts.map((p: Product) => (
                  <option key={p.id} value={p.id}>{p.name}</option>
                ))}
              </select>
            </div>
            <div className="form-row" style={{ flex: 1 }}>
              <label className="form-label">{t("dezmembrari.modal.dismantledQty")}</label>
              <input
                type="number"
                className="input"
                min="0.000001"
                step="0.000001"
                value={dismantledQty}
                onChange={(e) => setDismantledQty(parseFloat(e.target.value) || 0)}
              />
            </div>
          </div>

          {/* Component lines */}
          <div>
            <div className="form-label" style={{ marginBottom: 8 }}>{t("dezmembrari.modal.components")}</div>
            <table className="scr-table" style={{ marginBottom: 8 }}>
              <thead>
                <tr>
                  <th>{t("dezmembrari.modal.compProduct")}</th>
                  <th className="num" style={{ width: 100 }}>{t("dezmembrari.modal.compQty")}</th>
                  <th className="num" style={{ width: 140 }}>{t("dezmembrari.modal.compValue")}</th>
                  <th className="num" style={{ width: 100 }}>Total</th>
                  <th style={{ width: 36 }}></th>
                </tr>
              </thead>
              <tbody>
                {compRows.map((row) => (
                  <tr key={row.rowId}>
                    <td>
                      <select
                        className="select"
                        style={{ width: "100%" }}
                        value={row.productId}
                        onChange={(e) => updateComp(row.rowId, "productId", e.target.value)}
                      >
                        <option value="">{t("dezmembrari.modal.compProductPick")}</option>
                        {stockProducts.map((p: Product) => (
                          <option key={p.id} value={p.id}>{p.name}</option>
                        ))}
                      </select>
                    </td>
                    <td>
                      <input
                        type="number"
                        className="input"
                        style={{ textAlign: "right" }}
                        min="0"
                        step="0.000001"
                        value={row.qty}
                        onChange={(e) => updateComp(row.rowId, "qty", parseFloat(e.target.value) || 0)}
                      />
                    </td>
                    <td>
                      <input
                        type="number"
                        className="input"
                        style={{ textAlign: "right" }}
                        min="0"
                        step="0.01"
                        value={row.unitFairValue}
                        onChange={(e) => updateComp(row.rowId, "unitFairValue", parseFloat(e.target.value) || 0)}
                      />
                    </td>
                    <td className="num">{fmtRON(String(row.qty * row.unitFairValue))}</td>
                    <td>
                      <button
                        className="sq-btn ghost"
                        onClick={() => removeComp(row.rowId)}
                        disabled={compRows.length <= 1}
                        title="Elimină"
                      >
                        <svg width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.5" viewBox="0 0 24 24">
                          <path d="M6 18 18 6M6 6l12 12"/>
                        </svg>
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <button
              className="btn ghost"
              style={{ fontSize: 13 }}
              onClick={() => setCompRows((r) => [...r, emptyCompRow()])}
            >
              {t("dezmembrari.modal.addComponent")}
            </button>
            <div style={{ textAlign: "right", fontSize: 13, color: "var(--fg-muted)", marginTop: 4 }}>
              Total componente: <strong>{fmtRON(String(compTotal))} RON</strong>
            </div>
          </div>

          {/* GL preview toggle */}
          <div>
            <button
              className="btn ghost"
              style={{ fontSize: 13 }}
              onClick={() => setShowGlPreview((v) => !v)}
            >
              {showGlPreview ? "▲" : "▼"} {t("dezmembrari.glPreview.title")}
            </button>
            {showGlPreview && glRows.length > 0 && (
              <table className="scr-table" style={{ marginTop: 8, fontSize: 12 }}>
                <thead>
                  <tr>
                    <th style={{ width: 60 }}>{t("dezmembrari.glPreview.debit")}</th>
                    <th style={{ width: 60 }}>{t("dezmembrari.glPreview.credit")}</th>
                    <th>{t("dezmembrari.glPreview.desc")}</th>
                    <th className="num" style={{ width: 100 }}>{t("dezmembrari.glPreview.amount")}</th>
                  </tr>
                </thead>
                <tbody>
                  <tr>
                    <td>607</td>
                    <td>371</td>
                    <td>{dismantledProduct?.name ?? "—"} (cost de achiziție)</td>
                    <td className="num">— (calculat la postare)</td>
                  </tr>
                  {compRows.filter((r) => r.productId && r.qty > 0).map((r) => {
                    const prod = stockProducts.find((p: Product) => p.id === r.productId);
                    const total = r.qty * r.unitFairValue;
                    return (
                      <tr key={r.rowId}>
                        <td>371</td>
                        <td>607</td>
                        <td>{prod?.name ?? r.productId}</td>
                        <td className="num">{fmtRON(String(total))} RON</td>
                      </tr>
                    );
                  })}
                  {compTotal > 0 && (
                    <tr>
                      <td></td>
                      <td>7588</td>
                      <td>Diferență valoare</td>
                      <td className="num">{fmtRON(String(compTotal))} RON</td>
                    </tr>
                  )}
                </tbody>
              </table>
            )}
          </div>

          {/* Notes */}
          <div className="form-row">
            <label className="form-label">{t("dezmembrari.modal.notes")}</label>
            <textarea
              className="input"
              rows={2}
              placeholder={t("dezmembrari.modal.notesPlaceholder")}
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              style={{ resize: "vertical" }}
            />
          </div>
        </div>

        <div className="modal-foot" style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: "16px 24px" }}>
          <button className="btn ghost" onClick={animClose}>{t("dezmembrari.modal.close")}</button>
          <button className="btn-dark" onClick={handleSubmit} disabled={createMut.isPending}>
            {createMut.isPending ? t("dezmembrari.modal.saving") : t("dezmembrari.modal.create")}
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Detail modal (GL + lines view) ──────────────────────────────────────────

interface DetailModalProps {
  dwl: DezmembrareWithLines;
  products: Product[];
  onClose: () => void;
}

function DetailModal({ dwl, products, onClose }: DetailModalProps) {
  const { t } = useTranslation();
  const { dezmembrare: dz, lines } = dwl;

  const productName = (id: string) => products.find((p) => p.id === id)?.name ?? id;
  const compTotal = lines.reduce((s, l) => s + parseFloat(l.totalFairValue), 0);

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        className="modal-panel"
        style={{ maxWidth: 700, width: "100%" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-head">
          <span className="modal-title">{t("dezmembrari.title")} — {fmtRoDate(dz.dezmembrareDate)}</span>
          <button className="sq-btn ghost" onClick={onClose} aria-label="Închide">
            <svg width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" viewBox="0 0 24 24">
              <path d="M6 18 18 6M6 6l12 12"/>
            </svg>
          </button>
        </div>
        <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 16, padding: "20px 24px" }}>
          <div style={{ display: "flex", gap: 16, flexWrap: "wrap" }}>
            <div><span style={{ color: "var(--fg-muted)", fontSize: 12 }}>{t("dezmembrari.modal.dismantledProduct")}: </span><strong>{productName(dz.dismantledProductId)}</strong></div>
            <div><span style={{ color: "var(--fg-muted)", fontSize: 12 }}>Cantitate: </span><strong>{dz.dismantledQty}</strong></div>
            {dz.dismantledCarryingCost !== "0.00" && (
              <div><span style={{ color: "var(--fg-muted)", fontSize: 12 }}>Cost contabil: </span><strong>{fmtRON(dz.dismantledCarryingCost)} RON</strong></div>
            )}
          </div>

          <div className="form-label" style={{ marginBottom: 4 }}>{t("dezmembrari.modal.components")}</div>
          <table className="scr-table">
            <thead>
              <tr>
                <th>Produs</th>
                <th className="num">Cantitate</th>
                <th className="num">Val. justă / buc</th>
                <th className="num">Total</th>
              </tr>
            </thead>
            <tbody>
              {lines.map((l) => (
                <tr key={l.id}>
                  <td>{productName(l.productId)}</td>
                  <td className="num">{l.qty}</td>
                  <td className="num">{fmtRON(l.unitFairValue)} RON</td>
                  <td className="num">{fmtRON(l.totalFairValue)} RON</td>
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr>
                <td colSpan={3} style={{ textAlign: "right", fontWeight: 600 }}>Total componente</td>
                <td className="num" style={{ fontWeight: 700 }}>{fmtRON(String(compTotal))} RON</td>
              </tr>
            </tfoot>
          </table>

          {dz.status === "POSTED" && (
            <div className="banner info">
              Cost contabil debit 607: <strong>{fmtRON(dz.dismantledCarryingCost)} RON</strong>.
              Componente credit 371 total: <strong>{fmtRON(String(compTotal))} RON</strong>.
            </div>
          )}

          {dz.notes && (
            <div style={{ fontSize: 13, color: "var(--fg-muted)" }}>{dz.notes}</div>
          )}
        </div>
      </div>
    </div>
  );
}

// ─── Row actions ──────────────────────────────────────────────────────────────

interface RowActionsProps {
  dz: Dezmembrare;
  companyId: string;
  onView: () => void;
  onClose: () => void;
  anchor: DOMRect | null;
}

function RowActions({ dz, companyId, onView, onClose, anchor }: RowActionsProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [deleteConfirm, setDeleteConfirm] = useState(false);

  const postMut = useMutation({
    mutationFn: () => api.dezmembrari.post(companyId, dz.id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["dezmembrari", "list", companyId] });
      notify.success(t("dezmembrari.notify.posted"));
      onClose();
    },
    onError: (e: unknown) => { notify.error(t("dezmembrari.notify.postError") + " " + formatError(e)); onClose(); },
  });

  const style: React.CSSProperties = anchor
    ? { position: "fixed", top: anchor.bottom + 4, right: window.innerWidth - anchor.right, zIndex: 9999 }
    : { position: "fixed", top: 0, right: 0, zIndex: 9999 };

  return (
    <div className="pop" style={style}>
      <button className="pop-item" onClick={() => { onView(); onClose(); }}>Detalii</button>
      {dz.status === "DRAFT" && (
        <button className="pop-item" onClick={() => postMut.mutate()}>
          {t("dezmembrari.actions.post")}
        </button>
      )}
      {dz.status === "DRAFT" && (
        deleteConfirm
          ? <button className="pop-item danger" onClick={() => onClose()}>{t("dezmembrari.actions.confirmDelete")}</button>
          : <button className="pop-item danger" onClick={() => setDeleteConfirm(true)}>{t("dezmembrari.actions.delete")}</button>
      )}
    </div>
  );
}

// ─── Page ─────────────────────────────────────────────────────────────────────

export function DezmembrariPage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [tab, setTab] = useState<TabFilter>("all");
  const [search, setSearch] = useState("");
  const [modalOpen, setModalOpen] = useState(false);
  const [menuAnchor, setMenuAnchor] = useState<DOMRect | null>(null);
  const [menuDz, setMenuDz] = useState<Dezmembrare | undefined>();
  const [detailData, setDetailData] = useState<DezmembrareWithLines | null>(null);

  const { data: dezmembrari = [], isLoading, error } = useQuery({
    queryKey: ["dezmembrari", "list", activeCompanyId ?? ""],
    queryFn: () => api.dezmembrari.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const { data: products = [] } = useQuery({
    queryKey: ["products", "list", activeCompanyId, undefined],
    queryFn: () => api.products.list(activeCompanyId!),
    enabled: !!activeCompanyId,
    staleTime: 60_000,
  });

  const productName = (id: string) =>
    (products as Product[]).find((p) => p.id === id)?.name ?? id;

  const filtered = useMemo(() => {
    let items = dezmembrari;
    if (tab !== "all") items = items.filter((d) => d.status === tab.toUpperCase());
    if (search.trim()) {
      const s = search.toLowerCase();
      items = items.filter((d) =>
        productName(d.dismantledProductId).toLowerCase().includes(s) ||
        d.dezmembrareDate.includes(s)
      );
    }
    return items;
  }, [dezmembrari, tab, search, products]);

  const handleOpenMenu = useCallback((e: React.MouseEvent, d: Dezmembrare) => {
    e.stopPropagation();
    setMenuAnchor((e.currentTarget as HTMLElement).getBoundingClientRect());
    setMenuDz(d);
  }, []);

  const handleViewDetail = useCallback(async (d: Dezmembrare) => {
    if (!activeCompanyId) return;
    try {
      const dwl = await queryClient.fetchQuery({
        queryKey: ["dezmembrari", "detail", d.id],
        queryFn: () => api.dezmembrari.get(activeCompanyId, d.id),
        staleTime: 30_000,
      });
      setDetailData(dwl);
    } catch (err) {
      notify.error(formatError(err));
    }
  }, [activeCompanyId, queryClient]);

  if (!activeCompanyId) {
    return (
      <div className="page-body">
        <div className="banner info">{t("dezmembrari.selectCompany")}</div>
      </div>
    );
  }

  return (
    <div className="page-body">
      <div className="page-head">
        <div>
          <h1 className="page-title">{t("dezmembrari.title")}</h1>
          <div className="page-sub num">{filtered.length} {t("dezmembrari.title").toLowerCase()}</div>
        </div>
        <button className="btn-dark" onClick={() => setModalOpen(true)}>
          <Ic name="plus" />
          {t("dezmembrari.head.new")}
        </button>
      </div>

      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tabs">
            {(["all","draft","posted"] as TabFilter[]).map((tb) => (
              <button
                key={tb}
                className={`tab${tab === tb ? " active" : ""}`}
                onClick={() => setTab(tb)}
              >
                {t(`dezmembrari.tabs.${tb}`)}
              </button>
            ))}
          </div>
          <div className="spacer" />
          <div className="scr-search">
            <Ic name="search" />
            <input
              type="text"
              placeholder={t("dezmembrari.search")}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        </div>

        {isLoading && <div className="state-row">{t("dezmembrari.states.loading")}</div>}
        {error && <QueryErrorBanner label={t("dezmembrari.states.errorLabel")} error={error} />}

        {!isLoading && !error && (
          filtered.length === 0 ? (
            <div className="state-row muted">
              {search || tab !== "all" ? t("dezmembrari.states.emptyFiltered") : t("dezmembrari.states.emptyNone")}
            </div>
          ) : (
            <table className="scr-table">
              <thead>
                <tr>
                  <th>{t("dezmembrari.table.date")}</th>
                  <th>{t("dezmembrari.table.product")}</th>
                  <th className="num">{t("dezmembrari.table.qty")}</th>
                  <th className="num">{t("dezmembrari.table.carryingCost")}</th>
                  <th>{t("dezmembrari.table.status")}</th>
                  <th style={{ width: 40 }}></th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((d) => {
                  const chip = STATUS_CHIP[d.status] ?? { cls: "sent" };
                  return (
                    <tr key={d.id}>
                      <td>{fmtRoDate(d.dezmembrareDate)}</td>
                      <td>{productName(d.dismantledProductId)}</td>
                      <td className="num">{d.dismantledQty}</td>
                      <td className="num">
                        {d.status === "POSTED" ? `${fmtRON(d.dismantledCarryingCost)} RON` : "—"}
                      </td>
                      <td><span className={`chip ${chip.cls}`}>{t(`dezmembrari.status.${d.status}`)}</span></td>
                      <td>
                        <button
                          className="sq-btn ghost"
                          onClick={(e) => handleOpenMenu(e, d)}
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
            </table>
          )
        )}
      </div>

      {menuDz && (
        <RowActions
          dz={menuDz}
          companyId={activeCompanyId}
          anchor={menuAnchor}
          onView={() => void handleViewDetail(menuDz)}
          onClose={() => { setMenuDz(undefined); setMenuAnchor(null); }}
        />
      )}

      {modalOpen && (
        <CreateModal
          companyId={activeCompanyId}
          onClose={() => setModalOpen(false)}
        />
      )}

      {detailData && (
        <DetailModal
          dwl={detailData}
          products={products as Product[]}
          onClose={() => setDetailData(null)}
        />
      )}
    </div>
  );
}
