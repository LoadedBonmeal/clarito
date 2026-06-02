/**
 * Cote TVA — catalog GLOBAL editabil al cotelor TVA românești.
 * Re-skinned to rf kit (Wave 3).
 *
 * GLOBAL: cotele TVA nu sunt scoped pe companie — sunt reglementate la nivel național.
 * Nu există gardă de "companie activă" pe această pagină.
 *
 * Preserves: api.vatRates.list(false), create/edit modal → api.vatRates.create/update,
 * delete confirm → api.vatRates.delete(id),
 * active toggle → api.vatRates.setActive(id, active).
 *
 * Nota legală: cotele legale RO sunt 0/5/9/11/19/21%.
 */

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Icon } from "@/components/shared/Icon";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import {
  PageHeader, Btn, IconBtn, Badge, Card, Field, Input,
  Toggle, Banner, Empty, Modal,
} from "@/components/rf";
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
    () =>
      [...allRates].sort(
        (a, b) =>
          a.sortOrder - b.sortOrder || parseFloat(a.rate) - parseFloat(b.rate),
      ),
    [allRates],
  );

  const activeCount = allRates.filter((r) => r.active).length;

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.vatRates.delete(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.vatRates.all });
      notify.success("Cotă TVA ștearsă.");
    },
    onError: (e) =>
      notify.error(formatError(e, "Nu s-a putut șterge cota TVA.")),
  });

  const toggleActiveMutation = useMutation({
    mutationFn: ({ id, active }: { id: string; active: boolean }) =>
      api.vatRates.setActive(id, active),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.vatRates.all });
    },
    onError: (e) =>
      notify.error(formatError(e, "Nu s-a putut modifica starea cotei.")),
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
    <div className="rf-page">
      <PageHeader
        title="Cote TVA"
        sub={
          <Badge variant="neutral" dot={false}>
            {sortedRates.length} cote · {activeCount} active
          </Badge>
        }
        actions={
          <Btn
            variant="primary"
            icon="plus"
            size="sm"
            onClick={() => setModal("create")}
          >
            Cotă nouă
          </Btn>
        }
      />

      <div className="rf-page-body">
          <div style={{ marginBottom: 12 }}>
            <Banner variant="info">
              Cotele legale de TVA în România sunt{" "}
              <b>0% / 5% / 9% / 11% / 19% / 21%</b>. Cotele active aici alimentează lista din editorul de factură.
              Cotele non-standard pot fi respinse la validarea ANAF.
            </Banner>
          </div>

          <Card>
            {/* Toolbar */}
            <div
              className="rf-toolbar-row"
              style={{ padding: "10px 16px", borderBottom: "1px solid var(--rf-border)" }}
            >
              <span style={{ fontSize: 13, color: "var(--rf-text-muted)" }}>
                Catalog global — se aplică tuturor companiilor
              </span>
              <div style={{ marginLeft: "auto" }}>
                <IconBtn
                  icon="refresh"
                  title="Reîmprospătează"
                  onClick={() =>
                    void queryClient.invalidateQueries({
                      queryKey: queryKeys.vatRates.all,
                    })
                  }
                />
              </div>
            </div>

            {/* Table */}
            <div className="rf-tbl-wrap">
              {isLoading ? (
                <Empty icon="percent" title="Se încarcă…" />
              ) : isError ? (
                <QueryErrorBanner
                  error={error}
                  label="cotele TVA"
                  onRetry={() => void refetch()}
                />
              ) : sortedRates.length === 0 ? (
                <Empty icon="percent" title="Nicio cotă TVA">
                  Adaugă prima cotă sau rulează migrarea din nou.
                </Empty>
              ) : (
                <table className="rf-tbl">
                  <thead>
                    <tr>
                      <th style={{ width: 60, textAlign: "right" }}>Ordine</th>
                      <th style={{ width: 90, textAlign: "right" }}>Cotă %</th>
                      <th>Etichetă</th>
                      <th style={{ width: 90, textAlign: "center" }}>Activ</th>
                      <th style={{ width: 80 }}></th>
                    </tr>
                  </thead>
                  <tbody>
                    {sortedRates.map((r: VatRate) => (
                      <tr key={r.id} style={r.active ? undefined : { opacity: 0.55 }}>
                        <td
                          style={{ textAlign: "right", color: "var(--rf-text-muted)" }}
                          className="mono"
                        >
                          {r.sortOrder}
                        </td>
                        <td style={{ textAlign: "right" }} className="mono">
                          <b>{r.rate}%</b>
                        </td>
                        <td>{r.label}</td>
                        <td style={{ textAlign: "center" }}>
                          <Toggle
                            checked={r.active}
                            onChange={() => handleToggleActive(r)}
                            aria-label={r.active ? "Dezactivează" : "Activează"}
                            disabled={
                              toggleActiveMutation.isPending
                            }
                          />
                        </td>
                        <td onClick={(e) => e.stopPropagation()}>
                          <div className="rf-cell-actions">
                            <IconBtn
                              icon="pen"
                              title="Editează"
                              size={14}
                              onClick={() => setModal({ edit: r })}
                            />
                            <IconBtn
                              icon="trash"
                              title="Șterge"
                              size={14}
                              onClick={() => void handleDelete(r)}
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
                Total: <b>{sortedRates.length}</b> cote
              </span>
              <span>
                Active: <b>{activeCount}</b>
              </span>
            </div>
          </Card>
      </div>

      {/* Vat rate modal */}
      {modal !== null && (
        <VatRateModal
          rate={modal === "create" ? null : modal.edit}
          onClose={() => setModal(null)}
          onSaved={() => {
            void queryClient.invalidateQueries({
              queryKey: queryKeys.vatRates.all,
            });
            setModal(null);
          }}
        />
      )}
    </div>
  );
}

// ─── VatRateModal ─────────────────────────────────────────────────────────────

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
    onSuccess: () => {
      notify.success("Cotă TVA adăugată.");
      onSaved();
    },
    onError: (e) => setError(formatError(e, "Eroare la adăugare.")),
  });

  const updateMut = useMutation({
    mutationFn: (input: UpdateVatRateInput) =>
      api.vatRates.update(rate!.id, input),
    onSuccess: () => {
      notify.success("Cotă TVA salvată.");
      onSaved();
    },
    onError: (e) => setError(formatError(e, "Eroare la salvare.")),
  });

  const isPending = create.isPending || updateMut.isPending;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (isPending) return;
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
    <Modal
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={isEdit ? `Editează: ${rate.label}` : "Cotă TVA nouă"}
      width={420}
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
        <div className="rf-grid-2">
          <Field label="Cotă TVA %" required>
            <Input
              num
              type="number"
              step="0.01"
              min="0"
              max="100"
              placeholder="ex. 19"
              value={form.rate}
              onChange={(e) => setForm((f) => ({ ...f, rate: e.target.value }))}
              autoFocus
            />
          </Field>
          <Field label="Ordine afișare">
            <Input
              num
              type="number"
              step="1"
              min="0"
              placeholder="0"
              value={String(form.sortOrder ?? 0)}
              onChange={(e) =>
                setForm((f) => ({
                  ...f,
                  sortOrder: parseInt(e.target.value) || 0,
                }))
              }
            />
          </Field>
        </div>

        <Field label="Etichetă" required>
          <Input
            placeholder="ex. Standard 19%"
            value={form.label}
            onChange={(e) => setForm((f) => ({ ...f, label: e.target.value }))}
          />
        </Field>

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
            checked={form.active ?? true}
            onChange={(e) =>
              setForm((f) => ({ ...f, active: e.target.checked }))
            }
          />
          Cotă activă (vizibilă în dropdown-ul liniilor de factură)
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
