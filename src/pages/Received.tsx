/**
 * Facturi primite — verbatim port of the design "Facturi primite.html":
 *   .page-head (title + SPV-sync sub + pill-btn "Recalculează TVA din XML" ·
 *   pill-btn "Export CSV" · btn-dark "Sincronizează SPV") → .banner.warn
 *   (defalcare TVA incompletă) → .scr-card → .scr-toolbar (.tabs status
 *   counts · .scr-search · refresh sq-btn) → .bulkbar → .scr-table
 *   (cbx · furnizor .cli · CUI/serie .doc · date · net/TVA cu .missing ·
 *   total · monedă · status chips · .row-acts) → .tot-foot totals →
 *   modal "Defalcare TVA" (.modal-back/.modal cu .defal-grid).
 *
 * ALL wiring preserved: api.received.list({companyId, limit 10000}),
 * status tabs + search, multi-select + bulk approve/archive/reject,
 * api.received.updateStatus per-row, api.anaf.syncSpv (Sincronizează SPV),
 * api.received.reparseVat (Recalculează TVA din XML),
 * api.received.exportCsv (Export CSV selecție),
 * api.importData.invoiceXmlFromFile (Import XML), SpvInbox,
 * row click → /received/$id.
 */

import { useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { SpvInbox } from "@/components/shared/SpvInbox";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { ReceivedInvoice, ReceivedStatus } from "@/types";

type StatusFilter = ReceivedStatus | "all";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

/** Render at most this many rows (plain table, no virtualizer — design parity). */
const MAX_ROWS = 1000;

/** Compune numărul de document afișat în tabel. */
function invoiceNo(series: string | null, number: string | null, fallback: string): string {
  if (series && number) return `${series}-${number}`;
  if (number) return number;
  return fallback;
}

/** Inițialele furnizorului pentru .cli-ava (ascuns de CSS pe list-screens, dar păstrat în DOM ca în prototip). */
function initials(name: string): string {
  const parts = name.trim().split(/\s+/);
  return ((parts[0]?.[0] ?? "") + (parts[1]?.[0] ?? parts[0]?.[1] ?? "")).toUpperCase();
}

/** Defalcare TVA incompletă din XML — nu contribuie la TVA deductibilă. */
const isMissingVat = (inv: ReceivedInvoice) => inv.netAmount == null || inv.vatAmount == null;

// Inline SVG paths from the prototype for icons not in Ic.tsx.
const P_CLIPBOARD =
  "M15.75 15.75V18m-7.5-6.75h.008v.008H8.25v-.008Zm0 2.25h.008v.008H8.25V13.5ZM8.25 6h7.5v2.25h-7.5V6ZM12 2.25c-1.892 0-3.758.11-5.593.322C5.307 2.7 4.5 3.65 4.5 4.757V19.5a2.25 2.25 0 0 0 2.25 2.25h10.5a2.25 2.25 0 0 0 2.25-2.25V4.757c0-1.108-.806-2.057-1.907-2.185A48.507 48.507 0 0 0 12 2.25Z";
const P_TRASH =
  "m20.25 7.5-.625 10.632a2.25 2.25 0 0 1-2.247 2.118H6.622a2.25 2.25 0 0 1-2.247-2.118L3.75 7.5M10 11.25h4M3.375 7.5h17.25c.621 0 1.125-.504 1.125-1.125v-1.5c0-.621-.504-1.125-1.125-1.125H3.375c-.621 0-1.125.504-1.125 1.125v1.5c0 .621.504 1.125 1.125 1.125Z";
const P_CHECK_CIRCLE = "M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z";
const P_WARN_TRI =
  "M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z";

function InlineIc({ d, cls = "ic" }: { d: string; cls?: string }) {
  return <svg className={cls} viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: `<path d="${d}"/>` }} />;
}

// Status → design chip (.chip variants + icon + label i18n key), as in the prototype.
const STATUS_CHIP: Record<ReceivedStatus, { cls: string; icon: React.ReactNode; labelKey: string }> = {
  NEW:      { cls: "sent", icon: <Ic name="dot" cls="sic" />,             labelKey: "received.status.new" },
  REVIEWED: { cls: "wait", icon: <Ic name="clock" cls="sic" />,           labelKey: "received.status.reviewed" },
  APPROVED: { cls: "paid", icon: <InlineIc d={P_CHECK_CIRCLE} cls="sic" />, labelKey: "received.status.approved" },
  REJECTED: { cls: "late", icon: <Ic name="xMark" cls="sic" />,           labelKey: "received.status.rejected" },
  ARCHIVED: { cls: "sent", icon: <InlineIc d={P_TRASH} cls="sic" />,      labelKey: "received.status.archived" },
};

// ── DefalModal — design .modal-back/.modal "Defalcare TVA" ───────────────────
// propunere — neimplementat: salvarea defalcării manuale pe conturi de
// cheltuială nu are echivalent backend (există doar reparse-ul în masă din
// XML — butonul „Recalculează TVA din XML"). Modalul replică UX-ul din
// prototip; „Salvează defalcarea" → notify.info("În curând.").

const DEFAL_ACCOUNT_KEYS = ["a626", "a628", "a604", "a605", "a371"] as const;

function DefalModal({ inv, onClose }: { inv: ReceivedInvoice; onClose: () => void }) {
  const { t } = useTranslation();
  const [rows, setRows] = useState([0]);
  const docNo = invoiceNo(inv.series, inv.number, inv.anafDownloadId);
  const accounts = useMemo(() => DEFAL_ACCOUNT_KEYS.map((k) => t(`received.defal.accounts.${k}`)), [t]);
  const { closing, close } = useAnimatedClose(onClose);

  return createPortal(
    <div className={`modal-back ${closing ? "closing" : "show"}`} style={{ position: "fixed" }} onMouseDown={close}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <div>
            <div className="mt">{t("received.defal.title")}</div>
            <div className="ms num">
              {docNo} · {inv.issuerName} · {fmtRON(inv.totalAmount)} {inv.currency}
            </div>
          </div>
          <button className="modal-x" onClick={close}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="defal-head">
            <span>{t("received.defal.expenseAccount")}</span>
            <span>{t("received.defal.vatRate")}</span>
            <span style={{ textAlign: "right" }}>{t("received.defal.base")}</span>
          </div>
          {rows.map((key) => (
            <div className="defal-grid" key={key}>
              <select className="select" defaultValue={accounts[0]}>
                {accounts.map((a) => (
                  <option key={a}>{a}</option>
                ))}
              </select>
              <select className="select" defaultValue="21%">
                <option>21%</option>
                <option>11%</option>
                <option>0%</option>
              </select>
              <input className="input num" type="text" placeholder={t("received.defal.amountPlaceholder")} style={{ textAlign: "right" }} />
            </div>
          ))}
          <div
            className="add-line"
            style={{ padding: "10px 0 2px", display: "flex", alignItems: "center", gap: 7, fontSize: 12.5, fontWeight: 500, color: "var(--text-2)", cursor: "pointer" }}
            onClick={() => setRows((r) => [...r, (r[r.length - 1] ?? 0) + 1])}
          >
            <svg className="ic" viewBox="0 0 24 24" style={{ width: 14, height: 14 }} dangerouslySetInnerHTML={{ __html: '<path d="M12 4.5v15m7.5-7.5h-15"/>' }} />
            {t("received.defal.addLine")}
          </div>
          <div className="banner ok" style={{ margin: "14px 0 0" }}>
            <InlineIc d={P_CHECK_CIRCLE} />
            <span>
              {t("received.defal.notePre")} <b>{t("received.defal.noteBold")}</b> {t("received.defal.notePost")}
            </span>
          </div>
        </div>
        <div className="modal-foot">
          <button className="pill-btn" onClick={close}>{t("received.defal.cancel")}</button>
          <button
            className="btn-dark"
            onClick={() => {
              notify.info(t("received.defal.soon")); // propunere — neimplementat
              close();
            }}
          >
            <Ic name="check" />{t("received.defal.save")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ── ReceivedPage ──────────────────────────────────────────────────────────────

export function ReceivedPage() {
  const { t, i18n } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const missingTitle = t("received.missingVatTitle");

  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<StatusFilter>("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [defalFor, setDefalFor] = useState<ReceivedInvoice | null>(null);

  // Fetch received invoices — guarded: do not fetch when no company is active.
  // Pass an explicit large limit so realistic single-company data loads fully.
  const { data: paged, isLoading, isError, error, refetch } = useQuery({
    queryKey: queryKeys.received.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    queryFn: () => api.received.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    enabled: !!activeCompanyId,
  });

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });
  const activeCompany = companies.find((c) => c.id === activeCompanyId);

  // Update status mutation
  const { mutate: updateStatus } = useMutation({
    mutationFn: ({ id, status }: { id: string; status: ReceivedStatus }) => {
      if (!activeCompanyId) throw new Error(t("received.notify.noActiveCompany"));
      return api.received.updateStatus(id, activeCompanyId, status);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: queryKeys.received.list({ companyId: activeCompanyId ?? undefined }),
      });
    },
    onError: (e) => notify.error(formatError(e, t("received.notify.statusError"))),
  });

  // ANAF test mode
  const { data: testModeSetting } = useQuery({
    queryKey: queryKeys.anaf.testMode,
    queryFn: () => api.settings.get("use_anaf_test_env"),
  });
  const testMode = testModeSetting === "1";

  // Sync SPV mutation
  const { mutate: syncSpv, isPending: isSyncing } = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) throw new Error(t("received.notify.noActiveCompany"));
      return api.anaf.syncSpv(activeCompanyId, testMode);
    },
    onSuccess: (count) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
      if (count > 0) notify.success(t("received.notify.spvNew", { count }));
      else notify.info(t("received.notify.spvNone"));
    },
    onError: (e) => notify.error(formatError(e, t("received.notify.spvError"))),
  });

  // Reparse VAT mutation
  const { mutate: reparseVat, isPending: isReparsing } = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) throw new Error(t("received.notify.noActiveCompany"));
      return api.received.reparseVat(activeCompanyId);
    },
    onSuccess: (count) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      notify.success(t("received.notify.reparsed", { count }));
    },
    onError: (e) => notify.error(formatError(e, t("received.notify.reparseError"))),
  });

  const allInvoices = paged?.items ?? [];
  const totalCount = paged?.total ?? 0;

  // Client-side filter (status + text search)
  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return allInvoices
      .filter((i) => filter === "all" || i.status === filter)
      .filter(
        (i) =>
          !q ||
          invoiceNo(i.series, i.number, i.anafDownloadId).toLowerCase().includes(q) ||
          i.issuerName.toLowerCase().includes(q) ||
          i.issuerCui.toLowerCase().includes(q),
      );
  }, [allInvoices, filter, query]);

  // Footer totals — RON only to avoid mixing currencies.
  const ronList = list.filter((i) => i.currency === "RON");
  const nonRonCount = list.length - ronList.length;
  const totNet   = ronList.reduce((s, i) => s + (i.netAmount != null ? parseDec(i.netAmount) : 0), 0);
  const totVat   = ronList.reduce((s, i) => s + (i.vatAmount != null ? parseDec(i.vatAmount) : 0), 0);
  const totTotal = ronList.reduce((s, i) => s + parseDec(i.totalAmount), 0);

  // Status counts (from loaded page)
  const counts = {
    all:      totalCount,
    NEW:      allInvoices.filter((i) => i.status === "NEW").length,
    REVIEWED: allInvoices.filter((i) => i.status === "REVIEWED").length,
    APPROVED: allInvoices.filter((i) => i.status === "APPROVED").length,
    REJECTED: allInvoices.filter((i) => i.status === "REJECTED").length,
    ARCHIVED: allInvoices.filter((i) => i.status === "ARCHIVED").length,
  };

  // Facturi cu defalcare TVA incompletă din XML (banner warn din prototip).
  const missingVatCount = allInvoices.filter(isMissingVat).length;

  const tabs: Array<{ value: StatusFilter; label: string; count: number }> = [
    { value: "all",      label: t("received.tabs.all"),      count: counts.all },
    { value: "NEW",      label: t("received.tabs.new"),      count: counts.NEW },
    { value: "REVIEWED", label: t("received.tabs.reviewed"), count: counts.REVIEWED },
    { value: "APPROVED", label: t("received.tabs.approved"), count: counts.APPROVED },
    { value: "REJECTED", label: t("received.tabs.rejected"), count: counts.REJECTED },
    { value: "ARCHIVED", label: t("received.tabs.archived"), count: counts.ARCHIVED },
  ];

  const toggleOne = (id: string) => {
    const next = new Set(selected);
    next.has(id) ? next.delete(id) : next.add(id);
    setSelected(next);
  };

  const bulkStatus = (status: ReceivedStatus) => {
    [...selected].forEach((id) => updateStatus({ id, status }));
    setSelected(new Set());
  };

  async function handleImportXml() {
    if (!activeCompanyId) { notify.warn(t("received.notify.selectCompanyFirst")); return; }
    const { open } = await import("@tauri-apps/plugin-dialog");
    const filePath = await open({ filters: [{ name: "XML e-Factura", extensions: ["xml"] }] });
    if (!filePath || typeof filePath !== "string") return;
    try {
      const result = await api.importData.invoiceXmlFromFile(filePath, activeCompanyId);
      if (result.imported > 0) {
        notify.success(t("received.notify.imported", { number: result.invoiceNumber ?? "?", supplier: result.supplierName ?? "?" }));
        void queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      } else {
        notify.error(t("received.notify.importFailed", { errors: result.errors.join("; ") }));
      }
    } catch (e) {
      notify.error(formatError(e, t("received.notify.importError")));
    }
  }

  async function handleExportCsv() {
    if (selected.size === 0) { notify.warn(t("received.notify.selectForExport")); return; }
    if (!activeCompanyId) { notify.warn(t("received.notify.selectCompanyFirst")); return; }
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({ filters: [{ name: "CSV", extensions: ["csv"] }], defaultPath: "facturi-primite-selectie.csv" });
    if (!path) return;
    try {
      const csvText = await api.received.exportCsv(activeCompanyId, Array.from(selected));
      const { writeTextFile } = await import("@tauri-apps/plugin-fs");
      await writeTextFile(path, csvText);
      notify.success(t("received.notify.exported", { count: selected.size, path }));
    } catch (e) {
      notify.error(formatError(e, t("received.notify.exportError")));
    }
  }

  const visibleRows = list.slice(0, MAX_ROWS);

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("received.title")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("received.selectCompany")}
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("received.title")}</h1>
          <p className="sub">
            {list.length !== totalCount
              ? t("received.subCountFiltered", { shown: list.length, total: totalCount.toLocaleString(i18n.language) })
              : t("received.subCount", { n: totalCount.toLocaleString(i18n.language) })}
            {activeCompany ? ` · ${activeCompany.legalName}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button
            className="pill-btn"
            disabled={isReparsing}
            style={isReparsing ? { opacity: 0.6 } : undefined}
            onClick={() => reparseVat()}
          >
            <InlineIc d={P_CLIPBOARD} />
            {isReparsing ? t("received.actions.reparsing") : t("received.actions.reparseVat")}
          </button>
          {/* Real feature kept (prototype lacks it): Import XML manual */}
          <button className="pill-btn" onClick={() => void handleImportXml()}>
            <Ic name="docUp" />{t("received.actions.importXml")}
          </button>
          <button className="pill-btn" onClick={() => void handleExportCsv()}>
            <Ic name="dl" />{t("received.actions.exportCsv")}
          </button>
          <button
            className="btn-dark spin-btn"
            disabled={isSyncing}
            style={isSyncing ? { opacity: 0.7 } : undefined}
            onClick={() => syncSpv()}
          >
            <Ic name="sync" />
            {isSyncing ? t("received.actions.syncing") : t("received.actions.syncSpv")}
          </button>
        </div>
      </div>

      {/* banner: defalcare TVA incompletă */}
      {missingVatCount > 0 && (
        <div className="banner warn">
          <InlineIc d={P_WARN_TRI} />
          <span>
            <b>{t("received.banner.missing", { count: missingVatCount })}</b>{" "}
            {t("received.banner.missingDetail")}
          </span>
        </div>
      )}

      <div className="scr-card">
        {/* toolbar */}
        <div className="scr-toolbar">
          <div className="tabs">
            {tabs.map((t) => (
              <div
                key={t.value}
                className={`tab${filter === t.value ? " active" : ""}`}
                onClick={() => setFilter(t.value)}
              >
                {t.label}<span className="cnt">{t.count}</span>
              </div>
            ))}
          </div>
          <div className="spacer" />
          <div className="scr-search">
            <Ic name="lens" />
            <input
              type="text"
              placeholder={t("received.toolbar.searchPlaceholder")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <button className="sq-btn spin-btn" title={t("received.toolbar.refresh")} onClick={() => void refetch()}>
            <Ic name="sync" />
          </button>
        </div>

        {/* bulk bar — real multi-select actions kept (prototype lacks them) */}
        <div className={`bulkbar${selected.size > 0 ? " show" : ""}`}>
          <b>{t("received.bulk.selected", { n: selected.size })}</b>
          <span className="spacer" />
          <button className="pill-btn" onClick={() => bulkStatus("APPROVED")}>
            <Ic name="check" />{t("received.bulk.approveAll")}
          </button>
          <button className="pill-btn" onClick={() => bulkStatus("ARCHIVED")}>
            <InlineIc d={P_TRASH} />{t("received.bulk.archive")}
          </button>
          <button className="pill-btn" onClick={() => bulkStatus("REJECTED")}>
            <Ic name="xMark" />{t("received.bulk.reject")}
          </button>
          <button className="pill-btn" onClick={() => void handleExportCsv()}>
            <Ic name="dl" />{t("received.actions.exportCsv")}
          </button>
          <button className="pill-btn" onClick={() => setSelected(new Set())}>{t("received.bulk.deselect")}</button>
        </div>

        {/* truncation note */}
        {paged && paged.total > paged.items.length && (
          <div style={{ padding: "6px 16px", borderBottom: "1px solid var(--line)", fontSize: 12, color: "var(--amber)" }}>
            {t("received.states.truncated", { shown: paged.items.length.toLocaleString(i18n.language), total: paged.total.toLocaleString(i18n.language) })}
          </div>
        )}

        {/* table */}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("received.states.loading")}</div>
        ) : isError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={error} label={t("received.states.errorLabel")} onRetry={() => void refetch()} />
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {allInvoices.length === 0
              ? t("received.states.emptyAll")
              : t("received.states.emptyFiltered")}
          </div>
        ) : (
          <>
            <table className="scr-table">
              <thead>
                <tr>
                  <th style={{ width: 36 }}>
                    <button
                      className={`cbx${selected.size === list.length && list.length > 0 ? " on" : ""}`}
                      aria-label={t("received.table.selectAll")}
                      onClick={() =>
                        setSelected(selected.size === list.length ? new Set() : new Set(list.map((i) => i.id)))
                      }
                    />
                  </th>
                  <th>{t("received.table.supplier")}</th>
                  <th>{t("received.table.cui")}</th>
                  <th>{t("received.table.seriesNumber")}</th>
                  <th>{t("received.table.date")}</th>
                  <th className="r">{t("received.table.net")}</th>
                  <th className="r">{t("received.table.vat")}</th>
                  <th className="r">{t("received.table.total")}</th>
                  <th>{t("received.table.currency")}</th>
                  <th>{t("received.table.status")}</th>
                  <th className="r" style={{ width: 104 }}></th>
                </tr>
              </thead>
              <tbody>
                {visibleRows.map((inv) => {
                  const docNo = invoiceNo(inv.series, inv.number, inv.anafDownloadId);
                  const chip = STATUS_CHIP[inv.status] ?? STATUS_CHIP.NEW;
                  const missing = isMissingVat(inv);
                  return (
                    <tr
                      key={inv.id}
                      className={`clickable${selected.has(inv.id) ? " selected" : ""}`}
                      onClick={() => void navigate({ to: "/received/$id", params: { id: inv.id } })}
                    >
                      <td onClick={(e) => e.stopPropagation()}>
                        <button
                          className={`cbx row-cbx${selected.has(inv.id) ? " on" : ""}`}
                          onClick={() => toggleOne(inv.id)}
                        />
                      </td>
                      <td>
                        <div className="cli">
                          <span className="cli-ava">{initials(inv.issuerName)}</span>
                          {inv.issuerName}
                        </div>
                      </td>
                      <td><span className="doc">{inv.issuerCui}</span></td>
                      <td><span className="doc">{docNo}</span></td>
                      <td className="num">{fmtRoDate(inv.issueDate)}</td>
                      <td className="r num">
                        {inv.netAmount != null
                          ? fmtRON(inv.netAmount)
                          : <span className="missing" title={missingTitle}>—</span>}
                      </td>
                      <td className="r num">
                        {inv.vatAmount != null
                          ? fmtRON(inv.vatAmount)
                          : <span className="missing" title={missingTitle}>—</span>}
                      </td>
                      <td className="r num"><b>{fmtRON(inv.totalAmount)}</b></td>
                      <td>{inv.currency}</td>
                      <td>
                        <span className={`chip ${chip.cls}`}>{chip.icon}{t(chip.labelKey)}</span>
                      </td>
                      <td onClick={(e) => e.stopPropagation()}>
                        <div className="row-acts">
                          {(inv.status === "NEW" || inv.status === "REVIEWED") && (
                            <>
                              <button
                                className="mini-btn"
                                title={t("received.row.approve")}
                                onClick={() => updateStatus({ id: inv.id, status: "APPROVED" })}
                              >
                                <Ic name="check" />
                              </button>
                              <button
                                className="mini-btn"
                                title={t("received.row.reject")}
                                onClick={() => updateStatus({ id: inv.id, status: "REJECTED" })}
                              >
                                <Ic name="xMark" />
                              </button>
                              {missing && (
                                <button
                                  className="mini-btn"
                                  title={t("received.row.reanalyzeVat")}
                                  onClick={() => setDefalFor(inv)}
                                >
                                  <Ic name="sync" />
                                </button>
                              )}
                            </>
                          )}
                          {inv.status === "APPROVED" && (
                            <button
                              className="mini-btn"
                              title={t("received.row.archive")}
                              onClick={() => updateStatus({ id: inv.id, status: "ARCHIVED" })}
                            >
                              <InlineIc d={P_TRASH} />
                            </button>
                          )}
                          {inv.status === "REJECTED" && (
                            <button
                              className="mini-btn"
                              title={t("received.row.reanalyze")}
                              onClick={() => updateStatus({ id: inv.id, status: "REVIEWED" })}
                            >
                              <Ic name="sync" />
                            </button>
                          )}
                          <button
                            className="mini-btn"
                            title={t("received.row.view")}
                            onClick={() => void navigate({ to: "/received/$id", params: { id: inv.id } })}
                          >
                            <Ic name="eye" />
                          </button>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>

            {/* totals footer */}
            <div className="tot-foot">
              <span><b>{list.length}</b> {t("received.foot.invoicesWord", { count: list.length })}</span>
              <span>{t("received.foot.net")} <b className="num">{fmtRON(totNet)}</b></span>
              <span>{t("received.foot.vat")} <b className="num">{fmtRON(totVat)}</b></span>
              <span>{t("received.foot.total")} <b className="num">{fmtRON(totTotal)}</b></span>
              <span>{t("received.foot.toApprove")} <b>{counts.NEW + counts.REVIEWED}</b></span>
              <span className="spacer" style={{ flex: 1 }} />
              {list.length > MAX_ROWS && (
                <span className="muted">{t("received.foot.shownFirst", { shown: MAX_ROWS.toLocaleString(i18n.language), total: list.length.toLocaleString(i18n.language) })}</span>
              )}
              {nonRonCount > 0 && (
                <span className="muted">{t("received.foot.nonRonExcluded", { count: nonRonCount })}</span>
              )}
              <span className="muted">
                {t("received.foot.intraEu")}
              </span>
            </div>
          </>
        )}
      </div>

      {/* Real feature kept (prototype lacks it): inbox-ul general SPV
          (recipise, notificări, somații) — distinct de sincronizarea e-Factura. */}
      <SpvInbox />

      {/* modal defalcare TVA */}
      {defalFor && <DefalModal inv={defalFor} onClose={() => setDefalFor(null)} />}
    </div>
  );
}
