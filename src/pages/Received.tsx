/**
 * Facturi primite — re-skinned to rf kit (Wave 4).
 * Preserves 100% of wiring: api.received.list({companyId}), status tabs,
 * search, multi-select, Import XML → api.importData.invoiceXmlFromFile,
 * Export selecție, Descarcă din SPV → api.anaf.syncSpv,
 * Recalculează TVA din XML → api.received.reparseVat,
 * per-row status → api.received.updateStatus, row → navigate /received/$id.
 */

import { useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";

import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import {
  PageHeader, Btn, IconBtn, Badge, Card, SearchInput, Empty,
} from "@/components/rf";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { ReceivedStatus } from "@/types";

type StatusFilter = ReceivedStatus | "all";

/** Compune numărul de document afișat în tabel. */
function invoiceNo(series: string | null, number: string | null, fallback: string): string {
  if (series && number) return `${series}-${number}`;
  if (number) return number;
  return fallback;
}

const STATUS_TABS: { value: StatusFilter; label: string }[] = [
  { value: "all",      label: "Toate" },
  { value: "NEW",      label: "Noi" },
  { value: "REVIEWED", label: "De revizuit" },
  { value: "APPROVED", label: "Aprobate" },
  { value: "REJECTED", label: "Respinse" },
  { value: "ARCHIVED", label: "Arhivate" },
];

export function ReceivedPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<StatusFilter>("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());

  // Fetch received invoices — guarded: do not fetch when no company is active.
  // Pass an explicit large limit so realistic single-company data loads fully.
  const { data: paged, isLoading, isError, error, refetch } = useQuery({
    queryKey: queryKeys.received.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    queryFn: () => api.received.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    enabled: !!activeCompanyId,
  });

  // Update status mutation
  const { mutate: updateStatus } = useMutation({
    mutationFn: ({ id, status }: { id: string; status: ReceivedStatus }) => {
      if (!activeCompanyId) throw new Error("Nicio companie activă.");
      return api.received.updateStatus(id, activeCompanyId, status);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: queryKeys.received.list({ companyId: activeCompanyId ?? undefined }),
      });
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut actualiza statusul.")),
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
      if (!activeCompanyId) throw new Error("Nicio companie activă.");
      return api.anaf.syncSpv(activeCompanyId, testMode);
    },
    onSuccess: (count) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
      if (count > 0) notify.success(`${count} facturi noi descărcate din SPV.`);
      else notify.info("Nicio factură nouă în SPV.");
    },
    onError: (e) => notify.error(formatError(e, "Eroare sincronizare SPV.")),
  });

  // Reparse VAT mutation
  const { mutate: reparseVat, isPending: isReparsing } = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) throw new Error("Nicio companie activă.");
      return api.received.reparseVat(activeCompanyId);
    },
    onSuccess: (count) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      notify.success(`TVA recalculat pentru ${count} facturi.`);
    },
    onError: (e) => notify.error(formatError(e, "Eroare recalculare TVA.")),
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
  const ronListReceived = list.filter((i) => i.currency === "RON");
  const nonRonCountReceived = list.length - ronListReceived.length;
  const totalSum    = ronListReceived.reduce((s, i) => s + parseDec(i.totalAmount), 0);
  const totalNet    = ronListReceived.reduce((s, i) => s + parseDec(i.netAmount), 0);
  const totalVat    = ronListReceived.reduce((s, i) => s + parseDec(i.vatAmount), 0);

  // Status counts (from loaded page)
  const counts = {
    all:      totalCount,
    NEW:      allInvoices.filter((i) => i.status === "NEW").length,
    REVIEWED: allInvoices.filter((i) => i.status === "REVIEWED").length,
    APPROVED: allInvoices.filter((i) => i.status === "APPROVED").length,
    REJECTED: allInvoices.filter((i) => i.status === "REJECTED").length,
    ARCHIVED: allInvoices.filter((i) => i.status === "ARCHIVED").length,
  };

  const toggleOne = (id: string) => {
    const next = new Set(selected);
    next.has(id) ? next.delete(id) : next.add(id);
    setSelected(next);
  };

  if (!activeCompanyId) {
    return (
      <div className="rf-page">
        <PageHeader title="Facturi primite" />
        <div className="rf-page-body">
          <Card pad>
            <Empty icon="fileIn" title="Selectați o companie activă pentru a vedea facturile primite." />
          </Card>
        </div>
      </div>
    );
  }

  return (
    <div className="rf-page">
      <PageHeader
        title="Facturi primite"
        sub={<Badge variant="neutral">{list.length} facturi · {fmtRON(totalSum)} RON</Badge>}
        actions={
          <>
            <Btn
              variant="secondary"
              icon="refresh"
              size="sm"
              disabled={isReparsing || !activeCompanyId}
              onClick={() => reparseVat()}
            >
              Recalculează TVA din XML
            </Btn>
            <Btn
              variant="secondary"
              icon="upload"
              size="sm"
              onClick={async () => {
                if (!activeCompanyId) { notify.warn("Selectați o companie."); return; }
                const { open } = await import("@tauri-apps/plugin-dialog");
                const filePath = await open({ filters: [{ name: "XML e-Factura", extensions: ["xml"] }] });
                if (!filePath || typeof filePath !== "string") return;
                try {
                  const result = await api.importData.invoiceXmlFromFile(filePath, activeCompanyId);
                  if (result.imported > 0) {
                    notify.success(
                      `Factură importată: ${result.invoiceNumber ?? "?"} — ${result.supplierName ?? "?"}`,
                    );
                    void queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
                  } else {
                    notify.error(`Import eșuat: ${result.errors.join("; ")}`);
                  }
                } catch (e) {
                  notify.error(formatError(e, "Eroare import XML."));
                }
              }}
            >
              Import XML
            </Btn>
            <Btn
              variant="secondary"
              icon="download"
              size="sm"
              disabled={selected.size === 0}
              onClick={async () => {
                if (selected.size === 0) { notify.warn("Selectați facturi pentru export."); return; }
                if (!activeCompanyId) { notify.warn("Selectați o companie."); return; }
                const { save } = await import("@tauri-apps/plugin-dialog");
                const path = await save({ filters: [{ name: "CSV", extensions: ["csv"] }], defaultPath: "facturi-primite-selectie.csv" });
                if (!path) return;
                try {
                  const csvText = await api.received.exportCsv(activeCompanyId, Array.from(selected));
                  const { writeTextFile } = await import("@tauri-apps/plugin-fs");
                  await writeTextFile(path, csvText);
                  notify.success(`${selected.size} facturi exportate: ${path}`);
                } catch (e) {
                  notify.error(formatError(e, "Exportul CSV a eșuat."));
                }
              }}
            >
              Export selecție
            </Btn>
            <Btn
              variant="primary"
              icon="cloudDn"
              size="sm"
              disabled={isSyncing || !activeCompanyId}
              onClick={() => syncSpv()}
            >
              {isSyncing ? "Sincronizare…" : "Descarcă din SPV"}
            </Btn>
          </>
        }
      />

      <div className="rf-page-body">
        <Card>
          {/* Tabs + Toolbar */}
          <div style={{ borderBottom: "1px solid var(--rf-border)" }}>
            <div style={{ display: "flex", gap: 0, padding: "0 16px" }}>
              {STATUS_TABS.map((t) => {
                const count = counts[t.value];
                const active = filter === t.value;
                return (
                  <button
                    key={t.value}
                    type="button"
                    onClick={() => setFilter(t.value)}
                    style={{
                      padding: "10px 14px",
                      fontSize: 13,
                      fontWeight: active ? 600 : 400,
                      color: active ? "var(--rf-accent)" : "var(--rf-text-muted)",
                      background: "none",
                      border: "none",
                      borderBottom: active ? "2px solid var(--rf-accent)" : "2px solid transparent",
                      cursor: "pointer",
                      display: "flex",
                      alignItems: "center",
                      gap: 6,
                    }}
                  >
                    {t.label}
                    <span
                      style={{
                        fontSize: 11,
                        fontWeight: 600,
                        color: t.value === "NEW" && count > 0 ? "var(--rf-accent)" :
                               t.value === "REJECTED" && count > 0 ? "var(--rf-error)" :
                               "var(--rf-text-dim)",
                        background: "var(--rf-neutral-bg)",
                        borderRadius: 10,
                        padding: "1px 6px",
                      }}
                    >
                      {count}
                    </span>
                  </button>
                );
              })}
            </div>

            <div className="rf-toolbar-row" style={{ padding: "8px 16px" }}>
              <SearchInput
                placeholder="Caută după nr., CUI emitent sau denumire…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                style={{ width: 320 }}
              />
              <div style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
                {selected.size > 0 && (
                  <>
                    <span style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-text-muted)" }}>
                      {selected.size} selectate
                    </span>
                    <Btn
                      variant="primary"
                      size="sm"
                      icon="check"
                      onClick={() => {
                        [...selected].forEach((id) => updateStatus({ id, status: "APPROVED" }));
                        setSelected(new Set());
                      }}
                    >
                      Aprobă toate
                    </Btn>
                    <Btn
                      variant="secondary"
                      size="sm"
                      icon="bookmark"
                      onClick={() => {
                        [...selected].forEach((id) => updateStatus({ id, status: "ARCHIVED" }));
                        setSelected(new Set());
                      }}
                    >
                      Arhivează
                    </Btn>
                    <Btn
                      variant="danger"
                      size="sm"
                      icon="x"
                      onClick={() => {
                        [...selected].forEach((id) => updateStatus({ id, status: "REJECTED" }));
                        setSelected(new Set());
                      }}
                    >
                      Respinge
                    </Btn>
                  </>
                )}
                <IconBtn
                  icon="refresh"
                  title="Reîmprospătează"
                  onClick={() => void refetch()}
                />
              </div>
            </div>
          </div>

          {/* Truncation warning */}
          {paged && paged.total > paged.items.length && (
            <div
              style={{
                padding: "6px 16px",
                background: "var(--rf-warning-bg, #fffbeb)",
                borderBottom: "1px solid var(--rf-border)",
                fontSize: 12,
                color: "var(--rf-warning, #92400e)",
              }}
            >
              Afișate primele {paged.items.length.toLocaleString("ro-RO")} din {paged.total.toLocaleString("ro-RO")} facturi — restrânge filtrele pentru a vedea toate înregistrările.
            </div>
          )}

          {/* Table */}
          <div className="rf-tbl-wrap">
            {isLoading ? (
              <Empty icon="fileIn" title="Se încarcă…" />
            ) : isError ? (
              <QueryErrorBanner error={error} label="facturile primite" onRetry={() => void refetch()} />
            ) : list.length === 0 ? (
              <Empty icon="fileIn" title={allInvoices.length === 0 ? "Nicio factură primită" : "Nicio înregistrare pentru filtrele aplicate"}>
                {allInvoices.length === 0 && "Descărcați din SPV sau importați un XML."}
              </Empty>
            ) : (
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th className="rf-ck">
                      <input
                        type="checkbox"
                        className="rf-cbx"
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
                    <th>Furnizor</th>
                    <th>CUI</th>
                    <th>Serie-Număr</th>
                    <th>Data</th>
                    <th className="rf-num">Net</th>
                    <th className="rf-num">TVA</th>
                    <th className="rf-num">Total</th>
                    <th>Monedă</th>
                    <th>Status</th>
                    <th style={{ width: 120 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {list.map((inv) => {
                    const docNo = invoiceNo(inv.series, inv.number, inv.anafDownloadId);
                    return (
                      <tr
                        key={inv.id}
                        className="clickable"
                        style={{ cursor: "pointer" }}
                        onClick={() => navigate({ to: "/received/$id", params: { id: inv.id } })}
                      >
                        <td className="rf-ck" onClick={(e) => e.stopPropagation()}>
                          <input
                            type="checkbox"
                            className="rf-cbx"
                            checked={selected.has(inv.id)}
                            onChange={() => toggleOne(inv.id)}
                          />
                        </td>
                        <td style={{ fontWeight: 500 }}>{inv.issuerName}</td>
                        <td style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-text-muted)" }}>{inv.issuerCui}</td>
                        <td style={{ fontWeight: 600, fontFamily: "var(--rf-mono)" }}>{docNo}</td>
                        <td style={{ color: "var(--rf-text-muted)" }}>{inv.issueDate}</td>
                        <td className="rf-num" style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-text-muted)", fontVariantNumeric: "tabular-nums" }}>
                          {inv.netAmount != null ? fmtRON(inv.netAmount) : "—"}
                        </td>
                        <td className="rf-num" style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-text-muted)", fontVariantNumeric: "tabular-nums" }}>
                          {inv.vatAmount != null ? fmtRON(inv.vatAmount) : "—"}
                        </td>
                        <td className="rf-num" style={{ fontFamily: "var(--rf-mono)", fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>{fmtRON(inv.totalAmount)}</td>
                        <td style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-text-muted)" }}>{inv.currency}</td>
                        <td><StatusBadge status={inv.status} /></td>
                        <td onClick={(e) => e.stopPropagation()}>
                          <div className="rf-cell-actions">
                            {(inv.status === "NEW" || inv.status === "REVIEWED") && (
                              <>
                                <IconBtn
                                  icon="check"
                                  title="Aprobă"
                                  onClick={() => updateStatus({ id: inv.id, status: "APPROVED" })}
                                />
                                <IconBtn
                                  icon="x"
                                  title="Respinge"
                                  onClick={() => updateStatus({ id: inv.id, status: "REJECTED" })}
                                />
                              </>
                            )}
                            {inv.status === "APPROVED" && (
                              <IconBtn
                                icon="bookmark"
                                title="Arhivează"
                                onClick={() => updateStatus({ id: inv.id, status: "ARCHIVED" })}
                              />
                            )}
                            {inv.status === "REJECTED" && (
                              <IconBtn
                                icon="refresh"
                                title="Reanalizează"
                                onClick={() => updateStatus({ id: inv.id, status: "REVIEWED" })}
                              />
                            )}
                            <IconBtn
                              icon="eye"
                              title="Vizualizează"
                              onClick={() => navigate({ to: "/received/$id", params: { id: inv.id } })}
                            />
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
          </div>

          {/* Footer */}
          <div className="rf-tbl-footer">
            <span>
              <b>{list.length}</b> facturi
              {nonRonCountReceived > 0 && (
                <span style={{ marginLeft: 6, fontSize: 11, color: "var(--rf-text-dim)", fontWeight: 400 }}>
                  (+{nonRonCountReceived} în altă monedă, neincluse în total)
                </span>
              )}
            </span>
            <span>Net: <b style={{ fontFamily: "var(--rf-mono)" }}>{fmtRON(totalNet)} RON</b></span>
            <span>TVA: <b style={{ fontFamily: "var(--rf-mono)" }}>{fmtRON(totalVat)} RON</b></span>
            <span>Total: <b style={{ fontFamily: "var(--rf-mono)" }}>{fmtRON(totalSum)} RON</b></span>
            <span style={{ marginLeft: "auto" }}>De aprobat: <b style={{ color: "var(--rf-accent)" }}>{counts.NEW + counts.REVIEWED}</b></span>
          </div>
        </Card>
      </div>
    </div>
  );
}
