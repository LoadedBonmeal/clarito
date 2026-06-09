/**
 * Dashboard — Privire generală, date reale din backend.
 * Wave 5 — rf look: PageHeader + Segmented + Banner + StatCard + SectionCard + rf-tbl
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";

import {
  PageHeader,
  Segmented,
  Banner,
  Btn,
  StatCard,
  SectionCard,
  Empty,
} from "@/components/rf";
import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryClient, queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { parseDec, fmtRON } from "@/lib/utils";
import { notify } from "@/lib/toasts";

type PeriodMode = "today" | "week" | "month" | "ytd";

const PERIOD_OPTIONS: { value: PeriodMode; label: string }[] = [
  { value: "today",  label: "Astăzi"     },
  { value: "week",   label: "Săptămână"  },
  { value: "month",  label: "Lună"       },
  { value: "ytd",    label: "An"         },
];

function fmtTime(unix: number): string {
  return new Date(unix * 1000).toLocaleTimeString("ro-RO", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function notifColor(type: string): string {
  const t = type.toUpperCase();
  if (t.includes("REJECT")) return "error";
  if (t.includes("VALID"))  return "success";
  if (t.includes("WARN") || t.includes("EXPIR")) return "warning";
  return "info";
}

function notifIcon(type: string): string {
  const color = notifColor(type);
  if (color === "error")   return "xCircle";
  if (color === "success") return "checkCircle";
  if (color === "warning") return "alert";
  return "info";
}

export function DashboardPage() {
  const navigate  = useNavigate();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [periodMode, setPeriodMode] = useState<PeriodMode>("month");
  const [refreshing, setRefreshing] = useState(false);

  // ── Queries ────────────────────────────────────────────────────────────────

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn:  () => api.companies.list(),
  });

  const invoiceFilter = useMemo(
    () => ({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    [activeCompanyId],
  );
  const {
    data:   invoicesPage,
    isError: invoicesError,
    error:  invoicesErr,
    refetch: refetchInvoices,
  } = useQuery({
    queryKey: queryKeys.invoices.list(invoiceFilter),
    queryFn:  () => api.invoices.list(invoiceFilter),
    enabled:  !!activeCompanyId,
  });

  const { data: notifications = [] } = useQuery({
    queryKey: queryKeys.notifications.list(false),
    queryFn:  () => api.notifications.list(false),
  });

  const { data: unreadCount = 0 } = useQuery({
    queryKey: queryKeys.notifications.unreadCount(),
    queryFn:  () => api.notifications.unreadCount(),
    refetchInterval: 60_000,
  });

  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn:  () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled:  !!activeCompanyId,
  });

  const { data: isAnafAuth } = useQuery({
    queryKey: queryKeys.anaf.auth(activeCompanyId ?? ""),
    queryFn:  () => api.anaf.isAuthenticated(activeCompanyId!),
    enabled:  !!activeCompanyId,
    staleTime: 30_000,
  });
  const anafConnected = !activeCompanyId || !!isAnafAuth;

  // Micro-enterprise ceiling monitor (100.000 EUR, OUG 89/2025). Uses the current BNR EUR rate
  // (fallback 5.0 if the fetch fails) to convert the ceiling to RON.
  const currentYear = new Date().getFullYear();
  const { data: regimeStatus } = useQuery({
    queryKey: ["taxRegimeStatus", activeCompanyId, currentYear],
    enabled:  !!activeCompanyId,
    staleTime: 5 * 60_000,
    queryFn: async () => {
      let eur = 5.0;
      try {
        eur = await api.bnr.fetchRate("EUR", new Date().toISOString().slice(0, 10));
      } catch {
        /* offline / weekend — fall back to ~5.0 RON/EUR for the warning */
      }
      return api.companies.taxRegimeStatus(activeCompanyId!, currentYear, eur);
    },
  });

  const { data: intrastat } = useQuery({
    queryKey: ["intrastatStatus", activeCompanyId, currentYear],
    enabled: !!activeCompanyId,
    staleTime: 5 * 60_000,
    queryFn: () =>
      api.declarations.intrastatStatus(activeCompanyId!, new Date().toISOString().slice(0, 10)),
  });

  // ── Derived data ───────────────────────────────────────────────────────────

  const contactMap = useMemo(
    () => Object.fromEntries(contacts.map((c) => [c.id, c])),
    [contacts],
  );

  const invoices    = invoicesPage?.items ?? [];
  const invoiceTotal = invoicesPage?.total ?? 0;

  const now = new Date();
  const currentMonth = now.toISOString().split("T")[0].slice(0, 7);

  const [periodFrom, periodTo] = useMemo((): [string, string] => {
    const pad = (n: number) => String(n).padStart(2, "0");
    const fmt = (d: Date) =>
      `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
    const todayStr = fmt(now);
    if (periodMode === "today") return [todayStr, todayStr];
    if (periodMode === "week") {
      const day     = now.getDay();
      const diffToMon = day === 0 ? -6 : 1 - day;
      const mon = new Date(now);
      mon.setDate(now.getDate() + diffToMon);
      const sun = new Date(mon);
      sun.setDate(mon.getDate() + 6);
      return [fmt(mon), fmt(sun)];
    }
    if (periodMode === "ytd") return [`${now.getFullYear()}-01-01`, todayStr];
    const firstDay = new Date(now.getFullYear(), now.getMonth(), 1);
    const lastDay  = new Date(now.getFullYear(), now.getMonth() + 1, 0);
    return [fmt(firstDay), fmt(lastDay)];
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [periodMode, currentMonth]);

  const periodInvoices = useMemo(
    () => invoices.filter((inv) => inv.issueDate >= periodFrom && inv.issueDate <= periodTo),
    [invoices, periodFrom, periodTo],
  );

  const totalNet = useMemo(
    () => periodInvoices.reduce((s, inv) => s + parseDec(inv.subtotalAmount), 0),
    [periodInvoices],
  );
  const totalVat = useMemo(
    () => periodInvoices.reduce((s, inv) => s + parseDec(inv.vatAmount), 0),
    [periodInvoices],
  );

  const validatedCount = useMemo(
    () => periodInvoices.filter((inv) => inv.status === "VALIDATED").length,
    [periodInvoices],
  );
  const rejectedCount = useMemo(
    () => periodInvoices.filter((inv) => inv.status === "REJECTED").length,
    [periodInvoices],
  );
  const draftCount = useMemo(
    () => periodInvoices.filter((inv) => inv.status === "DRAFT").length,
    [periodInvoices],
  );
  const submittedCount = useMemo(
    () => periodInvoices.filter((inv) => inv.status === "SUBMITTED").length,
    [periodInvoices],
  );

  const lastRejected = useMemo(
    () => invoices.find((inv) => inv.status === "REJECTED"),
    [invoices],
  );

  const recentInvoices   = invoices.slice(0, 10);
  const timelineItems    = notifications.slice(0, 8);

  const activeCompany    = companies.find((c) => c.id === activeCompanyId) ?? companies[0];
  const monthLabel       = now.toLocaleDateString("ro-RO", { month: "long", year: "numeric" });

  const hour     = now.getHours();
  const greeting = hour < 12 ? "Bună dimineața" : hour < 17 ? "Bună ziua" : "Bună seara";

  // ── Render ────────────────────────────────────────────────────────────────

  if (!activeCompanyId) {
    return (
      <div className="rf-content">
        <PageHeader title="Privire generală" />
        <div className="rf-page-body">
          <Empty icon="buildings" title="Selectați o companie activă pentru a vedea datele din tabloul de bord.">
            Alegeți o companie din setări pentru a continua.
          </Empty>
        </div>
      </div>
    );
  }

  return (
    <div className="rf-content">
      <PageHeader
        title="Privire generală"
        desc={`${greeting}${activeCompany ? `, ${activeCompany.legalName}` : ""}. Iată ce s-a întâmplat cu afacerea dvs. în perioada selectată.`}
        actions={
          <>
            <Segmented
              options={PERIOD_OPTIONS}
              value={periodMode}
              onChange={setPeriodMode}
            />
            <Btn
              variant="primary"
              icon="fileOut"
              onClick={() => void navigate({ to: "/invoices/new" })}
            >
              Factură nouă
            </Btn>
            <Btn
              variant="secondary"
              icon="refresh"
              disabled={refreshing}
              onClick={async () => {
                setRefreshing(true);
                try {
                  await queryClient.refetchQueries({ type: "active" });
                  notify.success("Date actualizate");
                } finally {
                  setRefreshing(false);
                }
              }}
            >
              {refreshing ? "Se actualizează…" : "Reîmprospătează"}
            </Btn>
          </>
        }
      />

      <div className="rf-page-body">
        {/* ── Truncation warning ─────────────────────────────────────────── */}
        {invoicesPage && invoicesPage.total > invoicesPage.items.length && (
          <div
            style={{
              padding: "6px 0 10px",
              fontSize: 12,
              color: "var(--rf-warning, #92400e)",
            }}
          >
            Afișate primele {invoicesPage.items.length.toLocaleString("ro-RO")} din {invoicesPage.total.toLocaleString("ro-RO")} facturi — restrânge filtrele pentru a vedea toate înregistrările.
          </div>
        )}

        {/* ── Error banner ───────────────────────────────────────────────── */}
        {invoicesError && (
          <QueryErrorBanner
            error={invoicesErr}
            label="facturile"
            onRetry={() => void refetchInvoices()}
          />
        )}

        {/* ── Rejected invoice alert ─────────────────────────────────────── */}
        {lastRejected && (
          <Banner
            variant="error"
            title="Factură respinsă de ANAF"
            actions={
              <Btn
                variant="danger"
                size="sm"
                onClick={() =>
                  void navigate({ to: "/invoices/$id", params: { id: lastRejected.id } })
                }
              >
                Vezi factura
              </Btn>
            }
          >
            Factura <b className="rf-mono">{lastRejected.fullNumber}</b>
            {contactMap[lastRejected.contactId]?.legalName && (
              <> către <b>{contactMap[lastRejected.contactId]!.legalName}</b></>
            )}{" "}
            a fost respinsă de ANAF
            {lastRejected.rejectionReason && (
              <>: <i>{lastRejected.rejectionReason}</i></>
            )}
            .
          </Banner>
        )}

        {/* ── Micro-enterprise ceiling alert (100.000 EUR, OUG 89/2025) ───── */}
        {regimeStatus &&
          (regimeStatus.level === "exceeded" || regimeStatus.level === "approaching") && (
            <Banner
              variant={regimeStatus.level === "exceeded" ? "error" : "warning"}
              title={
                regimeStatus.level === "exceeded"
                  ? "Plafon microîntreprindere depășit"
                  : "Vă apropiați de plafonul de microîntreprindere"
              }
              actions={
                <Btn size="sm" onClick={() => void navigate({ to: "/companies" })}>
                  Setări firmă
                </Btn>
              }
            >
              Cifra de afaceri {currentYear}: <b className="rf-mono">{regimeStatus.ytdTurnoverRon}</b>{" "}
              lei ({regimeStatus.pct}% din plafonul de{" "}
              <b className="rf-mono">{regimeStatus.ceilingRon}</b> lei ≈ 100.000 EUR).
              {regimeStatus.note ? ` ${regimeStatus.note}` : ""}
            </Banner>
          )}

        {/* ── Cash-VAT plafon alert (5.000.000 lei, OUG 8/2026) ──────────── */}
        {regimeStatus &&
          (regimeStatus.cashVatLevel === "exceeded" || regimeStatus.cashVatLevel === "approaching") && (
            <Banner
              variant={regimeStatus.cashVatLevel === "exceeded" ? "error" : "warning"}
              title={
                regimeStatus.cashVatLevel === "exceeded"
                  ? "Plafon TVA la încasare depășit"
                  : "Vă apropiați de plafonul TVA la încasare"
              }
            >
              Cifra de afaceri {currentYear}: <b className="rf-mono">{regimeStatus.ytdTurnoverRon}</b>{" "}
              lei (plafon TVA la încasare <b className="rf-mono">{regimeStatus.cashVatPlafonRon}</b> lei).
              {regimeStatus.cashVatNote ? ` ${regimeStatus.cashVatNote}` : ""}
            </Banner>
          )}

        {/* ── Intrastat threshold alerts (1.000.000 lei/flux, Ord. INS 1604/2025) ── */}
        {intrastat &&
          ([
            { label: "expedieri", f: intrastat.dispatches },
            { label: "introduceri", f: intrastat.arrivals },
          ] as const)
            .filter(({ f }) => f.level === "exceeded" || f.level === "approaching")
            .map(({ label, f }) => (
              <Banner
                key={label}
                variant={f.level === "exceeded" ? "error" : "warning"}
                title={
                  f.level === "exceeded"
                    ? `Prag Intrastat depășit (${label}) — declarație lunară obligatorie`
                    : `Vă apropiați de pragul Intrastat (${label})`
                }
              >
                Valoare {label} {currentYear}: <b className="rf-mono">{f.ytdRon}</b> lei ({f.pct}% din
                pragul de <b className="rf-mono">{intrastat.thresholdRon}</b> lei). Peste prag,
                depuneți Intrastat lunar până pe 15 (Ord. INS 1604/2025).
              </Banner>
            ))}

        {/* ── Unread notifications note ──────────────────────────────────── */}
        {unreadCount > 0 && (
          <Banner
            variant="info"
            title={`${unreadCount} mesaje SPV neprocesate`}
            actions={
              <Btn
                size="sm"
                onClick={() => void navigate({ to: "/notifications" })}
              >
                Vezi notificări
              </Btn>
            }
          />
        )}

        {/* ── KPI stat cards ─────────────────────────────────────────────── */}
        <div className="rf-grid-3">
          <StatCard
            icon="chart"
            label={`Vânzări — ${monthLabel}`}
            value={fmtRON(totalNet + totalVat)}
            unit="RON"
            ctx={`${periodInvoices.length} facturi · ${fmtRON(totalNet)} net`}
          />
          <StatCard
            icon="bank"
            label="TVA colectată"
            value={fmtRON(totalVat)}
            unit="RON"
            ctx={`din ${periodInvoices.length} facturi în perioadă`}
          />
          <div className="rf-card rf-stat">
            <div className="rf-stat-top">
              <span className="rf-stat-ic">
                <Icon name="fileOut" size={20} />
              </span>
            </div>
            <div className="rf-label">Facturi emise</div>
            <div className="rf-value">
              {periodInvoices.length}
              <span className="rf-unit">facturi</span>
            </div>
            <div style={{ display: "flex", gap: 6, marginTop: 10, flexWrap: "wrap" }}>
              {validatedCount > 0 && (
                <StatusBadge status="VALIDATED" />
              )}
              {submittedCount > 0 && (
                <span style={{ fontSize: 11.5, color: "var(--rf-info)", fontWeight: 500 }}>
                  {submittedCount} trimise
                </span>
              )}
              {rejectedCount > 0 && (
                <StatusBadge status="REJECTED" />
              )}
              {draftCount > 0 && (
                <span style={{ fontSize: 11.5, color: "var(--rf-text-muted)", fontWeight: 500 }}>
                  {draftCount} schițe
                </span>
              )}
            </div>
          </div>
        </div>

        {/* ── Companies + Activity ───────────────────────────────────────── */}
        <div style={{ display: "grid", gridTemplateColumns: "1fr 340px", gap: 20, alignItems: "start" }}>
          {/* Companies table */}
          <SectionCard
            icon="building"
            title="Companii administrate"
            subtitle={`${companies.length} ${companies.length === 1 ? "companie" : "companii"}`}
            actions={
              <Btn
                variant="ghost"
                size="sm"
                iconRight="chevronRight"
                onClick={() => void navigate({ to: "/companies" })}
              >
                Vezi toate
              </Btn>
            }
          >
            <div className="rf-tbl-wrap">
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th>CUI</th>
                    <th>Denumire</th>
                    <th>Localitate</th>
                    <th style={{ textAlign: "center" }}>SPV</th>
                    <th>Serie</th>
                    <th className="right">Ultimul nr.</th>
                  </tr>
                </thead>
                <tbody>
                  {companies.slice(0, 8).map((c) => (
                    <tr
                      key={c.id}
                      className={`clickable${c.id === activeCompanyId ? " selected" : ""}`}
                      onClick={() => void navigate({ to: "/companies/$id", params: { id: c.id } })}
                    >
                      <td className="rf-mono">{c.cui}</td>
                      <td style={{ fontWeight: 500 }}>{c.legalName}</td>
                      <td style={{ color: "var(--rf-text-muted)" }}>{c.city}</td>
                      <td style={{ textAlign: "center" }}>
                        {c.spvEnabled ? (
                          <Icon name="checkCircle" size={15} style={{ color: "var(--rf-success)" }} />
                        ) : (
                          <Icon name="xCircle" size={15} style={{ color: "var(--rf-text-dim)" }} />
                        )}
                      </td>
                      <td className="rf-mono">{c.invoiceSeries}</td>
                      <td className="right rf-mono">{c.lastInvoiceNumber}</td>
                    </tr>
                  ))}
                  {companies.length === 0 && (
                    <tr>
                      <td colSpan={6} style={{ textAlign: "center", padding: 20, color: "var(--rf-text-muted)" }}>
                        Nicio companie. <button type="button" className="rf-link" onClick={() => void navigate({ to: "/companies/new" })}>Adaugă companie →</button>
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </SectionCard>

          {/* Activity timeline */}
          <SectionCard icon="clock" title="Activitate recentă">
            {/* ANAF status indicator */}
            <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "0 4px 12px", fontSize: 12 }}>
              <span
                style={{
                  width: 8,
                  height: 8,
                  borderRadius: "50%",
                  background: anafConnected ? "var(--rf-success)" : "var(--rf-error)",
                  flexShrink: 0,
                }}
              />
              <span style={{ color: "var(--rf-text-muted)" }}>
                ANAF SPV: <b style={{ color: anafConnected ? "var(--rf-success)" : "var(--rf-error)" }}>
                  {anafConnected ? "conectat" : "neautentificat"}
                </b>
              </span>
            </div>
            {timelineItems.length === 0 ? (
              <div style={{ padding: "12px 4px", fontSize: 12.5, color: "var(--rf-text-muted)", textAlign: "center" }}>
                Fără notificări recente
              </div>
            ) : (
              <div style={{ padding: "0 4px 4px" }}>
                {timelineItems.map((n, i) => {
                  const color = notifColor(n.notificationType);
                  const icon  = notifIcon(n.notificationType);
                  return (
                    <div key={n.id} style={{ display: "flex", gap: 11, padding: "8px 10px" }}>
                      <div style={{ display: "flex", flexDirection: "column", alignItems: "center" }}>
                        <span
                          style={{
                            width: 26,
                            height: 26,
                            borderRadius: "50%",
                            display: "grid",
                            placeItems: "center",
                            flexShrink: 0,
                            background: `var(--rf-${color}-bg)`,
                            color: `var(--rf-${color})`,
                          }}
                        >
                          <Icon name={icon} size={13} />
                        </span>
                        {i < timelineItems.length - 1 && (
                          <span style={{ width: 1.5, flex: 1, background: "var(--rf-border)", marginTop: 3 }} />
                        )}
                      </div>
                      <div style={{ paddingBottom: 4, minWidth: 0 }}>
                        <div style={{ fontSize: 12.5, lineHeight: 1.4, fontWeight: 500 }}>{n.title}</div>
                        <div style={{ fontSize: 11, color: "var(--rf-text-muted)", marginTop: 2 }}>
                          {fmtTime(n.createdAt)}
                        </div>
                        {n.body && (
                          <div style={{ fontSize: 11.5, color: "var(--rf-text-muted)", marginTop: 1, lineHeight: 1.4 }}>
                            {n.body}
                          </div>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </SectionCard>
        </div>

        {/* ── Recent invoices ────────────────────────────────────────────── */}
        <SectionCard
          icon="fileOut"
          title={`Facturi recente · ultimele ${Math.min(recentInvoices.length, 10)}`}
          subtitle={invoiceTotal > 0 ? `${invoiceTotal} total` : undefined}
          actions={
            <div style={{ display: "flex", gap: 8 }}>
              <Btn
                size="sm"
                icon="plus"
                variant="primary"
                onClick={() => void navigate({ to: "/invoices/new" })}
              >
                Factură nouă
              </Btn>
              <Btn
                size="sm"
                variant="ghost"
                iconRight="chevronRight"
                onClick={() => void navigate({ to: "/invoices" })}
              >
                Vezi toate ({invoiceTotal})
              </Btn>
            </div>
          }
        >
          <div className="rf-tbl-wrap">
            <table className="rf-tbl">
              <thead>
                <tr>
                  <th>Nr. factură</th>
                  <th>Data</th>
                  <th>Cumpărător</th>
                  <th className="right">Net (RON)</th>
                  <th className="right">TVA</th>
                  <th className="right">Total</th>
                  <th>Status ANAF</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {recentInvoices.length === 0 ? (
                  <tr>
                    <td colSpan={8} style={{ textAlign: "center", padding: 24, color: "var(--rf-text-muted)" }}>
                      Fără facturi.{" "}
                      <button type="button" className="rf-link" onClick={() => void navigate({ to: "/invoices/new" })}>
                        Creează prima factură →
                      </button>
                    </td>
                  </tr>
                ) : (
                  recentInvoices.map((inv) => (
                    <tr
                      key={inv.id}
                      className="clickable"
                      onClick={() => void navigate({ to: "/invoices/$id", params: { id: inv.id } })}
                    >
                      <td className="rf-mono" style={{ fontWeight: 600 }}>{inv.fullNumber}</td>
                      <td style={{ color: "var(--rf-text-muted)" }}>{inv.issueDate}</td>
                      <td>{contactMap[inv.contactId]?.legalName ?? <span style={{ color: "var(--rf-text-dim)" }}>—</span>}</td>
                      <td className="right rf-mono">{fmtRON(inv.subtotalAmount)}</td>
                      <td className="right rf-mono" style={{ color: "var(--rf-text-muted)" }}>{fmtRON(inv.vatAmount)}</td>
                      <td className="right rf-mono" style={{ fontWeight: 600 }}>{fmtRON(inv.totalAmount)}</td>
                      <td><StatusBadge status={inv.status} /></td>
                      <td><Icon name="chevronRight" size={13} style={{ color: "var(--rf-text-dim)" }} /></td>
                    </tr>
                  ))
                )}
              </tbody>
            </table>
          </div>
        </SectionCard>

        <div style={{ fontSize: 11.5, color: "var(--rf-text-dim)", paddingBottom: 8 }}>
          Datele se actualizează automat la fiecare 60 s. Toate sumele sunt în <b>RON</b>.
        </div>
      </div>
    </div>
  );
}
