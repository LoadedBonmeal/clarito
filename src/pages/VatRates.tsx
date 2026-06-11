/**
 * Cote TVA — verbatim port of the design "Cote TVA.html":
 *   .page-head (title + catalog sub + sq-btn refresh + btn-dark "Cotă nouă")
 *   .banner info (Legea 141/2025 — 19/9/5% → 21/11% de la 01.08.2025)
 *   .scr-card → .scr-table (Cota · Etichetă · Activă .tog · .row-acts pen/xMark)
 *   inactive (old) rates rendered with opacity .65, like the prototype.
 *
 * GLOBAL: cotele TVA nu sunt scoped pe companie — sunt reglementate la nivel
 * național. Nu există gardă de "companie activă" pe această pagină.
 *
 * ALL wiring preserved: api.vatRates.list(false), create/edit modal →
 * api.vatRates.create/update, delete confirm → api.vatRates.delete(id),
 * active toggle → api.vatRates.setActive(id, active).
 */

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { VatRate, VatRateInput, UpdateVatRateInput } from "@/types";

// Info-circle icon — not in Ic's set, inlined verbatim from the prototype.
const SVG_INFO =
  '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';
const SVG_WARN =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

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
    if (toggleActiveMutation.isPending) return;
    toggleActiveMutation.mutate({ id: r.id, active: !r.active });
  };

  return (
    <div className="main-inner">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Cote TVA</h1>
          <p className="sub">
            Catalog național, comun tuturor companiilor · cotele se aleg pe linia de factură
            {allRates.length > 0 ? ` · ${allRates.length} cote (${activeCount} active)` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button
            className="sq-btn spin-btn"
            title="Reîmprospătează"
            onClick={() => void refetch()}
          >
            <Ic name="sync" />
          </button>
          <button className="btn-dark" onClick={() => setModal("create")}>
            <Ic name="plus" />Cotă nouă
          </button>
        </div>
      </div>

      {/* Legea 141/2025 info banner */}
      <div className="banner">
        <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_INFO }} />
        <span>
          <b>Legea 141/2025:</b> cotele 19% / 9% / 5% se aplică până la <b>31 iul 2025</b>;
          de la <b>01 aug 2025</b> cotele standard sunt <b>21% / 11%</b>. Aplicația
          avertizează automat când cota aleasă nu corespunde datei de emitere a facturii.
        </span>
      </div>

      <div className="scr-card">
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
        ) : isError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner
              error={error}
              label="cotele TVA"
              onRetry={() => void refetch()}
            />
          </div>
        ) : sortedRates.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            Nicio cotă TVA. Adăugați prima cotă cu butonul „Cotă nouă”.
          </div>
        ) : (
          <table className="scr-table">
            <thead>
              <tr>
                <th className="r" style={{ width: 90 }}>Cota</th>
                <th>Etichetă</th>
                <th style={{ width: 90, textAlign: "center" }}>Activă</th>
                <th className="r" style={{ width: 90 }}></th>
              </tr>
            </thead>
            <tbody>
              {sortedRates.map((r) => (
                <tr key={r.id} style={r.active ? undefined : { opacity: 0.65 }}>
                  <td className="r num">
                    {r.active ? <b>{r.rate}%</b> : <>{r.rate}%</>}
                  </td>
                  <td>{r.label}</td>
                  <td style={{ textAlign: "center" }}>
                    <span
                      className={`tog${r.active ? " on" : ""}`}
                      role="switch"
                      aria-checked={r.active}
                      aria-label={r.active ? "Dezactivează cota" : "Activează cota"}
                      tabIndex={0}
                      onClick={() => handleToggleActive(r)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          e.preventDefault();
                          handleToggleActive(r);
                        }
                      }}
                    />
                  </td>
                  <td>
                    <div className="row-acts">
                      <button
                        className="mini-btn"
                        title="Editează"
                        onClick={() => setModal({ edit: r })}
                      >
                        <Ic name="pen" />
                      </button>
                      <button
                        className="mini-btn"
                        title="Șterge"
                        onClick={() => void handleDelete(r)}
                      >
                        <Ic name="xMark" />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {/* create / edit modal */}
      {modal !== null && (
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

// ─── VatRateModal — design .modal-back + .modal pattern ──────────────────────

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
    const payload = {
      rate: form.rate.trim(),
      label: form.label.trim(),
      active: form.active,
      sortOrder: form.sortOrder,
    };
    if (isEdit) updateMut.mutate(payload);
    else create.mutate(payload);
  };

  return (
    <div
      className="modal-back show"
      style={{ position: "fixed", zIndex: 80 }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="modal" style={{ width: 440 }}>
        <div className="modal-head">
          <div>
            <div className="mt">{isEdit ? `Editează: ${rate.label}` : "Cotă TVA nouă"}</div>
            <div className="ms">
              Cotele active alimentează lista din editorul de factură.
            </div>
          </div>
          <button className="modal-x" onClick={onClose} aria-label="Închide">
            <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: '<path d="M6 18 18 6M6 6l12 12"/>' }} />
          </button>
        </div>
        <form onSubmit={handleSubmit} style={{ display: "contents" }}>
          <div className="modal-body">
            <div className="fgrid">
              <div className="field">
                <label>Cotă TVA % <span className="req">*</span></label>
                <input
                  className={`input num${error && !form.rate?.trim() ? " invalid" : ""}`}
                  type="number"
                  step="0.01"
                  min="0"
                  max="100"
                  placeholder="ex. 21"
                  value={form.rate}
                  onChange={(e) => setForm((f) => ({ ...f, rate: e.target.value }))}
                  autoFocus
                />
              </div>
              <div className="field">
                <label>Ordine afișare</label>
                <input
                  className="input num"
                  type="number"
                  step="1"
                  min="0"
                  placeholder="0"
                  value={String(form.sortOrder ?? 0)}
                  onChange={(e) =>
                    setForm((f) => ({ ...f, sortOrder: parseInt(e.target.value) || 0 }))
                  }
                />
              </div>
              <div className="field span2">
                <label>Etichetă <span className="req">*</span></label>
                <input
                  className={`input${error && !form.label?.trim() ? " invalid" : ""}`}
                  placeholder="ex. Cota standard (de la 01.08.2025)"
                  value={form.label}
                  onChange={(e) => setForm((f) => ({ ...f, label: e.target.value }))}
                />
              </div>
              <label
                className="span2"
                style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, cursor: "pointer", userSelect: "none" }}
              >
                <button
                  type="button"
                  className={`cbx${form.active ? " on" : ""}`}
                  onClick={() => setForm((f) => ({ ...f, active: !f.active }))}
                  aria-label="Cotă activă"
                />
                Cotă activă (vizibilă în dropdown-ul liniilor de factură)
              </label>
            </div>
            {error && (
              <div className="banner danger" style={{ marginTop: 12, marginBottom: 0 }}>
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN }} />
                <span>{error}</span>
              </div>
            )}
          </div>
          <div className="modal-foot">
            <button type="button" className="pill-btn" onClick={onClose} disabled={isPending}>
              Anulează
            </button>
            <button type="submit" className="btn-dark" disabled={isPending}>
              <Ic name="check" />
              {isPending ? "Se salvează…" : isEdit ? "Salvează" : "Adaugă"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
