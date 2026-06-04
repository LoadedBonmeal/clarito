/**
 * Articole / Catalog de produse — re-skinned to rf kit (Wave 3).
 * Preserves: api.products.list(activeCompanyId), filter Toate/Active, search,
 * create/edit modal → api.products.create / api.products.update(id, companyId, input),
 * delete confirm → api.products.delete(id, companyId),
 * "select active company" guard.
 * Modal: cod/denumire/UM/preț/cotă TVA/activ.
 */

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Icon } from "@/components/shared/Icon";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import {
  PageHeader, Btn, IconBtn, Badge, Card, Field, Input, Select,
  Tabs, Empty, Modal, SearchInput,
} from "@/components/rf";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { Product, ProductInput, UpdateProductInput } from "@/types";
import { VAT_RATES, VAT_CATEGORIES, VAT_CATEGORY_LABELS } from "@/lib/constants";

// Art. 331 product categories (codPR) — Parameters_v7._listaCodPR
// Shown only when vatCategory="AE". For tip_partener=1 (default use-case).
const ART331_CODES: { value: string; label: string }[] = [
  { value: "22", label: "22 — Deșeuri feroase/neferoase" },
  { value: "23", label: "23 — Masă lemnoasă și materiale lemnoase" },
  { value: "24", label: "24 — Certificate CO₂/gaze cu efect de seră" },
  { value: "25", label: "25 — Energie electrică" },
  { value: "26", label: "26 — Certificate verzi" },
  { value: "27", label: "27 — Construcții/terenuri" },
  { value: "28", label: "28 — Aur de investiții" },
  { value: "29", label: "29 — Telefoane mobile" },
  { value: "30", label: "30 — Microprocesoare (circuite integrate)" },
  { value: "31", label: "31 — Console/tablete/laptopuri" },
  { value: "36", label: "36 — Gaze naturale" },
  { value: "1001", label: "1001 — Grâu comun/alac" },
  { value: "1002", label: "1002 — Secară" },
  { value: "1003", label: "1003 — Orz" },
  { value: "1004", label: "1004 — Ovăz" },
  { value: "1005", label: "1005 — Porumb" },
  { value: "1201", label: "1201 — Soia" },
  { value: "1205", label: "1205 — Rapiță" },
  { value: "120600", label: "120600 — Floarea-soarelui" },
  { value: "121291", label: "121291 — Sfeclă de zahăr" },
  { value: "10086000", label: "10086000 — Orez" },
  { value: "120400", label: "120400 — Semințe de in" },
];

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
    const base =
      filter === "active" ? allProducts.filter((p) => p.active) : allProducts;
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
      if (!activeCompanyId)
        return Promise.reject(new Error("Nicio companie activă."));
      return api.products.delete(id, activeCompanyId);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.products.all });
      notify.success("Articol șters.");
    },
    onError: (e) =>
      notify.error(formatError(e, "Nu s-a putut șterge articolul.")),
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
      <div className="rf-page">
        <PageHeader title="Articole" />
        <div className="rf-page-body">
          <Empty icon="package" title="Selectați o companie activă">
            Selectați o companie din bara laterală pentru a vedea catalogul de articole.
          </Empty>
        </div>
      </div>
    );
  }

  const filterTabs = [
    { value: "all" as const, label: "Toate", badge: allProducts.length },
    { value: "active" as const, label: "Active", badge: activeCount },
  ];

  return (
    <div className="rf-page">
      <PageHeader
        title="Articole"
        sub={
          <Badge variant="neutral" dot={false}>
            {list.length} articole
          </Badge>
        }
        actions={
          <Btn
            variant="primary"
            icon="plus"
            size="sm"
            onClick={() => setModal("create")}
          >
            Articol nou
          </Btn>
        }
      />

      <div className="rf-page-body">
        <Card>
          {/* Tabs + toolbar */}
          <div
            style={{
              padding: "10px 16px 0",
              borderBottom: "1px solid var(--rf-border)",
            }}
          >
            <Tabs tabs={filterTabs} value={filter} onChange={(v) => setFilter(v)} />
          </div>
          <div
            className="rf-toolbar-row"
            style={{ padding: "10px 16px", borderBottom: "1px solid var(--rf-border)" }}
          >
            <SearchInput
              placeholder="Caută după denumire sau cod…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              style={{ width: 300 }}
            />
            <div style={{ marginLeft: "auto" }}>
              <IconBtn
                icon="refresh"
                title="Reîmprospătează"
                onClick={() =>
                  void queryClient.invalidateQueries({
                    queryKey: queryKeys.products.all,
                  })
                }
              />
            </div>
          </div>

          {/* Table */}
          <div className="rf-tbl-wrap">
            {isLoading ? (
              <Empty icon="package" title="Se încarcă…" />
            ) : isError ? (
              <QueryErrorBanner
                error={error}
                label="articolele"
                onRetry={() => void refetch()}
              />
            ) : list.length === 0 ? (
              <Empty
                icon="package"
                title={
                  allProducts.length === 0
                    ? "Niciun articol"
                    : "Niciun rezultat pentru filtrele aplicate"
                }
                actions={
                  allProducts.length === 0 ? (
                    <Btn
                      variant="primary"
                      icon="plus"
                      onClick={() => setModal("create")}
                    >
                      Adaugă primul articol
                    </Btn>
                  ) : undefined
                }
              >
                {allProducts.length === 0 &&
                  "Adaugă produse sau servicii care vor apărea ca linii de factură."}
              </Empty>
            ) : (
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th>Denumire</th>
                    <th style={{ width: 120 }}>Cod</th>
                    <th style={{ width: 60 }}>UM</th>
                    <th style={{ width: 120, textAlign: "right" }}>Preț unitar</th>
                    <th style={{ width: 70, textAlign: "right" }}>TVA %</th>
                    <th style={{ width: 80 }}>Categorie</th>
                    <th style={{ width: 90, textAlign: "right" }}>Stoc</th>
                    <th style={{ width: 70, textAlign: "center" }}>Activ</th>
                    <th style={{ width: 80 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {list.map((p: Product) => (
                    <tr key={p.id}>
                      <td style={{ fontWeight: 500 }}>{p.name}</td>
                      <td className="mono">
                        {p.code ?? <span className="rf-dim">—</span>}
                      </td>
                      <td className="mono">{p.unit}</td>
                      <td style={{ textAlign: "right" }} className="mono">
                        {fmtRON(p.unitPrice)}
                      </td>
                      <td style={{ textAlign: "right" }} className="mono">
                        {p.vatRate}%
                      </td>
                      <td
                        className="mono"
                        style={{ color: "var(--rf-text-muted)" }}
                        title={
                          VAT_CATEGORY_LABELS[
                            p.vatCategory as keyof typeof VAT_CATEGORY_LABELS
                          ]
                        }
                      >
                        {p.vatCategory}
                      </td>
                      <td style={{ textAlign: "right" }} className="mono">
                        {p.stockQty != null ? (
                          p.stockQty
                        ) : (
                          <span className="rf-dim">—</span>
                        )}
                      </td>
                      <td style={{ textAlign: "center" }}>
                        {p.active ? (
                          <Badge variant="success" dot={false}>Activ</Badge>
                        ) : (
                          <Badge variant="neutral" dot={false}>Inactiv</Badge>
                        )}
                      </td>
                      <td onClick={(e) => e.stopPropagation()}>
                        <div className="rf-cell-actions">
                          <IconBtn
                            icon="pen"
                            title="Editează"
                            size={14}
                            onClick={() => setModal({ edit: p })}
                          />
                          <IconBtn
                            icon="trash"
                            title="Șterge"
                            size={14}
                            onClick={() => void handleDelete(p)}
                          />
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>

          {/* Footer */}
          <div className="rf-tbl-footer">
            <span>
              Total: <b>{list.length}</b> articole
            </span>
            <span>
              Active: <b>{activeCount}</b>
            </span>
            <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--rf-text-dim)" }}>
              Stoc: decrementare automată planificată v2
            </span>
          </div>
        </Card>
      </div>

      {/* Product modal */}
      {modal !== null && (
        <ProductModal
          companyId={activeCompanyId}
          product={modal === "create" ? null : modal.edit}
          onClose={() => setModal(null)}
          onSaved={() => {
            void queryClient.invalidateQueries({
              queryKey: queryKeys.products.all,
            });
            setModal(null);
          }}
        />
      )}
    </div>
  );
}

// ─── ProductModal ─────────────────────────────────────────────────────────────

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
    vatRate: product?.vatRate ?? "21", // 2026 standard (Legea 141/2025); editing preserves the existing rate
    vatCategory: product?.vatCategory ?? "S",
    code: product?.code ?? "",
    stockQty: product?.stockQty ?? "",
    art331Code: product?.art331Code ?? "",
    active: product?.active ?? true,
  });
  const [error, setError] = useState<string | null>(null);

  const create = useMutation({
    mutationFn: (input: ProductInput) => api.products.create(companyId, input),
    onSuccess: () => {
      notify.success("Articol adăugat.");
      onSaved();
    },
    onError: (e) => setError(formatError(e, "Eroare la adăugare.")),
  });

  const updateMut = useMutation({
    mutationFn: (input: UpdateProductInput) =>
      api.products.update(product!.id, companyId, input),
    onSuccess: () => {
      notify.success("Articol salvat.");
      onSaved();
    },
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
    if (isPending) return;
    setError(null);
    if (!form.name?.trim()) {
      setError("Denumirea este obligatorie.");
      return;
    }
    const input: ProductInput = {
      ...form,
      name: form.name.trim(),
      code: form.code?.trim() || undefined,
      stockQty: (form.stockQty as string)?.trim() || undefined,
      art331Code: (form.art331Code as string)?.trim() || undefined,
      unit: form.unit || "buc",
      unitPrice: form.unitPrice || "0.00",
      vatRate: form.vatRate || "21",
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
    <Modal
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={isEdit ? `Editează: ${product.name}` : "Articol nou"}
      width={500}
      footer={
        <>
          <Btn variant="secondary" onClick={onClose} disabled={isPending}>
            Anulează
          </Btn>
          <Btn
            variant="primary"
            icon="check"
            disabled={isPending}
            onClick={(e) => {
              e.preventDefault();
              void handleSubmit(e);
            }}
          >
            {isPending ? "Se salvează…" : isEdit ? "Salvează" : "Adaugă"}
          </Btn>
        </>
      }
    >
      <form
        onSubmit={handleSubmit}
        style={{ display: "flex", flexDirection: "column", gap: 14 }}
      >
        <Field label="Denumire" required error={error && !form.name?.trim() ? error : undefined}>
          <Input
            placeholder="ex. Servicii consultanță"
            {...field("name")}
            autoFocus
          />
        </Field>

        <div className="rf-grid-2">
          <Field label="Cod articol">
            <Input className="mono" placeholder="ex. SVC-001" {...field("code")} />
          </Field>
          <Field label="UM">
            <Input placeholder="buc / oră / kg…" {...field("unit")} />
          </Field>
        </div>

        <div className="rf-grid-2">
          <Field label="Preț unitar (fără TVA)">
            <Input
              num
              type="number"
              step="0.01"
              min="0"
              placeholder="0.00"
              {...field("unitPrice")}
            />
          </Field>
          <Field label="Cotă TVA %">
            <Select {...field("vatRate")}>
              {VAT_RATES.map((r) => (
                <option key={r} value={String(r)}>
                  {r}%
                </option>
              ))}
            </Select>
          </Field>
        </div>

        <div className="rf-grid-2">
          <Field label="Categorie TVA">
            <Select {...field("vatCategory")}>
              {VAT_CATEGORIES.map((cat) => (
                <option key={cat} value={cat}>
                  {cat} — {VAT_CATEGORY_LABELS[cat]}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Stoc (qty)">
            <Input
              num
              type="number"
              step="0.001"
              min="0"
              placeholder="—"
              {...field("stockQty")}
            />
          </Field>
        </div>

        {form.vatCategory === "AE" && (
          <Field label="Cod art. 331 (taxare inversă — D394 codPR)">
            <Select
              value={(form.art331Code as string) ?? ""}
              onChange={(e) =>
                setForm((f) => ({ ...f, art331Code: e.target.value || undefined }))
              }
            >
              <option value="">— implicit 22 (Deșeuri feroase/neferoase) —</option>
              {ART331_CODES.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </Select>
          </Field>
        )}

        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            fontSize: 13,
            cursor: "pointer",
          }}
        >
          <input
            type="checkbox"
            className="rf-cbx"
            checked={form.active as boolean}
            onChange={(e) => setForm((f) => ({ ...f, active: e.target.checked }))}
          />
          Articol activ
        </label>

        {error && (
          <div className="rf-banner rf-banner--error">
            <Icon name="xCircle" size={16} />
            <span>{error}</span>
          </div>
        )}
      </form>
    </Modal>
  );
}
