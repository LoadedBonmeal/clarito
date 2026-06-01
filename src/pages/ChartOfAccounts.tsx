/**
 * Plan de conturi — re-skinned to rf kit (Wave 3).
 * Preserves: api.accounts.list(activeCompanyId) grouped by class, search,
 * create/edit modal → api.accounts.create / api.accounts.update(id, companyId, input),
 * delete confirm → api.accounts.delete(id, companyId),
 * "Încarcă planul standard (PCG)" → api.accounts.seedStandard(activeCompanyId),
 * "select active company" guard.
 */

import { useMemo, useState, Fragment } from "react";
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
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { Account, AccountInput, UpdateAccountInput } from "@/types";

// ─── Account classes ──────────────────────────────────────────────────────────

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

// ─── Page ──────────────────────────────────────────────────────────────────────

export function ChartOfAccountsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<"all" | "active">("all");
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
    const base =
      filter === "active"
        ? allAccounts.filter((a) => a.active)
        : allAccounts;
    const q = query.trim().toLowerCase();
    if (!q) return base;
    return base.filter(
      (a) =>
        a.accountCode.toLowerCase().includes(q) ||
        a.accountName.toLowerCase().includes(q),
    );
  }, [allAccounts, query, filter]);

  const activeCount = allAccounts.filter((a) => a.active).length;

  // ── Seed standard ────────────────────────────────────────────────────────────
  const seedMutation = useMutation({
    mutationFn: () => {
      if (!activeCompanyId)
        return Promise.reject(new Error("Nicio companie activă."));
      return api.accounts.seedStandard(activeCompanyId);
    },
    onSuccess: (inserted) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts.all });
      notify.success(`${inserted} conturi standard încărcate.`);
    },
    onError: (e) =>
      notify.error(formatError(e, "Eroare la încărcarea planului standard.")),
  });

  // ── Delete ────────────────────────────────────────────────────────────────────
  const deleteMutation = useMutation({
    mutationFn: (id: string) => {
      if (!activeCompanyId)
        return Promise.reject(new Error("Nicio companie activă."));
      return api.accounts.delete(id, activeCompanyId);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts.all });
      notify.success("Cont șters.");
    },
    onError: (e) =>
      notify.error(formatError(e, "Nu s-a putut șterge contul.")),
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

  // ── Group by class ────────────────────────────────────────────────────────────
  const grouped = useMemo(() => {
    const map = new Map<number | null, Account[]>();
    for (const a of list) {
      const cls = a.accountClass;
      if (!map.has(cls)) map.set(cls, []);
      map.get(cls)!.push(a);
    }
    return [...map.entries()].sort(([a], [b]) => {
      if (a === null) return 1;
      if (b === null) return -1;
      return a - b;
    });
  }, [list]);

  if (!activeCompanyId) {
    return (
      <div className="rf-page">
        <PageHeader title="Plan de conturi" />
        <div className="rf-page-body">
          <Empty icon="book" title="Selectați o companie activă">
            Selectați o companie din bara laterală pentru a vedea planul de conturi.
          </Empty>
        </div>
      </div>
    );
  }

  const filterTabs = [
    { value: "all" as const, label: "Toate", badge: allAccounts.length },
    { value: "active" as const, label: "Active", badge: activeCount },
  ];

  return (
    <div className="rf-page">
      <PageHeader
        title="Plan de conturi"
        sub={
          <Badge variant="neutral" dot={false}>
            {list.length} conturi
          </Badge>
        }
        actions={
          <>
            <Btn
              variant="secondary"
              icon="download"
              size="sm"
              disabled={seedMutation.isPending}
              onClick={() => seedMutation.mutate()}
            >
              {seedMutation.isPending
                ? "Se încarcă…"
                : "Încarcă planul standard (PCG)"}
            </Btn>
            <Btn
              variant="primary"
              icon="plus"
              size="sm"
              onClick={() => setModal("create")}
            >
              Cont nou
            </Btn>
          </>
        }
      />

      <div className="rf-page-body">
        <Card>
          {/* Tabs */}
          <div
            style={{
              padding: "10px 16px 0",
              borderBottom: "1px solid var(--rf-border)",
            }}
          >
            <Tabs tabs={filterTabs} value={filter} onChange={(v) => setFilter(v)} />
          </div>

          {/* Toolbar */}
          <div
            className="rf-toolbar-row"
            style={{ padding: "10px 16px", borderBottom: "1px solid var(--rf-border)" }}
          >
            <SearchInput
              placeholder="Caută după cod sau denumire…"
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
                    queryKey: queryKeys.accounts.all,
                  })
                }
              />
            </div>
          </div>

          {/* Table */}
          <div className="rf-tbl-wrap">
            {isLoading ? (
              <Empty icon="book" title="Se încarcă…" />
            ) : isError ? (
              <QueryErrorBanner
                error={error}
                label="conturile"
                onRetry={() => void refetch()}
              />
            ) : allAccounts.length === 0 ? (
              <Empty
                icon="book"
                title="Niciun cont înregistrat"
                actions={
                  <Btn
                    variant="primary"
                    icon="download"
                    disabled={seedMutation.isPending}
                    onClick={() => seedMutation.mutate()}
                  >
                    {seedMutation.isPending
                      ? "Se încarcă…"
                      : "Încarcă planul standard (PCG)"}
                  </Btn>
                }
              >
                Puteți adăuga manual sau încărca planul standard român (PCG).
              </Empty>
            ) : list.length === 0 ? (
              <Empty icon="search" title="Niciun rezultat">
                Niciun rezultat pentru filtrele aplicate.
              </Empty>
            ) : (
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th style={{ width: 110 }}>Cod</th>
                    <th>Denumire</th>
                    <th style={{ width: 200 }}>Clasă</th>
                    <th style={{ width: 110 }}>Cont părinte</th>
                    <th style={{ width: 70, textAlign: "center" }}>Activ</th>
                    <th style={{ width: 80 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {grouped.map(([cls, entries]) => (
                    <Fragment key={`cls-${cls ?? "null"}`}>
                      {/* Group header */}
                      <tr>
                        <td
                          colSpan={6}
                          style={{
                            background: "var(--rf-bg, var(--rf-border))",
                            fontWeight: 700,
                            fontSize: 11,
                            color: "var(--rf-text-muted)",
                            padding: "5px 10px",
                            borderTop: "1px solid var(--rf-border)",
                            letterSpacing: "0.04em",
                            textTransform: "uppercase",
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
                          <td style={{ fontWeight: 500 }}>{a.accountName}</td>
                          <td
                            style={{ fontSize: 12, color: "var(--rf-text-muted)" }}
                          >
                            {a.accountClass != null
                              ? CLASS_LABELS[a.accountClass] ??
                                String(a.accountClass)
                              : <span className="rf-dim">—</span>}
                          </td>
                          <td className="mono">
                            {a.parentCode ?? (
                              <span className="rf-dim">—</span>
                            )}
                          </td>
                          <td style={{ textAlign: "center" }}>
                            {a.active ? (
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
                                onClick={() => setModal({ edit: a })}
                              />
                              <IconBtn
                                icon="trash"
                                title="Șterge"
                                size={14}
                                onClick={() => void handleDelete(a)}
                              />
                            </div>
                          </td>
                        </tr>
                      ))}
                    </Fragment>
                  ))}
                </tbody>
              </table>
            )}
          </div>

          {/* Footer */}
          <div className="rf-tbl-footer">
            <span>
              Total: <b>{list.length}</b> conturi
            </span>
            <span>
              Active: <b>{activeCount}</b>
            </span>
            <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--rf-text-dim)" }}>
              Integrarea cu înregistrările contabile dublu-intrare: planificată v2
            </span>
          </div>
        </Card>
      </div>

      {/* Account modal */}
      {modal !== null && (
        <AccountModal
          companyId={activeCompanyId}
          account={modal === "create" ? null : modal.edit}
          onClose={() => setModal(null)}
          onSaved={() => {
            void queryClient.invalidateQueries({
              queryKey: queryKeys.accounts.all,
            });
            setModal(null);
          }}
        />
      )}
    </div>
  );
}

// ─── AccountModal ─────────────────────────────────────────────────────────────

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
    onSuccess: () => {
      notify.success("Cont adăugat.");
      onSaved();
    },
    onError: (e) => setError(formatError(e, "Eroare la adăugare.")),
  });

  const updateMut = useMutation({
    mutationFn: (input: UpdateAccountInput) =>
      api.accounts.update(account!.id, companyId, input),
    onSuccess: () => {
      notify.success("Cont salvat.");
      onSaved();
    },
    onError: (e) => setError(formatError(e, "Eroare la salvare.")),
  });

  const isPending = createMut.isPending || updateMut.isPending;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (isPending) return;
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
    <Modal
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={
        isEdit
          ? `Editează: ${account.accountCode} — ${account.accountName}`
          : "Cont nou"
      }
      width={480}
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
          <Field label="Cod cont" required>
            <Input
              className="mono"
              placeholder="ex. 4111"
              value={form.accountCode ?? ""}
              onChange={(e) =>
                setForm((f) => ({ ...f, accountCode: e.target.value }))
              }
              autoFocus
            />
          </Field>
          <Field label="Clasă">
            <Select
              value={String(form.accountClass ?? "")}
              onChange={(e) =>
                setForm((f) => ({
                  ...f,
                  accountClass: e.target.value
                    ? Number(e.target.value)
                    : undefined,
                }))
              }
            >
              <option value="">— fără clasă —</option>
              {[1, 2, 3, 4, 5, 6, 7, 8, 9].map((cls) => (
                <option key={cls} value={cls}>
                  {cls} — {CLASS_LABELS[cls] ?? `Clasa ${cls}`}
                </option>
              ))}
            </Select>
          </Field>
        </div>

        <Field label="Denumire" required>
          <Input
            placeholder="ex. Clienți"
            value={form.accountName ?? ""}
            onChange={(e) =>
              setForm((f) => ({ ...f, accountName: e.target.value }))
            }
          />
        </Field>

        <Field label="Cont părinte (cod)">
          <Input
            className="mono"
            placeholder="ex. 411"
            value={form.parentCode ?? ""}
            onChange={(e) =>
              setForm((f) => ({
                ...f,
                parentCode: e.target.value || undefined,
              }))
            }
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
            checked={form.active as boolean}
            onChange={(e) =>
              setForm((f) => ({ ...f, active: e.target.checked }))
            }
          />
          Cont activ
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
