/**
 * Facturi primite — date REALE din backend (api.received.list),
 * cu vizualul Win32 portat din Claude Design.
 *
 * ReceivedInvoice conține deja issuerCui / issuerName — nu e nevoie
 * de un join suplimentar cu contactele.
 */

import { useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON } from "@/lib/utils";
import { fmtShortcut } from "@/lib/platform";
import type { ReceivedStatus } from "@/types";

type StatusFilter = ReceivedStatus | "all";

/** Compune numărul de document afișat în tabel. */
function invoiceNo(series: string | null, number: string | null, fallback: string): string {
  if (series && number) return `${series}-${number}`;
  if (number) return number;
  return fallback;
}

export function ReceivedPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<StatusFilter>("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [hoverId, setHoverId] = useState<string | null>(null);

  // Fetch received invoices
  const { data: paged, isLoading } = useQuery({
    queryKey: queryKeys.received.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.received.list({ companyId: activeCompanyId ?? undefined }),
  });

  // Update status mutation
  const { mutate: updateStatus } = useMutation({
    mutationFn: ({ id, status }: { id: string; status: ReceivedStatus }) =>
      api.received.updateStatus(id, status),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: queryKeys.received.list({ companyId: activeCompanyId ?? undefined }),
      });
    },
  });

  // ANAF test mode
  const { data: testModeSetting } = useQuery({
    queryKey: ["settings", "use_anaf_test_env"],
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
      if (count > 0) alert(`${count} facturi noi descărcate din SPV.`);
      else alert("Nicio factură nouă în SPV.");
    },
    onError: (e) => alert("Eroare sincronizare SPV: " + (e as Error).message),
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

  const totalSum = list.reduce((s, i) => s + i.totalAmount, 0);

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

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">e-Factura</span>
          Facturi primite
        </span>
        <span className="muted" style={{ fontSize: 11 }}>
          {list.length} facturi · {fmtRON(totalSum)} RON
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button type="button" className="btn">
            <Icon name="upload" size={12} /> Import XML
          </button>
          <button type="button" className="btn">
            <Icon name="download" size={12} /> Export selecție
          </button>
          <button
            type="button"
            className="btn primary"
            disabled={isSyncing || !activeCompanyId}
            onClick={() => syncSpv()}
          >
            <Icon name="cloudDn" size={12} /> {isSyncing ? "Sincronizare…" : "Descarcă din SPV"}{" "}
            {!isSyncing && (
              <span
                className="kbd"
                style={{
                  marginLeft: 6,
                  background: "rgba(255,255,255,0.18)",
                  border: "1px solid rgba(255,255,255,0.3)",
                  color: "#fff",
                }}
              >
                {fmtShortcut("F5")}
              </span>
            )}
          </button>
        </span>
      </div>

      {/* Saved views */}
      <div className="views-bar">
        <span
          className={"view-tab " + (filter === "all" ? "active" : "")}
          onClick={() => setFilter("all")}
        >
          Toate <span className="count">{counts.all}</span>
        </span>
        <span
          className={"view-tab " + (filter === "NEW" ? "active" : "")}
          onClick={() => setFilter("NEW")}
        >
          Noi{" "}
          <span className="count" style={{ color: "var(--accent)" }}>
            {counts.NEW}
          </span>
        </span>
        <span
          className={"view-tab " + (filter === "REVIEWED" ? "active" : "")}
          onClick={() => setFilter("REVIEWED")}
        >
          De revizuit <span className="count">{counts.REVIEWED}</span>
        </span>
        <span
          className={"view-tab " + (filter === "APPROVED" ? "active" : "")}
          onClick={() => setFilter("APPROVED")}
        >
          Aprobate <span className="count">{counts.APPROVED}</span>
        </span>
        <span
          className={"view-tab " + (filter === "REJECTED" ? "active" : "")}
          onClick={() => setFilter("REJECTED")}
        >
          Respinse{" "}
          <span className="count" style={{ color: "#DC2626" }}>
            {counts.REJECTED}
          </span>
        </span>
        <span
          className={"view-tab " + (filter === "ARCHIVED" ? "active" : "")}
          onClick={() => setFilter("ARCHIVED")}
        >
          Arhivate <span className="count">{counts.ARCHIVED}</span>
        </span>
        <span className="view-tab" style={{ color: "var(--accent)", borderRight: 0 }}>
          <Icon name="plus" size={11} /> Salvează vizualizarea
        </span>
      </div>

      {/* Toolbar */}
      <div className="content-toolbar">
        <div className="search">
          <Icon name="search" size={13} />
          <input
            placeholder="Caută după nr., CUI emitent sau denumire…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <span className="kbd-hint">{fmtShortcut("Ctrl F")}</span>
        </div>
        <span className="divider-v" style={{ margin: "0 4px" }} />
        <span className="chip">
          Perioadă: toate <Icon name="caret" size={10} />
        </span>
        <span className="chip">
          Sumă: orice <Icon name="caret" size={10} />
        </span>
        <span
          style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}
        >
          {selected.size > 0 ? (
            <>
              <span style={{ fontSize: 11, fontWeight: 600 }}>
                {selected.size} selectate
              </span>
              <button
                type="button"
                className="btn compact primary"
                onClick={() => {
                  [...selected].forEach((id) =>
                    updateStatus({ id, status: "APPROVED" }),
                  );
                  setSelected(new Set());
                }}
              >
                <Icon name="check" size={11} /> Aprobă toate
              </button>
              <button
                type="button"
                className="btn compact"
                onClick={() => {
                  [...selected].forEach((id) =>
                    updateStatus({ id, status: "ARCHIVED" }),
                  );
                  setSelected(new Set());
                }}
              >
                <Icon name="bookmark" size={11} /> Arhivează
              </button>
              <button
                type="button"
                className="btn compact danger"
                style={{ height: 22 }}
                onClick={() => {
                  [...selected].forEach((id) =>
                    updateStatus({ id, status: "REJECTED" }),
                  );
                  setSelected(new Set());
                }}
              >
                <Icon name="x" size={11} /> Respinge
              </button>
              <span className="divider-v" style={{ margin: "0 4px" }} />
            </>
          ) : (
            <span style={{ fontSize: 10.5, color: "var(--text-dim)" }}>
              Treci cu mouse-ul peste un rând pentru aprobare/respingere rapidă
            </span>
          )}
          <button type="button" className="btn-icon" title="Coloane">
            <Icon name="filter" size={14} />
          </button>
          <button type="button" className="btn-icon" title="Mai multe">
            <Icon name="more" size={14} />
          </button>
        </span>
      </div>

      <div className="content-body">
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 12, color: "var(--text-muted)" }}>
            Se încarcă…
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: 40, textAlign: "center", fontSize: 12, color: "var(--text-muted)" }}>
            {allInvoices.length === 0
              ? "Nicio factură primită. Descărcați din SPV sau importați un XML."
              : "Nicio înregistrare pentru filtrele aplicate."}
          </div>
        ) : (
          <table className="dt">
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
                <th style={{ width: 140 }}>Nr. document</th>
                <th style={{ width: 96 }} className="sortable sorted">
                  Data <span className="sort">▾</span>
                </th>
                <th style={{ width: 110 }}>CUI emitent</th>
                <th>Emitent</th>
                <th style={{ width: 70 }}>Monedă</th>
                <th className="num" style={{ width: 120 }}>
                  Total
                </th>
                <th style={{ width: 130 }}>Status</th>
                <th style={{ width: 110 }}>Index ANAF</th>
                <th style={{ width: 150 }}>Acțiuni</th>
              </tr>
            </thead>
            <tbody>
              {list.map((inv) => {
                const isHover = hoverId === inv.id;
                const docNo = invoiceNo(inv.series, inv.number, inv.anafDownloadId);
                return (
                  <tr
                    key={inv.id}
                    onMouseEnter={() => setHoverId(inv.id)}
                    onMouseLeave={() => setHoverId(null)}
                    className={selected.has(inv.id) ? "selected" : ""}
                    style={{ cursor: "pointer" }}
                    onClick={() => navigate({ to: "/received/$id", params: { id: inv.id } })}
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
                      <b>{docNo}</b>
                    </td>
                    <td className="muted">{inv.issueDate}</td>
                    <td className="mono">{inv.issuerCui}</td>
                    <td>{inv.issuerName}</td>
                    <td className="mono muted">{inv.currency}</td>
                    <td className="num tnum">
                      <b>{fmtRON(inv.totalAmount)}</b>
                    </td>
                    <td>
                      <StatusBadge status={inv.status} />
                    </td>
                    <td className="mono dim">{inv.anafIndex || "—"}</td>
                    <td onClick={(e) => e.stopPropagation()}>
                      {isHover ? (
                        <div style={{ display: "flex", gap: 2 }}>
                          {(inv.status === "NEW" || inv.status === "REVIEWED") && (
                            <>
                              <button
                                type="button"
                                className="btn compact primary"
                                title="Aprobă"
                                onClick={() =>
                                  updateStatus({ id: inv.id, status: "APPROVED" })
                                }
                              >
                                <Icon name="check" size={11} /> Aprobă
                              </button>
                              <button
                                type="button"
                                className="btn compact"
                                style={{ borderColor: "#FCA5A5", color: "#B91C1C" }}
                                title="Respinge"
                                onClick={() =>
                                  updateStatus({ id: inv.id, status: "REJECTED" })
                                }
                              >
                                <Icon name="x" size={11} /> Respinge
                              </button>
                            </>
                          )}
                          {inv.status === "APPROVED" && (
                            <button
                              type="button"
                              className="btn compact"
                              onClick={() =>
                                updateStatus({ id: inv.id, status: "ARCHIVED" })
                              }
                            >
                              <Icon name="bookmark" size={11} /> Arhivează
                            </button>
                          )}
                          {inv.status === "REJECTED" && (
                            <button
                              type="button"
                              className="btn compact"
                              onClick={() =>
                                updateStatus({ id: inv.id, status: "REVIEWED" })
                              }
                            >
                              <Icon name="refresh" size={11} /> Reanalizează
                            </button>
                          )}
                          <button
                            type="button"
                            className="btn-icon"
                            title="Descarcă XML"
                          >
                            <Icon name="download" size={13} />
                          </button>
                        </div>
                      ) : (
                        <div
                          className="dim"
                          style={{ display: "flex", gap: 2, opacity: 0.6 }}
                        >
                          <button type="button" className="btn-icon">
                            <Icon name="download" size={13} />
                          </button>
                          <button type="button" className="btn-icon">
                            <Icon name="more" size={13} />
                          </button>
                        </div>
                      )}
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
          Total: <b style={{ color: "var(--text)" }}>{list.length}</b> facturi
        </span>
        <span>
          Sumă:{" "}
          <b style={{ color: "var(--text)" }} className="tnum">
            {fmtRON(totalSum)} RON
          </b>
        </span>
        <span>
          De aprobat:{" "}
          <b style={{ color: "var(--accent)" }}>{counts.NEW + counts.REVIEWED}</b>
        </span>
        <span style={{ marginLeft: "auto" }}>
          <span className="kbd">A</span> aprobă ·{" "}
          <span className="kbd">R</span> respinge ·{" "}
          <span className="kbd">↑↓</span> navighează
        </span>
      </div>
    </div>
  );
}
