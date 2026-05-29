/**
 * Companii administrate — date REALE din backend (api.companies.list),
 * cu vizualul Win32 portat din Claude Design (views-bar, content-toolbar,
 * tabel dens .dt).
 */

import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";

import { Icon } from "@/components/shared/Icon";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { fmtShortcut } from "@/lib/platform";
import { notify } from "@/lib/toasts";
import type { AppErrorPayload, Company } from "@/types";

const TIER_LIMITS: Record<string, number> = {
  TRIAL: 3,
  SOLO: 1,
  ACCOUNTANT: 15,
  FIRM: Infinity,
};

/** Culoare deterministă per companie (din CUI) pentru bulina din tabel. */
const DOT_COLORS = [
  "#2848A1", "#7C3AED", "#0891B2", "#D97706", "#16A34A",
  "#0369A1", "#E11D48", "#65A30D", "#525252", "#B45309",
];
function dotColor(cui: string): string {
  let h = 0;
  for (let i = 0; i < cui.length; i++) h = (h * 31 + cui.charCodeAt(i)) >>> 0;
  return DOT_COLORS[h % DOT_COLORS.length];
}

type SpvFilter = "all" | "yes" | "no";

export function CompaniesPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [query, setQuery] = useState("");
  const [filterSPV, setFilterSPV] = useState<SpvFilter>("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());

  const { data: companies = [], isLoading, isError: companiesError, error: companiesErr, refetch: refetchCompanies } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  const { data: license } = useQuery({
    queryKey: ["license"],
    queryFn: () => api.license.get(),
  });

  const tierLimit = license ? (TIER_LIMITS[license.tier] ?? Infinity) : Infinity;
  const atLimit = companies.length >= tierLimit;

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return companies
      .filter(
        (c) =>
          !q ||
          c.legalName.toLowerCase().includes(q) ||
          c.cui.toLowerCase().includes(q) ||
          c.city.toLowerCase().includes(q),
      )
      .filter((c) =>
        filterSPV === "all" ? true : filterSPV === "yes" ? c.spvEnabled : !c.spvEnabled,
      );
  }, [companies, query, filterSPV]);

  const withSpv = companies.filter((c) => c.spvEnabled).length;

  const handleDelete = async (c: Company) => {
    if (!window.confirm(`Ștergeți compania "${c.legalName}"? Această acțiune nu poate fi anulată.`)) return;
    try {
      await api.companies.delete(c.id);
      void queryClient.invalidateQueries({ queryKey: queryKeys.companies.all });
    } catch (err) {
      const payload = err as AppErrorPayload;
      notify.error(payload?.message ?? "Eroare la ștergerea companiei.");
    }
  };

  const toggleAll = () => {
    setSelected(
      selected.size === list.length ? new Set() : new Set(list.map((c) => c.id)),
    );
  };
  const toggleOne = (id: string) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setSelected(next);
  };

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">Date</span>
          Companii administrate
        </span>
        <span className="muted" style={{ fontSize: 11 }}>
          {list.length} din {companies.length} companii
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button
            type="button"
            className="btn"
            onClick={() => notify.info("Funcție în curs de implementare")}
          >
            <Icon name="upload" size={12} /> Import CSV
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => notify.info("Funcție în curs de implementare")}
          >
            <Icon name="download" size={12} /> Export
          </button>
          <button
            type="button"
            className="btn primary"
            disabled={atLimit}
            title={atLimit ? "Limita planului tău este atinsă" : undefined}
            onClick={() => !atLimit && navigate({ to: "/companies/new" })}
          >
            <Icon name="plus" size={12} /> Adaugă companie
          </button>
        </span>
      </div>

      {/* Saved views */}
      <div className="views-bar">
        <span
          className={"view-tab " + (filterSPV === "all" ? "active" : "")}
          onClick={() => setFilterSPV("all")}
        >
          Toate <span className="count">{companies.length}</span>
        </span>
        <span
          className={"view-tab " + (filterSPV === "yes" ? "active" : "")}
          onClick={() => setFilterSPV("yes")}
        >
          Cu SPV activ <span className="count">{withSpv}</span>
        </span>
        <span
          className={"view-tab " + (filterSPV === "no" ? "active" : "")}
          onClick={() => setFilterSPV("no")}
        >
          Fără SPV <span className="count">{companies.length - withSpv}</span>
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
            placeholder="Caută după nume, CUI sau localitate…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <span className="kbd-hint">{fmtShortcut("Ctrl F")}</span>
        </div>
        <span className="divider-v" style={{ margin: "0 4px" }} />
        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>SPV:</span>
        <div className="seg">
          <span
            className={"seg-item " + (filterSPV === "all" ? "active" : "")}
            onClick={() => setFilterSPV("all")}
          >
            Toate
          </span>
          <span
            className={"seg-item " + (filterSPV === "yes" ? "active" : "")}
            onClick={() => setFilterSPV("yes")}
          >
            Activ
          </span>
          <span
            className={"seg-item " + (filterSPV === "no" ? "active" : "")}
            onClick={() => setFilterSPV("no")}
          >
            Inactiv
          </span>
        </div>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
          {selected.size > 0 && (
            <>
              <span style={{ fontSize: 11, fontWeight: 600 }}>
                {selected.size} selectate
              </span>
              <button
                type="button"
                className="btn compact"
                onClick={() => notify.info("Funcție în curs de implementare")}
              >
                <Icon name="cloudUp" size={11} /> Sync SPV
              </button>
              <span className="divider-v" style={{ margin: "0 4px" }} />
            </>
          )}
          <button type="button" className="btn-icon" title="Coloane">
            <Icon name="filter" size={14} />
          </button>
          <button type="button" className="btn-icon" title="Reîmprospătează">
            <Icon name="refresh" size={14} />
          </button>
        </span>
      </div>

      <div className="content-body">
        {license && tierLimit < Infinity && (
          <div style={{ padding: "6px 14px", background: "var(--bg-hover)", borderBottom: "1px solid var(--border)", fontSize: 11, display: "flex", gap: 8, alignItems: "center" }}>
            <span>
              Plan <b>{license.tier}</b>: {companies.length}/{tierLimit} companii.
            </span>
            {atLimit && (
              <button
                type="button"
                className="btn"
                style={{ fontSize: 10.5 }}
                onClick={() => notify.info("Contactați-ne pentru upgrade la support@efactura.ro")}
              >
                Upgrade
              </button>
            )}
          </div>
        )}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 12, color: "var(--text-muted)" }}>
            Se încarcă…
          </div>
        ) : companiesError ? (
          <QueryErrorBanner error={companiesErr} label="companiile" onRetry={() => void refetchCompanies()} />
        ) : list.length === 0 ? (
          <div style={{ padding: 40, textAlign: "center", fontSize: 12, color: "var(--text-muted)" }}>
            {companies.length === 0
              ? "Nicio companie. Adăugați prima companie cu butonul \"Adaugă companie\"."
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
                    onChange={toggleAll}
                  />
                </th>
                <th style={{ width: 110 }} className="sortable sorted">
                  CUI <span className="sort">▾</span>
                </th>
                <th className="sortable">
                  Denumire <span className="sort">▴▾</span>
                </th>
                <th style={{ width: 140 }}>Localitate</th>
                <th style={{ width: 60 }}>Județ</th>
                <th style={{ width: 64 }} className="num">SPV</th>
                <th style={{ width: 84 }}>Serie</th>
                <th style={{ width: 80 }} className="num">Nr. ultim</th>
                <th style={{ width: 140 }}>Reg. Comerțului</th>
                <th style={{ width: 90 }}>Acțiuni</th>
              </tr>
            </thead>
            <tbody>
              {list.map((c: Company) => (
                <tr
                  key={c.id}
                  onClick={() => navigate({ to: "/companies/$id", params: { id: c.id } })}
                  style={{ cursor: "pointer" }}
                >
                  <td className="ck" onClick={(e) => e.stopPropagation()}>
                    <input
                      type="checkbox"
                      className="cbx"
                      checked={selected.has(c.id)}
                      onChange={() => toggleOne(c.id)}
                    />
                  </td>
                  <td className="mono">{c.cui}</td>
                  <td>
                    <span
                      style={{
                        display: "inline-block",
                        width: 6,
                        height: 6,
                        background: dotColor(c.cui),
                        marginRight: 6,
                        verticalAlign: "middle",
                      }}
                    />
                    <b>{c.legalName}</b>
                    {c.tradeName && (
                      <span className="muted" style={{ marginLeft: 6, fontSize: 11 }}>
                        ({c.tradeName})
                      </span>
                    )}
                  </td>
                  <td>{c.city}</td>
                  <td className="mono">{c.county}</td>
                  <td className="num">
                    {c.spvEnabled ? (
                      <span style={{ color: "#16A34A", display: "inline-flex" }}>
                        <Icon name="check" size={13} />
                      </span>
                    ) : (
                      <span className="dim">
                        <Icon name="x" size={13} />
                      </span>
                    )}
                  </td>
                  <td className="mono">{c.invoiceSeries}</td>
                  <td className="num tnum">
                    {c.lastInvoiceNumber.toLocaleString("ro-RO")}
                  </td>
                  <td className="mono muted">{c.registryNumber ?? "—"}</td>
                  <td onClick={(e) => e.stopPropagation()}>
                    <button
                      type="button"
                      className="btn-icon"
                      title="Deschide"
                      onClick={() =>
                        navigate({ to: "/companies/$id", params: { id: c.id } })
                      }
                    >
                      <Icon name="external" size={13} />
                    </button>
                    <button
                      type="button"
                      className="btn-icon"
                      title="Editează"
                      onClick={() =>
                        navigate({ to: "/companies/$id/edit", params: { id: c.id } })
                      }
                    >
                      <Icon name="pen" size={13} />
                    </button>
                    <button
                      type="button"
                      className="btn-icon"
                      title="Șterge"
                      onClick={() => void handleDelete(c)}
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
          Total: <b style={{ color: "var(--text)" }}>{list.length}</b> companii
        </span>
        <span>
          Cu SPV: <b style={{ color: "var(--text)" }}>{list.filter((c) => c.spvEnabled).length}</b>
        </span>
        <span style={{ marginLeft: "auto" }}>
          Click pe rând pentru detalii ·{" "}
          <span className="kbd">{fmtShortcut("Ctrl K Ctrl C")}</span> selector rapid companie
        </span>
      </div>
    </div>
  );
}
