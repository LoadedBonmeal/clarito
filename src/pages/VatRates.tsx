/**
 * Cote TVA — catalog global editabil al cotelor TVA românești.
 *
 * Tabel GLOBAL: cotele TVA nu sunt scoped pe companie — sunt reglementate
 * la nivel național și se aplică tuturor companiilor din aplicație.
 * Nu există nicio gardă de "companie activă" pe această pagină (spre
 * deosebire de Articole sau Contacte).
 *
 * Nota fiscală: cotele legale din România sunt 0/5/9/11/19/21%.
 * Cotele non-standard pot fi respinse la validarea ANAF.
 */

import { useMemo, useState, useId, isValidElement, cloneElement } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Icon } from "@/components/shared/Icon";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { VatRate, VatRateInput, UpdateVatRateInput } from "@/types";

export function VatRatesPage() {
  const queryClient = useQueryClient();
  const [modal, setModal] = useState<"create" | { edit: VatRate } | null>(null);

  const {
    data: allRates = [],
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: queryKeys.vatRates.list(false),
    queryFn: () => api.vatRates.list(false),
  });

  const sortedRates = useMemo(
    () => [...allRates].sort((a, b) => a.sortOrder - b.sortOrder || parseFloat(a.rate) - parseFloat(b.rate)),
    [allRates],
  );

  const activeCount = allRates.filter((r) => r.active).length;

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.vatRates.delete(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.vatRates.all });
      notify.success("Cotă TVA ștearsă.");
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge cota TVA.")),
  });

  const toggleActiveMutation = useMutation({
    mutationFn: ({ id, active }: { id: string; active: boolean }) =>
      api.vatRates.setActive(id, active),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.vatRates.all });
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut modifica starea cotei.")),
  });

  const handleDelete = async (r: VatRate) => {
    const ok = await confirm(
      `Șterge cota "${r.label} (${r.rate}%)"? Această acțiune nu poate fi anulată.`,
      { title: "Confirmare ștergere", kind: "warning" },
    );
    if (!ok) return;
    deleteMutation.mutate(r.id);
  };

  const handleToggleActive = (r: VatRate) => {
    toggleActiveMutation.mutate({ id: r.id, active: !r.active });
  };

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">Date</span>
          Cote TVA
        </span>
        <span className="muted" style={{ fontSize: 11 }}>
          {sortedRates.length} cote · {activeCount} active
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button
            type="button"
            className="btn primary"
            onClick={() => setModal("create")}
          >
            <Icon name="plus" size={12} /> Cotă nouă
          </button>
        </span>
      </div>

      <div className="content-toolbar">
        <span style={{ marginLeft: "auto" }}>
          <button
            type="button"
            className="btn-icon"
            title="Reîmprospătează"
            onClick={() =>
              void queryClient.invalidateQueries({ queryKey: queryKeys.vatRates.all })
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
            label="cotele TVA"
            onRetry={() => void refetch()}
          />
        ) : sortedRates.length === 0 ? (
          <div
            style={{
              padding: 40,
              textAlign: "center",
              fontSize: 12,
              color: "var(--text-muted)",
            }}
          >
            Nicio cotă TVA. Adaugă prima cotă sau rulează migrarea din nou.
          </div>
        ) : (
          <table className="dt">
            <thead>
              <tr>
                <th style={{ width: 48 }} className="num">Ordine</th>
                <th style={{ width: 80 }} className="num">Cotă %</th>
                <th>Etichetă</th>
                <th style={{ width: 80 }}>Activ</th>
                <th style={{ width: 120 }}>Acțiuni</th>
              </tr>
            </thead>
            <tbody>
              {sortedRates.map((r: VatRate) => (
                <tr key={r.id} style={r.active ? undefined : { opacity: 0.5 }}>
                  <td className="num tnum" style={{ color: "var(--text-muted)" }}>
                    {r.sortOrder}
                  </td>
                  <td className="num">
                    <b>{r.rate}%</b>
                  </td>
                  <td>{r.label}</td>
                  <td>
                    <button
                      type="button"
                      className="btn-icon"
                      title={r.active ? "Dezactivează" : "Activează"}
                      onClick={() => handleToggleActive(r)}
                    >
                      {r.active ? (
                        <span style={{ color: "#16A34A", display: "inline-flex" }}>
                          <Icon name="check" size={13} />
                        </span>
                      ) : (
                        <span className="dim">
                          <Icon name="x" size={13} />
                        </span>
                      )}
                    </button>
                  </td>
                  <td onClick={(e) => e.stopPropagation()}>
                    <button
                      type="button"
                      className="btn-icon"
                      title="Editează"
                      onClick={() => setModal({ edit: r })}
                    >
                      <Icon name="pen" size={13} />
                    </button>
                    <button
                      type="button"
                      className="btn-icon"
                      title="Șterge"
                      onClick={() => void handleDelete(r)}
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
          Total: <b style={{ color: "var(--text)" }}>{sortedRates.length}</b> cote
        </span>
        <span>
          Active: <b style={{ color: "var(--text)" }}>{activeCount}</b>
        </span>
        <span style={{ color: "var(--text-dim)", fontSize: 10, marginLeft: "auto" }}>
          Cotele legale din România sunt fixe (0/5/9/11/19/21%). Cotele non-standard pot fi respinse la validarea ANAF.
        </span>
      </div>

      {modal && (
        <VatRateModal
          rate={modal === "create" ? null : modal.edit}
          onClose={() => setModal(null)}
          onSaved={() => {
            void queryClient.invalidateQueries({ queryKey: queryKeys.vatRates.all });
            setModal(null);
          }}
        />
      )}
    </div>
  );
}

// ─── Modal ──────────────────────────────────────────────────────────────────

function VatRateModal({
  rate,
  onClose,
  onSaved,
}: {
  rate: VatRate | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const isEdit = rate !== null;

  const [form, setForm] = useState<VatRateInput>({
    rate: rate?.rate ?? "",
    label: rate?.label ?? "",
    active: rate?.active ?? true,
    sortOrder: rate?.sortOrder ?? 0,
  });
  const [error, setError] = useState<string | null>(null);

  const create = useMutation({
    mutationFn: (input: VatRateInput) => api.vatRates.create(input),
    onSuccess: () => { notify.success("Cotă TVA adăugată."); onSaved(); },
    onError: (e) => setError(formatError(e, "Eroare la adăugare.")),
  });

  const updateMut = useMutation({
    mutationFn: (input: UpdateVatRateInput) =>
      api.vatRates.update(rate!.id, input),
    onSuccess: () => { notify.success("Cotă TVA salvată."); onSaved(); },
    onError: (e) => setError(formatError(e, "Eroare la salvare.")),
  });

  const isPending = create.isPending || updateMut.isPending;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (create.isPending || updateMut.isPending) return;
    setError(null);
    if (!form.rate?.trim()) {
      setError("Cota TVA este obligatorie.");
      return;
    }
    if (!form.label?.trim()) {
      setError("Eticheta este obligatorie.");
      return;
    }
    const parsed = parseFloat(form.rate);
    if (isNaN(parsed) || parsed < 0 || parsed > 100) {
      setError("Cota TVA trebuie să fie un număr între 0 și 100.");
      return;
    }
    if (isEdit) {
      updateMut.mutate({
        rate: form.rate.trim(),
        label: form.label.trim(),
        active: form.active,
        sortOrder: form.sortOrder,
      });
    } else {
      create.mutate({
        rate: form.rate.trim(),
        label: form.label.trim(),
        active: form.active,
        sortOrder: form.sortOrder,
      });
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
          width: 380,
          background: "var(--bg-content)",
          border: "1px solid var(--border-strong)",
          boxShadow: "0 4px 24px rgba(0,0,0,0.12)",
          padding: "20px 24px 18px",
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
            {isEdit ? `Editează: ${rate.label}` : "Cotă TVA nouă"}
          </h3>
          <button type="button" className="btn-icon" onClick={onClose}>
            <Icon name="x" size={14} />
          </button>
        </div>

        <form
          onSubmit={handleSubmit}
          style={{ display: "flex", flexDirection: "column", gap: 9 }}
        >
          <div style={{ display: "flex", gap: 9 }}>
            <MField label="Cotă TVA % *" style={{ width: 100 }}>
              <input
                className="field num"
                type="number"
                step="0.01"
                min="0"
                max="100"
                placeholder="ex. 19"
                value={form.rate}
                onChange={(e) => setForm((f) => ({ ...f, rate: e.target.value }))}
                autoFocus
              />
            </MField>
            <MField label="Ordine afișare" style={{ width: 100 }}>
              <input
                className="field num"
                type="number"
                step="1"
                min="0"
                placeholder="0"
                value={form.sortOrder ?? 0}
                onChange={(e) =>
                  setForm((f) => ({ ...f, sortOrder: parseInt(e.target.value) || 0 }))
                }
              />
            </MField>
          </div>

          <MField label="Etichetă *">
            <input
              className="field"
              placeholder="ex. Standard 19%"
              value={form.label}
              onChange={(e) => setForm((f) => ({ ...f, label: e.target.value }))}
            />
          </MField>

          <div style={{ display: "flex", alignItems: "center", gap: 8, paddingTop: 2 }}>
            <input
              id="vr-active"
              type="checkbox"
              className="cbx"
              checked={form.active ?? true}
              onChange={(e) => setForm((f) => ({ ...f, active: e.target.checked }))}
            />
            <label
              htmlFor="vr-active"
              style={{ fontSize: 12, cursor: "pointer", userSelect: "none" }}
            >
              Cotă activă (vizibilă în dropdown-ul liniilor de factură)
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

// ─── MField helper ──────────────────────────────────────────────────────────

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
    <div style={{ display: "flex", flexDirection: "column", gap: 3, flex: 1, ...style }}>
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
