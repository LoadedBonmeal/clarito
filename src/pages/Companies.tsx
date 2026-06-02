/**
 * Companii administrate — re-skinned to rf kit (Wave 3).
 * Preserves: api.companies.list(), tabs Toate/Cu SPV/Fără SPV, search,
 * license-tier limit (api.license.get), row click → navigate /companies/$id,
 * set active company (useAppStore setActiveCompanyId),
 * delete confirm → api.companies.delete(id),
 * "Companie nouă" → /companies/new.
 * Companies stay as ROUTES (no modal conversion).
 */

import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Icon } from "@/components/shared/Icon";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import {
  PageHeader, Btn, IconBtn, Badge, Card, Segmented, Empty,
} from "@/components/rf";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { AppErrorPayload, Company } from "@/types";

const TIER_LIMITS: Record<string, number> = {
  TRIAL: 3,
  SOLO: 1,
  ACCOUNTANT: 15,
  FIRM: Infinity,
};

/** Deterministic color per company (from CUI) for the dot indicator. */
const DOT_COLORS = [
  "#4F46E5", "#7C3AED", "#0891B2", "#D97706", "#16A34A",
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
  const setActiveCompanyId = useAppStore((s) => s.setActiveCompanyId);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [query, setQuery] = useState("");
  const [filterSPV, setFilterSPV] = useState<SpvFilter>("all");

  const {
    data: companies = [],
    isLoading,
    isError: companiesError,
    error: companiesErr,
    refetch: refetchCompanies,
  } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  const { data: license } = useQuery({
    queryKey: queryKeys.license,
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
    const ok = await confirm(
      `Ștergeți compania "${c.legalName}"? Această acțiune nu poate fi anulată.`,
      { title: "Confirmare ștergere", kind: "warning" },
    );
    if (!ok) return;
    try {
      await api.companies.delete(c.id);
      if (activeCompanyId === c.id) setActiveCompanyId(null);
      void queryClient.invalidateQueries({ queryKey: queryKeys.companies.all });
    } catch (err) {
      const payload = err as AppErrorPayload;
      notify.error(formatError(payload, "Eroare la ștergerea companiei."));
    }
  };

  const spvOptions = [
    { value: "all" as SpvFilter, label: `Toate (${companies.length})` },
    { value: "yes" as SpvFilter, label: `Cu SPV (${withSpv})` },
    { value: "no" as SpvFilter, label: `Fără SPV (${companies.length - withSpv})` },
  ];

  return (
    <div className="rf-page">
      <PageHeader
        title="Companii"
        sub={<Badge variant="neutral" dot={false}>{companies.length} companii</Badge>}
        actions={
          <Btn
            variant="primary"
            icon="plus"
            size="sm"
            disabled={atLimit}
            title={atLimit ? "Limita planului tău este atinsă" : undefined}
            onClick={() => {
              if (!atLimit) void navigate({ to: "/companies/new" });
            }}
          >
            Companie nouă
          </Btn>
        }
      />

      <div className="rf-page-body">
        {license && tierLimit < Infinity && (
          <div
            style={{
              padding: "8px 16px",
              marginBottom: 12,
              background: "var(--rf-info-bg)",
              border: "1px solid var(--rf-info-bd)",
              borderRadius: "var(--rf-radius)",
              fontSize: 12,
              display: "flex",
              gap: 10,
              alignItems: "center",
              color: "var(--rf-info)",
            }}
          >
            <Icon name="info" size={15} />
            <span>
              Plan <b>{license.tier}</b>: {companies.length}/{tierLimit} companii.
            </span>
            {atLimit && (
              <Btn
                variant="secondary"
                size="sm"
                onClick={() =>
                  notify.info("Contactați-ne pentru upgrade la support@efactura.ro")
                }
              >
                Upgrade
              </Btn>
            )}
          </div>
        )}

        <Card>
          {/* Toolbar */}
          <div
            className="rf-toolbar-row"
            style={{ padding: "10px 16px", borderBottom: "1px solid var(--rf-border)" }}
          >
            <div className="rf-search" style={{ width: 300 }}>
              <Icon name="search" size={14} />
              <input
                placeholder="Caută după CUI, denumire sau localitate…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </div>
            <Segmented
              options={spvOptions}
              value={filterSPV}
              onChange={(v) => setFilterSPV(v)}
            />
            <div style={{ marginLeft: "auto" }}>
              <IconBtn
                icon="refresh"
                title="Reîmprospătează"
                onClick={() =>
                  void queryClient.invalidateQueries({
                    queryKey: queryKeys.companies.all,
                  })
                }
              />
            </div>
          </div>

          {/* Table */}
          <div className="rf-tbl-wrap">
            {isLoading ? (
              <Empty icon="building" title="Se încarcă…" />
            ) : companiesError ? (
              <QueryErrorBanner
                error={companiesErr}
                label="companiile"
                onRetry={() => void refetchCompanies()}
              />
            ) : list.length === 0 ? (
              <Empty
                icon="building"
                title={
                  companies.length === 0
                    ? "Nicio companie"
                    : "Nicio înregistrare pentru filtrele aplicate"
                }
                actions={
                  companies.length === 0 ? (
                    <Btn
                      variant="primary"
                      icon="plus"
                      onClick={() => void navigate({ to: "/companies/new" })}
                    >
                      Adaugă prima companie
                    </Btn>
                  ) : undefined
                }
              />
            ) : (
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th style={{ width: 120 }}>CUI</th>
                    <th>Denumire</th>
                    <th style={{ width: 140 }}>Localitate</th>
                    <th style={{ width: 50 }}>Județ</th>
                    <th style={{ width: 60, textAlign: "center" }}>SPV</th>
                    <th style={{ width: 80 }}>Serie</th>
                    <th style={{ width: 80 }}>Reg. Com.</th>
                    <th style={{ width: 60, textAlign: "center" }}>Activă</th>
                    <th style={{ width: 110 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {list.map((c: Company) => (
                    <tr
                      key={c.id}
                      className="clickable"
                      onClick={() =>
                        void navigate({ to: "/companies/$id", params: { id: c.id } })
                      }
                    >
                      <td className="mono">{c.cui}</td>
                      <td>
                        <span
                          style={{
                            display: "inline-block",
                            width: 8,
                            height: 8,
                            borderRadius: "50%",
                            background: dotColor(c.cui),
                            marginRight: 8,
                            flexShrink: 0,
                            verticalAlign: "middle",
                          }}
                        />
                        <span style={{ fontWeight: 500 }}>{c.legalName}</span>
                        {c.tradeName && (
                          <span
                            style={{
                              marginLeft: 6,
                              fontSize: 11,
                              color: "var(--rf-text-muted)",
                            }}
                          >
                            ({c.tradeName})
                          </span>
                        )}
                      </td>
                      <td style={{ color: "var(--rf-text-muted)" }}>{c.city}</td>
                      <td className="mono" style={{ color: "var(--rf-text-muted)" }}>
                        {c.county}
                      </td>
                      <td style={{ textAlign: "center" }}>
                        {c.spvEnabled ? (
                          <Icon name="checkCircle" size={15} style={{ color: "var(--rf-success)" }} />
                        ) : (
                          <span className="rf-dim">
                            <Icon name="xCircle" size={15} />
                          </span>
                        )}
                      </td>
                      <td className="mono">{c.invoiceSeries}</td>
                      <td className="mono" style={{ fontSize: 11, color: "var(--rf-text-muted)" }}>
                        {c.registryNumber ?? "—"}
                      </td>
                      <td style={{ textAlign: "center" }}>
                        {activeCompanyId === c.id ? (
                          <Badge variant="success" dot={false}>Activă</Badge>
                        ) : (
                          <Badge variant="neutral" dot={false}>—</Badge>
                        )}
                      </td>
                      <td onClick={(e) => e.stopPropagation()}>
                        <div className="rf-cell-actions">
                          <IconBtn
                            icon="check"
                            title="Setează ca activă"
                            size={14}
                            onClick={() => setActiveCompanyId(c.id)}
                          />
                          <IconBtn
                            icon="pen"
                            title="Editează"
                            size={14}
                            onClick={() =>
                              void navigate({
                                to: "/companies/$id/edit",
                                params: { id: c.id },
                              })
                            }
                          />
                          <IconBtn
                            icon="trash"
                            title="Șterge"
                            size={14}
                            onClick={() => void handleDelete(c)}
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
              Total: <b>{list.length}</b> companii
            </span>
            <span>
              Cu SPV: <b>{list.filter((c) => c.spvEnabled).length}</b>
            </span>
            <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--rf-text-muted)" }}>
              Click pe rând pentru detalii
            </span>
          </div>
        </Card>
      </div>
    </div>
  );
}
