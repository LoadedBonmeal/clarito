/**
 * Bon de Transfer Inter-Gestiune (formular 14-3-3A, OMFP 2634/2015).
 *
 * Trei view-uri:
 *   list   — tabelul tuturor transferurilor pentru compania activă
 *   create — formular de transfer (produs, gestiune sursă/destinație, cantitate, dată, referință)
 *   detail — vizualizare bon de transfer + print (14-3-3A)
 *
 * Neutralitate GL: transferul inter-gestiune NU generează note contabile sintetice.
 * Contul 371 rămâne nemodificat la nivel de societate — mișcarea este pur analitică.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { Ic } from "@/components/shared/Ic";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { StockTransfer, TransferInput, Gestiune, Product } from "@/types";

// ─── Types ────────────────────────────────────────────────────────────────────

type View = "list" | "create" | "detail";

// ─── List view ────────────────────────────────────────────────────────────────

function TransferList({
  companyId,
  onNew,
  onView,
}: {
  companyId: string;
  onNew: () => void;
  onView: (t: StockTransfer) => void;
}) {
  const { t } = useTranslation();

  const { data: transfers = [], isLoading, error } = useQuery({
    queryKey: ["stockTransfers", companyId],
    queryFn: () => api.stockTransfer.list(companyId),
    enabled: !!companyId,
  });

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

  const productName = (id: string) =>
    products.find((p: Product) => p.id === id)?.name ?? id;
  const gesName = (id: string) =>
    gestiuni.find((g: Gestiune) => g.id === id)?.denumire ?? id;

  return (
    <div className="main-inner">
      <div className="page-head">
        <div>
          <h1 className="page-title">{t("stockTransfer.title")}</h1>
        </div>
        <button className="btn-dark" onClick={onNew}>
          <Ic name="plus" /> {t("stockTransfer.new")}
        </button>
      </div>

      {error && (
        <QueryErrorBanner error={error} label={t("stockTransfer.errorLabel")} />
      )}

      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="spacer" />
        </div>
        {isLoading && <div className="state-row">{t("stockTransfer.loading")}</div>}
        {!isLoading && !error && transfers.length === 0 && (
          <div className="state-row muted">{t("stockTransfer.empty")}</div>
        )}
        {!isLoading && transfers.length > 0 && (
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("stockTransfer.colDate")}</th>
                <th>{t("stockTransfer.colProduct")}</th>
                <th>{t("stockTransfer.colFrom")}</th>
                <th>{t("stockTransfer.colTo")}</th>
                <th style={{ textAlign: "right" }}>{t("stockTransfer.colQty")}</th>
                <th style={{ textAlign: "right" }}>{t("stockTransfer.colValue")}</th>
                <th>{t("stockTransfer.colRef")}</th>
              </tr>
            </thead>
            <tbody>
              {transfers.map((tr: StockTransfer) => (
                <tr
                  key={tr.id}
                  style={{ cursor: "pointer" }}
                  onClick={() => onView(tr)}
                >
                  <td>{tr.transferDate}</td>
                  <td>{productName(tr.productId)}</td>
                  <td>{gesName(tr.fromGestiuneId)}</td>
                  <td>{gesName(tr.toGestiuneId)}</td>
                  <td style={{ textAlign: "right" }}>
                    {parseFloat(tr.qty).toFixed(3)}
                  </td>
                  <td style={{ textAlign: "right" }}>{tr.value}</td>
                  <td>{tr.transferRef ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

// ─── Create form ──────────────────────────────────────────────────────────────

function TransferCreate({
  companyId,
  onSaved,
  onCancel,
}: {
  companyId: string;
  onSaved: (tr: StockTransfer) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const today = new Date().toISOString().slice(0, 10);

  const [productId, setProductId] = useState("");
  const [fromGestiuneId, setFromGestiuneId] = useState("");
  const [toGestiuneId, setToGestiuneId] = useState("");
  const [qty, setQty] = useState("");
  const [transferDate, setTransferDate] = useState(today);
  const [transferRef, setTransferRef] = useState("");
  const [notes, setNotes] = useState("");

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

  // Show stock on hand in source gestiune for selected product.
  const { data: onHandData } = useQuery({
    queryKey: ["stockOnHand", companyId, productId, fromGestiuneId],
    queryFn: () =>
      api.gestiuni.stockOnHand(companyId, productId, fromGestiuneId),
    enabled: !!(companyId && productId && fromGestiuneId),
  });
  const onHandQty = onHandData ? parseFloat(onHandData[0]).toFixed(3) : null;

  const mutation = useMutation({
    mutationFn: (input: TransferInput) =>
      api.stockTransfer.transfer(companyId, input),
    onSuccess: (result) => {
      void queryClient.invalidateQueries({ queryKey: ["stockTransfers", companyId] });
      void queryClient.invalidateQueries({ queryKey: ["stockOnHand"] });
      void queryClient.invalidateQueries({ queryKey: ["products", companyId] });
      notify.success(t("stockTransfer.saved"));
      onSaved(result);
    },
    onError: (err) => {
      notify.error(formatError(err) ?? t("stockTransfer.saveError"));
    },
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    mutation.mutate({
      productId,
      fromGestiuneId,
      toGestiuneId,
      transferDate,
      qty,
      transferRef: transferRef || undefined,
      notes: notes || undefined,
    });
  };

  // Stockable products only.
  const stockableProducts = products.filter((p: Product) => !p.isService);

  return (
    <div className="main-inner">
      <div className="page-head">
        <div>
          <button className="btn-ghost" onClick={onCancel}>
            ← {t("stockTransfer.backToList")}
          </button>
          <h1 className="page-title">{t("stockTransfer.new")}</h1>
        </div>
      </div>

      <div className="banner"><Ic name="info" /><span>{t("stockTransfer.glNeutralNote")}</span></div>

      <div className="scr-card" style={{ maxWidth: 640 }}>
        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          {/* Product */}
          <div className="field">
            <label>{t("stockTransfer.fieldProduct")}</label>
            <select
              className="input"
              value={productId}
              onChange={(e) => setProductId(e.target.value)}
              required
            >
              <option value="">{t("stockTransfer.selectProduct")}</option>
              {stockableProducts.map((p: Product) => (
                <option key={p.id} value={p.id}>
                  {p.name} {p.code ? `(${p.code})` : ""}
                </option>
              ))}
            </select>
          </div>

          {/* From gestiune */}
          <div className="field">
            <label>{t("stockTransfer.fieldFrom")}</label>
            <select
              className="input"
              value={fromGestiuneId}
              onChange={(e) => setFromGestiuneId(e.target.value)}
              required
            >
              <option value="">{t("stockTransfer.selectGestiune")}</option>
              {gestiuni.map((g: Gestiune) => (
                <option key={g.id} value={g.id}>
                  {g.denumire} ({g.cod})
                </option>
              ))}
            </select>
            {onHandQty !== null && (
              <span style={{ fontSize: 12, color: "var(--text-2)", marginTop: 4 }}>
                Stoc disponibil: {onHandQty}
              </span>
            )}
          </div>

          {/* To gestiune */}
          <div className="field">
            <label>{t("stockTransfer.fieldTo")}</label>
            <select
              className="input"
              value={toGestiuneId}
              onChange={(e) => setToGestiuneId(e.target.value)}
              required
            >
              <option value="">{t("stockTransfer.selectGestiune")}</option>
              {gestiuni
                .filter((g: Gestiune) => g.id !== fromGestiuneId)
                .map((g: Gestiune) => (
                  <option key={g.id} value={g.id}>
                    {g.denumire} ({g.cod})
                  </option>
                ))}
            </select>
          </div>

          {/* Qty */}
          <div className="field">
            <label>{t("stockTransfer.fieldQty")}</label>
            <input
              className="input"
              type="number"
              step="0.000001"
              min="0.000001"
              value={qty}
              onChange={(e) => setQty(e.target.value)}
              required
              placeholder="0.000000"
            />
          </div>

          {/* Date */}
          <div className="field">
            <label>{t("stockTransfer.fieldDate")}</label>
            <input
              className="input"
              type="date"
              value={transferDate}
              onChange={(e) => setTransferDate(e.target.value)}
              required
            />
          </div>

          {/* Reference */}
          <div className="field">
            <label>{t("stockTransfer.fieldRef")}</label>
            <input
              className="input"
              type="text"
              value={transferRef}
              onChange={(e) => setTransferRef(e.target.value)}
              placeholder="BON-001"
            />
          </div>

          {/* Notes */}
          <div className="field">
            <label>{t("stockTransfer.fieldNotes")}</label>
            <textarea
              className="input"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              rows={2}
            />
          </div>

          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
            <button type="button" className="btn-ghost" onClick={onCancel}>
              {t("stockTransfer.cancel")}
            </button>
            <button
              type="submit"
              className="btn-dark"
              disabled={mutation.isPending}
            >
              {mutation.isPending
                ? t("stockTransfer.saving")
                : t("stockTransfer.submit")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

// ─── Detail / Print view ──────────────────────────────────────────────────────

function TransferDetail({
  transfer,
  companyId,
  onBack,
}: {
  transfer: StockTransfer;
  companyId: string;
  onBack: () => void;
}) {
  const { t } = useTranslation();

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

  const product = products.find((p: Product) => p.id === transfer.productId);
  const fromGes = gestiuni.find((g: Gestiune) => g.id === transfer.fromGestiuneId);
  const toGes = gestiuni.find((g: Gestiune) => g.id === transfer.toGestiuneId);

  const printTitle = t("stockTransfer.printTitle", {
    ref: transfer.transferRef ?? transfer.id.slice(0, 8).toUpperCase(),
  });

  return (
    <div className="main-inner">
      <div className="page-head">
        <div>
          <button className="btn-ghost" onClick={onBack}>
            ← {t("stockTransfer.backToList")}
          </button>
          <h1 className="page-title">{t("stockTransfer.viewDetail")}</h1>
        </div>
        <button className="btn-ghost" onClick={() => window.print()}>
          {t("stockTransfer.print")}
        </button>
      </div>

      {/* BON DE TRANSFER 14-3-3A print view */}
      <div className="scr-card print-doc" style={{ maxWidth: 800 }}>
        {/* Header */}
        <div style={{ textAlign: "center", marginBottom: 24 }}>
          <div style={{ fontWeight: 700, fontSize: 18 }}>{printTitle}</div>
          <div style={{ fontSize: 13, marginTop: 4 }}>
            {t("stockTransfer.printDate")}: {transfer.transferDate}
          </div>
        </div>

        {/* Gestiune info */}
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: 12,
            marginBottom: 20,
            fontSize: 13,
          }}
        >
          <div>
            <strong>{t("stockTransfer.printFrom")}:</strong>{" "}
            {fromGes ? `${fromGes.denumire} (${fromGes.cod})` : transfer.fromGestiuneId}
          </div>
          <div>
            <strong>{t("stockTransfer.printTo")}:</strong>{" "}
            {toGes ? `${toGes.denumire} (${toGes.cod})` : transfer.toGestiuneId}
          </div>
        </div>

        {/* Table — 14-3-3A column order */}
        <table className="scr-table" style={{ marginBottom: 24 }}>
          <thead>
            <tr>
              <th style={{ width: 40 }}>{t("stockTransfer.printColNo")}</th>
              <th>{t("stockTransfer.printColDenumire")}</th>
              <th>{t("stockTransfer.printColCod")}</th>
              <th>{t("stockTransfer.printColUm")}</th>
              <th style={{ textAlign: "right" }}>{t("stockTransfer.printColQty")}</th>
              <th style={{ textAlign: "right" }}>{t("stockTransfer.printColUnitCost")}</th>
              <th style={{ textAlign: "right" }}>{t("stockTransfer.printColValue")}</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td style={{ textAlign: "center" }}>1</td>
              <td>{product?.name ?? transfer.productId}</td>
              <td>{product?.code ?? "—"}</td>
              <td>{product?.unit ?? "buc"}</td>
              <td style={{ textAlign: "right" }}>
                {parseFloat(transfer.qty).toFixed(3)}
              </td>
              <td style={{ textAlign: "right" }}>{transfer.unitCost}</td>
              <td style={{ textAlign: "right" }}>{transfer.value}</td>
            </tr>
          </tbody>
          <tfoot>
            <tr>
              <td colSpan={4} />
              <td colSpan={2} style={{ fontWeight: 600 }}>
                TOTAL
              </td>
              <td style={{ textAlign: "right", fontWeight: 600 }}>
                {transfer.value}
              </td>
            </tr>
          </tfoot>
        </table>

        {/* Notes */}
        {transfer.notes && (
          <div style={{ marginBottom: 24, fontSize: 13 }}>
            <strong>{t("stockTransfer.printNotes")}:</strong> {transfer.notes}
          </div>
        )}

        {/* Signatures — predat/primit */}
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: 32,
            marginTop: 40,
            fontSize: 13,
          }}
        >
          <div>
            <div style={{ fontWeight: 600, marginBottom: 8 }}>
              {t("stockTransfer.printPredator")}
            </div>
            <div style={{ marginBottom: 40 }}>
              {t("stockTransfer.printSemnatura")}: ____________________
            </div>
            <div>
              {t("stockTransfer.printData")}: ____________________
            </div>
          </div>
          <div>
            <div style={{ fontWeight: 600, marginBottom: 8 }}>
              {t("stockTransfer.printPrimitor")}
            </div>
            <div style={{ marginBottom: 40 }}>
              {t("stockTransfer.printSemnatura")}: ____________________
            </div>
            <div>
              {t("stockTransfer.printData")}: ____________________
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

// ─── Page root ─────────────────────────────────────────────────────────────────

export function StockTransferPage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const [view, setView] = useState<View>("list");
  const [selectedTransfer, setSelectedTransfer] = useState<StockTransfer | null>(null);

  if (!activeCompanyId) {
    return (
      <div className="main-inner">
        <div className="state-row muted">
          {t("stockTransfer.selectCompany")}
        </div>
      </div>
    );
  }

  if (view === "create") {
    return (
      <TransferCreate
        companyId={activeCompanyId}
        onSaved={(tr) => {
          setSelectedTransfer(tr);
          setView("detail");
        }}
        onCancel={() => setView("list")}
      />
    );
  }

  if (view === "detail" && selectedTransfer) {
    return (
      <TransferDetail
        transfer={selectedTransfer}
        companyId={activeCompanyId}
        onBack={() => setView("list")}
      />
    );
  }

  return (
    <TransferList
      companyId={activeCompanyId}
      onNew={() => setView("create")}
      onView={(tr) => {
        setSelectedTransfer(tr);
        setView("detail");
      }}
    />
  );
}
