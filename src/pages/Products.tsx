/**
 * Articole / Catalog de produse — company-scoped, reusable invoice lines.
 *
 * Listează produsele companiei active, permite adăugare/editare via modal
 * și ștergere cu confirmare. Dacă nicio companie nu e activă, afișează
 * mesajul "selectați o companie".
 */

import { useMemo, useState, useId, isValidElement, cloneElement } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Icon } from "@/components/shared/Icon";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { Product, ProductInput, UpdateProductInput } from "@/types";
import { VAT_RATES, VAT_CATEGORIES, VAT_CATEGORY_LABELS } from "@/lib/constants";

export function ProductsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<"all" | "active">("all");
  const [modal, setModal] = useState<"create" | { edit: Product } | null>(null);

  const {
    data: allProducts = [],
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: queryKeys.products.list(activeCompanyId ?? "", undefined),
    queryFn: () => api.products.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const list = useMemo(() => {
    const base = filter === "active" ? allProducts.filter((p) => p.active) : allProducts;
    const q = query.trim().toLowerCase();
    if (!q) return base;
    return base.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        (p.code ?? "").toLowerCase().includes(q),
    );
  }, [allProducts, query, filter]);

  const activeCount = allProducts.filter((p) => p.active).length;

  const deleteMutation = useMutation({
    mutationFn: (id: string) => {
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă."));
      return api.products.delete(id, activeCompanyId);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.products.all });
      notify.success("Articol șters.");
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge articolul.")),
  });

  const handleDelete = async (p: Product) => {
    if (!activeCompanyId) return;
    const ok = await confirm(
      `Șterge articolul "${p.name}"? Această acțiune nu poate fi anulată.`,
      { title: "Confirmare ștergere", kind: "warning" },
    );
    if (!ok) return;
    deleteMutation.mutate(p.id);
  };

  if (!activeCompanyId) {
    return (
      <div className="content">
        <div className="content-titlebar">
          <span className="content-title">
            <span className="crumb">Date</span>
            Articole
          </span>
        </div>
        <div style={{ padding: 40, textAlign: "center", fontSize: 12, color: "var(--text-muted)" }}>
          Selectați o companie activă pentru a vedea catalogul de articole.
        </div>
      </div>
    );
  }

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">Date</span>
          Articole
        </span>
        <span className="muted" style={{ fontSize: 11 }}>
          {list.length} din {allProducts.length} articole
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button
            type="button"
            className="btn primary"
            onClick={() => setModal("create")}
          >
            <Icon name="plus" size={12} /> Articol nou
          </button>
        </span>
      </div>

      <div className="views-bar">
        <span
          className={`view-tab${filter === "all" ? " active" : ""}`}
          onClick={() => setFilter("all")}
          style={{ cursor: "pointer" }}
        >
          Toate <span className="count">{allProducts.length}</span>
        </span>
        <span
          className={`view-tab${filter === "active" ? " active" : ""}`}
          onClick={() => setFilter("active")}
          style={{ cursor: "pointer" }}
        >
          Active <span className="count">{activeCount}</span>
        </span>
      </div>

      <div className="content-toolbar">
        <div className="search">
          <Icon name="search" size={13} />
          <input
            placeholder="Caută după denumire sau cod…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <span style={{ marginLeft: "auto" }}>
          <button
            type="button"
            className="btn-icon"
            title="Reîmprospătează"
            onClick={() =>
              void queryClient.invalidateQueries({ queryKey: queryKeys.products.all })
            }
          >
            <Icon name="refresh" size={14} />
          </button>
        </span>
      </div>

      <div className="content-body">
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 12, color: "var(--text-muted)" }}>
            Se încarcă…
          </div>
        ) : isError ? (
          <QueryErrorBanner
            error={error}
            label="articolele"
            onRetry={() => void refetch()}
          />
        ) : list.length === 0 ? (
          <div
            style={{
              padding: 40,
              textAlign: "center",
              fontSize: 12,
              color: "var(--text-muted)",
            }}
          >
            {allProducts.length === 0
              ? "Niciun articol. Adaugă primul produs sau serviciu."
              : "Niciun rezultat pentru filtrele aplicate."}
          </div>
        ) : (
          <table className="dt">
            <thead>
              <tr>
                <th>Denumire</th>
                <th style={{ width: 120 }}>Cod</th>
                <th style={{ width: 64 }}>UM</th>
                <th style={{ width: 110 }} className="num">
                  Preț unitar
                </th>
                <th style={{ width: 64 }} className="num">
                  TVA %
                </th>
                <th style={{ width: 80 }}>Categorie</th>
                <th style={{ width: 100 }} className="num">
                  Stoc
                </th>
                <th style={{ width: 60 }}>Activ</th>
                <th style={{ width: 80 }}>Acțiuni</th>
              </tr>
            </thead>
            <tbody>
              {list.map((p: Product) => (
                <tr key={p.id}>
                  <td>
                    <b>{p.name}</b>
                  </td>
                  <td className="mono">{p.code ?? <span className="dim">—</span>}</td>
                  <td className="mono">{p.unit}</td>
                  <td className="num tnum">{fmtRON(p.unitPrice)}</td>
                  <td className="num">{p.vatRate}%</td>
                  <td className="mono" title={VAT_CATEGORY_LABELS[p.vatCategory as keyof typeof VAT_CATEGORY_LABELS]}>
                    {p.vatCategory}
                  </td>
                  <td className="num">
                    {p.stockQty != null ? (
                      <span className="tnum">{p.stockQty}</span>
                    ) : (
                      <span className="dim">—</span>
                    )}
                  </td>
                  <td>
                    {p.active ? (
                      <span style={{ color: "#16A34A", display: "inline-flex" }}>
                        <Icon name="check" size={13} />
                      </span>
                    ) : (
                      <span className="dim">
                        <Icon name="x" size={13} />
                      </span>
                    )}
                  </td>
                  <td onClick={(e) => e.stopPropagation()}>
                    <button
                      type="button"
                      className="btn-icon"
                      title="Editează"
                      onClick={() => setModal({ edit: p })}
                    >
                      <Icon name="pen" size={13} />
                    </button>
                    <button
                      type="button"
                      className="btn-icon"
                      title="Șterge"
                      onClick={() => void handleDelete(p)}
                    >
                      <Icon name="x" size={13} />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <div
        style={{
          padding: "6px 14px",
          borderTop: "1px solid var(--border)",
          background: "var(--bg)",
          display: "flex",
          gap: 16,
          fontSize: 11,
          color: "var(--text-muted)",
        }}
      >
        <span>
          Total: <b style={{ color: "var(--text)" }}>{list.length}</b> articole
        </span>
        <span>
          Active: <b style={{ color: "var(--text)" }}>{activeCount}</b>
        </span>
        <span style={{ color: "var(--text-dim)", fontSize: 10, marginLeft: "auto" }}>
          Notă: decrementarea automată a stocului la facturare nu este implementată (planificat v2).
        </span>
      </div>

      {modal && (
        <ProductModal
          companyId={activeCompanyId}
          product={modal === "create" ? null : modal.edit}
          onClose={() => setModal(null)}
          onSaved={() => {
            void queryClient.invalidateQueries({ queryKey: queryKeys.products.all });
            setModal(null);
          }}
        />
      )}
    </div>
  );
}

// ─── Modal ──────────────────────────────────────────────────────────────────

function ProductModal({
  companyId,
  product,
  onClose,
  onSaved,
}: {
  companyId: string;
  product: Product | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const isEdit = product !== null;

  const [form, setForm] = useState<ProductInput>({
    name: product?.name ?? "",
    unit: product?.unit ?? "buc",
    unitPrice: product?.unitPrice ?? "0.00",
    vatRate: product?.vatRate ?? "19",
    vatCategory: product?.vatCategory ?? "S",
    code: product?.code ?? "",
    stockQty: product?.stockQty ?? "",
    active: product?.active ?? true,
  });
  const [error, setError] = useState<string | null>(null);

  const create = useMutation({
    mutationFn: (input: ProductInput) => api.products.create(companyId, input),
    onSuccess: () => { notify.success("Articol adăugat."); onSaved(); },
    onError: (e) => setError(formatError(e, "Eroare la adăugare.")),
  });

  const updateMut = useMutation({
    mutationFn: (input: UpdateProductInput) =>
      api.products.update(product!.id, companyId, input),
    onSuccess: () => { notify.success("Articol salvat."); onSaved(); },
    onError: (e) => setError(formatError(e, "Eroare la salvare.")),
  });

  const isPending = create.isPending || updateMut.isPending;

  const field = (key: keyof ProductInput) => ({
    value: (form[key] as string) ?? "",
    onChange: (
      e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>,
    ) => setForm((f) => ({ ...f, [key]: e.target.value })),
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (create.isPending || updateMut.isPending) return;
    setError(null);
    if (!form.name?.trim()) {
      setError("Denumirea este obligatorie.");
      return;
    }
    const input: ProductInput = {
      ...form,
      name: form.name.trim(),
      code: form.code?.trim() || undefined,
      stockQty: form.stockQty?.trim() || undefined,
      unit: form.unit || "buc",
      unitPrice: form.unitPrice || "0.00",
      vatRate: form.vatRate || "19",
      vatCategory: form.vatCategory || "S",
    };
    if (isEdit) {
      const { active, ...rest } = input;
      updateMut.mutate({ ...rest, active });
    } else {
      create.mutate(input);
    }
  };

  return (
    <div
      className="palette-scrim"
      style={{ alignItems: "center", paddingTop: 0 }}
      onClick={onClose}
    >
      <div
        style={{
          width: 440,
          background: "var(--bg-content)",
          border: "1px solid var(--border-strong)",
          boxShadow: "0 4px 24px rgba(0,0,0,0.12)",
          padding: "20px 24px 18px",
          maxHeight: "90vh",
          overflowY: "auto",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: 16,
          }}
        >
          <h3 style={{ fontSize: 14, fontWeight: 700, margin: 0 }}>
            {isEdit ? `Editează: ${product.name}` : "Articol nou"}
          </h3>
          <button type="button" className="btn-icon" onClick={onClose}>
            <Icon name="x" size={14} />
          </button>
        </div>

        <form
          onSubmit={handleSubmit}
          style={{ display: "flex", flexDirection: "column", gap: 9 }}
        >
          <MField label="Denumire *">
            <input
              className="field"
              placeholder="ex. Servicii consultanță"
              {...field("name")}
              autoFocus
            />
          </MField>

          <div style={{ display: "flex", gap: 9 }}>
            <MField label="Cod articol" style={{ flex: 1 }}>
              <input
                className="field mono"
                placeholder="ex. SVC-001"
                {...field("code")}
              />
            </MField>
            <MField label="UM" style={{ flex: 1 }}>
              <input
                className="field"
                placeholder="buc / ora / kg…"
                {...field("unit")}
              />
            </MField>
          </div>

          <div style={{ display: "flex", gap: 9 }}>
            <MField label="Preț unitar (fără TVA)" style={{ flex: 1 }}>
              <input
                className="field num"
                type="number"
                step="0.01"
                min="0"
                placeholder="0.00"
                {...field("unitPrice")}
              />
            </MField>
            <MField label="Cotă TVA %" style={{ flex: 1 }}>
              <select className="field num" {...field("vatRate")}>
                {VAT_RATES.map((r) => (
                  <option key={r} value={String(r)}>
                    {r}%
                  </option>
                ))}
              </select>
            </MField>
          </div>

          <div style={{ display: "flex", gap: 9 }}>
            <MField label="Categorie TVA" style={{ flex: 1 }}>
              <select className="field" {...field("vatCategory")}>
                {VAT_CATEGORIES.map((cat) => (
                  <option key={cat} value={cat}>
                    {cat} — {VAT_CATEGORY_LABELS[cat]}
                  </option>
                ))}
              </select>
            </MField>
            <MField label="Stoc (qty)" style={{ flex: 1 }}>
              <input
                className="field num"
                type="number"
                step="0.001"
                min="0"
                placeholder="—"
                {...field("stockQty")}
              />
            </MField>
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: 8, paddingTop: 2 }}>
            <input
              id="m-active"
              type="checkbox"
              className="cbx"
              checked={form.active as boolean}
              onChange={(e) => setForm((f) => ({ ...f, active: e.target.checked }))}
            />
            <label
              htmlFor="m-active"
              style={{ fontSize: 12, cursor: "pointer", userSelect: "none" }}
            >
              Articol activ
            </label>
          </div>

          {error && (
            <div
              style={{
                padding: "6px 10px",
                background: "#FEE2E2",
                border: "1px solid #FECACA",
                fontSize: 11,
                color: "#991B1B",
              }}
            >
              {error}
            </div>
          )}

          <div
            style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 4 }}
          >
            <button type="button" className="btn" onClick={onClose}>
              Anulează
            </button>
            <button type="submit" className="btn primary" disabled={isPending}>
              {isPending ? "Se salvează…" : isEdit ? "Salvează" : "Adaugă"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

// ─── MField helper ─────────────────────────────────────────────────────────

function MField({
  label,
  children,
  style,
}: {
  label: string;
  children: React.ReactNode;
  style?: React.CSSProperties;
}) {
  const fieldId = useId();
  const child = isValidElement(children)
    ? cloneElement(children as React.ReactElement<{ id?: string }>, { id: fieldId })
    : children;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 3, ...style }}>
      <label
        htmlFor={fieldId}
        style={{ fontSize: 11, fontWeight: 600, color: "var(--text-muted)" }}
      >
        {label}
      </label>
      {child}
    </div>
  );
}
