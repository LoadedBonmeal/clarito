/**
 * Producție / BOM (P2 Wave 5 + full-cost upgrade, OMFP 1802/2014 pct. 8, IAS 2).
 *
 * Monografie:
 *   Consum materie primă:       D 601 = C 301
 *   Consum material consumabil: D 602 = C 302
 *   Obținere produs finit:      D 345 = C 711  (la FULL COST: materiale + manoperă + regie absorbită)
 *
 * Costul 345 = materiale + manoperă directă + regie absorbită IAS 2.
 * Regia fixă neabsorbită rămâne cheltuiala perioadei (NU în 345).
 *
 * Trei view-uri:
 *   list   — tab BOM + tab Ordine producție
 *   bom    — creare / editare rețetă
 *   order  — lansare producție + detalii ordin (bon de consum 14-3-4A / bon de predare 14-3-3A)
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type {
  Bom,
  BomWithLines,
  BomInput,
  BomLineInput,
  ProductieOrder,
  ProduceInput,
  CreatePlannedOrderInput,
  CostEstimate,
  Product,
  Gestiune,
} from "@/types";

// ─── Types ────────────────────────────────────────────────────────────────────

type MainView = "list" | "bom-form" | "produce-form" | "planned-form" | "order-detail";

// ─── Status badge ─────────────────────────────────────────────────────────────

function StatusBadge({ status, t }: { status: string; t: (k: string) => string }) {
  const cfg: Record<string, { cls: string; label: string }> = {
    finalized: { cls: "bg-green-100 text-green-800", label: t("productie.order.statusFinalized") },
    planned: { cls: "bg-blue-100 text-blue-800", label: t("productie.order.statusPlanned") },
    in_progress: { cls: "bg-yellow-100 text-yellow-800", label: t("productie.order.statusInProgress") },
    cancelled: { cls: "bg-red-100 text-red-700", label: t("productie.order.statusCancelled") },
    draft: { cls: "bg-gray-100 text-gray-600", label: t("productie.order.statusDraft") },
  };
  const c = cfg[status] ?? { cls: "bg-gray-100 text-gray-600", label: status };
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium ${c.cls}`}>
      {c.label}
    </span>
  );
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function fmt(n: string | number) {
  const d = parseFloat(String(n));
  if (isNaN(d)) return n;
  return d.toLocaleString("ro-RO", { minimumFractionDigits: 2, maximumFractionDigits: 6 });
}

function fmt2(n: string | number) {
  const d = parseFloat(String(n));
  if (isNaN(d)) return n;
  return d.toLocaleString("ro-RO", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

// ─── BOM List tab ────────────────────────────────────────────────────────────

function BomListTab({
  companyId,
  products,
  onNew,
  onEdit,
}: {
  companyId: string;
  products: Product[];
  onNew: () => void;
  onEdit: (bom: Bom) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const { data: boms = [], isLoading, error } = useQuery({
    queryKey: ["bom", companyId],
    queryFn: () => api.productie.listBom(companyId),
    enabled: !!companyId,
  });

  const deleteMut = useMutation({
    mutationFn: (bomId: string) => api.productie.deleteBom(companyId, bomId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["bom", companyId] });
      notify.success(t("productie.bom.deleted"));
    },
    onError: (e) => notify.error(formatError(e)),
  });

  const pname = (id: string) => products.find((p) => p.id === id)?.name ?? id;

  if (isLoading) return <p className="text-muted-foreground text-sm">{t("productie.loading")}</p>;
  if (error) return <QueryErrorBanner error={error} label={t("productie.bom.title")} />;

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h2 className="text-lg font-semibold">{t("productie.bom.title")}</h2>
        <button
          onClick={onNew}
          className="btn btn-primary text-sm px-4 py-2"
        >
          {t("productie.bom.new")}
        </button>
      </div>
      {boms.length === 0 ? (
        <p className="text-muted-foreground text-sm">{t("productie.bom.empty")}</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="table-compact w-full text-sm">
            <thead>
              <tr>
                <th>{t("productie.bom.colName")}</th>
                <th>{t("productie.bom.colProduct")}</th>
                <th className="text-right">{t("productie.bom.colOutputQty")}</th>
                <th className="text-right">{t("productie.bom.colLines")}</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {boms.map((bom) => (
                <tr key={bom.id}>
                  <td className="font-medium">{bom.name}</td>
                  <td>{pname(bom.productId)}</td>
                  <td className="text-right">{fmt(bom.outputQty)}</td>
                  <td className="text-right">—</td>
                  <td className="text-right space-x-2">
                    <button
                      onClick={() => onEdit(bom)}
                      className="text-primary hover:underline text-xs"
                    >
                      {t("productie.bom.edit")}
                    </button>
                    <button
                      onClick={() => {
                        if (window.confirm(t("productie.bom.confirmDelete", { name: bom.name })))
                          deleteMut.mutate(bom.id);
                      }}
                      className="text-destructive hover:underline text-xs"
                    >
                      {t("productie.bom.delete")}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

// ─── Orders List tab ─────────────────────────────────────────────────────────

function OrdersListTab({
  companyId,
  products,
  gestiuni,
  boms,
  onNew,
  onNewPlanned,
  onView,
}: {
  companyId: string;
  products: Product[];
  gestiuni: Gestiune[];
  boms: Bom[];
  onNew: () => void;
  onNewPlanned: () => void;
  onView: (order: ProductieOrder) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const { data: orders = [], isLoading, error } = useQuery({
    queryKey: ["productie-orders", companyId],
    queryFn: () => api.productie.listOrders(companyId),
    enabled: !!companyId,
  });

  const executeMut = useMutation({
    mutationFn: (orderId: string) => api.productie.executeOrder(companyId, orderId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["productie-orders", companyId] });
      queryClient.invalidateQueries({ queryKey: ["products", companyId] });
      notify.success(t("productie.order.planned.executed"));
    },
    onError: (e) => notify.error(formatError(e)),
  });

  const cancelMut = useMutation({
    mutationFn: (orderId: string) => api.productie.cancelOrder(companyId, orderId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["productie-orders", companyId] });
      notify.success(t("productie.order.planned.cancelled"));
    },
    onError: (e) => notify.error(formatError(e)),
  });

  const pname = (id: string) => products.find((p) => p.id === id)?.name ?? id;
  const gname = (id: string) => gestiuni.find((g) => g.id === id)?.denumire ?? id;
  const bname = (id: string) => boms.find((b) => b.id === id)?.name ?? id;

  if (isLoading) return <p className="text-muted-foreground text-sm">{t("productie.loading")}</p>;
  if (error) return <QueryErrorBanner error={error} label={t("productie.order.title")} />;

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h2 className="text-lg font-semibold">{t("productie.order.title")}</h2>
        <div className="flex gap-2">
          <button
            onClick={onNewPlanned}
            className="btn btn-outline text-sm px-4 py-2"
          >
            {t("productie.order.newPlanned")}
          </button>
          <button
            onClick={onNew}
            className="btn btn-primary text-sm px-4 py-2"
          >
            {t("productie.order.new")}
          </button>
        </div>
      </div>
      {orders.length === 0 ? (
        <p className="text-muted-foreground text-sm">{t("productie.order.empty")}</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="table-compact w-full text-sm">
            <thead>
              <tr>
                <th>{t("productie.order.colStatus")}</th>
                <th>{t("productie.order.colDate")}</th>
                <th>{t("productie.order.colBom")}</th>
                <th>{t("productie.order.colProduct")}</th>
                <th>{t("productie.order.colGestiune")}</th>
                <th className="text-right">{t("productie.order.colQty")}</th>
                <th className="text-right">{t("productie.order.colFullCost")}</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {orders.map((order) => {
                const isActive = order.status === "planned" || order.status === "in_progress" || order.status === "draft";
                return (
                  <tr key={order.id} className={order.status === "cancelled" ? "opacity-50" : ""}>
                    <td><StatusBadge status={order.status} t={t} /></td>
                    <td>{order.plannedDate && order.status !== "finalized" ? `${order.plannedDate} (planif.)` : order.productionDate}</td>
                    <td>{bname(order.bomId)}</td>
                    <td>{pname(order.productId)}</td>
                    <td>{gname(order.gestiuneId)}</td>
                    <td className="text-right">{fmt(order.qtyProduced)}</td>
                    <td className="text-right">{order.status === "finalized" ? fmt2(order.fullCost) : "—"}</td>
                    <td className="text-right space-x-2 whitespace-nowrap">
                      {isActive && (
                        <>
                          <button
                            onClick={() => {
                              if (window.confirm(t("productie.order.planned.confirmExecute")))
                                executeMut.mutate(order.id);
                            }}
                            disabled={executeMut.isPending}
                            className="text-green-700 hover:underline text-xs font-medium"
                          >
                            {t("productie.order.planned.executeBtn")}
                          </button>
                          <button
                            onClick={() => {
                              if (window.confirm(t("productie.order.planned.confirmCancel")))
                                cancelMut.mutate(order.id);
                            }}
                            disabled={cancelMut.isPending}
                            className="text-destructive hover:underline text-xs"
                          >
                            {t("productie.order.planned.cancelBtn")}
                          </button>
                        </>
                      )}
                      {order.status === "finalized" && (
                        <button
                          onClick={() => onView(order)}
                          className="text-primary hover:underline text-xs"
                        >
                          {t("productie.order.viewDetail")}
                        </button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

// ─── BOM Form (create / edit) ─────────────────────────────────────────────────

function BomForm({
  companyId,
  products,
  editing,
  onDone,
}: {
  companyId: string;
  products: Product[];
  editing: BomWithLines | null;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const [name, setName] = useState(editing?.name ?? "");
  const [productId, setProductId] = useState(editing?.productId ?? "");
  const [outputQty, setOutputQty] = useState(editing?.outputQty ? String(parseFloat(editing.outputQty)) : "1");
  const [lines, setLines] = useState<BomLineInput[]>(
    editing?.lines.map((l) => ({
      componentProductId: l.componentProductId,
      qty: String(parseFloat(l.qty)),
      um: l.um ?? undefined,
      lineNo: l.lineNo,
    })) ?? [{ componentProductId: "", qty: "", lineNo: 1 }]
  );

  const saveMut = useMutation({
    mutationFn: (input: BomInput) =>
      editing
        ? api.productie.updateBom(companyId, editing.id, input)
        : api.productie.createBom(companyId, input),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["bom", companyId] });
      notify.success(t("productie.bom.saved"));
      onDone();
    },
    onError: (e) => notify.error(formatError(e)),
  });

  const addLine = () =>
    setLines((prev) => [
      ...prev,
      { componentProductId: "", qty: "", lineNo: prev.length + 1 },
    ]);

  const removeLine = (idx: number) =>
    setLines((prev) => prev.filter((_, i) => i !== idx).map((l, i) => ({ ...l, lineNo: i + 1 })));

  const updateLine = (idx: number, patch: Partial<BomLineInput>) =>
    setLines((prev) => prev.map((l, i) => (i === idx ? { ...l, ...patch } : l)));

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    saveMut.mutate({
      productId,
      name,
      outputQty,
      lines,
    });
  };

  // Filtrăm produsele disponibile ca produs finit (345) vs componente (301/302)
  const finishedProducts = products.filter(
    (p) => p.productType === "produs_finit" || p.stockAccount === "345"
  );
  const componentProducts = products.filter(
    (p) =>
      p.productType === "materie_prima" ||
      p.productType === "material_consumabil" ||
      p.stockAccount === "301" ||
      p.stockAccount === "302"
  );

  return (
    <form onSubmit={handleSubmit} className="space-y-6 max-w-2xl">
      <div className="flex items-center gap-2">
        <button type="button" onClick={onDone} className="text-muted-foreground hover:text-foreground text-sm">
          ← {t("productie.order.backToList")}
        </button>
        <span className="text-muted-foreground">/</span>
        <h2 className="text-lg font-semibold">
          {editing ? t("productie.bom.edit") : t("productie.bom.new")}
        </h2>
      </div>

      {/* Cap BOM */}
      <div className="grid gap-4">
        <div>
          <label className="label text-sm">{t("productie.bom.fieldName")}</label>
          <input
            className="input input-bordered w-full"
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
          />
        </div>
        <div>
          <label className="label text-sm">{t("productie.bom.fieldProduct")}</label>
          <select
            className="select select-bordered w-full"
            value={productId}
            onChange={(e) => setProductId(e.target.value)}
            required
          >
            <option value="">{t("productie.bom.selectProduct")}</option>
            {finishedProducts.map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
            {/* Fallback: toate produsele dacă filtrarea e goală */}
            {finishedProducts.length === 0 &&
              products.map((p) => (
                <option key={p.id} value={p.id}>{p.name}</option>
              ))}
          </select>
        </div>
        <div>
          <label className="label text-sm">{t("productie.bom.fieldOutputQty")}</label>
          <input
            className="input input-bordered w-32"
            type="number"
            min="0.000001"
            step="any"
            value={outputQty}
            onChange={(e) => setOutputQty(e.target.value)}
            required
          />
        </div>
      </div>

      {/* Linii componente */}
      <div>
        <div className="flex justify-between items-center mb-2">
          <h3 className="font-medium text-sm">{t("productie.bom.linesTitle")}</h3>
          <button type="button" onClick={addLine} className="btn btn-ghost btn-xs">
            + {t("productie.bom.addLine")}
          </button>
        </div>
        <div className="space-y-2">
          {lines.map((line, idx) => (
            <div key={idx} className="flex gap-2 items-center">
              <select
                className="select select-bordered flex-1 text-sm"
                value={line.componentProductId}
                onChange={(e) => updateLine(idx, { componentProductId: e.target.value })}
                required
              >
                <option value="">{t("productie.bom.selectProduct")}</option>
                {componentProducts.map((p) => (
                  <option key={p.id} value={p.id}>{p.name}</option>
                ))}
                {componentProducts.length === 0 &&
                  products.map((p) => (
                    <option key={p.id} value={p.id}>{p.name}</option>
                  ))}
              </select>
              <input
                className="input input-bordered w-24 text-sm"
                type="number"
                min="0.000001"
                step="any"
                placeholder={t("productie.bom.lineQty")}
                value={line.qty}
                onChange={(e) => updateLine(idx, { qty: e.target.value })}
                required
              />
              <input
                className="input input-bordered w-16 text-sm"
                placeholder={t("productie.bom.lineUm")}
                value={line.um ?? ""}
                onChange={(e) => updateLine(idx, { um: e.target.value || undefined })}
              />
              {lines.length > 1 && (
                <button
                  type="button"
                  onClick={() => removeLine(idx)}
                  className="text-destructive text-xs hover:underline shrink-0"
                >
                  {t("productie.bom.removeLine")}
                </button>
              )}
            </div>
          ))}
        </div>
      </div>

      <div className="flex gap-3">
        <button
          type="submit"
          disabled={saveMut.isPending}
          className="btn btn-primary"
        >
          {saveMut.isPending ? t("productie.bom.saving") : t("productie.bom.submit")}
        </button>
        <button type="button" onClick={onDone} className="btn btn-ghost">
          {t("productie.bom.cancel")}
        </button>
      </div>
    </form>
  );
}

// ─── Lansare producție form ───────────────────────────────────────────────────

function ProduceForm({
  companyId,
  boms,
  gestiuni,
  products,
  onDone,
}: {
  companyId: string;
  boms: Bom[];
  gestiuni: Gestiune[];
  products: Product[];
  onDone: (order?: ProductieOrder) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const [bomId, setBomId] = useState("");
  const [gestiuneId, setGestiuneId] = useState(gestiuni[0]?.id ?? "");
  const [qtyProduced, setQtyProduced] = useState("");
  const [productionDate, setProductionDate] = useState(new Date().toISOString().slice(0, 10));
  const [notes, setNotes] = useState("");
  // Full-cost fields
  const [labourCost, setLabourCost] = useState("0");
  const [overheadCost, setOverheadCost] = useState("0");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [overheadFixed, setOverheadFixed] = useState("");
  const [overheadVariable, setOverheadVariable] = useState("");
  const [normalCapacityQty, setNormalCapacityQty] = useState("");

  // Preview BOM lines pentru bomul selectat
  const { data: selectedBom } = useQuery({
    queryKey: ["bom-detail", companyId, bomId],
    queryFn: () => api.productie.getBom(companyId, bomId),
    enabled: !!companyId && !!bomId,
  });

  const pname = (id: string) => products.find((p) => p.id === id)?.name ?? id;

  const produceMut = useMutation({
    mutationFn: (input: ProduceInput) => api.productie.produce(companyId, input),
    onSuccess: (order) => {
      queryClient.invalidateQueries({ queryKey: ["productie-orders", companyId] });
      queryClient.invalidateQueries({ queryKey: ["products", companyId] });
      notify.success(t("productie.order.saved"));
      onDone(order);
    },
    onError: (e) => notify.error(formatError(e)),
  });

  // Compute preview of full cost and unabsorbed overhead for UI display
  const previewCost = (() => {
    const mat = 0; // unknown until production runs (depends on FIFO/CMP valuation)
    const labour = parseFloat(labourCost) || 0;
    const ovhd = parseFloat(overheadCost) || 0;
    const ovFixed = parseFloat(overheadFixed) || 0;
    const ovVar = parseFloat(overheadVariable) || 0;
    const normCap = parseFloat(normalCapacityQty) || 0;
    const qty = parseFloat(qtyProduced) || 0;

    let absorbed = ovhd;
    let unabsorbed = 0;
    if (showAdvanced && ovFixed > 0 && normCap > 0 && qty > 0) {
      const ratio = Math.min(1, qty / normCap);
      const varPart = ovVar || (ovhd - ovFixed);
      absorbed = (ovVar >= 0 ? ovVar : 0) + round2_js(ovFixed * ratio);
      unabsorbed = round2_js(ovFixed * (1 - ratio));
      void varPart; // suppress unused
    } else if (showAdvanced && (ovFixed > 0 || ovVar > 0)) {
      absorbed = ovFixed + ovVar;
      unabsorbed = 0;
    }
    return { labour, absorbed, unabsorbed, mat };
  })();

  function round2_js(n: number) {
    return Math.round(n * 100) / 100;
  }

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    produceMut.mutate({
      bomId,
      gestiuneId,
      qtyProduced,
      productionDate,
      notes: notes || undefined,
      labourCost: labourCost || "0",
      overheadCost: overheadCost || "0",
      overheadFixed: showAdvanced && overheadFixed ? overheadFixed : undefined,
      overheadVariable: showAdvanced && overheadVariable ? overheadVariable : undefined,
      normalCapacityQty: showAdvanced && normalCapacityQty ? normalCapacityQty : undefined,
    });
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-6 max-w-2xl">
      <div className="flex items-center gap-2">
        <button type="button" onClick={() => onDone()} className="text-muted-foreground hover:text-foreground text-sm">
          ← {t("productie.order.backToList")}
        </button>
        <span className="text-muted-foreground">/</span>
        <h2 className="text-lg font-semibold">{t("productie.order.new")}</h2>
      </div>

      <div className="grid gap-4">
        <div>
          <label className="label text-sm">{t("productie.order.fieldBom")}</label>
          <select
            className="select select-bordered w-full"
            value={bomId}
            onChange={(e) => setBomId(e.target.value)}
            required
          >
            <option value="">{t("productie.order.selectBom")}</option>
            {boms.map((b) => (
              <option key={b.id} value={b.id}>{b.name} — {pname(b.productId)}</option>
            ))}
          </select>
        </div>

        {selectedBom && (
          <div className="rounded-lg border p-3 bg-muted/30 text-sm space-y-1">
            <p className="font-medium">
              {t("productie.bom.linesTitle")} ({pname(selectedBom.productId)}):
            </p>
            {selectedBom.lines.map((l) => (
              <div key={l.id} className="flex gap-2 text-xs text-muted-foreground">
                <span>{l.lineNo}.</span>
                <span>{pname(l.componentProductId)}</span>
                <span>×</span>
                <span>{fmt(l.qty)} {l.um ?? ""}</span>
              </div>
            ))}
            <p className="text-xs text-muted-foreground">
              {t("productie.bom.fieldOutputQty")}: {fmt(selectedBom.outputQty)}
            </p>
          </div>
        )}

        <div>
          <label className="label text-sm">{t("productie.order.fieldGestiune")}</label>
          <select
            className="select select-bordered w-full"
            value={gestiuneId}
            onChange={(e) => setGestiuneId(e.target.value)}
            required
          >
            <option value="">{t("productie.order.selectGestiune")}</option>
            {gestiuni.map((g) => (
              <option key={g.id} value={g.id}>{g.denumire}</option>
            ))}
          </select>
        </div>

        <div>
          <label className="label text-sm">{t("productie.order.fieldQty")}</label>
          <input
            className="input input-bordered w-40"
            type="number"
            min="0.000001"
            step="any"
            value={qtyProduced}
            onChange={(e) => setQtyProduced(e.target.value)}
            required
          />
        </div>

        <div>
          <label className="label text-sm">{t("productie.order.fieldDate")}</label>
          <input
            className="input input-bordered w-44"
            type="date"
            value={productionDate}
            onChange={(e) => setProductionDate(e.target.value)}
            required
          />
        </div>

        <div>
          <label className="label text-sm">{t("productie.order.fieldNotes")}</label>
          <textarea
            className="textarea textarea-bordered w-full"
            rows={2}
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
          />
        </div>

        {/* Manoperă directă */}
        <div>
          <label className="label text-sm">{t("productie.order.fieldLabourCost")}</label>
          <input
            className="input input-bordered w-44"
            type="number"
            min="0"
            step="0.01"
            value={labourCost}
            onChange={(e) => setLabourCost(e.target.value)}
          />
        </div>

        {/* Regie */}
        <div>
          <label className="label text-sm">{t("productie.order.fieldOverheadCost")}</label>
          <input
            className="input input-bordered w-44"
            type="number"
            min="0"
            step="0.01"
            value={overheadCost}
            onChange={(e) => setOverheadCost(e.target.value)}
          />
        </div>

        {/* Advanced: split fix/variabil + capacitate normală */}
        <div>
          <button
            type="button"
            className="text-xs text-primary hover:underline"
            onClick={() => setShowAdvanced((v) => !v)}
          >
            {showAdvanced ? "▲ " : "▶ "}{t("productie.order.advancedAbsorption")}
          </button>
          {showAdvanced && (
            <div className="mt-3 space-y-3 pl-3 border-l-2 border-muted">
              <div>
                <label className="label text-xs">{t("productie.order.fieldOverheadFixed")}</label>
                <input
                  className="input input-bordered w-44 text-sm"
                  type="number"
                  min="0"
                  step="0.01"
                  placeholder="0"
                  value={overheadFixed}
                  onChange={(e) => setOverheadFixed(e.target.value)}
                />
              </div>
              <div>
                <label className="label text-xs">{t("productie.order.fieldOverheadVariable")}</label>
                <input
                  className="input input-bordered w-44 text-sm"
                  type="number"
                  min="0"
                  step="0.01"
                  placeholder="0"
                  value={overheadVariable}
                  onChange={(e) => setOverheadVariable(e.target.value)}
                />
              </div>
              <div>
                <label className="label text-xs">{t("productie.order.fieldNormalCapacity")}</label>
                <input
                  className="input input-bordered w-44 text-sm"
                  type="number"
                  min="0"
                  step="any"
                  placeholder="ex: 100"
                  value={normalCapacityQty}
                  onChange={(e) => setNormalCapacityQty(e.target.value)}
                />
              </div>
              {/* Preview regie fixă neabsorbită */}
              {previewCost.unabsorbed > 0 && (
                <p className="text-xs text-amber-600 bg-amber-50 rounded p-2">
                  {t("productie.order.unabsorbedPreview", { amount: previewCost.unabsorbed.toFixed(2) })}
                </p>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Notă cost complet */}
      <p className="text-xs text-muted-foreground bg-muted/30 rounded p-2">
        {t("productie.order.fullCostNote")}
      </p>

      <div className="flex gap-3">
        <button
          type="submit"
          disabled={produceMut.isPending}
          className="btn btn-primary"
        >
          {produceMut.isPending ? t("productie.order.saving") : t("productie.order.submit")}
        </button>
        <button type="button" onClick={() => onDone()} className="btn btn-ghost">
          {t("productie.order.cancel")}
        </button>
      </div>
    </form>
  );
}

// ─── Planned order form ───────────────────────────────────────────────────────

function PlannedOrderForm({
  companyId,
  boms,
  gestiuni,
  products,
  onDone,
}: {
  companyId: string;
  boms: Bom[];
  gestiuni: Gestiune[];
  products: Product[];
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const [bomId, setBomId] = useState("");
  const [gestiuneId, setGestiuneId] = useState(gestiuni[0]?.id ?? "");
  const [qtyProduced, setQtyProduced] = useState("");
  const [plannedDate, setPlannedDate] = useState(new Date().toISOString().slice(0, 10));
  const [notes, setNotes] = useState("");
  const [labourCost, setLabourCost] = useState("0");
  const [overheadCost, setOverheadCost] = useState("0");

  const [estimate, setEstimate] = useState<CostEstimate | null>(null);

  const { data: selectedBom } = useQuery({
    queryKey: ["bom-detail", companyId, bomId],
    queryFn: () => api.productie.getBom(companyId, bomId),
    enabled: !!companyId && !!bomId,
  });

  const pname = (id: string) => products.find((p) => p.id === id)?.name ?? id;

  const planMut = useMutation({
    mutationFn: (input: CreatePlannedOrderInput) =>
      api.productie.createPlannedOrder(companyId, input),
    onSuccess: ([, est]) => {
      queryClient.invalidateQueries({ queryKey: ["productie-orders", companyId] });
      setEstimate(est);
      notify.success(t("productie.order.planned.saved"));
      onDone();
    },
    onError: (e) => notify.error(formatError(e)),
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    planMut.mutate({
      bomId,
      gestiuneId,
      qtyProduced,
      plannedDate,
      notes: notes || undefined,
      labourCost: labourCost || "0",
      overheadCost: overheadCost || "0",
    });
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-6 max-w-2xl">
      <div className="flex items-center gap-2">
        <button type="button" onClick={onDone} className="text-muted-foreground hover:text-foreground text-sm">
          ← {t("productie.order.backToList")}
        </button>
        <span className="text-muted-foreground">/</span>
        <h2 className="text-lg font-semibold">{t("productie.order.planned.title")}</h2>
      </div>

      <div className="grid gap-4">
        <div>
          <label className="label text-sm">{t("productie.order.fieldBom")}</label>
          <select
            className="select select-bordered w-full"
            value={bomId}
            onChange={(e) => setBomId(e.target.value)}
            required
          >
            <option value="">{t("productie.order.selectBom")}</option>
            {boms.map((b) => (
              <option key={b.id} value={b.id}>{b.name} — {pname(b.productId)}</option>
            ))}
          </select>
        </div>

        {selectedBom && (
          <div className="rounded-lg border p-3 bg-muted/30 text-sm space-y-1">
            <p className="font-medium">{pname(selectedBom.productId)}</p>
            {selectedBom.lines.map((l) => (
              <div key={l.id} className="text-xs text-muted-foreground flex gap-2">
                <span>{l.lineNo}.</span>
                <span>{pname(l.componentProductId)}</span>
                <span>×</span>
                <span>{fmt(l.qty)} {l.um ?? ""}</span>
              </div>
            ))}
          </div>
        )}

        <div>
          <label className="label text-sm">{t("productie.order.fieldGestiune")}</label>
          <select
            className="select select-bordered w-full"
            value={gestiuneId}
            onChange={(e) => setGestiuneId(e.target.value)}
            required
          >
            <option value="">{t("productie.order.selectGestiune")}</option>
            {gestiuni.map((g) => (
              <option key={g.id} value={g.id}>{g.denumire}</option>
            ))}
          </select>
        </div>

        <div>
          <label className="label text-sm">{t("productie.order.fieldQty")}</label>
          <input
            className="input input-bordered w-40"
            type="number"
            min="0.000001"
            step="any"
            value={qtyProduced}
            onChange={(e) => setQtyProduced(e.target.value)}
            required
          />
        </div>

        <div>
          <label className="label text-sm">{t("productie.order.fieldPlannedDate")}</label>
          <input
            className="input input-bordered w-44"
            type="date"
            value={plannedDate}
            onChange={(e) => setPlannedDate(e.target.value)}
            required
          />
        </div>

        <div>
          <label className="label text-sm">{t("productie.order.fieldLabourCost")}</label>
          <input
            className="input input-bordered w-44"
            type="number"
            min="0"
            step="0.01"
            value={labourCost}
            onChange={(e) => setLabourCost(e.target.value)}
          />
        </div>

        <div>
          <label className="label text-sm">{t("productie.order.fieldOverheadCost")}</label>
          <input
            className="input input-bordered w-44"
            type="number"
            min="0"
            step="0.01"
            value={overheadCost}
            onChange={(e) => setOverheadCost(e.target.value)}
          />
        </div>

        <div>
          <label className="label text-sm">{t("productie.order.fieldNotes")}</label>
          <textarea
            className="textarea textarea-bordered w-full"
            rows={2}
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
          />
        </div>
      </div>

      {estimate && (
        <div className="rounded-lg border p-3 bg-blue-50 text-sm space-y-1">
          <p className="font-medium text-blue-800">{t("productie.order.planned.estimateTitle")}</p>
          <div className="flex justify-between text-xs text-blue-700">
            <span>{t("productie.order.planned.estimateMat")}</span>
            <span>{fmt2(estimate.estimatedMaterialCost)} RON</span>
          </div>
          <div className="flex justify-between text-xs text-blue-700">
            <span>{t("productie.order.planned.estimateLabour")}</span>
            <span>{fmt2(estimate.labourCost)} RON</span>
          </div>
          <div className="flex justify-between text-xs text-blue-700 font-semibold">
            <span>{t("productie.order.planned.estimateFull")}</span>
            <span>{fmt2(estimate.estimatedFullCost)} RON</span>
          </div>
        </div>
      )}

      <div className="flex gap-3">
        <button
          type="submit"
          disabled={planMut.isPending}
          className="btn btn-primary"
        >
          {planMut.isPending ? t("productie.order.planned.saving") : t("productie.order.planned.submit")}
        </button>
        <button type="button" onClick={onDone} className="btn btn-ghost">
          {t("productie.order.cancel")}
        </button>
      </div>
    </form>
  );
}

// ─── Order detail + print ─────────────────────────────────────────────────────

function OrderDetail({
  companyId,
  order,
  products,
  gestiuni,
  boms,
  onBack,
}: {
  companyId: string;
  order: ProductieOrder;
  products: Product[];
  gestiuni: Gestiune[];
  boms: Bom[];
  onBack: () => void;
}) {
  const { t } = useTranslation();

  const { data: bomDetail } = useQuery({
    queryKey: ["bom-detail", companyId, order.bomId],
    queryFn: () => api.productie.getBom(companyId, order.bomId),
    enabled: !!companyId && !!order.bomId,
  });

  const pname = (id: string) => products.find((p) => p.id === id)?.name ?? id;
  const gname = (id: string) => gestiuni.find((g) => g.id === id)?.denumire ?? id;
  const bname = (id: string) => boms.find((b) => b.id === id)?.name ?? id;

  const scale =
    bomDetail && parseFloat(bomDetail.outputQty) > 0
      ? parseFloat(order.qtyProduced) / parseFloat(bomDetail.outputQty)
      : 1;

  // ── Bon de consum (14-3-4A) HTML pentru print ──
  const printBonConsum = () => {
    if (!bomDetail) return;
    const win = window.open("", "_blank");
    if (!win) return;
    const totalVal = parseFloat(order.totalMaterialCost).toFixed(2);
    const linesHtml = bomDetail.lines
      .map((l, i) => {
        const qty = (parseFloat(l.qty) * scale).toFixed(6);
        return `<tr>
          <td>${i + 1}</td>
          <td>${pname(l.componentProductId)}</td>
          <td></td>
          <td>${l.um ?? ""}</td>
          <td style="text-align:right">${parseFloat(qty).toLocaleString("ro-RO", { minimumFractionDigits: 2 })}</td>
          <td style="text-align:right">—</td>
          <td style="text-align:right">—</td>
        </tr>`;
      })
      .join("");
    win.document.write(`<!DOCTYPE html><html><head><meta charset="utf-8">
<title>Bon de consum</title>
<style>
body{font-family:Arial,sans-serif;font-size:11px;margin:20px}
h2{text-align:center;font-size:13px}
table{width:100%;border-collapse:collapse;margin-top:12px}
th,td{border:1px solid #000;padding:3px 5px}
th{background:#eee;text-align:center}
.footer{margin-top:24px;display:flex;gap:40px}
.footer div{flex:1;border-top:1px solid #000;padding-top:4px;text-align:center}
</style></head><body>
<h2>${t("productie.print.bonConsum")}</h2>
<p>${t("productie.print.date")}: <strong>${order.productionDate}</strong> &nbsp;|&nbsp;
   ${t("productie.print.gestiune")}: <strong>${gname(order.gestiuneId)}</strong> &nbsp;|&nbsp;
   ${t("productie.print.orderRef")}: <strong>${order.id.slice(-8).toUpperCase()}</strong></p>
<table>
<thead><tr>
<th>${t("productie.print.colNo")}</th>
<th>${t("productie.print.colDenumire")}</th>
<th>${t("productie.print.colCod")}</th>
<th>${t("productie.print.colUm")}</th>
<th>${t("productie.print.colQty")}</th>
<th>${t("productie.print.colUnitCost")}</th>
<th>${t("productie.print.colValue")}</th>
</tr></thead>
<tbody>${linesHtml}</tbody>
<tfoot><tr><td colspan="6" style="text-align:right"><strong>${t("productie.print.totalValue")}</strong></td>
<td style="text-align:right"><strong>${parseFloat(totalVal).toLocaleString("ro-RO",{minimumFractionDigits:2})}</strong></td></tr></tfoot>
</table>
<div class="footer">
<div>${t("productie.print.semnEliberat")}<br>${t("productie.print.semnatura")}</div>
<div>${t("productie.print.semnPrimitor")}<br>${t("productie.print.semnatura")}</div>
</div>
</body></html>`);
    win.document.close();
    win.print();
  };

  // ── Bon de predare produse (14-3-3A) HTML pentru print ──
  const printBonPredare = () => {
    const win = window.open("", "_blank");
    if (!win) return;
    win.document.write(`<!DOCTYPE html><html><head><meta charset="utf-8">
<title>Bon de predare</title>
<style>
body{font-family:Arial,sans-serif;font-size:11px;margin:20px}
h2{text-align:center;font-size:13px}
table{width:100%;border-collapse:collapse;margin-top:12px}
th,td{border:1px solid #000;padding:3px 5px}
th{background:#eee;text-align:center}
.footer{margin-top:24px;display:flex;gap:40px}
.footer div{flex:1;border-top:1px solid #000;padding-top:4px;text-align:center}
</style></head><body>
<h2>${t("productie.print.bonPredare")}</h2>
<p>${t("productie.print.date")}: <strong>${order.productionDate}</strong> &nbsp;|&nbsp;
   ${t("productie.print.gestiune")}: <strong>${gname(order.gestiuneId)}</strong> &nbsp;|&nbsp;
   ${t("productie.print.orderRef")}: <strong>${order.id.slice(-8).toUpperCase()}</strong></p>
<table>
<thead><tr>
<th>${t("productie.print.colNo")}</th>
<th>${t("productie.print.colDenumire")}</th>
<th>${t("productie.print.colCod")}</th>
<th>${t("productie.print.colUm")}</th>
<th>${t("productie.print.colQty")}</th>
<th>${t("productie.print.colUnitCost")}</th>
<th>${t("productie.print.colValue")}</th>
</tr></thead>
<tbody>
<tr>
  <td>1</td>
  <td>${pname(order.productId)}</td>
  <td></td>
  <td>buc</td>
  <td style="text-align:right">${parseFloat(order.qtyProduced).toLocaleString("ro-RO",{minimumFractionDigits:2})}</td>
  <td style="text-align:right">${parseFloat(order.unitCost).toLocaleString("ro-RO",{minimumFractionDigits:2})}</td>
  <td style="text-align:right">${parseFloat(order.totalMaterialCost).toLocaleString("ro-RO",{minimumFractionDigits:2})}</td>
</tr>
</tbody>
<tfoot><tr><td colspan="6" style="text-align:right"><strong>${t("productie.print.totalValue")}</strong></td>
<td style="text-align:right"><strong>${parseFloat(order.totalMaterialCost).toLocaleString("ro-RO",{minimumFractionDigits:2})}</strong></td></tr></tfoot>
</table>
<div class="footer">
<div>${t("productie.print.semnEliberat")}<br>${t("productie.print.semnatura")}</div>
<div>${t("productie.print.semnPrimitor")}<br>${t("productie.print.semnatura")}</div>
</div>
</body></html>`);
    win.document.close();
    win.print();
  };

  return (
    <div className="space-y-6 max-w-3xl">
      <div className="flex items-center gap-2">
        <button onClick={onBack} className="text-muted-foreground hover:text-foreground text-sm">
          ← {t("productie.order.backToList")}
        </button>
        <span className="text-muted-foreground">/</span>
        <h2 className="text-lg font-semibold">{t("productie.order.viewDetail")}</h2>
      </div>

      {/* Rezumat ordin */}
      <div className="rounded-lg border p-4 space-y-2 text-sm">
        <div className="grid grid-cols-2 gap-2">
          <div><span className="text-muted-foreground">{t("productie.order.colDate")}:</span> <strong>{order.productionDate}</strong></div>
          <div><span className="text-muted-foreground">{t("productie.order.colBom")}:</span> <strong>{bname(order.bomId)}</strong></div>
          <div><span className="text-muted-foreground">{t("productie.order.colProduct")}:</span> <strong>{pname(order.productId)}</strong></div>
          <div><span className="text-muted-foreground">{t("productie.order.colGestiune")}:</span> <strong>{gname(order.gestiuneId)}</strong></div>
          <div><span className="text-muted-foreground">{t("productie.order.colQty")}:</span> <strong>{fmt(order.qtyProduced)}</strong></div>
          <div><span className="text-muted-foreground">{t("productie.order.colCostTotal")}:</span> <strong>{fmt2(order.totalMaterialCost)} RON</strong></div>
          <div><span className="text-muted-foreground">{t("productie.order.colLabourCost")}:</span> <strong>{fmt2(order.labourCost)} RON</strong></div>
          <div><span className="text-muted-foreground">{t("productie.order.colOverheadAbsorbed")}:</span> <strong>{fmt2(order.overheadAbsorbed)} RON</strong></div>
          {parseFloat(order.overheadUnabsorbed) > 0 && (
            <div className="col-span-2">
              <span className="text-amber-600 text-xs">{t("productie.order.colOverheadUnabsorbed")}:</span>{" "}
              <strong className="text-amber-600">{fmt2(order.overheadUnabsorbed)} RON</strong>
              <span className="text-xs text-muted-foreground ml-1">({t("productie.order.unabsorbedIsExpense")})</span>
            </div>
          )}
          <div><span className="text-muted-foreground font-semibold">{t("productie.order.colFullCost")}:</span> <strong>{fmt2(order.fullCost)} RON</strong></div>
          <div><span className="text-muted-foreground font-semibold">{t("productie.order.colFullUnitCost")}:</span> <strong>{fmt2(order.fullUnitCost)} RON</strong></div>
        </div>
        {order.notes && (
          <div className="text-muted-foreground text-xs">{order.notes}</div>
        )}
        <p className="text-xs text-muted-foreground bg-muted/30 rounded p-2 mt-2">
          {t("productie.order.fullCostNote")}
        </p>
      </div>

      {/* Consum componente */}
      {bomDetail && (
        <div>
          <h3 className="font-medium text-sm mb-2">{t("productie.print.bonConsum")}</h3>
          <table className="table-compact w-full text-sm">
            <thead>
              <tr>
                <th>{t("productie.print.colNo")}</th>
                <th>{t("productie.print.colDenumire")}</th>
                <th>{t("productie.print.colUm")}</th>
                <th className="text-right">{t("productie.print.colQty")}</th>
              </tr>
            </thead>
            <tbody>
              {bomDetail.lines.map((l, i) => (
                <tr key={l.id}>
                  <td>{i + 1}</td>
                  <td>{pname(l.componentProductId)}</td>
                  <td>{l.um ?? ""}</td>
                  <td className="text-right">{fmt(parseFloat(l.qty) * scale)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Buttons print */}
      <div className="flex gap-3">
        <button onClick={printBonConsum} className="btn btn-outline btn-sm">
          {t("productie.print.printBonConsum")}
        </button>
        <button onClick={printBonPredare} className="btn btn-outline btn-sm">
          {t("productie.print.printBonPredare")}
        </button>
      </div>
    </div>
  );
}

// ─── Main page ────────────────────────────────────────────────────────────────

export function ProductiePage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const companyId = activeCompanyId ?? "";

  const [view, setView] = useState<MainView>("list");
  const [activeTab, setActiveTab] = useState<"bom" | "orders">("bom");
  const [editingBom, setEditingBom] = useState<BomWithLines | null>(null);
  const [viewingOrder, setViewingOrder] = useState<ProductieOrder | null>(null);

  const { data: products = [] } = useQuery({
    queryKey: ["products", companyId],
    queryFn: () => api.products.list(companyId),
    enabled: !!companyId,
  });

  const { data: gestiuni = [] } = useQuery({
    queryKey: ["gestiuni", companyId],
    queryFn: () => api.gestiuni.list(companyId),
    enabled: !!companyId,
  });

  const { data: boms = [] } = useQuery({
    queryKey: ["bom", companyId],
    queryFn: () => api.productie.listBom(companyId),
    enabled: !!companyId,
  });

  if (!companyId) {
    return <p className="text-muted-foreground text-sm p-4">{t("productie.selectCompany")}</p>;
  }

  // ── BOM Form ──
  if (view === "bom-form") {
    return (
      <div className="p-4">
        <BomForm
          companyId={companyId}
          products={products}
          editing={editingBom}
          onDone={() => {
            setEditingBom(null);
            setView("list");
          }}
        />
      </div>
    );
  }

  // ── Lansare producție ──
  if (view === "produce-form") {
    return (
      <div className="p-4">
        <ProduceForm
          companyId={companyId}
          boms={boms}
          gestiuni={gestiuni}
          products={products}
          onDone={(order) => {
            if (order) {
              setViewingOrder(order);
              setView("order-detail");
            } else {
              setView("list");
            }
          }}
        />
      </div>
    );
  }

  // ── Comandă planificată ──
  if (view === "planned-form") {
    return (
      <div className="p-4">
        <PlannedOrderForm
          companyId={companyId}
          boms={boms}
          gestiuni={gestiuni}
          products={products}
          onDone={() => {
            setView("list");
            setActiveTab("orders");
          }}
        />
      </div>
    );
  }

  // ── Detalii ordin ──
  if (view === "order-detail" && viewingOrder) {
    return (
      <div className="p-4">
        <OrderDetail
          companyId={companyId}
          order={viewingOrder}
          products={products}
          gestiuni={gestiuni}
          boms={boms}
          onBack={() => {
            setViewingOrder(null);
            setView("list");
            setActiveTab("orders");
          }}
        />
      </div>
    );
  }

  // ── List (tab-uri BOM + Ordine) ──
  return (
    <div className="p-4 space-y-4">
      <h1 className="text-2xl font-bold">{t("productie.title")}</h1>

      {/* Tab bar */}
      <div className="flex gap-1 border-b">
        <button
          onClick={() => setActiveTab("bom")}
          className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
            activeTab === "bom"
              ? "border-primary text-primary"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          {t("productie.tabBom")}
        </button>
        <button
          onClick={() => setActiveTab("orders")}
          className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
            activeTab === "orders"
              ? "border-primary text-primary"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          {t("productie.tabOrders")}
        </button>
      </div>

      {activeTab === "bom" && (
        <BomListTab
          companyId={companyId}
          products={products}
          onNew={() => {
            setEditingBom(null);
            setView("bom-form");
          }}
          onEdit={async (bom) => {
            const detail = await api.productie.getBom(companyId, bom.id);
            setEditingBom(detail);
            setView("bom-form");
          }}
        />
      )}

      {activeTab === "orders" && (
        <OrdersListTab
          companyId={companyId}
          products={products}
          gestiuni={gestiuni}
          boms={boms}
          onNew={() => setView("produce-form")}
          onNewPlanned={() => setView("planned-form")}
          onView={(order) => {
            setViewingOrder(order);
            setView("order-detail");
          }}
        />
      )}
    </div>
  );
}
