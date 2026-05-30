/**
 * Facturi emise — date REALE din backend (api.invoices.list + api.contacts.list),
 * cu vizualul Win32 portat din Claude Design (views-bar, content-toolbar, tabel .dt).
 */

import { useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useTranslation } from "react-i18next";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { CsvImportModal } from "@/components/shared/CsvImportModal";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { formatOptionalRon } from "@/lib/formatters";

import { fmtShortcut } from "@/lib/platform";
import { notify } from "@/lib/toasts";
import type { InvoiceStatus } from "@/types";

type StatusFilter = InvoiceStatus | "all";

export function InvoicesPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const setSelectedInvoiceId = useAppStore((s) => s.setSelectedInvoiceId);
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<StatusFilter>("all");
  const [errorsOnly, setErrorsOnly] = useState(false);
  const [amountMin, setAmountMin] = useState("");
  const [amountMax, setAmountMax] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [showImportModal, setShowImportModal] = useState(false);

  // Fetch invoices (default page: first 50)
  const { data: paged, isLoading, isError: pagedError, error: pagedErr, refetch: refetchPaged } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.invoices.list({ companyId: activeCompanyId ?? undefined }),
  });

  // Fetch contacts for this company so we can show client name / CUI in the table
  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const contactMap = useMemo(() => {
    const m = new Map<string, { legalName: string; cui: string | null }>();
    for (const c of contacts) m.set(c.id, { legalName: c.legalName, cui: c.cui });
    return m;
  }, [contacts]);

  const allInvoices = paged?.items ?? [];
  const totalCount = paged?.total ?? 0;

  // Client-side filter (status + text search + amount range + errors-only)
  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    const minVal = amountMin.trim() ? parseFloat(amountMin) : null;
    const maxVal = amountMax.trim() ? parseFloat(amountMax) : null;
    return allInvoices
      .filter((i) => {
        if (errorsOnly) return i.status === "REJECTED";
        return filter === "all" || i.status === filter;
      })
      .filter((i) => {
        if (minVal !== null && !isNaN(minVal) && parseDec(i.totalAmount) < minVal) return false;
        if (maxVal !== null && !isNaN(maxVal) && parseDec(i.totalAmount) > maxVal) return false;
        return true;
      })
      .filter((i) => {
        if (!q) return true;
        const client = contactMap.get(i.contactId);
        return (
          i.fullNumber.toLowerCase().includes(q) ||
          (client?.legalName.toLowerCase().includes(q) ?? false) ||
          (client?.cui?.toLowerCase().includes(q) ?? false)
        );
      });
  }, [allInvoices, filter, errorsOnly, amountMin, amountMax, query, contactMap]);

  // Status counts (from currently loaded page)
  const counts = {
    VALIDATED: allInvoices.filter((i) => i.status === "VALIDATED").length,
    SUBMITTED: allInvoices.filter((i) => i.status === "SUBMITTED").length,
    REJECTED:  allInvoices.filter((i) => i.status === "REJECTED").length,
    DRAFT:     allInvoices.filter((i) => i.status === "DRAFT").length,
    QUEUED:    allInvoices.filter((i) => i.status === "QUEUED").length,
    STORNED:   allInvoices.filter((i) => i.status === "STORNED").length,
  };

  const toggleOne = (id: string) => {
    const next = new Set(selected);
    next.has(id) ? next.delete(id) : next.add(id);
    setSelected(next);
  };

  // Virtual scrolling — renders only visible rows (32px/row estimate)
  const tableBodyRef = useRef<HTMLDivElement>(null);
  const rowVirtualizer = useVirtualizer({
    count: list.length,
    getScrollElement: () => tableBodyRef.current,
    estimateSize: () => 32,
    overscan: 10,
  });

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">e-Factura</span>
          {t('invoices.title')}
        </span>
        <span className="muted" style={{ fontSize: 11 }}>
          {list.length} din {totalCount.toLocaleString("ro-RO")} facturi
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button
            type="button"
            className="btn"
            title="Export SAGA CSV"
            onClick={async () => {
              if (!activeCompanyId) return;
              const { save } = await import("@tauri-apps/plugin-dialog");
              const path = await save({ filters: [{ name: "CSV", extensions: ["csv"] }], defaultPath: "facturi-saga.csv" });
              if (path) {
                const today = new Date().toISOString().slice(0, 10);
                const yearStart = `${new Date().getFullYear()}-01-01`;
                try {
                      await api.integrations.exportSagaCsv(activeCompanyId, yearStart, today, path);
                      notify.success(`Export SAGA salvat: ${path}`);
                } catch (e) {
                  const err = e as unknown as { message?: string };
                  notify.error(`Eroare export SAGA: ${err.message ?? e}`);
                }
              }
            }}
          >
            <Icon name="download" size={12} /> SAGA CSV
          </button>
          <button
            type="button"
            className="btn"
            onClick={async () => {
              if (!activeCompanyId) { notify.warn("Selectați o companie."); return; }
              const { save } = await import("@tauri-apps/plugin-dialog");
              const path = await save({ filters: [{ name: "Excel", extensions: ["xlsx"] }], defaultPath: "facturi.xlsx" });
              if (path) {
                try {
                  await api.integrations.exportInvoicesXlsx({ companyId: activeCompanyId ?? undefined }, path);
                  notify.success(`Export salvat: ${path}`);
                } catch (e) {
                  const err = e as unknown as { message?: string };
                  notify.error(`Eroare export XLSX: ${err.message ?? e}`);
                }
              }
            }}
          >
            <Icon name="table" size={12} /> {t('invoices.exportXlsx')}
          </button>
          <button
            type="button"
            className="btn"
            onClick={async () => {
              if (!activeCompanyId) { notify.warn("Selectați o companie."); return; }
              const { open } = await import("@tauri-apps/plugin-dialog");
              const filePath = await open({ filters: [{ name: "XML e-Factura", extensions: ["xml"] }] });
              if (!filePath || typeof filePath !== "string") return;
              try {
                // Citim fișierul în Rust (ocolim scope-ul FS plugin care permite doar $APPDATA):
                const result = await api.importData.invoiceXmlFromFile(filePath, activeCompanyId);
                if (result.imported > 0) {
                  notify.success(
                    `Factură importată: ${result.invoiceNumber ?? "?"} — ${result.supplierName ?? "?"} · ${formatOptionalRon(result.totalAmount)}`,
                  );
                  void queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
                } else {
                  notify.error(`Import eșuat: ${result.errors.join("; ")}`);
                }
              } catch (e) {
                const err = e as unknown as { message?: string };
                notify.error(`Eroare import XML: ${err.message ?? e}`);
              }
            }}
          >
            <Icon name="upload" size={12} /> Import XML
          </button>
          <button type="button" className="btn" onClick={() => setShowImportModal(true)}>
            <Icon name="upload" size={12} /> {t('invoices.importCsv')}
          </button>
          <button
            type="button"
            className="btn primary"
            onClick={() => navigate({ to: "/invoices/new" })}
          >
            <Icon name="plus" size={12} /> {t('invoices.newInvoice')}
            <span
              className="kbd"
              style={{
                marginLeft: 6,
                background: "rgba(255,255,255,0.18)",
                border: "1px solid rgba(255,255,255,0.3)",
                color: "#fff",
              }}
            >
              {fmtShortcut("Ctrl N")}
            </span>
          </button>
        </span>
      </div>

      {/* Bulk action bar */}
      {selected.size > 0 && (
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 16px", background: "var(--accent-dim, rgba(var(--accent-rgb),0.08))", borderBottom: "1px solid var(--border)" }}>
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>{selected.size} selectate</span>
          <button className="btn compact" onClick={async () => {
            if (!activeCompanyId) return;
            const ids = Array.from(selected);
            let ok = 0; const errs: string[] = [];
            for (const id of ids) {
              try {
                await api.anaf.submitInvoice(activeCompanyId, id);
                ok++;
              } catch (e) {
                const err = e as unknown as { message?: string };
                errs.push(err.message ?? String(e));
              }
            }
            void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
            setSelected(new Set());
            if (errs.length) notify.error(`${ok} trimise, ${errs.length} erori: ${errs.slice(0, 3).join("; ")}`);
            else notify.success(`${ok} facturi trimise la ANAF`);
          }}>Trimite selectate la ANAF</button>
          <button className="btn compact" onClick={() => setSelected(new Set())}>Deselectează</button>
        </div>
      )}

      {/* Saved views */}
      <div className="views-bar">
        <span
          className={"view-tab " + (filter === "all" ? "active" : "")}
          onClick={() => setFilter("all")}
        >
          Toate <span className="count">{totalCount.toLocaleString("ro-RO")}</span>
        </span>
        <span
          className={"view-tab " + (filter === "VALIDATED" ? "active" : "")}
          onClick={() => setFilter("VALIDATED")}
        >
          Validate <span className="count">{counts.VALIDATED}</span>
        </span>
        <span
          className={"view-tab " + (filter === "SUBMITTED" ? "active" : "")}
          onClick={() => setFilter("SUBMITTED")}
        >
          Trimise <span className="count">{counts.SUBMITTED}</span>
        </span>
        <span
          className={"view-tab " + (filter === "REJECTED" ? "active" : "")}
          onClick={() => setFilter("REJECTED")}
        >
          Respinse <span className="count">{counts.REJECTED}</span>
        </span>
        <span
          className={"view-tab " + (filter === "DRAFT" ? "active" : "")}
          onClick={() => setFilter("DRAFT")}
        >
          Schițe <span className="count">{counts.DRAFT}</span>
        </span>
        <span
          className={"view-tab " + (filter === "STORNED" ? "active" : "")}
          onClick={() => setFilter("STORNED")}
        >
          Stornate <span className="count">{counts.STORNED}</span>
        </span>
        <span className="view-tab" style={{ color: "var(--accent)", borderRight: 0, opacity: 0.4, cursor: "not-allowed", pointerEvents: "none" }}>
          <Icon name="plus" size={11} /> Salvează vizualizarea
        </span>
      </div>

      {/* Toolbar */}
      <div className="content-toolbar">
        <div className="search">
          <Icon name="search" size={13} />
          <input
            placeholder="Caută după nr., CUI cumpărător sau denumire…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <span className="kbd-hint">{fmtShortcut("Ctrl F")}</span>
        </div>
        <span className="divider-v" style={{ margin: "0 4px" }} />
        {/* Errors-only toggle */}
        <label
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 5,
            fontSize: 11,
            color: errorsOnly ? "#DC2626" : "var(--text-muted)",
            cursor: "pointer",
            userSelect: "none",
            whiteSpace: "nowrap",
          }}
        >
          <input
            type="checkbox"
            className="cbx"
            checked={errorsOnly}
            onChange={(e) => {
              setErrorsOnly(e.target.checked);
              if (e.target.checked) setFilter("all");
            }}
          />
          Cu erori
        </label>
        <span className="divider-v" style={{ margin: "0 4px" }} />
        {/* Amount range */}
        <span style={{ fontSize: 11, color: "var(--text-muted)", whiteSpace: "nowrap" }}>Total:</span>
        <input
          type="number"
          placeholder="Min"
          value={amountMin}
          onChange={(e) => setAmountMin(e.target.value)}
          style={{
            width: 64,
            height: 22,
            fontSize: 11,
            padding: "0 5px",
            border: "1px solid var(--border)",
            background: "var(--bg-content)",
            color: "var(--text)",
            fontFamily: "var(--font-mono)",
          }}
        />
        <span style={{ fontSize: 10, color: "var(--text-dim)" }}>–</span>
        <input
          type="number"
          placeholder="Max"
          value={amountMax}
          onChange={(e) => setAmountMax(e.target.value)}
          style={{
            width: 64,
            height: 22,
            fontSize: 11,
            padding: "0 5px",
            border: "1px solid var(--border)",
            background: "var(--bg-content)",
            color: "var(--text)",
            fontFamily: "var(--font-mono)",
          }}
        />
        <span className="divider-v" style={{ margin: "0 4px" }} />
        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>Status:</span>
        <div className="seg">
          <span
            className={"seg-item " + (filter === "all" ? "active" : "")}
            onClick={() => setFilter("all")}
          >
            Toate
          </span>
          <span
            className={"seg-item " + (filter === "VALIDATED" ? "active" : "")}
            onClick={() => setFilter("VALIDATED")}
          >
            Validate
          </span>
          <span
            className={"seg-item " + (filter === "SUBMITTED" ? "active" : "")}
            onClick={() => setFilter("SUBMITTED")}
          >
            Trimise
          </span>
          <span
            className={"seg-item " + (filter === "REJECTED" ? "active" : "")}
            onClick={() => setFilter("REJECTED")}
          >
            Respinse
          </span>
          <span
            className={"seg-item " + (filter === "DRAFT" ? "active" : "")}
            onClick={() => setFilter("DRAFT")}
          >
            Schițe
          </span>
          <span
            className={"seg-item " + (filter === "STORNED" ? "active" : "")}
            onClick={() => setFilter("STORNED")}
          >
            Stornate
          </span>
        </div>
        <span
          style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}
        >
          {selected.size > 0 && (
            <>
              <span style={{ fontSize: 11, fontWeight: 600 }}>
                {selected.size} selectate
              </span>
              <button type="button" className="btn compact primary" onClick={async () => {
                if (!activeCompanyId) return;
                const ids = Array.from(selected);
                let ok = 0; const errs: string[] = [];
                for (const id of ids) {
                  try { await api.anaf.submitInvoice(activeCompanyId, id); ok++; }
                  catch (e) { errs.push((e as { message?: string }).message ?? String(e)); }
                }
                void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
                setSelected(new Set());
                if (errs.length) notify.error(`${ok} trimise, ${errs.length} erori: ${errs.slice(0, 3).join("; ")}`);
                else notify.success(`${ok} facturi trimise la ANAF`);
              }}>
                <Icon name="cloudUp" size={11} /> Trimite la ANAF
              </button>
              <button type="button" className="btn compact" onClick={() => window.print()}>
                <Icon name="printer" size={11} /> Tipărește
              </button>
              <span className="divider-v" style={{ margin: "0 4px" }} />
            </>
          )}
          <button type="button" className="btn-icon" title="Coloane" disabled>
            <Icon name="filter" size={14} />
          </button>
          <button type="button" className="btn-icon" title="Mai multe" disabled>
            <Icon name="more" size={14} />
          </button>
        </span>
      </div>

      <div
        className="content-body"
        ref={tableBodyRef}
        style={{ overflowY: "auto", flex: 1 }}
      >
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 12, color: "var(--text-muted)" }}>
            Se încarcă…
          </div>
        ) : pagedError ? (
          <QueryErrorBanner error={pagedErr} label="facturile" onRetry={() => void refetchPaged()} />
        ) : list.length === 0 ? (
          <div style={{ padding: 40, textAlign: "center", fontSize: 12, color: "var(--text-muted)" }}>
            {allInvoices.length === 0
              ? "Nicio factură emisă. Creați prima factură cu butonul \"Factură nouă\"."
              : "Nicio înregistrare pentru filtrele aplicate."}
          </div>
        ) : (
          <table className="dt" style={{ width: "100%" }}>
            <thead>
              <tr>
                <th className="ck">
                  <input
                    type="checkbox"
                    className="cbx"
                    checked={selected.size === list.length && list.length > 0}
                    onChange={() =>
                      setSelected(
                        selected.size === list.length
                          ? new Set()
                          : new Set(list.map((i) => i.id)),
                      )
                    }
                  />
                </th>
                <th style={{ width: 134 }} className="sortable sorted">
                  {t('invoices.columns.number')} <span className="sort">▾</span>
                </th>
                <th style={{ width: 92 }}>{t('invoices.columns.date')}</th>
                <th>{t('invoices.columns.customer')}</th>
                <th style={{ width: 100 }}>CUI</th>
                <th className="num" style={{ width: 110 }}>
                  Net (RON)
                </th>
                <th className="num" style={{ width: 90 }}>
                  TVA
                </th>
                <th className="num" style={{ width: 120 }}>
                  {t('invoices.columns.total')}
                </th>
                <th style={{ width: 100 }}>Scadență</th>
                <th style={{ width: 124 }}>{t('invoices.columns.status')}</th>
                <th style={{ width: 110 }}>Index ANAF</th>
                <th style={{ width: 24 }}></th>
              </tr>
            </thead>
            <tbody
              style={{
                height: `${rowVirtualizer.getTotalSize()}px`,
                position: "relative",
                display: "block",
              }}
            >
              {rowVirtualizer.getVirtualItems().map((virtualRow) => {
                const inv = list[virtualRow.index];
                const client = contactMap.get(inv.contactId);
                return (
                  <tr
                    key={inv.id}
                    data-index={virtualRow.index}
                    ref={rowVirtualizer.measureElement}
                    onClick={() => {
                      setSelectedInvoiceId(inv.id);
                      navigate({ to: "/invoices/$id", params: { id: inv.id } });
                    }}
                    className={selected.has(inv.id) ? "selected" : ""}
                    style={{
                      cursor: "pointer",
                      position: "absolute",
                      top: 0,
                      left: 0,
                      width: "100%",
                      transform: `translateY(${virtualRow.start}px)`,
                    }}
                  >
                    <td className="ck" onClick={(e) => e.stopPropagation()}>
                      <input
                        type="checkbox"
                        className="cbx"
                        checked={selected.has(inv.id)}
                        onChange={() => toggleOne(inv.id)}
                      />
                    </td>
                    <td className="mono">
                      <b>{inv.fullNumber}</b>
                    </td>
                    <td className="muted">{inv.issueDate}</td>
                    <td>{client?.legalName ?? <span className="dim">—</span>}</td>
                    <td className="mono muted">{client?.cui ?? "—"}</td>
                    <td className="num tnum muted">{fmtRON(inv.subtotalAmount)}</td>
                    <td className="num tnum dim">{fmtRON(inv.vatAmount)}</td>
                    <td className="num tnum">
                      <b>{fmtRON(inv.totalAmount)}</b>
                    </td>
                    <td className="muted">{inv.dueDate}</td>
                    <td>
                      <StatusBadge status={inv.status} />
                    </td>
                    <td className="mono dim">{inv.anafIndex || "—"}</td>
                    <td>
                      <Icon
                        name="chevronRight"
                        size={12}
                        style={{ color: "var(--text-dim)" }}
                      />
                    </td>
                  </tr>
                );
              })}
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
          Validate: <b style={{ color: "#16A34A" }}>{counts.VALIDATED}</b>
        </span>
        <span>
          Trimise: <b style={{ color: "#1E40AF" }}>{counts.SUBMITTED}</b>
        </span>
        <span>
          Respinse: <b style={{ color: "#DC2626" }}>{counts.REJECTED}</b>
        </span>
        <span>Schițe: <b>{counts.DRAFT}</b></span>
        <span style={{ marginLeft: "auto" }}>
          <span className="kbd">↑↓</span> selectează ·{" "}
          <span className="kbd">Enter</span> deschide ·{" "}
          <span className="kbd">Space</span> bifează
        </span>
      </div>

      {showImportModal && (
        <CsvImportModal
          type="invoices"
          companyId={activeCompanyId ?? ""}
          onClose={() => setShowImportModal(false)}
          onSuccess={() => {
            void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
            setShowImportModal(false);
          }}
        />
      )}
    </div>
  );
}
