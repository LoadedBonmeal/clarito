/**
 * Stornate — verbatim port of the design "Stornate.html":
 *   .page-head (breadcrumb "e-Factura › Stornate" + h1 "Facturi stornate" +
 *   count sub) → .banner info (valoarea originală pozitivă; reversarea fiscală
 *   e purtată de nota de credit) → .scr-card → .scr-table
 *   (Număr .doc · Data · Client cu .cli-ava · Total stornat r/num · chip Stornată).
 *
 * ALL wiring preserved: api.invoices.list (filtrat STORNED), api.contacts.list
 * pentru numele clientului, row click → /invoices/$id + setSelectedInvoiceId.
 */

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Trans, useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON } from "@/lib/utils";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

/** Initials for the .cli-ava chip (design parity with Dashboard/Invoices). */
const ini = (name?: string | null) =>
  (name ?? "")
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("") || "—";

// ── StornatePage ──────────────────────────────────────────────────────────────

export function StornatePage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const setSelectedInvoiceId = useAppStore((s) => s.setSelectedInvoiceId);

  const { data: paged, isLoading, isError, error, refetch } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.invoices.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });
  const activeCompany = companies.find((c) => c.id === activeCompanyId);

  const contactMap = useMemo(() => {
    const m = new Map<string, string>();
    for (const c of contacts) m.set(c.id, c.legalName);
    return m;
  }, [contacts]);

  // Filter to STORNED status only — same data source as Invoices.tsx
  const storned = useMemo(
    () => (paged?.items ?? []).filter((inv) => inv.status === "STORNED"),
    [paged],
  );

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head">
          <div>
            <div style={{ fontSize: 12, color: "var(--dim)", marginBottom: 4 }}>{t("invoices.stornate.breadcrumb")}</div>
            <h1>{t("invoices.stornate.title")}</h1>
          </div>
        </div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("invoices.stornate.selectCompany")}
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <div style={{ fontSize: 12, color: "var(--dim)", marginBottom: 4 }}>{t("invoices.stornate.breadcrumb")}</div>
          <h1>{t("invoices.stornate.title")}</h1>
          <p className="sub">
            {t("invoices.stornate.count", { count: storned.length })}
            {activeCompany ? ` · ${activeCompany.legalName}` : ""}
          </p>
        </div>
      </div>

      {/* info banner */}
      <div className="banner">
        <svg
          className="ic"
          viewBox="0 0 24 24"
          dangerouslySetInnerHTML={{
            __html:
              '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>',
          }}
        />
        <span>
          <Trans i18nKey="invoices.stornate.banner" components={{ b: <b /> }} />
        </span>
      </div>

      <div className="scr-card">
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("invoices.states.loading")}</div>
        ) : isError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={error} label={t("invoices.stornate.errorLabel")} onRetry={() => void refetch()} />
          </div>
        ) : storned.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {t("invoices.stornate.empty")}
          </div>
        ) : (
          <table className="scr-table">
            <thead>
              <tr>
                <th style={{ width: 170 }}>{t("invoices.table.number")}</th>
                <th style={{ width: 130 }}>{t("invoices.table.date")}</th>
                <th>{t("invoices.table.client")}</th>
                <th className="r" style={{ width: 160 }}>{t("invoices.stornate.totalStorned")}</th>
                <th style={{ width: 130 }}>{t("invoices.table.status")}</th>
              </tr>
            </thead>
            <tbody>
              {storned.map((inv) => {
                const clientName = contactMap.get(inv.contactId);
                return (
                  <tr
                    key={inv.id}
                    style={{ cursor: "pointer" }}
                    onClick={() => {
                      setSelectedInvoiceId(inv.id);
                      void navigate({ to: "/invoices/$id", params: { id: inv.id } });
                    }}
                  >
                    <td><span className="doc" style={{ fontWeight: 700, color: "var(--text)" }}>{inv.fullNumber}</span></td>
                    <td className="num">{fmtRoDate(inv.issueDate)}</td>
                    <td>
                      <div className="cli">
                        <span className="cli-ava">{ini(clientName)}</span>
                        {clientName ?? "—"}
                      </div>
                    </td>
                    <td className="r num"><b>{fmtRON(inv.totalAmount)} {inv.currency}</b></td>
                    <td>
                      <span className="chip wait"><Ic name="undo" cls="sic" />{t("invoices.status.storned")}</span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
