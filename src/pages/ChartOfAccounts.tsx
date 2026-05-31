/**
 * Plan de conturi — company-scoped chart of accounts catalog.
 *
 * Listează conturile companiei active grupate/sortate după codul de cont,
 * permite adăugare/editare via modal și ștergere cu confirmare.
 * Butonul "Încarcă planul standard" este vizibil când lista e goală.
 *
 * Notă: Aceasta este o pagină de catalog de referință (CRUD + seed).
 * Integrarea cu înregistrările contabile dublu-intrare este planificată
 * pentru o versiune viitoare și este în afara scopului acestui modul.
 */

import { useMemo, useState, useId, isValidElement, cloneElement } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Icon } from "@/components/shared/Icon";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { Account, AccountInput, UpdateAccountInput } from "@/types";

// ─── Account classes ────────────────────────────────────────────────────────

const CLASS_LABELS: Record<number, string> = {
  1: "Clasa 1 — Capitaluri",
  2: "Clasa 2 — Imobilizări",
  3: "Clasa 3 — Stocuri",
  4: "Clasa 4 — Terți",
  5: "Clasa 5 — Trezorerie",
  6: "Clasa 6 — Cheltuieli",
  7: "Clasa 7 — Venituri",
  8: "Clasa 8 — Speciale",
  9: "Clasa 9 — Interne",
};

// ─── Page ───────────────────────────────────────────────────────────────────

export function ChartOfAccountsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [query, setQuery] = useState("");
  const [modal, setModal] = useState<"create" | { edit: Account } | null>(null);

  const {
    data: allAccounts = [],
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: queryKeys.accounts.list(activeCompanyId ?? ""),
    queryFn: () => api.accounts.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return allAccounts;
    return allAccounts.filter(
      (a) =>
        a.accountCode.toLowerCase().includes(q) ||
        a.accountName.toLowerCase().includes(q),
    );
  }, [allAccounts, query]);

  const activeCount = allAccounts.filter((a) => a.active).length;

  // ── Seed standard ──────────────────────────────────────────────────────────
  const seedMutation = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă."));
      return api.accounts.seedStandard(activeCompanyId);
    },
    onSuccess: (inserted) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts.all });
      notify.success(`${inserted} conturi standard încărcate.`);
    },
    onError: (e) => notify.error(formatError(e, "Eroare la încărcarea planului standard.")),
  });

  // ── Delete ─────────────────────────────────────────────────────────────────
  const deleteMutation = useMutation({
    mutationFn: (id: string) => {
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă."));
      return api.accounts.delete(id, activeCompanyId);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts.all });
      notify.success("Cont șters.");
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge contul.")),
  });

  const handleDelete = async (a: Account) => {
    if (!activeCompanyId) return;
    const ok = await confirm(
      `Șterge contul ${a.accountCode} — "${a.accountName}"? Această acțiune nu poate fi anulată.`,
      { title: "Confirmare ștergere", kind: "warning" },
    );
    if (!ok) return;
    deleteMutation.mutate(a.id);
  };

  // ── Group by class ──────────────────────────────────────────────────────────
  const grouped = useMemo(() => {
    const map = new Map<number | null, Account[]>();
    for (const a of list) {
      const cls = a.accountClass;
      if (!map.has(cls)) map.set(cls, []);
      map.get(cls)!.push(a);
    }
    // Sort class keys numerically; null last.
    return [...map.entries()].sort(([a], [b]) => {
      if (a === null) return 1;
      if (b === null) return -1;
      return a - b;
    });
  }, [list]);

  if (!activeCompanyId) {
    return (
      <div className="content">
        <div className="content-titlebar">
          <span className="content-title">
            <span className="crumb">Date</span>
            Plan de conturi
          </span>
        </div>
        <div style={{ padding: 40, textAlign: "center", fontSize: 12, color: "var(--text-muted)" }}>
          Selectați o companie activă pentru a vedea planul de conturi.
        </div>
      </div>
    );
  }

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">Date</span>
          Plan de conturi
        </span>
        <span className="muted" style={{ fontSize: 11 }}>
          {list.length} din {allAccounts.length} conturi
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button
            type="button"
            className="btn primary"
            onClick={() => setModal("create")}
          >
            <Icon name="plus" size={12} /> Cont nou
          </button>
        </span>
      </div>

      <div className="views-bar">
        <span className="view-tab active">
          Toate <span className="count">{allAccounts.length}</span>
        </span>
        <span className="view-tab">
          Active <span className="count">{activeCount}</span>
        </span>
      </div>

      <div className="content-toolbar">
        <div className="search">
          <Icon name="search" size={13} />
          <input
            placeholder="Caută după cod sau denumire…"
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
              void queryClient.invalidateQueries({ queryKey: queryKeys.accounts.all })
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
            label="conturile"
            onRetry={() => void refetch()}
          />
        ) : allAccounts.length === 0 ? (
          <div
            style={{
              padding: 48,
              textAlign: "center",
              fontSize: 12,
              color: "var(--text-muted)",
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 12,
            }}
          >
            <p>Niciun cont înregistrat. Puteți adăuga manual sau încărca planul standard român (PCG).</p>
            <button
              type="button"
              className="btn primary"
              onClick={() => seedMutation.mutate()}
              disabled={seedMutation.isPending}
            >
              {seedMutation.isPending ? "Se încarcă…" : "Încarcă planul standard (PCG)"}
            </button>
          </div>
        ) : list.length === 0 ? (
          <div
            style={{
              padding: 40,
              textAlign: "center",
              fontSize: 12,
              color: "var(--text-muted)",
            }}
          >
            Niciun rezultat pentru filtrele aplicate.
          </div>
        ) : (
          <table className="dt">
            <thead>
              <tr>
                <th style={{ width: 100 }}>Cod</th>
                <th>Denumire</th>
                <th style={{ width: 180 }}>Clasă</th>
                <th style={{ width: 110 }}>Cont părinte</th>
                <th style={{ width: 60 }}>Activ</th>
                <th style={{ width: 80 }}>Acțiuni</th>
              </tr>
            </thead>
            <tbody>
              {grouped.map(([cls, entries]) => (
                <>
                  {/* Group header */}
                  <tr key={`cls-${cls ?? "null"}`}>
                    <td
                      colSpan={6}
                      style={{
                        background: "var(--bg-subtle, var(--bg))",
                        fontWeight: 700,
                        fontSize: 11,
                        color: "var(--text-muted)",
                        padding: "4px 8px",
                        borderTop: "1px solid var(--border)",
                        letterSpacing: "0.03em",
                      }}
                    >
                      {cls != null
                        ? CLASS_LABELS[cls] ?? `Clasa ${cls}`
                        : "Fără clasă"}
                    </td>
                  </tr>
                  {entries.map((a) => (
                    <tr key={a.id}>
                      <td className="mono" style={{ fontWeight: 600 }}>
                        {a.accountCode}
                      </td>
                      <td>{a.accountName}</td>
                      <td className="muted" style={{ fontSize: 11 }}>
                        {a.accountClass != null
                          ? CLASS_LABELS[a.accountClass] ?? String(a.accountClass)
                          : <span className="dim">—</span>}
                      </td>
                      <td className="mono">
                        {a.parentCode ?? <span className="dim">—</span>}
                      </td>
                      <td>
                        {a.active ? (
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
                          onClick={() => setModal({ edit: a })}
                        >
                          <Icon name="pen" size={13} />
                        </button>
                        <button
                          type="button"
                          className="btn-icon"
                          title="Șterge"
                          onClick={() => void handleDelete(a)}
                        >
                          <Icon name="x" size={13} />
                        </button>
                      </td>
                    </tr>
                  ))}
                </>
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
          Total: <b style={{ color: "var(--text)" }}>{list.length}</b> conturi
        </span>
        <span>
          Active: <b style={{ color: "var(--text)" }}>{activeCount}</b>
        </span>
        <span style={{ color: "var(--text-dim)", fontSize: 10, marginLeft: "auto" }}>
          Notă: integrarea cu înregistrările contabile dublu-intrare este planificată pentru o versiune viitoare.
        </span>
      </div>

      {modal && (
        <AccountModal
          companyId={activeCompanyId}
          account={modal === "create" ? null : modal.edit}
          onClose={() => setModal(null)}
          onSaved={() => {
            void queryClient.invalidateQueries({ queryKey: queryKeys.accounts.all });
            setModal(null);
          }}
        />
      )}
    </div>
  );
}

// ─── Modal ──────────────────────────────────────────────────────────────────

function AccountModal({
  companyId,
  account,
  onClose,
  onSaved,
}: {
  companyId: string;
  account: Account | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const isEdit = account !== null;

  const [form, setForm] = useState<AccountInput>({
    accountCode: account?.accountCode ?? "",
    accountName: account?.accountName ?? "",
    accountClass: account?.accountClass ?? undefined,
    parentCode: account?.parentCode ?? undefined,
    active: account?.active ?? true,
  });
  const [error, setError] = useState<string | null>(null);

  const createMut = useMutation({
    mutationFn: (input: AccountInput) => api.accounts.create(companyId, input),
    onSuccess: () => { notify.success("Cont adăugat."); onSaved(); },
    onError: (e) => setError(formatError(e, "Eroare la adăugare.")),
  });

  const updateMut = useMutation({
    mutationFn: (input: UpdateAccountInput) =>
      api.accounts.update(account!.id, companyId, input),
    onSuccess: () => { notify.success("Cont salvat."); onSaved(); },
    onError: (e) => setError(formatError(e, "Eroare la salvare.")),
  });

  const isPending = createMut.isPending || updateMut.isPending;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    if (!form.accountCode?.trim()) {
      setError("Codul de cont este obligatoriu.");
      return;
    }
    if (!form.accountName?.trim()) {
      setError("Denumirea este obligatorie.");
      return;
    }
    const input: AccountInput = {
      accountCode: form.accountCode.trim(),
      accountName: form.accountName.trim(),
      accountClass: form.accountClass,
      parentCode: form.parentCode?.trim() || undefined,
      active: form.active,
    };
    if (isEdit) {
      updateMut.mutate(input);
    } else {
      createMut.mutate(input);
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
            {isEdit
              ? `Editează: ${account.accountCode} — ${account.accountName}`
              : "Cont nou"}
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
            <MField label="Cod cont *" style={{ flex: 1 }}>
              <input
                className="field mono"
                placeholder="ex. 4111"
                value={form.accountCode ?? ""}
                onChange={(e) => setForm((f) => ({ ...f, accountCode: e.target.value }))}
                autoFocus
              />
            </MField>
            <MField label="Clasă" style={{ flex: 1 }}>
              <select
                className="field"
                value={form.accountClass ?? ""}
                onChange={(e) =>
                  setForm((f) => ({
                    ...f,
                    accountClass: e.target.value ? Number(e.target.value) : undefined,
                  }))
                }
              >
                <option value="">— fără clasă —</option>
                {[1, 2, 3, 4, 5, 6, 7, 8, 9].map((cls) => (
                  <option key={cls} value={cls}>
                    {cls} — {CLASS_LABELS[cls] ?? `Clasa ${cls}`}
                  </option>
                ))}
              </select>
            </MField>
          </div>

          <MField label="Denumire *">
            <input
              className="field"
              placeholder="ex. Clienți"
              value={form.accountName ?? ""}
              onChange={(e) => setForm((f) => ({ ...f, accountName: e.target.value }))}
            />
          </MField>

          <MField label="Cont părinte (cod)">
            <input
              className="field mono"
              placeholder="ex. 411"
              value={form.parentCode ?? ""}
              onChange={(e) =>
                setForm((f) => ({
                  ...f,
                  parentCode: e.target.value || undefined,
                }))
              }
            />
          </MField>

          <div style={{ display: "flex", alignItems: "center", gap: 8, paddingTop: 2 }}>
            <input
              id="m-active-account"
              type="checkbox"
              className="cbx"
              checked={form.active as boolean}
              onChange={(e) => setForm((f) => ({ ...f, active: e.target.checked }))}
            />
            <label
              htmlFor="m-active-account"
              style={{ fontSize: 12, cursor: "pointer", userSelect: "none" }}
            >
              Cont activ
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
