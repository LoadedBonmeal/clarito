/**
 * Facturi stornate — Polish-Wave 6 dedicated page.
 *
 * LAYOUT (matches "Claude Design" reference):
 *   Breadcrumb: e-Factura › Stornate
 *   Title: "Facturi stornate"
 *   Single card-wrapped table with columns:
 *     NUMĂR (mono) · DATA · CLIENT · TOTAL STORNAT (right-aligned, negative) · STATUS
 *
 * Data: api.invoices.list filtered to status === "STORNED"
 * Client names: resolved via api.contacts.list (same pattern as Invoices.tsx)
 */

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";

import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { PageHeader, Empty } from "@/components/rf";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, formatDate } from "@/lib/utils";

// ── StornatePage ──────────────────────────────────────────────────────────────

export function StornatePage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const setSelectedInvoiceId = useAppStore((s) => s.setSelectedInvoiceId);

  // Guard: no active company
  if (!activeCompanyId) {
    return (
      <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "var(--rf-app-bg)" }}>
        <PageHeader screen="e-Factura › Stornate" title="Facturi stornate" />
        <Empty icon="buildings" title="Selectați o companie activă">
          Alegeți o companie din meniul din stânga pentru a vedea facturile stornate.
        </Empty>
      </div>
    );
  }

  return <StornatPageInner activeCompanyId={activeCompanyId} setSelectedInvoiceId={setSelectedInvoiceId} />;
}

// ── Inner component (activeCompanyId is guaranteed non-null here) ──────────────

interface InnerProps {
  activeCompanyId: string;
  setSelectedInvoiceId: (id: string | null) => void;
}

function StornatPageInner({ activeCompanyId, setSelectedInvoiceId }: InnerProps) {
  const navigate = useNavigate();

  // Fetch all invoices for this company
  const {
    data: paged,
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId }),
    queryFn: () => api.invoices.list({ companyId: activeCompanyId }),
  });

  // Fetch contacts for client name resolution — same approach as Invoices.tsx
  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId }),
  });

  const contactMap = useMemo(() => {
    const m = new Map<string, string>();
    for (const c of contacts) m.set(c.id, c.legalName);
    return m;
  }, [contacts]);

  // Filter to STORNED status only
  const storned = useMemo(
    () => (paged?.items ?? []).filter((inv) => inv.status === "STORNED"),
    [paged],
  );

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "var(--rf-app-bg)" }}>

      {/* ── Header ──────────────────────────────────────────────────────────── */}
      <PageHeader
        screen="e-Factura › Stornate"
        title="Facturi stornate"
      />

      {/* ── Content ─────────────────────────────────────────────────────────── */}
      <div style={{ flex: 1, overflowY: "auto", padding: "24px 32px" }}>

        {isLoading ? (
          <SkeletonRows />
        ) : isError ? (
          <QueryErrorBanner error={error} label="facturile stornate" onRetry={() => void refetch()} />
        ) : storned.length === 0 ? (
          <Empty icon="storno" title="Nicio factură stornată">
            Facturile stornate vor apărea aici.
          </Empty>
        ) : (
          <div className="rf-card" style={{ overflow: "hidden" }}>
            <div className="rf-tbl-wrap">
              <table className="rf-tbl" style={{ width: "100%" }}>
                <thead>
                  <tr>
                    <th style={{ width: 170 }}>NUMĂR</th>
                    <th style={{ width: 130 }}>DATA</th>
                    <th>CLIENT</th>
                    <th className="right" style={{ width: 160 }}>TOTAL STORNAT</th>
                    <th style={{ width: 130 }}>STATUS</th>
                  </tr>
                </thead>
                <tbody>
                  {storned.map((inv) => {
                    const clientName = contactMap.get(inv.contactId);
                    // REG-STORNO: the STORNED invoice holds its ORIGINAL (positive)
                    // amount — the fiscal reversal is carried by the negative credit note,
                    // not by this row. Display the real amount so accountants are not misled.
                    const displayTotal = fmtRON(inv.totalAmount);

                    return (
                      <tr
                        key={inv.id}
                        className="clickable"
                        style={{ cursor: "pointer", height: 52 }}
                        onClick={() => {
                          setSelectedInvoiceId(inv.id);
                          void navigate({ to: "/invoices/$id", params: { id: inv.id } });
                        }}
                      >
                        <td
                          style={{
                            fontFamily: "var(--rf-mono)",
                            fontWeight: 700,
                          }}
                        >
                          {inv.fullNumber}
                        </td>
                        <td style={{ color: "var(--rf-text-muted)" }}>
                          {formatDate(inv.issueDate)}
                        </td>
                        <td style={{ fontWeight: 500 }}>
                          {clientName ?? (
                            <span style={{ color: "var(--rf-text-dim)" }}>—</span>
                          )}
                        </td>
                        <td
                          className="right"
                          style={{
                            fontFamily: "var(--rf-mono)",
                            fontVariantNumeric: "tabular-nums",
                            color: "var(--rf-text-muted)",
                          }}
                        >
                          {displayTotal}
                          <span
                            style={{
                              marginLeft: 8,
                              fontSize: 10,
                              fontFamily: "var(--rf-sans)",
                              fontWeight: 600,
                              letterSpacing: "0.06em",
                              textTransform: "uppercase",
                              color: "var(--rf-warning, #b45309)",
                              verticalAlign: "middle",
                            }}
                          >
                            anulată
                          </span>
                        </td>
                        <td>
                          <StatusBadge status={inv.status} />
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>

            {/* Footer with count */}
            <div className="rf-tbl-footer">
              <span>
                Total:{" "}
                <b>{storned.length}</b>{" "}
                {storned.length === 1 ? "factură stornată" : "facturi stornate"}
              </span>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ── Loading skeleton ───────────────────────────────────────────────────────────

function SkeletonRows() {
  return (
    <div className="rf-card" style={{ overflow: "hidden" }}>
      <div className="rf-tbl-wrap">
        <table className="rf-tbl" style={{ width: "100%" }}>
          <thead>
            <tr>
              <th style={{ width: 170 }}>NUMĂR</th>
              <th style={{ width: 130 }}>DATA</th>
              <th>CLIENT</th>
              <th className="right" style={{ width: 160 }}>TOTAL STORNAT</th>
              <th style={{ width: 130 }}>STATUS</th>
            </tr>
          </thead>
          <tbody>
            {Array.from({ length: 5 }).map((_, i) => (
              <tr key={i} style={{ height: 52 }}>
                {Array.from({ length: 5 }).map((__, j) => (
                  <td key={j}>
                    <span
                      style={{
                        display: "inline-block",
                        width: j === 2 ? "60%" : j === 3 ? "80px" : "70%",
                        height: 14,
                        background: "var(--rf-skeleton, rgba(0,0,0,0.08))",
                        borderRadius: 4,
                        animation: "pulse 1.5s ease-in-out infinite",
                      }}
                    />
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
