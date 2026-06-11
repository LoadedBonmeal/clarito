/**
 * Plan de conturi — verbatim port of the design "Plan de conturi.html":
 *   .page-head (title + sub "Plan de conturi propriu companiei · N conturi ·
 *   soldurile se consultă în Balanță" + pill-btn "Populează planul standard (PCG)"
 *   + btn-dark "Cont nou") · .scr-card → .scr-toolbar (.tabs Toate/Active ·
 *   .scr-search 220px · sq-btn refresh) → .scr-table (cod .doc · denumire ·
 *   clasă .muted "1 · Conturi de capitaluri" · cont părinte .doc · .tog activ ·
 *   .row-acts pen/trash) → .pager real (client-side) · group header rows pe
 *   clasele 1–9 (funcționalitate reală păstrată, restilizată).
 *
 * ALL wiring preserved: api.accounts.list(activeCompanyId) grouped by class,
 * search, create/edit modal → api.accounts.create / api.accounts.update,
 * delete confirm → api.accounts.delete, toggle activ → api.accounts.update,
 * "Populează planul standard (PCG)" → api.accounts.seedStandard,
 * "select active company" guard.
 */

import { useEffect, useMemo, useState, Fragment } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { Account, AccountInput, UpdateAccountInput } from "@/types";

const PAGE_SIZE = 50;

// Inline icons absent from Ic (verbatim from the prototype / heroicons outline).
const SVG_TRASH = '<path d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>';
const SVG_CHEV_L = '<path d="M15.75 19.5 8.25 12l7.5-7.5"/>';
const SVG_WARN = '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

// ─── Account classes (label per prototip: "1 · Conturi de capitaluri") ────────

const CLASS_LABELS: Record<number, string> = {
  1: "1 · Conturi de capitaluri",
  2: "2 · Conturi de imobilizări",
  3: "3 · Conturi de stocuri",
  4: "4 · Conturi de terți",
  5: "5 · Conturi de trezorerie",
  6: "6 · Conturi de cheltuieli",
  7: "7 · Conturi de venituri",
  8: "8 · Conturi speciale",
  9: "9 · Conturi interne (gestiune)",
};

// ─── Page ──────────────────────────────────────────────────────────────────────

export function ChartOfAccountsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<"all" | "active">("all");
  const [modal, setModal] = useState<"create" | { edit: Account } | null>(null);
  const [pageRaw, setPageRaw] = useState(1);

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

  // Sortat pe clasă (1–9, fără clasă la final) apoi pe cod → grupare reală.
  const list = useMemo(() => {
    const base =
      filter === "active"
        ? allAccounts.filter((a) => a.active)
        : allAccounts;
    const q = query.trim().toLowerCase();
    const filtered = !q
      ? base
      : base.filter(
          (a) =>
            a.accountCode.toLowerCase().includes(q) ||
            a.accountName.toLowerCase().includes(q),
        );
    return [...filtered].sort((a, b) => {
      const ca = a.accountClass ?? 99;
      const cb = b.accountClass ?? 99;
      if (ca !== cb) return ca - cb;
      return a.accountCode.localeCompare(b.accountCode, "ro", { numeric: true });
    });
  }, [allAccounts, query, filter]);

  const activeCount = allAccounts.filter((a) => a.active).length;

  // Paginare reală client-side (design .pager).
  useEffect(() => { setPageRaw(1); }, [query, filter]);
  const totalPages = Math.max(1, Math.ceil(list.length / PAGE_SIZE));
  const page = Math.min(pageRaw, totalPages);
  const visibleRows = list.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE);
  const pageWindow = useMemo(() => {
    const start = Math.max(1, Math.min(page - 2, totalPages - 4));
    const end = Math.min(totalPages, start + 4);
    const out: number[] = [];
    for (let i = start; i <= end; i++) out.push(i);
    return out;
  }, [page, totalPages]);

  // ── Seed standard (PCG) ──────────────────────────────────────────────────────
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

  // ── Delete ───────────────────────────────────────────────────────────────────
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

  // ── Toggle activ (design .tog) ───────────────────────────────────────────────
  const toggleMutation = useMutation({
    mutationFn: (a: Account) => {
      if (!activeCompanyId)
        return Promise.reject(new Error("Nicio companie activă."));
      const input: UpdateAccountInput = {
        accountCode: a.accountCode,
        accountName: a.accountName,
        accountClass: a.accountClass ?? undefined,
        parentCode: a.parentCode ?? undefined,
        active: !a.active,
      };
      return api.accounts.update(a.id, activeCompanyId, input);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts.all });
    },
    onError: (e) =>
      notify.error(formatError(e, "Nu s-a putut actualiza contul.")),
  });

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>Plan de conturi</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Selectați o companie activă pentru a vedea planul de conturi.
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Plan de conturi</h1>
          <p className="sub">
            Plan de conturi propriu companiei · {list.length.toLocaleString("ro-RO")} conturi · soldurile se consultă în Balanță (Jurnal contabil)
          </p>
        </div>
        <div className="head-actions">
          <button
            className="pill-btn"
            disabled={seedMutation.isPending}
            onClick={() => seedMutation.mutate()}
          >
            <Ic name="book" />
            {seedMutation.isPending ? "Se încarcă…" : "Populează planul standard (PCG)"}
          </button>
          <button className="btn-dark" onClick={() => setModal("create")}>
            <Ic name="plus" />Cont nou
          </button>
        </div>
      </div>

      <div className="scr-card">
        {/* toolbar */}
        <div className="scr-toolbar">
          <div className="tabs">
            <div
              className={`tab${filter === "all" ? " active" : ""}`}
              onClick={() => setFilter("all")}
            >
              Toate<span className="cnt">{allAccounts.length}</span>
            </div>
            <div
              className={`tab${filter === "active" ? " active" : ""}`}
              onClick={() => setFilter("active")}
            >
              Active<span className="cnt">{activeCount}</span>
            </div>
          </div>
          <div className="spacer" />
          <div className="scr-search" style={{ width: 220 }}>
            <Ic name="lens" />
            <input
              type="text"
              placeholder="Cod sau denumire…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <button className="sq-btn spin-btn" title="Reîmprospătează" onClick={() => void refetch()}>
            <Ic name="sync" />
          </button>
        </div>

        {/* table */}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
        ) : isError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={error} label="conturile" onRetry={() => void refetch()} />
          </div>
        ) : allAccounts.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            <div style={{ marginBottom: 12 }}>
              Niciun cont înregistrat. Puteți adăuga manual sau încărca planul standard român (PCG).
            </div>
            <button
              className="pill-btn"
              style={{ margin: "0 auto" }}
              disabled={seedMutation.isPending}
              onClick={() => seedMutation.mutate()}
            >
              <Ic name="book" />
              {seedMutation.isPending ? "Se încarcă…" : "Populează planul standard (PCG)"}
            </button>
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            Niciun rezultat pentru filtrele aplicate.
          </div>
        ) : (
          <>
            <table className="scr-table">
              <thead>
                <tr>
                  <th style={{ width: 110 }}>Cod</th>
                  <th>Denumire</th>
                  <th style={{ width: 210 }}>Clasa</th>
                  <th style={{ width: 110 }}>Cont părinte</th>
                  <th style={{ width: 70, textAlign: "center" }}>Activ</th>
                  <th className="r" style={{ width: 90 }}></th>
                </tr>
              </thead>
              <tbody>
                {visibleRows.map((a, idx) => {
                  const prev = visibleRows[idx - 1];
                  const newGroup = idx === 0 || (prev?.accountClass ?? null) !== (a.accountClass ?? null);
                  return (
                    <Fragment key={a.id}>
                      {/* group header — grupare reală pe clasele 1–9 (restilizată) */}
                      {newGroup && (
                        <tr>
                          <td
                            colSpan={6}
                            style={{
                              background: "var(--fill)",
                              fontWeight: 700,
                              fontSize: 11,
                              color: "var(--text-2)",
                              padding: "5px 16px",
                              letterSpacing: ".04em",
                              textTransform: "uppercase",
                            }}
                          >
                            {a.accountClass != null
                              ? CLASS_LABELS[a.accountClass] ?? `Clasa ${a.accountClass}`
                              : "Fără clasă"}
                          </td>
                        </tr>
                      )}
                      <tr>
                        <td><span className="doc">{a.accountCode}</span></td>
                        <td>{a.accountName}</td>
                        <td className="muted">
                          {a.accountClass != null
                            ? CLASS_LABELS[a.accountClass] ?? `Clasa ${a.accountClass}`
                            : "—"}
                        </td>
                        <td>{a.parentCode ? <span className="doc">{a.parentCode}</span> : <span className="muted">—</span>}</td>
                        <td style={{ textAlign: "center" }}>
                          <span
                            className={`tog${a.active ? " on" : ""}`}
                            role="switch"
                            aria-checked={a.active}
                            title={a.active ? "Dezactivează contul" : "Activează contul"}
                            style={{ cursor: "pointer", opacity: toggleMutation.isPending ? 0.6 : 1 }}
                            onClick={() => { if (!toggleMutation.isPending) toggleMutation.mutate(a); }}
                          />
                        </td>
                        <td>
                          <div className="row-acts">
                            <button className="mini-btn" title="Editează" onClick={() => setModal({ edit: a })}>
                              <Ic name="pen" />
                            </button>
                            <button className="mini-btn" title="Șterge" onClick={() => void handleDelete(a)}>
                              <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_TRASH }} />
                            </button>
                          </div>
                        </td>
                      </tr>
                    </Fragment>
                  );
                })}
              </tbody>
            </table>

            {/* pager */}
            <div className="pager">
              <span>
                Afișezi <b>{((page - 1) * PAGE_SIZE + 1).toLocaleString("ro-RO")}–{Math.min(page * PAGE_SIZE, list.length).toLocaleString("ro-RO")}</b> din <b>{list.length.toLocaleString("ro-RO")}</b> conturi
              </span>
              <div className="pg-btns">
                <button
                  className="pg-btn"
                  disabled={page <= 1}
                  onClick={() => setPageRaw(page - 1)}
                  aria-label="Pagina anterioară"
                >
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_CHEV_L }} />
                </button>
                {pageWindow.map((n) => (
                  <button
                    key={n}
                    className={`pg-btn${n === page ? " cur" : ""}`}
                    onClick={() => setPageRaw(n)}
                  >
                    {n}
                  </button>
                ))}
                <button
                  className="pg-btn"
                  disabled={page >= totalPages}
                  onClick={() => setPageRaw(page + 1)}
                  aria-label="Pagina următoare"
                >
                  <Ic name="chevR" />
                </button>
              </div>
            </div>
          </>
        )}
      </div>

      {/* account modal */}
      {modal !== null && (
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

// ─── AccountModal — design .modal-back/.modal pattern ─────────────────────────

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
    <div
      className="modal-back show"
      style={{ position: "fixed", zIndex: 80 }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="modal">
        <div className="modal-head">
          <div>
            <div className="mt">
              {isEdit ? `Editează: ${account.accountCode} — ${account.accountName}` : "Cont nou"}
            </div>
            <div className="ms">
              Codurile urmează Planul de Conturi General (OMFP 1802/2014).
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
                <label>Cod cont <span className="req">*</span></label>
                <input
                  className={`input num${error && !form.accountCode?.trim() ? " invalid" : ""}`}
                  placeholder="ex. 4111"
                  autoFocus
                  value={form.accountCode ?? ""}
                  onChange={(e) => setForm((f) => ({ ...f, accountCode: e.target.value }))}
                />
                {error && !form.accountCode?.trim() && (
                  <span className="err">
                    <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN }} />
                    {error}
                  </span>
                )}
              </div>
              <div className="field">
                <label>Clasă</label>
                <select
                  className="select"
                  value={String(form.accountClass ?? "")}
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
                      {CLASS_LABELS[cls] ?? `Clasa ${cls}`}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field span2">
                <label>Denumire <span className="req">*</span></label>
                <input
                  className={`input${error && !form.accountName?.trim() && form.accountCode?.trim() ? " invalid" : ""}`}
                  placeholder="ex. Clienți"
                  value={form.accountName ?? ""}
                  onChange={(e) => setForm((f) => ({ ...f, accountName: e.target.value }))}
                />
                {error && !form.accountName?.trim() && form.accountCode?.trim() && (
                  <span className="err">
                    <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN }} />
                    {error}
                  </span>
                )}
              </div>
              <div className="field">
                <label>Cont părinte (cod)</label>
                <input
                  className="input num"
                  placeholder="ex. 411"
                  value={form.parentCode ?? ""}
                  onChange={(e) => setForm((f) => ({ ...f, parentCode: e.target.value || undefined }))}
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
                  aria-label="Cont activ"
                />
                Cont activ
              </label>
            </div>
            {error && form.accountCode?.trim() && form.accountName?.trim() && (
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
